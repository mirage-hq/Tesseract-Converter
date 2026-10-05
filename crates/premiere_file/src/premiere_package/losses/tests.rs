//! Structural observation of the real lowerer, not a native Adobe/fidelity test.

use std::{fs, path::Path};

use fx_schema::LayerId;
#[cfg(feature = "ffmpeg-library")]
use fx_schema::{
    animator::{AnimationGraph, AnimationGraphEntry, PropertyAnimator},
    EffectId, FxItemId, PropType, PropertyTarget, PropertyValue,
};
use serde_json::{json, Value};
use tesseract_file::{AssetKind, TesseractFileBuilder};

#[cfg(feature = "ffmpeg-library")]
use crate::{ExportField, ExportLossDomain, ExportLossKind};
use crate::{ExportLossSource, FrameRate, Premiere};

fn write_archive(directory: &Path, value: &Value) -> tesseract_file::TesseractFile {
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(value).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/video-30fps.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .write(directory.join("input.tsrct"))
        .unwrap()
}

fn inspect(archive: &tesseract_file::TesseractFile) -> crate::ExportLossReport {
    Premiere
        .inspect_export_losses(archive, archive.project(), &Default::default())
        .unwrap()
}

fn legacy_check(
    directory: &Path,
) -> crate::error::Result<fx_conv::ConversionReport<crate::Omission>> {
    crate::premiere_package::save_tesseract_as_premiere(
        &directory.join("input.tsrct"),
        &directory.join("output"),
        FrameRate::Fps30,
        true,
    )
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn ordinary_native_document_retains_the_existing_diagnostics_without_output() {
    let root = tempfile::tempdir().unwrap();
    let archive = write_archive(root.path(), &crate::test_support::editable_document());
    let original = fs::read(root.path().join("input.tsrct")).unwrap();
    let report = inspect(&archive);
    assert!(report.has_native_content);
    assert!(!report.losses_truncated);
    assert!(report.losses.is_empty(), "{report:?}");
    assert_eq!(
        report.diagnostics,
        legacy_check(root.path()).unwrap().diagnostics
    );
    assert!(!root.path().join("output").exists());
    assert_eq!(fs::read(root.path().join("input.tsrct")).unwrap(), original);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn actual_field_guards_distinguish_metadata_picture_and_audio() {
    let mut value = crate::test_support::editable_document();
    let video = &mut value["composition"]["layers"][0];
    video["description"] = json!("Description containing layer 999, masks and audio: not an ID");
    video["cornerRadius"] = json!(12);
    video["preserveAudioPitch"] = json!(true);
    let root = tempfile::tempdir().unwrap();
    let archive = write_archive(root.path(), &value);
    let report = inspect(&archive);
    for (field, domain) in [
        (ExportField::Description, ExportLossDomain::Metadata),
        (ExportField::CornerRadius, ExportLossDomain::Picture),
        (ExportField::AudioPitchPreservation, ExportLossDomain::Audio),
    ] {
        assert!(
            report.losses.iter().any(|loss| loss.source
                == ExportLossSource::Layer(LayerId::new(1))
                && loss.kind == ExportLossKind::Field(field)
                && loss.domain == domain),
            "{report:?}"
        );
    }
    assert_eq!(
        report.diagnostics,
        legacy_check(root.path()).unwrap().diagnostics
    );
}

fn script_document(code: &str) -> Value {
    let mut value = crate::test_support::editable_document();
    value["composition"]["dynamics"] = json!({"entries":[{
        "target":{"kind":"layer","layerId":1,"propertyType":"opacity"},
        "animator":{"type":"jsScript","layerTimeJsCode":code}
    }]});
    value
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn supported_script_preparation_matches_ordinary_export_diagnostics() {
    let root = tempfile::tempdir().unwrap();
    let archive = write_archive(
        root.path(),
        &script_document("return 100 - 20 * input.time.seconds;"),
    );
    let report = inspect(&archive);
    let ordinary = legacy_check(root.path()).unwrap();
    assert_eq!(report.diagnostics, ordinary.diagnostics);
    assert!(report.has_native_content);
    let summary = report
        .losses
        .iter()
        .find(|loss| {
            loss.omission.reason == "1 of 1 baked JS animation tracks were written as native keys"
        })
        .unwrap();
    assert_eq!(summary.source, ExportLossSource::Document);
    assert_eq!(summary.domain, ExportLossDomain::Unclassified);
    assert_eq!(summary.kind, ExportLossKind::Unclassified);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn rejected_script_preparation_matches_ordinary_export_diagnostics() {
    let root = tempfile::tempdir().unwrap();
    let archive = write_archive(
        root.path(),
        &script_document("return 'not a numeric value';"),
    );
    let report = inspect(&archive);
    assert_eq!(
        report.diagnostics,
        legacy_check(root.path()).unwrap().diagnostics
    );
    assert!(report.has_native_content);
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.source == ExportLossSource::Document
            && loss.domain == ExportLossDomain::Unclassified));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn discarded_script_keys_match_ordinary_export_diagnostics() {
    let root = tempfile::tempdir().unwrap();
    let mut value = script_document("return 100 - 20 * input.time.seconds;");
    let mut other = value["composition"]["layers"][0].clone();
    other["id"] = json!(4);
    value["composition"]["layers"][0]["masks"] = json!([{"id":20,"mode":"subtract","layer":3}]);
    let layers = value["composition"]["layers"].as_array_mut().unwrap();
    layers.push(json!({
        "type":"Rect", "id":3, "name":"Guide",
        "activeRange":{"start":0,"duration":1000},
        "transform":{"anchorPoint":[0,0],"position":[0,0],"scale":[100,100],"rotation":0,"opacity":100},
        "rect":{"size":[1920,1080],"fillColor":[0,0,0,1]}
    }));
    layers.push(other);
    let archive = write_archive(root.path(), &value);
    let report = inspect(&archive);
    assert_eq!(
        report.diagnostics,
        legacy_check(root.path()).unwrap().diagnostics
    );
    assert!(report.has_native_content);
    let discarded = report
        .losses
        .iter()
        .find(|loss| loss.omission.reason.starts_with("baked JS animation of"))
        .unwrap();
    // Aggregated script diagnostics retain their existing owner text; inspection
    // deliberately does not parse that text into a typed layer/field identity.
    assert_eq!(discarded.source, ExportLossSource::Document);
    assert_eq!(discarded.domain, ExportLossDomain::Unclassified);
    assert_eq!(discarded.kind, ExportLossKind::Unclassified);
}

#[test]
fn unsupported_only_baked_script_reports_discarded_keys() {
    let mut value = script_document("return 100 - 20 * input.time.seconds;");
    value["composition"]["layers"][0]["masks"] = json!([{"id":20,"mode":"subtract","layer":3}]);
    value["composition"]["layers"].as_array_mut().unwrap().push(json!({
        "type":"Rect", "id":3, "name":"Guide",
        "activeRange":{"start":0,"duration":1000},
        "transform":{"anchorPoint":[0,0],"position":[0,0],"scale":[100,100],"rotation":0,"opacity":100},
        "rect":{"size":[1920,1080],"fillColor":[0,0,0,1]}
    }));
    let root = tempfile::tempdir().unwrap();
    let archive = write_archive(root.path(), &value);
    let report = inspect(&archive);
    assert!(!report.has_native_content);
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.omission.reason.starts_with("baked JS animation of")),
        "{report:?}"
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn a_description_alone_is_not_a_picture_loss() {
    let mut value = crate::test_support::editable_document();
    value["composition"]["layers"][0]["description"] = json!("optional note");
    let root = tempfile::tempdir().unwrap();
    let archive = write_archive(root.path(), &value);
    let report = inspect(&archive);
    assert_eq!(report.losses.len(), 1, "{report:?}");
    assert_eq!(report.losses[0].domain, ExportLossDomain::Metadata);
    assert_eq!(
        report.diagnostics,
        legacy_check(root.path()).unwrap().diagnostics
    );
}

#[cfg(feature = "ffmpeg-library")]
fn entry(target: PropertyTarget) -> AnimationGraphEntry {
    AnimationGraphEntry {
        target,
        animator: PropertyAnimator::constant(PropertyValue::Float(1.0)).unwrap(),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn graph_targets_keep_typed_namespaces_even_when_human_messages_deduplicate() {
    let targets = [
        PropertyTarget::layer(LayerId::new(1), PropType::PositionX),
        PropertyTarget::effect_param(EffectId::new(1), "unmapped"),
        PropertyTarget::fx_item(FxItemId::new(1), "unmapped"),
    ];
    let mut value = crate::test_support::editable_document();
    value["composition"]["dynamics"] = serde_json::to_value(
        AnimationGraph::from_entries(targets.iter().cloned().map(entry).collect()).unwrap(),
    )
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let archive = write_archive(root.path(), &value);
    let report = inspect(&archive);
    for target in targets {
        assert!(
            report
                .losses
                .iter()
                .any(|loss| loss.source == ExportLossSource::Property(target.clone())),
            "{report:?}"
        );
    }
    assert_eq!(report.losses.len(), 3);
    assert_eq!(report.diagnostics.len(), 2);
    assert_eq!(
        report.diagnostics,
        legacy_check(root.path()).unwrap().diagnostics
    );
}

#[test]
fn unsupported_picture_retains_losses_even_when_no_native_content_exists() {
    let mut value = crate::test_support::editable_document();
    let child = value["composition"]["layers"][1].clone();
    value["composition"]["layers"] = json!([{
        "type":"BooleanOperation", "id":10, "name":"Unsupported Boolean",
        "activeRange":{"start":0,"duration":1000},
        "transform":child["transform"], "effects":[], "op":"union", "layers":[child]
    }]);
    let root = tempfile::tempdir().unwrap();
    let archive = write_archive(root.path(), &value);
    let report = inspect(&archive);
    assert!(!report.has_native_content);
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.source == ExportLossSource::LayerSubtree(LayerId::new(10))));
    let error = legacy_check(root.path()).unwrap_err().to_string();
    assert_eq!(error, format!(
        "unsupported conversion: no convertible video or audio layers; no Premiere project published:\n{}",
        report.diagnostics.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n")
    ));
    assert!(!root.path().join("output").exists());
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn audio_layer_description_and_captions_are_not_misclassified_as_sound() {
    let mut value = crate::test_support::editable_document();
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(
            0,
            json!({
                "type":"Audio", "id":3, "name":"Music", "description":"sound note",
                "playback": crate::test_support::linear_playback(json!({"start":0,"duration":200}), json!({"start":0,"duration":200})),
                "sourceRange":{"start":0,"duration":200}, "sourceIntrinsicDuration":200,
                "volume":0.5, "source":{"assetId":"music"},
                "preserveAudioPitch":true, "captionsEnabled":true
            }),
        );
    let root = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let archive = TesseractFileBuilder::from_project_json(&serde_json::to_vec(&value).unwrap())
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
    let report = inspect(&archive);
    assert!(report.has_native_content);
    for (field, domain) in [
        (ExportField::Description, ExportLossDomain::Metadata),
        (ExportField::AudioPitchPreservation, ExportLossDomain::Audio),
        (ExportField::Captions, ExportLossDomain::Picture),
    ] {
        assert!(
            report.losses.iter().any(|loss| loss.source
                == ExportLossSource::Layer(LayerId::new(3))
                && loss.kind == ExportLossKind::Field(field)
                && loss.domain == domain),
            "{report:?}"
        );
    }
    assert_eq!(
        report.diagnostics,
        legacy_check(root.path()).unwrap().diagnostics
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn audio_animation_losses_do_not_become_picture_losses() {
    let mut value = crate::test_support::editable_document();
    let target = PropertyTarget::layer(LayerId::new(1), PropType::AudioVolume);
    value["composition"]["dynamics"] =
        serde_json::to_value(AnimationGraph::from_entries(vec![entry(target.clone())]).unwrap())
            .unwrap();
    let root = tempfile::tempdir().unwrap();
    let archive = write_archive(root.path(), &value);
    let report = inspect(&archive);
    let loss = report
        .losses
        .iter()
        .find(|loss| loss.source == ExportLossSource::Property(target.clone()))
        .unwrap();
    assert_eq!(loss.domain, ExportLossDomain::Audio);
    assert_eq!(
        report.diagnostics,
        legacy_check(root.path()).unwrap().diagnostics
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn nested_child_losses_do_not_steal_the_parents_buffered_motion_context() {
    let mut value = crate::test_support::editable_document();
    let mut first = value["composition"]["layers"][0].clone();
    first["parent"] = json!(10);
    first["playback"] = crate::test_support::linear_playback(
        json!({"start": first["playback"]["inputRange"]["start"], "duration": 500}),
        json!({"start": first["sourceRange"]["start"], "duration": 500}),
    );
    first["sourceRange"]["duration"] = json!(500);
    let mut second = first.clone();
    second["id"] = json!(3);
    second["playback"]["inputRange"]["start"] = json!(500);
    second["playback"]["mapping"]["input"]["start"] = json!(500);
    second["sourceRange"]["start"] = json!(500);
    second["playback"]["mapping"]["output"]["start"] = json!(500);
    second["description"] = json!("child note");
    let background = value["composition"]["layers"][1].clone();
    let mut transform = first["transform"].clone();
    transform["skew"] = json!(5);
    value["composition"]["layers"] = json!([{
        "type":"Group", "id":10, "name":"Native nest", "description":"parent note",
        "playback": crate::test_support::linear_playback(json!({"start":0,"duration":1000}), json!({"start": 0, "duration": 1000})), "transform":transform,
        "layers":[first, second]
    }, background]);
    let root = tempfile::tempdir().unwrap();
    let archive = write_archive(root.path(), &value);
    let report = inspect(&archive);
    assert!(report.has_native_content, "{report:?}");
    for id in [3, 10] {
        assert!(
            report.losses.iter().any(|loss| loss.source
                == ExportLossSource::Layer(LayerId::new(id))
                && loss.kind == ExportLossKind::Field(ExportField::Description)),
            "{report:?}"
        );
    }
    assert!(
        report.losses.iter().any(|loss| loss.source
            == ExportLossSource::LayerSubtree(LayerId::new(10))
            && loss.kind == ExportLossKind::Unclassified),
        "{report:?}"
    );
    assert_eq!(
        report.diagnostics,
        legacy_check(root.path()).unwrap().diagnostics
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn stage_and_child_descriptions_keep_two_owners_before_human_deduplication() {
    let mut value = crate::test_support::editable_document();
    let mut children = value["composition"]["layers"].clone();
    for child in children.as_array_mut().unwrap() {
        child["parent"] = json!(10);
    }
    children[0]["description"] = json!("child note");
    value["composition"]["layers"] = json!([{
        "type":"Group", "id":10, "name":"Masked stage", "description":"parent note",
        "playback": crate::test_support::linear_playback(json!({"start":0,"duration":1000}), json!({"start": 0, "duration": 1000})), "transform":children[0]["transform"],
        "masks":[{"id":30,"layer":2,"mode":"add"}], "layers":children
    }]);
    let root = tempfile::tempdir().unwrap();
    let archive = write_archive(root.path(), &value);
    let report = inspect(&archive);
    assert!(report.has_native_content, "{report:?}");
    let descriptions = report
        .losses
        .iter()
        .filter(|loss| loss.kind == ExportLossKind::Field(ExportField::Description))
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), 2, "{report:?}");
    assert_eq!(
        descriptions[0].source,
        ExportLossSource::Layer(LayerId::new(1))
    );
    assert_eq!(
        descriptions[1].source,
        ExportLossSource::Layer(LayerId::new(10))
    );
    assert_eq!(descriptions[0].omission, descriptions[1].omission);
    assert_eq!(
        report.diagnostics,
        legacy_check(root.path()).unwrap().diagnostics
    );
}

#[test]
fn a_custom_canvas_without_native_content_keeps_the_ordinary_no_content_semantics() {
    let mut value = crate::test_support::editable_document();
    value["dimensions"] = json!({"width":1280,"height":720});
    value["composition"]["layers"] = json!([]);
    let root = tempfile::tempdir().unwrap();
    let archive = write_archive(root.path(), &value);
    let report = inspect(&archive);
    assert!(!report.has_native_content, "{report:?}");
    let error = legacy_check(root.path()).unwrap_err().to_string();
    assert_eq!(error, format!(
        "unsupported conversion: no convertible video or audio layers; no Premiere project published:\n{}",
        report.diagnostics.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n")
    ));
    assert!(!root.path().join("output").exists());
}

#[test]
fn inspection_uses_the_supplied_document_not_unrelated_archive_layers() {
    let mut value = crate::test_support::editable_document();
    let root = tempfile::tempdir().unwrap();
    let broken_media = root.path().join("broken.mp4");
    fs::write(&broken_media, b"not a media container").unwrap();
    let archive = TesseractFileBuilder::from_project_json(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .add_asset("premiere-video-1", &broken_media, AssetKind::Video)
        .unwrap()
        .write(root.path().join("input.tsrct"))
        .unwrap();
    assert!(Premiere
        .inspect_export_losses(&archive, archive.project(), &Default::default())
        .is_err());
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    let selected = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let report = Premiere
        .inspect_export_losses(&archive, &selected, &Default::default())
        .unwrap();
    assert!(!report.has_native_content);
}
