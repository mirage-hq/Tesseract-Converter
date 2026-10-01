use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
};

use fx_conv::{ArtifactKind, ConversionMode, ExportFromTesseract};
use fx_schema::{
    KeyframeId, LayerId, PropType, PropertyKeyframeEasing, PropertyValue, TimeOffset,
    animator::{AnimationGraphEntry, PropertyAnimator, PropertyKeyframe, PropertyKeyframeTrack},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tesseract_file::{AssetKind, TesseractFileBuilder};

use super::*;
use crate::structure::{ItemKind, read_project};

const RECT: &str = include_str!("../../../../tests/fixtures/hybrid/rect-identity.fx.json");

fn document(value: Value) -> EditableFxCompositionDocument {
    EditableFxCompositionDocument::from_json_value(value).unwrap()
}

fn rect() -> EditableFxCompositionDocument {
    document(serde_json::from_str(RECT).unwrap())
}

fn archive(parent: &Path) -> TesseractFile {
    TesseractFileBuilder::try_new(rect())
        .unwrap()
        .write(parent.join("input.tsrct"))
        .unwrap()
}

fn scripted_rect() -> EditableFxCompositionDocument {
    let mut value: Value = serde_json::from_str(RECT).unwrap();
    value["duration"] = json!(0.1);
    value["composition"]["layers"][0]["activeRange"]["duration"] = json!(100);
    value["composition"]["dynamics"]["entries"] = json!([{
        "target": {"kind": "layer", "layerId": 900, "propertyType": "positionX"},
        "animator": {
            "type": "jsScript",
            "layerTimeJsCode": "return 20 + input.time.milliseconds;"
        }
    }]);
    document(value)
}

static SCRIPT_PREPARED_WRITES: AtomicUsize = AtomicUsize::new(0);

fn observe_script_prepared_writer(
    documents: ExportDocumentViews<'_>,
    resolved_media: &std::collections::BTreeMap<
        String,
        crate::export_document::media::ResolvedMediaSource,
    >,
    fps: f64,
    progress: Progress<'_>,
) -> Result<crate::export_document::ExportedDocument, crate::writer::AepWriteError> {
    assert_eq!(SCRIPT_PREPARED_WRITES.fetch_add(1, Ordering::Relaxed), 0);
    assert!(!std::ptr::eq(documents.original(), documents.prepared()));
    assert!(
        documents.original().composition().dynamics().entries()[0]
            .animator
            .is_js_script()
    );
    assert!(
        documents.prepared().composition().dynamics().entries()[0]
            .animator
            .keyframe_track()
            .is_some()
    );
    to_aep_with_document_views_and_media_and_fps_with_progress(
        documents,
        resolved_media,
        fps,
        progress,
    )
}

static UNCHANGED_PREPARED_WRITES: AtomicUsize = AtomicUsize::new(0);

fn observe_unchanged_writer(
    documents: ExportDocumentViews<'_>,
    resolved_media: &std::collections::BTreeMap<
        String,
        crate::export_document::media::ResolvedMediaSource,
    >,
    fps: f64,
    progress: Progress<'_>,
) -> Result<crate::export_document::ExportedDocument, crate::writer::AepWriteError> {
    assert_eq!(UNCHANGED_PREPARED_WRITES.fetch_add(1, Ordering::Relaxed), 0);
    assert!(std::ptr::eq(documents.original(), documents.prepared()));
    to_aep_with_document_views_and_media_and_fps_with_progress(
        documents,
        resolved_media,
        fps,
        progress,
    )
}

#[test]
fn staging_writer_receives_original_and_distinct_script_prepared_views_once() {
    SCRIPT_PREPARED_WRITES.store(0, Ordering::Relaxed);
    let parent = tempfile::tempdir().unwrap();
    let archive = archive(parent.path());
    let original = scripted_rect();
    let before = original.to_json_value().unwrap();
    let directory = tempfile::tempdir_in(parent.path()).unwrap();
    let staged = prepare_with_writer(
        &archive,
        &original,
        directory,
        &AfterEffectsExportOptions { fps: 30.0 },
        observe_script_prepared_writer,
        None,
        Progress::default(),
    )
    .unwrap();

    assert_eq!(SCRIPT_PREPARED_WRITES.load(Ordering::Relaxed), 1);
    assert!(staged.directory().join("project.aep").is_file());
    assert_eq!(original.to_json_value().unwrap(), before);
}

#[test]
fn staging_writer_reuses_original_view_when_preparation_is_unchanged() {
    UNCHANGED_PREPARED_WRITES.store(0, Ordering::Relaxed);
    let parent = tempfile::tempdir().unwrap();
    let archive = archive(parent.path());
    let directory = tempfile::tempdir_in(parent.path()).unwrap();
    let staged = prepare_with_writer(
        &archive,
        archive.project(),
        directory,
        &AfterEffectsExportOptions { fps: 30.0 },
        observe_unchanged_writer,
        None,
        Progress::default(),
    )
    .unwrap();

    assert_eq!(UNCHANGED_PREPARED_WRITES.load(Ordering::Relaxed), 1);
    assert!(staged.directory().join("project.aep").is_file());
}

#[test]
fn staged_generated_project_digests_match_exact_writer_outputs() {
    let parent = tempfile::tempdir().unwrap();
    let archive = archive(parent.path());
    let options = AfterEffectsExportOptions { fps: 30.0 };
    let staged = AfterEffects
        .stage_document(&archive, archive.project(), parent.path(), &options)
        .unwrap();
    let bytes = fs::read(staged.directory().join("project.aep")).unwrap();
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    assert_eq!(staged.generated_project_sha256(), &digest);

    let picture = AfterEffects
        .stage_picture_only_document(&archive, archive.project(), parent.path(), &options)
        .unwrap();
    let bytes = fs::read(picture.directory().join("project.aep")).unwrap();
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    assert_eq!(picture.generated_project_sha256(), &digest);
}

#[test]
fn staged_picture_and_ordinary_exports_report_typed_failed_source_layers() {
    let parent = tempfile::tempdir().unwrap();
    let archive = archive(parent.path());
    let mut value: Value = serde_json::from_str(RECT).unwrap();
    let mut failed = value["composition"]["layers"][0].clone();
    failed["id"] = json!(901);
    failed["name"] = json!("x".repeat(256));
    failed["type"] = json!("Shape");
    failed.as_object_mut().unwrap().remove("rect");
    failed["shape"] = json!({
        "path":{"commands":[]},
        "ellipse":{"size":[100.0,50.0],"position":[0.0,0.0]},
        "fills":[{"paint":{"type":"solid","color":[1.0,0.0,0.0,1.0]}}]
    });
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(failed);
    let selected = document(value);
    let options = AfterEffectsExportOptions { fps: 30.0 };
    let staged = AfterEffects
        .stage_document(&archive, &selected, parent.path(), &options)
        .unwrap();
    let picture = AfterEffects
        .stage_picture_layers(&archive, &selected, 0..2, parent.path(), &options)
        .unwrap();
    let expected = [LayerId::new(901)].into_iter().collect();
    assert_eq!(staged.omitted_layer_ids(), &expected);
    assert_eq!(picture.omitted_layer_ids(), &expected);
    let unaffected = AfterEffects
        .stage_picture_layers(&archive, &selected, 0..1, parent.path(), &options)
        .unwrap();
    assert!(unaffected.omitted_layer_ids().is_empty());
    assert!(picture.report().diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(901)) && diagnostic.message.contains("omitted")
    }));
}

