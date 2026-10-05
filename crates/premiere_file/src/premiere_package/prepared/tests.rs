//! Operation reuse/ownership tests, not Adobe acceptance or complete coverage.

mod black_canvas;
mod export_media;

use std::{fs, io::Read};

#[cfg(feature = "ffmpeg-library")]
use fx_schema::animator::AnimatorData;
use serde_json::{json, Value};
use tesseract_file::{AssetKind, TesseractFileBuilder};

use super::*;
use crate::convert::bake_scripts;

fn archive(root: &Path, value: &Value) -> TesseractFile {
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(value).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/video-30fps.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .write(root.join("input.tsrct"))
        .unwrap()
}

fn scripted(code: &str) -> Value {
    let mut value = crate::test_support::editable_document();
    value["composition"]["dynamics"] = json!({"entries":[{
        "target":{"kind":"layer","layerId":1,"propertyType":"opacity"},
        "animator":{"type":"jsScript","layerTimeJsCode":code}
    }]});
    value
}

#[cfg(feature = "ffmpeg-library")]
fn calls() -> usize {
    crate::convert::PREPARATION_CALLS.with(|count| count.get())
}

fn read_native(path: &Path) -> PrProjectFile {
    let bytes = fs::read(path).unwrap();
    let mut xml = String::new();
    flate2::read::GzDecoder::new(bytes.as_slice())
        .read_to_string(&mut xml)
        .unwrap();
    let (project, omissions) = crate::format::inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    project
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn natural_video_frame_uses_active_media_and_preserves_the_original_view() {
    let root = tempfile::tempdir().unwrap();
    let mut value = crate::test_support::editable_document();
    let source = &mut value["composition"]["layers"][0]["source"];
    source.as_object_mut().unwrap().remove("sourceRect");
    source["eyeContact"] = json!({
        "enabled": true, "eyeContactAssetId": "replacement"
    });
    let file = TesseractFileBuilder::from_project_json(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/video-30fps.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset(
            "replacement",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/source_rect_64x36.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .write(root.path().join("input.tsrct"))
        .unwrap();
    let original = file.project_json().unwrap();
    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    assert_eq!(
        operation.original_document().to_json_value().unwrap(),
        original
    );
    assert_eq!(file.project_json().unwrap(), original);
    let prepared = operation.prepared_document().to_json_value().unwrap();
    assert_eq!(
        prepared["composition"]["layers"][0]["source"]["sourceRect"],
        json!({
            "x": 0, "y": 0, "width": 64, "height": 36
        })
    );
    assert_eq!(
        prepared["composition"]["layers"][0]["source"]["eyeContact"],
        original["composition"]["layers"][0]["source"]["eyeContact"]
    );
    let project = operation.project.unwrap();
    assert_eq!(project.media.len(), 1);
    let source = project
        .media
        .values()
        .next()
        .unwrap()
        .video
        .as_ref()
        .unwrap();
    assert_eq!((source.width, source.height), (64, 36));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn a_long_plain_group_keeps_child_script_keys_but_an_omitted_effect_group_does_not() {
    for (duration, effect, written) in [(500, false, 1), (1000, false, 1), (1000, true, 0)] {
        let root = tempfile::tempdir().unwrap();
        let mut value = scripted("return 100 - 20 * input.time.seconds;");
        let mut child = value["composition"]["layers"][0].clone();
        child["parent"] = json!(10);
        child["playback"] = crate::test_support::linear_playback(
            json!({"start": child["playback"]["inputRange"]["start"], "duration": 500}),
            json!({"start": child["sourceRange"]["start"], "duration": 500}),
        );
        child["sourceRange"]["duration"] = json!(500);
        value["composition"]["layers"][0] = json!({
            "type": "Group", "id": 10, "name": "Group with a short child",
            "playback": crate::test_support::linear_playback(json!({"start": 0, "duration": duration}), json!({"start": 0, "duration": duration})),
            "transform": value["composition"]["layers"][1]["transform"],
            "layers": [child]
        });
        if effect {
            value["composition"]["layers"][0]["effects"] = json!([{
                "id": 100, "effect": {"type": "gaussianBlur", "blurriness": 5}
            }]);
        }
        let file = archive(root.path(), &value);
        let prepared = Premiere
            .prepare_export(&file, file.project(), &Default::default())
            .unwrap();
        assert_eq!(
            prepared.original_document().to_json_value().unwrap(),
            file.project_json().unwrap()
        );
        if duration == 1000 && !effect {
            assert!(
                prepared.losses().losses.iter().any(|loss| loss.source
                    == crate::ExportLossSource::Layer(10.into())
                    && loss.domain == crate::ExportLossDomain::Picture
                    && loss.kind == crate::ExportLossKind::Field(crate::ExportField::Placement)
                    && loss.omission.kind == crate::OmissionKind::Approximated),
                "{:?}",
                prepared.losses().losses
            );
        }
        let expected =
            format!("{written} of 1 baked JS animation tracks were written as native keys");
        assert!(
            prepared
                .losses()
                .diagnostics
                .iter()
                .any(|item| item.reason == expected),
            "{duration}: {:?}",
            prepared.losses().diagnostics
        );
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn uncovered_leading_internal_and_trailing_gaps_export_without_changing_content() {
    let root = tempfile::tempdir().unwrap();
    let mut value = crate::test_support::editable_document();
    let layers = value["composition"]["layers"].as_array_mut().unwrap();
    layers.truncate(1);
    layers[0]["playback"] = crate::test_support::linear_playback(
        json!({"start": 200, "duration": 200}),
        json!({"start": 0, "duration": 200}),
    );
    layers[0]["sourceRange"] = json!({"start": 0, "duration": 200});
    let mut second = layers[0].clone();
    second["id"] = json!(3);
    second["name"] = json!("Second cut");
    second["playback"] = crate::test_support::linear_playback(
        json!({"start": 600, "duration": 200}),
        json!({"start": 200, "duration": 200}),
    );
    second["sourceRange"] = json!({"start": 200, "duration": 200});
    layers.push(second);
    let file = archive(root.path(), &value);
    let input = root.path().join("input.tsrct");
    let before = fs::read(&input).unwrap();
    let original = file.project_json().unwrap();
    let prepare = || {
        Premiere
            .prepare_export(&file, file.project(), &Default::default())
            .unwrap()
    };
    let assert_content = |project: &PrProjectFile| {
        let ms = crate::schema::TICKS_PER_MILLISECOND;
        let sequence = project.single_sequence().unwrap();
        assert_eq!(sequence.occurrence_end_ticks(), 800 * ms);
        assert_eq!(sequence.video_tracks.len(), 1);
        assert_eq!(sequence.video_items().count(), 2, "no background inserted");
        assert_eq!(project.media.len(), 1);
        assert_eq!(
            sequence
                .video_occurrences()
                .map(|clip| (clip.timeline_ticks(), clip.source_ticks(), clip.enabled))
                .collect::<Vec<_>>(),
            [
                (200 * ms..400 * ms, 0..200 * ms, true),
                (600 * ms..800 * ms, 200 * ms..400 * ms, true),
            ]
        );
    };
    // The reader still ends at the last occurrence, so check the declared tail
    // in the lowered model and independently in the written native work area.
    let assert_work_area = |path: &Path| {
        let bytes = fs::read(path).unwrap();
        let mut xml = String::new();
        flate2::read::GzDecoder::new(bytes.as_slice())
            .read_to_string(&mut xml)
            .unwrap();
        assert!(xml.contains("<MZ.WorkOutPoint>254016000000</MZ.WorkOutPoint>"));
    };
    let (native, _) = prepare().into_native().unwrap();
    assert_content(&native);
    let ms = crate::schema::TICKS_PER_MILLISECOND;
    let sequence = native.single_sequence().unwrap();
    assert_eq!(sequence.end_ticks(), 1000 * ms);
    assert_eq!(
        sequence.gaps(&native.media),
        [0..200 * ms, 400 * ms..600 * ms, 800 * ms..1000 * ms]
    );
    let output = root.path().join("output");
    let staged = prepare()
        .stage_with_picture_replacements(root.path(), &output, &[])
        .unwrap();
    assert_content(&read_native(&staged.directory().join("project.prproj")));
    assert_work_area(&staged.directory().join("project.prproj"));
    let temporary = staged.directory().to_owned();
    drop(staged);
    assert!(!temporary.exists());
    assert!(!output.exists());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);

    let checked = crate::tesseract_to_premiere(&input, &output, true).unwrap();
    assert!(!output.exists());
    let written = crate::tesseract_to_premiere(&input, &output, false).unwrap();
    assert_eq!(checked, written);
    assert_content(&read_native(&output.join("project.prproj")));
    assert_work_area(&output.join("project.prproj"));
    assert_eq!(fs::read(&input).unwrap(), before);
    assert_eq!(file.project_json().unwrap(), original);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn inspect_then_stage_uses_one_real_preparation_and_keeps_original_script() {
    let root = tempfile::tempdir().unwrap();
    let file = archive(
        root.path(),
        &scripted("return 100 - 20 * input.time.seconds;"),
    );
    let before = fs::read(root.path().join("input.tsrct")).unwrap();
    let count = calls();
    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    assert_eq!(calls(), count + 1);
    assert!(std::ptr::eq(operation.original_document(), file.project()));
    assert!(matches!(
        operation
            .original_document()
            .composition()
            .dynamics()
            .entries()[0]
            .animator
            .data(),
        AnimatorData::JsScript { .. }
    ));
    assert!(matches!(
        operation
            .prepared_document()
            .composition()
            .dynamics()
            .entries()[0]
            .animator
            .data(),
        AnimatorData::Keyframes { .. }
    ));
    let report = operation.losses().clone();
    assert_eq!(operation.losses(), &report);
    let output = root.path().join("final");
    let staged = operation
        .stage_with_picture_replacements(root.path(), &output, &[])
        .unwrap();
    assert_eq!(calls(), count + 1, "inspection/emission must not rebake");
    assert_eq!(staged.report().diagnostics, report.diagnostics);
    let native = read_native(&staged.directory().join("project.prproj"));
    let clips: Vec<_> = native
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .collect();
    assert_eq!(clips.len(), 1);
    assert!(clips[0].animations.iter().any(|animation| matches!(animation, crate::schema::PrPropertyAnimation::Opacity(keys) if keys.len() >= 2 && keys[0].value == 100.0)));
    assert!(!output.exists());
    assert_eq!(fs::read(root.path().join("input.tsrct")).unwrap(), before);
    for artifact in &staged.report().artifacts {
        assert!(staged.directory().join(&artifact.path).is_file());
    }
    let temporary = staged.directory().to_owned();
    drop(staged);
    assert!(!temporary.exists());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn source_slot_stage_reuses_preparation_and_retains_native_audio() {
    let root = tempfile::tempdir().unwrap();
    let mut value = crate::test_support::editable_document();
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "type":"Audio", "id":4, "name":"Independent music",
            "playback": crate::test_support::linear_playback(json!({"start":100,"duration":200}), json!({"start":0,"duration":200})),
            "sourceRange":{"start":0,"duration":200}, "sourceIntrinsicDuration":200,
            "volume":0.5, "source":{"assetId":"music"}
        }));
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let file = TesseractFileBuilder::from_project_json(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            fixtures.join("video-30fps.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset("music", fixtures.join("audio-mono.wav"), AssetKind::Audio)
        .unwrap()
        .write(root.path().join("input.tsrct"))
        .unwrap();
    let count = calls();
    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    let recipe = operation.packing_recipe();
    assert!(recipe.is_complete());
    let container = recipe
        .containers()
        .into_iter()
        .find(|container| container.token == recipe.root())
        .unwrap();
    let boundary = recipe
        .boundaries()
        .into_iter()
        .find(|boundary| boundary.layer == fx_schema::LayerId::new(1))
        .unwrap();
    let replacement = PictureReplacement {
        packing_id: recipe.id(),
        container: recipe.root(),
        boundaries: vec![boundary.token],
        picture: crate::AfterEffectsPicture {
            composition_guid: "00000001-0000-0000-0000-000000000000".into(),
            relative_path: "media/ae-0001/compositions.aep".into(),
            dimensions: container.dimensions,
            frame_rate: container.frame_rate,
            intrinsic_duration_ticks: container.timeline_end_ticks,
            timeline_ticks: 0..container.timeline_end_ticks,
            source_ticks: 0..container.timeline_end_ticks,
            enabled: true,
        },
    };
    let audio_count = operation
        .project
        .as_ref()
        .unwrap()
        .single_sequence()
        .unwrap()
        .audio
        .len();
    let staged = operation
        .stage_with_picture_replacements(root.path(), &root.path().join("final"), &[replacement])
        .unwrap();
    assert_eq!(calls(), count + 1);
    assert_eq!(
        staged.after_effects_paths(),
        &[PathBuf::from("media/ae-0001/compositions.aep")]
    );
    assert!(!staged
        .directory()
        .join("media/ae-0001/compositions.aep")
        .exists());
    let native = read_native(&staged.directory().join("project.prproj"));
    assert_eq!(
        audio_count, 1,
        "fixture must exercise a retained sound owner"
    );
    let sequence = native.single_sequence().unwrap();
    assert_eq!(sequence.audio.len(), audio_count);
    let sound = &sequence.audio[0];
    assert_eq!(sound.start_ticks, 25_401_600_000);
    assert_eq!(sound.end_ticks, 76_204_800_000);
    assert_eq!(sound.in_ticks, 0);
    assert_eq!(sound.out_ticks, 50_803_200_000);
    assert!((sound.volume.as_f64() - 0.5).abs() < 1e-6);
    assert!(!root.path().join("final").exists());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn rejected_video_picture_keeps_embedded_sound_once_through_replacement() {
    let root = tempfile::tempdir().unwrap();
    let mut value = crate::test_support::editable_document();
    value["duration"] = json!(0.3);
    let layers = value["composition"]["layers"].as_array_mut().unwrap();
    layers[0]["volume"] = json!(0.5);
    layers[0]["playback"] = crate::test_support::linear_playback(
        json!({"start": layers[0]["playback"]["inputRange"]["start"], "duration": 200}),
        json!({"start": layers[0]["sourceRange"]["start"], "duration": 200}),
    );
    layers[0]["sourceRange"]["duration"] = json!(200);
    layers[0]["sourceIntrinsicDuration"] = json!(200);
    layers[0]["masks"] = json!([{"id":20,"mode":"subtract","layer":3}]);
    layers[1]["activeRange"]["duration"] = json!(300);
    layers.push(json!({
        "type":"Rect", "id":3, "name":"Guide", "activeRange":{"start":0,"duration":300},
        "transform":{"anchorPoint":[0,0],"position":[0,0],"scale":[100,100],"rotation":0,"opacity":100},
        "rect":{"size":[1920,1080],"fillColor":[0,0,0,1]}
    }));
    let file = TesseractFileBuilder::from_project_json(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/video-with-audio.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .write(root.path().join("input.tsrct"))
        .unwrap();
    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    let native = operation
        .project
        .as_ref()
        .unwrap()
        .single_sequence()
        .unwrap();
    assert_eq!(
        native.audio.len(),
        1,
        "picture omission must not discard embedded sound"
    );
    assert!(native
        .video_occurrences()
        .all(|clip| clip.media.as_str() != "premiere-video-1"));
    let recipe = operation.packing_recipe();
    let container = recipe
        .containers()
        .into_iter()
        .find(|container| container.token == recipe.root())
        .unwrap();
    let replacement = PictureReplacement {
        packing_id: recipe.id(),
        container: recipe.root(),
        boundaries: container.boundaries,
        picture: crate::AfterEffectsPicture {
            composition_guid: "00000001-0000-0000-0000-000000000000".into(),
            relative_path: "media/ae-0001/compositions.aep".into(),
            dimensions: container.dimensions,
            frame_rate: container.frame_rate,
            intrinsic_duration_ticks: container.timeline_end_ticks,
            timeline_ticks: 0..container.timeline_end_ticks,
            source_ticks: 0..container.timeline_end_ticks,
            enabled: true,
        },
    };
    let staged = operation
        .stage_with_picture_replacements(root.path(), &root.path().join("final"), &[replacement])
        .unwrap();
    let native = read_native(&staged.directory().join("project.prproj"));
    let sounds = &native.single_sequence().unwrap().audio;
    assert_eq!(sounds.len(), 1);
    assert_eq!(
        (
            sounds[0].start_ticks,
            sounds[0].end_ticks,
            sounds[0].in_ticks,
            sounds[0].out_ticks
        ),
        (0, 50_803_200_000, 0, 50_803_200_000)
    );
    assert!((sounds[0].volume.as_f64() - 0.5).abs() < 1e-6);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn retained_static_motion_values_are_written_without_repreparation() {
    let root = tempfile::tempdir().unwrap();
    let mut value = crate::test_support::editable_document();
    value["composition"]["layers"][0]["transform"] = json!({
        "anchorPoint":[960,540], "position":[800,450], "scale":[150,150],
        "rotation":15, "opacity":75
    });
    let file = archive(root.path(), &value);
    let count = calls();
    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    let staged = operation
        .stage_with_picture_replacements(root.path(), &root.path().join("final"), &[])
        .unwrap();
    assert_eq!(calls(), count + 1);
    let native = read_native(&staged.directory().join("project.prproj"));
    let clip = native
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!(clip.transform.anchor_point, [0.5, 0.5]);
    assert_eq!(clip.transform.scale, [150.0, 150.0]);
    assert_eq!(clip.transform.rotation, 15.0);
    assert_eq!(clip.opacity, 75.0);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn nested_blended_picture_and_skew_omission_keep_native_sibling() {
    for skewed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut value = crate::test_support::editable_document();
        let mut child = value["composition"]["layers"][1].clone();
        let group_transform = child["transform"].clone();
        child["id"] = json!(3);
        child["blendMode"] = json!("multiply");
        if skewed {
            child["transform"]["skew"] = json!(20);
        }
        value["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .insert(
                1,
                json!({
                    "type":"Group", "id":10, "name":"Nested rectangle",
                    "playback": crate::test_support::linear_playback(json!({"start":0,"duration":1000}), json!({"start": 0, "duration": 1000})),
                    "transform":group_transform, "layers":[child]
                }),
            );
        let file = archive(root.path(), &value);
        let operation = Premiere
            .prepare_export(&file, file.project(), &Default::default())
            .unwrap();
        let staged = operation
            .stage_with_picture_replacements(root.path(), &root.path().join("final"), &[])
            .unwrap();
        let path = staged.directory().join("project.prproj");
        let target = PrProjectFile::import_targets(&path)
            .unwrap()
            .into_iter()
            .find(|target| target.name == file.project().composition().name())
            .unwrap();
        let (native, _) = PrProjectFile::load_selected(&path, Some(&target.id)).unwrap();
        let sequence = native.single_sequence().unwrap();
        assert_eq!(sequence.video_occurrences().count(), 1);
        let nests: Vec<_> = sequence.nest_occurrences().collect();
        assert_eq!(nests.len(), usize::from(!skewed));
        if let Some(nest) = nests.first() {
            assert!(nest.sequence.video_items().any(|item| matches!(item,
                crate::format::PrVideoItem::Graphic(graphic)
                if graphic.blend_mode == crate::schema::PrBlendMode::Multiply
            )));
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn unsupported_motion_keeps_native_sibling() {
    let root = tempfile::tempdir().unwrap();
    let mut value = crate::test_support::editable_document();
    let mut sibling = value["composition"]["layers"][0].clone();
    sibling["id"] = json!(3);
    sibling["transform"]["skew"] = json!(12);
    value["composition"]["layers"][0]["transform"]["scale"] = json!([-100, 100]);
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(1, sibling);
    let file = archive(root.path(), &value);
    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    let staged = operation
        .stage_with_picture_replacements(root.path(), &root.path().join("final"), &[])
        .unwrap();
    let native = read_native(&staged.directory().join("project.prproj"));
    assert_eq!(
        native
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .count(),
        1
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn rejected_script_is_not_retried_during_emission() {
    let root = tempfile::tempdir().unwrap();
    let file = archive(root.path(), &scripted("return 'not numeric';"));
    let count = calls();
    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    let diagnostics = operation.losses().diagnostics.clone();
    assert!(!diagnostics.is_empty());
    let staged = operation
        .stage_with_picture_replacements(root.path(), &root.path().join("final"), &[])
        .unwrap();
    assert_eq!(calls(), count + 1);
    assert_eq!(staged.report().diagnostics, diagnostics);
    let native = read_native(&staged.directory().join("project.prproj"));
    assert!(native
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .all(|clip| clip.animations.is_empty()));
}

#[test]
fn no_native_content_retains_inspection_but_keeps_the_old_emission_error() {
    let root = tempfile::tempdir().unwrap();
    let mut value = scripted("return 100 - 20 * input.time.seconds;");
    value["composition"]["layers"][0]["masks"] = json!([{"id":20,"mode":"subtract","layer":3}]);
    value["composition"]["layers"].as_array_mut().unwrap().push(json!({
        "type":"Rect", "id":3, "name":"Guide", "activeRange":{"start":0,"duration":1000},
        "transform":{"anchorPoint":[0,0],"position":[0,0],"scale":[100,100],"rotation":0,"opacity":100},
        "rect":{"size":[1920,1080],"fillColor":[0,0,0,1]}
    }));
    let file = archive(root.path(), &value);
    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    assert!(!operation.losses().has_native_content);
    assert!(operation
        .losses()
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.reason.starts_with("baked JS animation of")));
    // Reconstruct the pre-refactor ordinary path, rather than compare two wrappers.
    let mut old_diagnostics = Vec::new();
    let baked = bake_scripts(file.project(), &mut old_diagnostics).unwrap();
    let media = inspect_media(&file, baked.document()).unwrap();
    let audio = inspect_audio(&file, baked.document(), &media).unwrap();
    let old = crate::convert::export_document(
        baked.document(),
        &media,
        &audio,
        &file.metadata().fonts,
        FrameRate::Fps30,
        &mut old_diagnostics,
    )
    .unwrap_err();
    let error = operation
        .stage_with_picture_replacements(root.path(), &root.path().join("final"), &[])
        .unwrap_err();
    assert_eq!(error.to_string(), old.to_string());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn empty_root_link_checks_original_content_and_full_clock_before_staging() {
    let ticks = crate::schema::TICKS;
    for (has_layers, partial, emptied_view) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (true, false, true),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut value = crate::test_support::editable_document();
        if !has_layers {
            value["composition"]["layers"] = json!([]);
        }
        let file = archive(root.path(), &value);
        if emptied_view {
            value["composition"]["layers"] = json!([]);
        }
        let view =
            EditableFxCompositionDocument::from_json_slice(&serde_json::to_vec(&value).unwrap())
                .unwrap();
        let prepared = Premiere
            .prepare_export(&file, &view, &Default::default())
            .unwrap();
        let picture = AfterEffectsPicture {
            composition_guid: "00000001-0000-0000-0000-000000000000".into(),
            relative_path: "media/ae-0001/compositions.aep".into(),
            dimensions: [1920, 1080],
            frame_rate: FrameRate::Fps30,
            intrinsic_duration_ticks: ticks,
            timeline_ticks: 0..if partial { ticks / 2 } else { ticks },
            source_ticks: 0..if partial { ticks / 2 } else { ticks },
            enabled: true,
        };
        let output = root.path().join("output");
        let result = prepared.stage_empty_root_with_after_effects(root.path(), &output, &picture);
        if has_layers || partial {
            let error = result.unwrap_err();
            assert!(error.is_unsupported());
            let error = error.to_string();
            assert!(
                error.contains(if has_layers {
                    "originally empty"
                } else {
                    "full source and sequence clocks"
                }),
                "{error}"
            );
        } else {
            let stage = result.unwrap();
            assert_eq!(
                stage.report().artifacts.len(),
                1,
                "unused source media is not copied"
            );
            assert_eq!(
                stage.after_effects_paths(),
                [PathBuf::from("media/ae-0001/compositions.aep")]
            );
        }
        assert!(!output.exists());
        assert_eq!(
            fs::read_dir(root.path()).unwrap().count(),
            1,
            "private staging cleaned"
        );
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn writer_rejection_cleans_private_staging() {
    let root = tempfile::tempdir().unwrap();
    let mut value = crate::test_support::editable_document();
    value["composition"]["name"] = json!("x".repeat(256));
    let file = archive(root.path(), &value);
    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    assert!(operation.losses().has_native_content);
    let error = operation
        .stage_with_picture_replacements(root.path(), &root.path().join("final"), &[])
        .unwrap_err();
    assert!(error.to_string().contains("sequence name"), "{error}");
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn destination_collision_keeps_user_files_and_does_not_reprepare() {
    let root = tempfile::tempdir().unwrap();
    let file = archive(root.path(), &crate::test_support::editable_document());
    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    let count = calls();
    let output = root.path().join("final");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("user"), b"keep").unwrap();
    assert!(operation
        .stage_with_picture_replacements(root.path(), &output, &[])
        .is_err());
    assert_eq!(calls(), count);
    assert_eq!(fs::read(output.join("user")).unwrap(), b"keep");
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn changed_media_payload_cannot_supply_staged_native_media() {
    let root = tempfile::tempdir().unwrap();
    let file = archive(root.path(), &crate::test_support::editable_document());
    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    let media =
        fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/video-30fps.mp4"))
            .unwrap();
    let source = root.path().join("input.tsrct");
    let mut bytes = fs::read(&source).unwrap();
    // Assets are stored uncompressed. Change actual payload, not unrelated ZIP
    // metadata: whole-archive freshness remains the coordinator's responsibility.
    let start = bytes
        .windows(media.len())
        .position(|window| window == media)
        .unwrap();
    bytes[start + media.len() / 2] ^= 1;
    fs::write(source, bytes).unwrap();
    assert!(operation
        .stage_with_picture_replacements(root.path(), &root.path().join("final"), &[])
        .is_err());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn ordinary_check_and_write_each_prepare_once_with_legacy_diagnostics() {
    let root = tempfile::tempdir().unwrap();
    let file = archive(
        root.path(),
        &scripted("return 100 - 20 * input.time.seconds;"),
    );
    let mut legacy = Vec::new();
    let baked = bake_scripts(file.project(), &mut legacy).unwrap();
    let media = inspect_media(&file, baked.document()).unwrap();
    let audio = inspect_audio(&file, baked.document(), &media).unwrap();
    let exported = crate::convert::export_document(
        baked.document(),
        &media,
        &audio,
        &file.metadata().fonts,
        FrameRate::Fps30,
        &mut legacy,
    )
    .unwrap();
    baked.report_discarded(&exported.written, &mut legacy);
    let mut expected = exported.project;
    super::super::bind_media(&mut expected, &file, &root.path().join("legacy-output")).unwrap();
    let legacy_path = root.path().join("legacy.prproj");
    PremiereProjectXml::new(&expected)
        .unwrap()
        .write_new(&legacy_path)
        .unwrap();
    // Read both writers: the native wire canonicalizes the first key's unused
    // incoming easing, so comparing readback to a pre-writer model is incorrect.
    let expected = read_native(&legacy_path);
    let source = root.path().join("input.tsrct");
    for (name, check) in [("check", true), ("write", false)] {
        let output = root.path().join(name);
        let before = calls();
        let report = crate::premiere_package::save_tesseract_as_premiere(
            &source,
            &output,
            FrameRate::Fps30,
            check,
        )
        .unwrap();
        assert_eq!(calls(), before + 1);
        assert_eq!(report.diagnostics, legacy);
        assert_eq!(output.exists(), !check);
        if !check {
            let actual = read_native(&output.join("project.prproj"));
            let expected = expected
                .single_sequence()
                .unwrap()
                .video_occurrences()
                .next()
                .unwrap();
            let actual = actual
                .single_sequence()
                .unwrap()
                .video_occurrences()
                .next()
                .unwrap();
            assert_eq!(actual.animations, expected.animations);
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn supplied_view_not_archive_document_is_retained_and_emitted() {
    let root = tempfile::tempdir().unwrap();
    let file = archive(root.path(), &crate::test_support::editable_document());
    let mut value = crate::test_support::editable_document();
    value["composition"]["name"] = json!("Caller view");
    let view = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let operation = Premiere
        .prepare_export(
            &file,
            &view,
            &crate::PremiereExportOptions {
                frame_rate: Some(FrameRate::Fps25),
            },
        )
        .unwrap();
    assert!(std::ptr::eq(operation.original_document(), &view));
    let staged = operation
        .stage_with_picture_replacements(root.path(), &root.path().join("final"), &[])
        .unwrap();
    let native = read_native(&staged.directory().join("project.prproj"));
    assert_eq!(native.single_sequence().unwrap().name, "Caller view");
    assert_eq!(
        native.single_sequence().unwrap().frame_rate,
        FrameRate::Fps25
    );
}