#[test]
fn staged_root_matches_the_nonempty_native_observed_rect_bytes() {
    let parent = tempfile::tempdir().unwrap();
    let archive = archive(parent.path());
    let staged = AfterEffects
        .stage_document(
            &archive,
            archive.project(),
            parent.path(),
            &AfterEffectsExportOptions { fps: 30.0 },
        )
        .unwrap();
    let bytes = fs::read(staged.directory().join("project.aep")).unwrap();
    // This pins the exact generated file opened by AE26.5x89, not a render oracle.
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "0fe75b59a4118b48a965ab1001a9ee14b48e48e614c73a191b07b89e69939290"
    );
    let identity = staged.root_composition();
    assert_eq!(identity.item_id(), 1);
    assert_eq!(
        identity.dynamic_link_guid(),
        "00000001-0000-0000-0000-000000000000"
    );
    assert_eq!(identity.name(), "Hybrid_Rect_Identity_30fps");
    assert_eq!(identity.dimensions(), (1920, 1080));
    assert_eq!(identity.frame_rate(), 30.0);
    assert_eq!(identity.duration_secs(), 2.0);
    let native = read_project(&bytes).unwrap();
    let ItemKind::Composition(comp) = &native.item(identity.item_id()).unwrap().kind else {
        panic!("generated root composition");
    };
    assert_eq!(comp.layers.len(), 1);
    assert_eq!(comp.layers[0].name.as_ref(), "Editable identity rectangle");
    assert_eq!(
        staged.report().artifacts,
        vec![Artifact::project("project.aep")]
    );
    assert!(!staged.report().diagnostics.is_empty());
}

#[test]
fn staged_selected_document_uses_its_own_name_canvas_and_rounded_clock() {
    let parent = tempfile::tempdir().unwrap();
    let archive = archive(parent.path());
    let mut value: Value = serde_json::from_str(RECT).unwrap();
    value["composition"]["name"] = json!("Selected overlay");
    value["dimensions"] = json!({"width":640,"height":360});
    value["duration"] = json!(1.001);
    let selected = document(value);
    let staged = AfterEffects
        .stage_document(
            &archive,
            &selected,
            parent.path(),
            &AfterEffectsExportOptions { fps: 29.97 },
        )
        .unwrap();
    let identity = staged.root_composition();
    let native = read_project(&fs::read(staged.directory().join("project.aep")).unwrap()).unwrap();
    let ItemKind::Composition(comp) = &native.item(identity.item_id()).unwrap().kind else {
        panic!("generated root composition");
    };
    assert_eq!(identity.name(), "Selected overlay");
    assert_eq!(identity.dimensions(), (640, 360));
    assert_eq!(identity.frame_rate(), comp.frame_rate);
    assert!((identity.duration_secs() - comp.duration_secs).abs() < 0.00001);
    // Native 1/24576-second tick quantization may fall below the decimal request.
    assert!((identity.duration_secs() - 1.001).abs() < 1.0 / 24_576.0);
    assert_eq!(
        archive.project().composition().name(),
        "Hybrid_Rect_Identity_30fps"
    );
}

#[test]
fn staged_drop_and_failed_preparation_preserve_unrelated_parent_entries() {
    let parent = tempfile::tempdir().unwrap();
    let archive = archive(parent.path());
    let sentinel = parent.path().join("keep.txt");
    fs::write(&sentinel, b"keep").unwrap();
    let staged = AfterEffects
        .stage_document(
            &archive,
            archive.project(),
            parent.path(),
            &Default::default(),
        )
        .unwrap();
    let directory = staged.directory().to_owned();
    assert!(directory.join("project.aep").is_file());
    drop(staged);
    assert!(!directory.exists());
    assert!(
        AfterEffects
            .stage_document(
                &archive,
                archive.project(),
                parent.path(),
                &AfterEffectsExportOptions { fps: 0.0 },
            )
            .is_err()
    );
    assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 2);
    assert_eq!(fs::read(&sentinel).unwrap(), b"keep");
    assert!(
        AfterEffects
            .stage_document(
                &archive,
                archive.project(),
                &parent.path().join("absent"),
                &Default::default(),
            )
            .is_err()
    );
    assert!(!parent.path().join("absent").exists());
}

#[test]
fn staged_check_and_write_have_identical_inventory_diagnostics_and_bytes() {
    let parent = tempfile::tempdir().unwrap();
    let archive = archive(parent.path());
    let input = parent.path().join("input.tsrct");
    let original = fs::read(&input).unwrap();
    let destination = parent.path().join("published");
    let options = AfterEffectsExportOptions { fps: 30.0 };
    let staged = AfterEffects
        .stage_document(&archive, archive.project(), parent.path(), &options)
        .unwrap();
    let checked = AfterEffects
        .export_from_tesseract(&input, &destination, &options, ConversionMode::Check)
        .unwrap();
    assert!(!destination.exists());
    let written = AfterEffects
        .export_from_tesseract(&input, &destination, &options, ConversionMode::Write)
        .unwrap();
    assert_eq!(staged.report(), &checked);
    assert_eq!(checked, written);
    assert_eq!(
        fs::read(staged.directory().join("project.aep")).unwrap(),
        fs::read(destination.join("project.aep")).unwrap()
    );
    assert_eq!(fs::read(&input).unwrap(), original);
    assert!(
        AfterEffects
            .export_from_tesseract(&input, &destination, &options, ConversionMode::Write)
            .is_err()
    );
    assert_eq!(fs::read(&input).unwrap(), original);
}

#[test]
fn staged_media_inventory_has_relative_paths_and_verified_source_bytes() {
    let parent = tempfile::tempdir().unwrap();
    let wave = b"RIFF\x26\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x40\x1f\0\0\x80\x3e\0\0\x02\0\x10\0data\x02\0\0\0\0\0";
    let media = parent.path().join("voice.wav");
    fs::write(&media, wave).unwrap();
    let mut value: Value = serde_json::from_str(RECT).unwrap();
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "type":"Audio","id":901,"name":"Packaged audio","parent":null,
            "playback":{"type":"windowed","inputRange":{"start":0,"duration":1},"mapping":{"type":"linear","input":{"start":0,"duration":1},"output":{"start":0,"duration":1}},"inputOffsetMs":0},
            "sourceRange":{"start":0,"duration":1},"sourceIntrinsicDuration":1,
            "volume":1,"source":{"assetId":"voice"}
        }));
    let archive = TesseractFileBuilder::try_new(document(value))
        .unwrap()
        .add_asset("voice", &media, AssetKind::Audio)
        .unwrap()
        .write(parent.path().join("input.tsrct"))
        .unwrap();
    let staged = AfterEffects
        .stage_document(
            &archive,
            archive.project(),
            parent.path(),
            &Default::default(),
        )
        .unwrap();
    let artifacts = &staged.report().artifacts;
    assert_eq!(artifacts.len(), 2);
    let asset = artifacts
        .iter()
        .find(|entry| entry.kind == ArtifactKind::Media)
        .unwrap();
    assert!(asset.path.starts_with("media"));
    assert_eq!(
        fs::read(staged.directory().join(&asset.path)).unwrap(),
        wave
    );
    let bytes = fs::read(staged.directory().join("project.aep")).unwrap();
    let native = read_project(&bytes).unwrap();
    let source = native
        .items
        .iter()
        .find_map(|item| item.media.as_ref())
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(
        Path::new(&source.authored_path),
        Path::new(".").join(&asset.path)
    );
    assert!(!Path::new(&source.authored_path).is_absolute());
    assert!(
        !staged
            .directory()
            .join(".asset-materialization-cache")
            .exists()
    );
}

fn native_layer_facts(bytes: &[u8]) -> Vec<(String, u32, u32, bool, bool)> {
    let native = read_project(bytes).unwrap();
    let mut facts = native
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some(comp.layers.iter()),
            _ => None,
        })
        .flatten()
        .map(|layer| {
            let flags = layer.record.flags();
            (
                layer.name.to_string(),
                layer.record.id(),
                layer.record.source_id(),
                flags.enabled,
                flags.audio_enabled,
            )
        })
        .collect::<Vec<_>>();
    facts.sort();
    facts
}

fn source_selector(layer: u64) -> AnimationGraphEntry {
    let keys = [(0, "movie"), (1000, "replacement")]
        .into_iter()
        .enumerate()
        .map(|(index, (time, asset))| {
            PropertyKeyframe::new(
                KeyframeId::new(format!("source-{index}")),
                TimeOffset::from_millis(time),
                PropertyValue::String(asset.to_owned()),
                PropertyKeyframeEasing::Hold,
            )
        })
        .collect();
    AnimationGraphEntry {
        target: fx_schema::PropertyTarget::layer(LayerId::new(layer), PropType::MediaSourceAssetId),
        animator: PropertyAnimator::keyframes(PropertyKeyframeTrack::new(keys).unwrap()),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

#[test]
fn picture_only_staging_disables_root_and_nested_fx_audio_switches() {
    let parent = tempfile::tempdir().unwrap();
    let mut value: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/audio_e2e/fx/group-affine-playback.json"
    ))
    .unwrap();
    let mut root_audio = value["composition"]["layers"][0]["layers"][0].clone();
    root_audio["id"] = json!(702);
    root_audio["name"] = json!("root-audio");
    root_audio["parent"] = Value::Null;
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(root_audio);
    value["composition"]["dynamics"] = json!({"entries":[]});
    let archive = TesseractFileBuilder::try_new(document(value))
        .unwrap()
        .add_asset(
            "sound",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio_e2e/sound.wav"),
            AssetKind::Audio,
        )
        .unwrap()
        .add_asset(
            "replacement",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio_e2e/other.wav"),
            AssetKind::Audio,
        )
        .unwrap()
        .write(parent.path().join("audio.tsrct"))
        .unwrap();
    let ordinary = AfterEffects
        .stage_document(
            &archive,
            archive.project(),
            parent.path(),
            &Default::default(),
        )
        .unwrap();
    let picture = AfterEffects
        .stage_picture_only_document(
            &archive,
            archive.project(),
            parent.path(),
            &Default::default(),
        )
        .unwrap();
    let ordinary_facts =
        native_layer_facts(&fs::read(ordinary.directory().join("project.aep")).unwrap());
    let picture_facts =
        native_layer_facts(&fs::read(picture.directory().join("project.aep")).unwrap());
    for name in ["audio-700", "root-audio"] {
        assert!(
            ordinary_facts.iter().any(|fact| fact.0 == name && fact.4),
            "{ordinary_facts:?}; {:?}",
            ordinary.report().diagnostics
        );
    }
    let native =
        read_project(&fs::read(ordinary.directory().join("project.aep")).unwrap()).unwrap();
    assert!(
        native
            .items
            .iter()
            .filter(|item| matches!(item.kind, ItemKind::Composition(_)))
            .count()
            >= 2,
        "nested audio must actually emit a precomposition"
    );
    assert!(picture_facts.iter().all(|fact| !fact.4));
    assert_eq!(
        ordinary_facts
            .iter()
            .map(|fact| (&fact.0, fact.1, fact.2, fact.3))
            .collect::<Vec<_>>(),
        picture_facts
            .iter()
            .map(|fact| (&fact.0, fact.1, fact.2, fact.3))
            .collect::<Vec<_>>()
    );
    assert_eq!(ordinary.report(), picture.report());
    assert_eq!(ordinary.root_composition(), picture.root_composition());
}

#[test]
fn picture_only_staging_keeps_embedded_audio_owner_media_and_input_unchanged() {
    let parent = tempfile::tempdir().unwrap();
    let mut value: Value = serde_json::from_str(RECT).unwrap();
    let transform = value["composition"]["layers"][0]["transform"].clone();
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .extend([
            json!({
                "type":"Video","id":901,"name":"movie-picture","parent":null,
                "playback":{"type":"windowed","inputRange":{"start":0,"duration":2000},"mapping":{"type":"linear","input":{"start":0,"duration":2000},"output":{"start":0,"duration":2000}},"inputOffsetMs":0},
                "sourceRange":{"start":0,"duration":2000},"sourceIntrinsicDuration":8000,
                "volume":1,"transform":transform,"source":{"assetId":"movie","fit":"contain"}
            }),
            json!({
                "type":"Audio","id":902,"name":"movie-embedded-audio","parent":null,
                "playback":{"type":"windowed","inputRange":{"start":0,"duration":2000},"mapping":{"type":"linear","input":{"start":0,"duration":2000},"output":{"start":0,"duration":2000}},"inputOffsetMs":0},
                "sourceRange":{"start":0,"duration":2000},"sourceIntrinsicDuration":8000,
                "volume":1,"source":{"assetId":"movie"}
            }),
        ]);
    value["composition"]["dynamics"] = json!({"entries":[source_selector(901)]});
    let archive_path = parent.path().join("movie.tsrct");
    let archive = TesseractFileBuilder::try_new(document(value))
        .unwrap()
        .add_asset(
            "movie",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio_e2e/movie.mov"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset(
            "replacement",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio_e2e/movie.mov"),
            AssetKind::Video,
        )
        .unwrap()
        .write(&archive_path)
        .unwrap();
    let original = fs::read(&archive_path).unwrap();
    let ordinary = AfterEffects
        .stage_document(
            &archive,
            archive.project(),
            parent.path(),
            &Default::default(),
        )
        .unwrap();
    let picture = AfterEffects
        .stage_picture_only_document(
            &archive,
            archive.project(),
            parent.path(),
            &Default::default(),
        )
        .unwrap();
    let ordinary_facts =
        native_layer_facts(&fs::read(ordinary.directory().join("project.aep")).unwrap());
    let picture_facts =
        native_layer_facts(&fs::read(picture.directory().join("project.aep")).unwrap());
    assert!(
        ordinary_facts
            .iter()
            .any(|fact| fact.0 == "movie-embedded-audio" && fact.4),
        "{ordinary_facts:?}; {:?}",
        ordinary.report().diagnostics
    );
    assert!(
        ordinary_facts
            .iter()
            .any(|fact| fact.0 == "movie-picture" && fact.3),
        "{ordinary_facts:?}; {:?}",
        ordinary.report().diagnostics
    );
    let native =
        read_project(&fs::read(ordinary.directory().join("project.aep")).unwrap()).unwrap();
    let file_sources: std::collections::BTreeSet<_> = native
        .items
        .iter()
        .filter(|item| item.media.is_some())
        .map(|item| item.id)
        .collect();
    let visible_files: std::collections::BTreeSet<_> = ordinary_facts
        .iter()
        .filter(|fact| fact.3 && file_sources.contains(&fact.2))
        .map(|fact| fact.2)
        .collect();
    assert!(
        visible_files.len() >= 2,
        "both source variants must actually retain picture occurrences: {ordinary_facts:?}; {:?}",
        ordinary.report().diagnostics
    );
    assert!(picture_facts.iter().all(|fact| !fact.4));
    assert_eq!(ordinary.report(), picture.report());
    assert_eq!(fs::read(&archive_path).unwrap(), original);
    for artifact in picture
        .report()
        .artifacts
        .iter()
        .filter(|artifact| artifact.kind == ArtifactKind::Media)
    {
        assert_eq!(
            fs::read(picture.directory().join(&artifact.path)).unwrap(),
            fs::read(ordinary.directory().join(&artifact.path)).unwrap()
        );
    }
}
