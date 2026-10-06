use std::{
    fs,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use fx_conv::{ArtifactKind, ConversionMode, ConversionProgress, ExportFromTesseract};
use fx_schema::{
    KeyframeId, LayerId, PropType, PropertyKeyframeEasing, PropertyValue, TimeOffset,
    animator::{AnimationGraphEntry, PropertyAnimator, PropertyKeyframe, PropertyKeyframeTrack},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tesseract_file::{AssetKind, TesseractFileBuilder};

use super::*;
use crate::structure::{ItemKind, read_project};

mod viewport_approximation;

const RECT: &str = include_str!("../../../../tests/fixtures/hybrid/rect-identity.fx.json");

fn document(value: Value) -> EditableFxCompositionDocument {
    EditableFxCompositionDocument::from_json_value(value).unwrap()
}

fn rect() -> EditableFxCompositionDocument {
    document(serde_json::from_str(RECT).unwrap())
}

#[test]
fn published_package_copies_only_emitted_text_fonts_with_exact_bytes_and_hash() {
    let parent = tempfile::tempdir().unwrap();
    let mut value: Value = serde_json::from_str(RECT).unwrap();
    value["composition"]["layers"] = json!([{
        "type": "Text", "id": 1, "name": "Editable font",
        "activeRange": {"start": 0, "duration": 1000},
        "transform": {"position": [100, 100], "anchorPoint": [0, 0],
                      "scale": [100, 100], "rotation": 0, "opacity": 100},
        "sourceText": {"text": "Font package", "fontFamily": "Arial",
                       "fontStyle": "Bold", "fontSize": 48,
                       "fillColor": [1, 1, 1, 1]}
    }]);
    let font = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/references/premiere/fonts/Arial-BoldMT.ttf");
    let bytes = fs::read(&font).unwrap();
    let input = parent.path().join("font.tsrct");
    let archive = TesseractFileBuilder::try_new(document(value))
        .unwrap()
        .add_asset("embedded-font", &font, AssetKind::Font)
        .unwrap()
        .write(&input)
        .unwrap();
    let destination = parent.path().join("published");
    let report = AfterEffects
        .export_from_tesseract(
            &input,
            &destination,
            &AfterEffectsExportOptions { fps: 30.0 },
            ConversionMode::Write,
        )
        .unwrap();
    let manifest: Value =
        serde_json::from_slice(&fs::read(destination.join("fonts/manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest.as_array().unwrap().len(), 1);
    let face = &manifest[0];
    assert_eq!(face["family"], "Arial");
    assert_eq!(face["style"], "Bold");
    assert_eq!(face["postscript_name"], "Arial-BoldMT");
    assert_eq!(face["sha256"], format!("{:x}", Sha256::digest(&bytes)));
    assert_eq!(
        fs::read(destination.join(face["file"].as_str().unwrap())).unwrap(),
        bytes
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|warning| warning.message.contains("Install these fonts"))
    );
    let fonts = crate::export_document::fonts::ArchiveFonts::prepare(&archive).unwrap();
    let empty = parent.path().join("unused");
    fs::create_dir(&empty).unwrap();
    assert!(
        package::fonts::prepare(&fonts, &BTreeSet::new(), &empty)
            .unwrap()
            .is_empty()
    );
    assert!(!empty.join("fonts").exists());
}

fn archive(parent: &Path) -> TesseractFile {
    TesseractFileBuilder::try_new(rect())
        .unwrap()
        .write(parent.join("input.tsrct"))
        .unwrap()
}

fn prepared_video_value() -> Value {
    let mut value: Value = serde_json::from_str(RECT).unwrap();
    value["duration"] = json!(0.2);
    let video = &mut value["composition"]["layers"][0];
    video["type"] = json!("Video");
    video.as_object_mut().unwrap().remove("rect");
    video.as_object_mut().unwrap().remove("activeRange");
    video["source"] = json!({"assetId":"rgb","fit":"contain"});
    video["sourceRange"] = json!({"start":0,"duration":250});
    video["sourceIntrinsicDuration"] = json!(250);
    video["playback"] = json!({"type":"windowed", "inputRange":{"start":0,"duration":200},
        "mapping":{"type":"timeRemap","property":{"before":"inactive","after":"inactive","keyframes":[
            {"id":"in","time":0,"value":0,"easing":{"type":"linear"}},
            {"id":"middle","time":100,"value":50,"easing":{"type":"linear"}},
            {"id":"out","time":200,"value":200,"easing":{"type":"linear"}}
        ]}},"inputOffsetMs":0});
    value
}

fn premiere_media(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../premiere_file/tests/fixtures")
        .join(name)
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn selected_picture_admits_mp4_without_rewriting_editable_remap_or_originals() {
    let parent = tempfile::tempdir().unwrap();
    let source = premiere_media("video-24fps.mp4");
    let source_hash = fx_conv::sha256_file(&source).unwrap();
    let archive = TesseractFileBuilder::try_new(document(prepared_video_value()))
        .unwrap()
        .add_asset("rgb", &source, AssetKind::Video)
        .unwrap()
        .write(parent.path().join("input.tsrct"))
        .unwrap();
    let original = archive.project().to_json_value().unwrap();
    let stage = AfterEffects
        .stage_picture_layers(
            &archive,
            archive.project(),
            0..1,
            parent.path(),
            &AfterEffectsExportOptions { fps: 30.0 },
        )
        .unwrap();
    assert!(stage.omitted_layer_ids().is_empty(), "{:?}", stage.report());
    let media = stage
        .report()
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == ArtifactKind::Media)
        .unwrap();
    assert_eq!(media.path.extension().and_then(|v| v.to_str()), Some("mp4"));
    let project = read_project(&fs::read(stage.directory().join("project.aep")).unwrap()).unwrap();
    fn remap(chunks: &[crate::rifx::Chunk]) -> Option<crate::properties::NumericProperty> {
        if let Ok(runs) = crate::properties::runs(chunks) {
            for (name, run) in runs {
                if name == "ADBE Time Remapping" {
                    return Some(
                        crate::properties::read_numeric(
                            crate::properties::unique_list(run, *b"tdbs").unwrap(),
                        )
                        .unwrap(),
                    );
                }
            }
        }
        chunks
            .iter()
            .filter_map(crate::rifx::Chunk::children)
            .find_map(remap)
    }
    let keys = project
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(comp) => Some(comp),
            _ => None,
        })
        .flat_map(|comp| &comp.layers)
        .find_map(|layer| remap(&layer.content))
        .unwrap()
        .keyframes;
    assert_eq!(keys.len(), 3);
    assert_eq!(
        keys.iter()
            .map(|key| (key.time_secs, key.values[0]))
            .collect::<Vec<_>>(),
        [(0.0, 0.0), (0.1, 0.05), (0.2, 0.2)]
    );
    assert_eq!(archive.project().to_json_value().unwrap(), original);
    assert_eq!(fx_conv::sha256_file(&source).unwrap(), source_hash);
    assert!(
        !stage
            .report()
            .diagnostics
            .iter()
            .any(|d| d.message.contains("AE destination media"))
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn selected_picture_rejects_complete_rgb_matte_scope_when_one_source_clock_differs() {
    let parent = tempfile::tempdir().unwrap();
    let mut value = prepared_video_value();
    let mut matte = value["composition"]["layers"][0].clone();
    matte["id"] = json!(901);
    matte["source"]["assetId"] = json!("matte");
    value["composition"]["layers"][0]["trackMatte"] = json!({"layer":901,"mode":"alpha"});
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(0, matte);
    let archive = TesseractFileBuilder::try_new(document(value))
        .unwrap()
        .add_asset("rgb", premiere_media("video-24fps.mp4"), AssetKind::Video)
        .unwrap()
        .add_asset(
            "matte",
            premiere_media("video-with-audio.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .write(parent.path().join("input.tsrct"))
        .unwrap();
    let before = fs::read_dir(parent.path()).unwrap().count();
    let error = AfterEffects
        .stage_picture_layers(
            &archive,
            archive.project(),
            0..2,
            parent.path(),
            &AfterEffectsExportOptions { fps: 30.0 },
        )
        .unwrap_err();
    let omissions = error
        .picture_scope_omissions()
        .expect("failed scope is recoverable by coordinator");
    assert!(
        omissions
            .iter()
            .any(|d| d.layer_id == Some(LayerId::new(901))
                && d.message.contains("intrinsic duration differs")),
        "{omissions:?}"
    );
    assert_eq!(
        fs::read_dir(parent.path()).unwrap().count(),
        before,
        "no partial RGB package survives"
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn selected_picture_prunes_rgb_when_its_native_matte_owner_fails() {
    let parent = tempfile::tempdir().unwrap();
    let mut value = prepared_video_value();
    let mut matte = value["composition"]["layers"][0].clone();
    matte["id"] = json!(901);
    matte["name"] = json!("x".repeat(256));
    matte["source"]["assetId"] = json!("matte");
    value["composition"]["layers"][0]["trackMatte"] = json!({"layer":901,"mode":"alpha"});
    let mut sibling: Value = serde_json::from_str(RECT).unwrap();
    sibling = sibling["composition"]["layers"][0].take();
    sibling["id"] = json!(902);
    sibling["name"] = json!("Retained unrelated rectangle");
    sibling["activeRange"] = json!({"start":0,"duration":200});
    let roots = value["composition"]["layers"].as_array_mut().unwrap();
    roots.insert(0, matte);
    roots.push(sibling);
    let source = premiere_media("video-24fps.mp4");
    let archive = TesseractFileBuilder::try_new(document(value))
        .unwrap()
        .add_asset("rgb", &source, AssetKind::Video)
        .unwrap()
        .add_asset("matte", &source, AssetKind::Video)
        .unwrap()
        .write(parent.path().join("input.tsrct"))
        .unwrap();
    let staged = AfterEffects
        .stage_picture_layers(
            &archive,
            archive.project(),
            0..3,
            parent.path(),
            &AfterEffectsExportOptions { fps: 30.0 },
        )
        .unwrap();
    assert_eq!(
        staged.omitted_layer_ids(),
        &[LayerId::new(900), LayerId::new(901)].into_iter().collect()
    );
    assert!(staged.report().diagnostics.iter().any(|d| {
        d.layer_id == Some(LayerId::new(900))
            && d.message
                .contains("matte provider FX layer 901 was omitted")
    }));
    let native = read_project(&fs::read(staged.directory().join("project.aep")).unwrap()).unwrap();
    let ItemKind::Composition(root) = &native.item(1).unwrap().kind else {
        panic!("native root")
    };
    assert_eq!(root.layers.len(), 1);
    assert_eq!(root.layers[0].name.as_ref(), "Retained unrelated rectangle");
    assert_eq!(
        staged.report().artifacts.len(),
        1,
        "neither half of the failed pair is packaged"
    );
}

fn primary_grade_value() -> Value {
    serde_json::to_value(fx_schema::LayerEffect::PrimaryGrade(
        fx_schema::PrimaryGrade {
            exposure: 1.0,
            contrast: 0.5,
            ..Default::default()
        },
    ))
    .unwrap()
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn selected_picture_prepared_movie_pair_retains_bound_alpha_matte_and_unmapped_effect_owner() {
    let parent = tempfile::tempdir().unwrap();
    let mut value = prepared_video_value();
    value["composition"]["layers"][0]["sourceIntrinsicDuration"] = json!(1000);
    let mut matte = value["composition"]["layers"][0].clone();
    matte["id"] = json!(901);
    matte["name"] = json!("Retained alpha provider");
    matte["source"]["assetId"] = json!("matte");
    value["composition"]["layers"][0]["trackMatte"] = json!({"layer":901,"mode":"alpha"});
    value["composition"]["layers"][0]["effects"] = json!([
        {"id":100,"enabled":true,"effect":primary_grade_value()},
        {"id":101,"enabled":true,"effect":{
            "type":"exposure","exposure":0.5,"offset":0,"gammaCorrection":1
        }}
    ]);
    let mut sibling: Value = serde_json::from_str(RECT).unwrap();
    sibling = sibling["composition"]["layers"][0].take();
    sibling["id"] = json!(902);
    sibling["name"] = json!("Retained unrelated rectangle");
    sibling["activeRange"] = json!({"start":0,"duration":200});
    let roots = value["composition"]["layers"].as_array_mut().unwrap();
    roots.insert(0, matte);
    roots.push(sibling);
    // This public 30-frame QTRLE fixture requires ordinary whole-source
    // preparation; no effect output or rendered layer is used as its source.
    let source = premiere_media("alpha-media/animation.mov");
    let archive = TesseractFileBuilder::try_new(document(value))
        .unwrap()
        .add_asset("rgb", &source, AssetKind::Video)
        .unwrap()
        .add_asset("matte", &source, AssetKind::Video)
        .unwrap()
        .write(parent.path().join("input.tsrct"))
        .unwrap();
    let original = archive.project().to_json_value().unwrap();
    let source_hash = fx_conv::sha256_file(&source).unwrap();
    let archive_bytes = fs::read(parent.path().join("input.tsrct")).unwrap();
    let stage = AfterEffects
        .stage_picture_layers(
            &archive,
            archive.project(),
            0..3,
            parent.path(),
            &AfterEffectsExportOptions { fps: 30.0 },
        )
        .unwrap();
    assert!(stage.omitted_layer_ids().is_empty(), "{:?}", stage.report());
    assert!(stage.report().diagnostics.iter().any(|d| {
        d.layer_id == Some(LayerId::new(900))
            && d.message.contains("Effect primaryGrade:")
            && d.message.contains("omitted, owner retained")
    }));
    assert_eq!(
        stage
            .report()
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("AE destination media Transcode"))
            .count(),
        2,
        "both bound sources are prepared despite the omitted grade"
    );
    let native = read_project(&fs::read(stage.directory().join("project.aep")).unwrap()).unwrap();
    let ItemKind::Composition(root) = &native.item(1).unwrap().kind else {
        panic!("native root")
    };
    assert_eq!(root.layers.len(), 3);
    assert!(
        root.layers
            .iter()
            .any(|layer| layer.name.as_ref() == "Retained unrelated rectangle")
    );
    let matte = root
        .layers
        .iter()
        .find(|layer| layer.name.as_ref() == "Retained alpha provider")
        .unwrap();
    let rgb = root
        .layers
        .iter()
        .find(|layer| layer.record.matte_layer_id().is_some())
        .unwrap();
    assert_eq!(rgb.record.matte_layer_id(), Some(matte.record.id()));
    assert_eq!(rgb.record.track_matte_type(), 1);
    assert_eq!(rgb.record.in_point(), Some(0.0));
    assert_eq!(rgb.record.out_point(), Some(0.2));
    let (_, remap) = crate::properties::root_runs(&rgb.content)
        .unwrap()
        .into_iter()
        .find(|(name, _)| *name == "ADBE Time Remapping")
        .unwrap();
    let remap =
        crate::properties::read_numeric(crate::properties::unique_list(remap, *b"tdbs").unwrap())
            .unwrap();
    assert_eq!(
        remap
            .keyframes
            .iter()
            .map(|key| (key.time_secs, key.values[0]))
            .collect::<Vec<_>>(),
        [(0.0, 0.0), (0.1, 0.05), (0.2, 0.2)]
    );
    let (effects, warnings) = crate::effects::native::read_effects(&rgb.content, [16.0, 16.0]);
    // The existing source-free Exposure export control has the same canonical
    // custom/UI default diagnostics. They must not be confused with failed
    // media preparation or permission to lose supported Exposure controls.
    let baseline_input = document(
        serde_json::from_str(include_str!(
            "../../../../tests/fixtures/effects/fx_export_panel/exposure-master.fx.json"
        ))
        .unwrap(),
    );
    let baseline_output = crate::export_document::to_aep(&baseline_input).unwrap();
    assert!(baseline_output.omitted_layer_ids.is_empty());
    let baseline = read_project(&baseline_output.bytes).unwrap();
    let ItemKind::Composition(baseline) = &baseline.item(1).unwrap().kind else {
        panic!("source-free Exposure control composition")
    };
    assert_eq!(baseline.layers.len(), 1);
    let (baseline_effects, baseline_warnings) =
        crate::effects::native::read_effects(&baseline.layers[0].content, [320.0, 180.0]);
    assert_eq!(baseline_effects.len(), 1);
    assert_eq!(baseline_effects[0].match_name, "ADBE Exposure2");
    assert!(baseline_effects[0].enabled);
    for (parameter, expected) in [("0003", 1.25), ("0004", 0.125), ("0005", 0.8)] {
        let property = baseline_effects[0]
            .parameters
            .iter()
            .find(|property| property.match_name == format!("ADBE Exposure2-{parameter}"))
            .unwrap();
        assert_eq!(property.numeric.as_ref().unwrap().values, [expected]);
    }
    let expected_warnings = ["0002", "0006", "0007", "0011", "0012", "0016", "0017", "0021"]
        .map(|suffix| format!("ADBE Exposure2/ADBE Exposure2-{suffix}: unsupported or malformed property: unsupported effect default kind"));
    assert_eq!(baseline_warnings, expected_warnings);
    assert_eq!(warnings, baseline_warnings, "no new readback diagnostic");
    assert_eq!(effects.len(), 1, "only the supported effect is emitted");
    assert_eq!(effects[0].match_name, "ADBE Exposure2");
    assert!(effects[0].enabled);
    for (parameter, expected) in [("0003", 0.5), ("0004", 0.0), ("0005", 1.0)] {
        let property = effects[0]
            .parameters
            .iter()
            .find(|property| property.match_name == format!("ADBE Exposure2-{parameter}"))
            .unwrap();
        assert_eq!(property.numeric.as_ref().unwrap().values, [expected]);
    }
    assert_eq!(
        stage
            .report()
            .artifacts
            .iter()
            .filter(|artifact| artifact.kind == ArtifactKind::Media)
            .count(),
        2
    );
    assert_eq!(archive.project().to_json_value().unwrap(), original);
    assert_eq!(fx_conv::sha256_file(&source).unwrap(), source_hash);
    assert_eq!(
        fs::read(parent.path().join("input.tsrct")).unwrap(),
        archive_bytes
    );
}

/// Selected video 900 with an unmapped shader on it, or on unselected root 902.
fn unmapped_shader_value(enabled: bool, in_selected_scope: bool) -> Value {
    let mut value = prepared_video_value();
    let shader = serde_json::to_value(fx_schema::LayerEffect::CustomShader {
        name: "Source-pixel alpha extraction".into(),
        description: String::new(),
        wgsl: "unmapped author-supplied shader".into(),
        params: vec![],
        texture_inputs: vec![],
    })
    .unwrap();
    let effects = json!([{"id":100,"enabled":enabled,"effect":shader}]);
    if in_selected_scope {
        value["composition"]["layers"][0]["effects"] = effects;
    } else {
        let mut outside: Value = serde_json::from_str(RECT).unwrap();
        outside = outside["composition"]["layers"][0].take();
        outside["id"] = json!(902);
        outside["effects"] = effects;
        value["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .push(outside);
    }
    value
}

/// Original admission sends these invalid WebM bytes to preparation, where
/// probing fails fatally with or without the FFmpeg library feature. This
/// distinguishes source validation from a recoverable optional-effect veto.
const UNPREPARABLE_VIDEO: &[u8] = b"WebM bytes outside native AE admission";

#[test]
fn selected_picture_unmapped_effect_does_not_withhold_source_preparation() {
    for (effect, enabled, in_selected_scope) in [
        (primary_grade_value(), true, true),
        (primary_grade_value(), false, true),
        (primary_grade_value(), true, false),
        (
            json!({"type":"chromaticAberration", "amount":0.3, "direction":90}),
            true,
            true,
        ),
    ] {
        let parent = tempfile::tempdir().unwrap();
        let source = parent.path().join("source.webm");
        fs::write(&source, UNPREPARABLE_VIDEO).unwrap();
        let input = parent.path().join("input.tsrct");
        let mut value = unmapped_shader_value(enabled, in_selected_scope);
        for layer in value["composition"]["layers"].as_array_mut().unwrap() {
            if let Some(records) = layer.get_mut("effects").and_then(Value::as_array_mut) {
                for record in records {
                    record["effect"] = effect.clone();
                }
            }
        }
        let archive = TesseractFileBuilder::try_new(document(value))
            .unwrap()
            .add_asset("rgb", &source, AssetKind::Video)
            .unwrap()
            .write(&input)
            .unwrap();
        let archive_bytes = fs::read(&input).unwrap();
        let events = Mutex::new(Vec::new());
        let callback = |event: ConversionProgress| {
            if event.phase == "prepare AEP media" {
                events.lock().unwrap_or_else(|e| e.into_inner()).push(event);
            }
        };
        let error = AfterEffects
            .stage_picture_layers_with_progress(
                &archive,
                archive.project(),
                0..1,
                parent.path(),
                &AfterEffectsExportOptions { fps: 30.0 },
                Progress::new(&callback),
            )
            .unwrap_err();
        assert_eq!(
            *events.lock().unwrap_or_else(|e| e.into_inner()),
            vec![preparation_event(0, 1, true)],
            "a failed source probe is not a completed asset"
        );
        assert!(
            matches!(
                error,
                AepConversionError::MediaPreparation(
                    media_transcode::TranscodeError::Backend { .. }
                )
            ),
            "ordinary source preparation must run regardless of its omitted effect: {error}"
        );
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 2);
        assert_eq!(fs::read(&input).unwrap(), archive_bytes);
    }
}

#[test]
fn dropped_shader_does_not_block_ordinary_selected_video_preparation() {
    let parent = tempfile::tempdir().unwrap();
    let source = parent.path().join("source.webm");
    fs::write(&source, UNPREPARABLE_VIDEO).unwrap();
    let archive = TesseractFileBuilder::try_new(document(unmapped_shader_value(true, true)))
        .unwrap()
        .add_asset("rgb", &source, AssetKind::Video)
        .unwrap()
        .write(parent.path().join("input.tsrct"))
        .unwrap();
    let error = AfterEffects
        .stage_picture_layers(
            &archive,
            archive.project(),
            0..1,
            parent.path(),
            &AfterEffectsExportOptions { fps: 30.0 },
        )
        .unwrap_err();
    assert!(
        matches!(
            error,
            AepConversionError::MediaPreparation(media_transcode::TranscodeError::Backend { .. })
        ),
        "the same ordinary video preparation must run without its shader: {error}"
    );
}

#[test]
fn selected_picture_admitted_shader_movie_retains_owner_and_picture() {
    let parent = tempfile::tempdir().unwrap();
    let movie = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio_e2e/movie.mov");
    let mut value = unmapped_shader_value(true, true);
    value["composition"]["layers"][0]["sourceIntrinsicDuration"] = json!(8000);
    let archive = TesseractFileBuilder::try_new(document(value))
        .unwrap()
        .add_asset("rgb", &movie, AssetKind::Video)
        .unwrap()
        .write(parent.path().join("input.tsrct"))
        .unwrap();
    let staged = AfterEffects
        .stage_picture_layers(
            &archive,
            archive.project(),
            0..1,
            parent.path(),
            &AfterEffectsExportOptions { fps: 30.0 },
        )
        .expect("movie content survives without its shader");
    assert!(staged.omitted_layer_ids().is_empty());
    assert!(staged.report().diagnostics.iter().any(|d| {
        d.layer_id == Some(LayerId::new(900))
            && d.message
                .contains("CustomShader \"Source-pixel alpha extraction\" dropped")
    }));
}

#[test]
fn selected_picture_unmapped_effect_still_verifies_later_assets() {
    // The admitted first source has an omitted effect; later source integrity
    // and malformed-container failures must remain fatal.
    let payload = b"later selected QuickTime payload";
    for corrupt_archive in [true, false] {
        let parent = tempfile::tempdir().unwrap();
        let mut value = unmapped_shader_value(true, true);
        value["composition"]["layers"][0]["sourceIntrinsicDuration"] = json!(8000);
        value["composition"]["layers"][0]["effects"][0]["effect"] = primary_grade_value();
        let mut later = value["composition"]["layers"][0].clone();
        later["id"] = json!(901);
        later.as_object_mut().unwrap().remove("effects");
        later["source"]["assetId"] = json!("z-later");
        value["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .push(later);
        let video =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio_e2e/movie.mov");
        let movie = parent.path().join("later.mov");
        fs::write(&movie, payload).unwrap();
        let input = parent.path().join("input.tsrct");
        let archive = TesseractFileBuilder::try_new(document(value))
            .unwrap()
            .add_asset("rgb", &video, AssetKind::Video)
            .unwrap()
            .add_asset("z-later", &movie, AssetKind::Video)
            .unwrap()
            .write(&input)
            .unwrap();
        if corrupt_archive {
            let mut bytes = fs::read(&input).unwrap();
            let offsets: Vec<_> = bytes
                .windows(payload.len())
                .enumerate()
                .filter_map(|(offset, window)| (window == payload).then_some(offset))
                .collect();
            assert_eq!(offsets.len(), 1, "stored payload must be unique");
            bytes[offsets[0]] ^= 1;
            fs::write(&input, bytes).unwrap();
        }
        let error = AfterEffects
            .stage_picture_layers(
                &archive,
                archive.project(),
                0..2,
                parent.path(),
                &AfterEffectsExportOptions { fps: 30.0 },
            )
            .unwrap_err();
        assert!(error.picture_scope_omissions().is_none(), "{error}");
        if corrupt_archive {
            assert!(error.to_string().contains("integrity fields"), "{error}");
        } else {
            assert!(matches!(error, AepConversionError::Input(_)), "{error}");
        }
    }
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
        NativeWriteTarget {
            write: observe_script_prepared_writer,
            roots: None,
            alias_directory: None,
        },
        AepPreparationControl::default(),
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
        NativeWriteTarget {
            write: observe_unchanged_writer,
            roots: None,
            alias_directory: None,
        },
        AepPreparationControl::default(),
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

const WAVE: &[u8] = b"RIFF\x26\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x40\x1f\0\0\x80\x3e\0\0\x02\0\x10\0data\x02\0\0\0\0\0";

fn audio_layer(id: u64, asset: &str) -> Value {
    json!({
        "type":"Audio","id":id,"name":"Packaged audio","parent":null,
        "playback":{"type":"windowed","inputRange":{"start":0,"duration":1},"mapping":{"type":"linear","input":{"start":0,"duration":1},"output":{"start":0,"duration":1}},"inputOffsetMs":0},
        "sourceRange":{"start":0,"duration":1},"sourceIntrinsicDuration":1,
        "volume":1,"source":{"assetId":asset}
    })
}

fn preparation_event(completed: usize, total: usize, started: bool) -> ConversionProgress {
    ConversionProgress {
        phase: "prepare AEP media",
        completed: Some(completed),
        total: Some(total),
        unit: Some("assets"),
        started,
    }
}

#[test]
fn preparation_progress_counts_unique_assets_and_resets_without_changing_outputs() {
    let parent = tempfile::tempdir().unwrap();
    let media = parent.path().join("voice.wav");
    fs::write(&media, WAVE).unwrap();
    let mut value: Value = serde_json::from_str(RECT).unwrap();
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .extend([
            audio_layer(901, "a-voice"),
            audio_layer(902, "a-voice"),
            audio_layer(903, "b-voice"),
        ]);
    let input = parent.path().join("input.tsrct");
    let archive = TesseractFileBuilder::try_new(document(value))
        .unwrap()
        .add_asset("a-voice", &media, AssetKind::Audio)
        .unwrap()
        .add_asset("b-voice", &media, AssetKind::Audio)
        .unwrap()
        .write(&input)
        .unwrap();
    let original = fs::read(&input).unwrap();
    let events = Mutex::new(Vec::new());
    let callback = |event: ConversionProgress| {
        if event.phase == "prepare AEP media" {
            events.lock().unwrap_or_else(|e| e.into_inner()).push(event);
        }
    };
    let ordinary = AfterEffects
        .stage_document(
            &archive,
            archive.project(),
            parent.path(),
            &Default::default(),
        )
        .unwrap();
    for _ in 0..2 {
        let observed = AfterEffects
            .stage_document_with_progress(
                &archive,
                archive.project(),
                parent.path(),
                &Default::default(),
                Progress::new(&callback),
            )
            .unwrap();
        assert_eq!(observed.report(), ordinary.report());
        assert_eq!(
            observed.generated_project_sha256(),
            ordinary.generated_project_sha256()
        );
        assert_eq!(observed.report().artifacts.len(), 3);
        for artifact in &observed.report().artifacts {
            let bytes = fs::read(observed.directory().join(&artifact.path)).unwrap();
            assert_eq!(
                bytes,
                fs::read(ordinary.directory().join(&artifact.path)).unwrap()
            );
            if artifact.kind == ArtifactKind::Media {
                assert_eq!(bytes, WAVE);
            }
        }
    }
    let cancelled = AtomicBool::new(false);
    for token in [None, Some(&cancelled)] {
        let controlled = AfterEffects
            .stage_document_with_control(
                &archive,
                archive.project(),
                parent.path(),
                &Default::default(),
                AepPreparationControl {
                    progress: Progress::default(),
                    cancelled: token,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(controlled.report(), ordinary.report());
        for artifact in &controlled.report().artifacts {
            assert_eq!(
                fs::read(controlled.directory().join(&artifact.path)).unwrap(),
                fs::read(ordinary.directory().join(&artifact.path)).unwrap()
            );
        }
    }
    assert_eq!(fs::read(&input).unwrap(), original);
    assert_eq!(
        *events.lock().unwrap_or_else(|e| e.into_inner()),
        [
            preparation_event(0, 2, true),
            preparation_event(1, 2, false),
            preparation_event(2, 2, false),
            preparation_event(0, 2, true),
            preparation_event(1, 2, false),
            preparation_event(2, 2, false),
        ]
    );
}

#[test]
fn preparation_progress_reports_empty_scope() {
    let parent = tempfile::tempdir().unwrap();
    let archive = archive(parent.path());
    let events = Mutex::new(Vec::new());
    let callback = |event: ConversionProgress| {
        if event.phase == "prepare AEP media" {
            events.lock().unwrap_or_else(|e| e.into_inner()).push(event);
        }
    };
    let staged = AfterEffects
        .stage_document_with_progress(
            &archive,
            archive.project(),
            parent.path(),
            &Default::default(),
            Progress::new(&callback),
        )
        .unwrap();
    assert_eq!(
        staged.report().artifacts,
        [Artifact::project("project.aep")]
    );
    assert_eq!(
        *events.lock().unwrap_or_else(|e| e.into_inner()),
        [preparation_event(0, 0, true)]
    );
}

#[test]
fn preparation_progress_counts_omission_but_not_a_failed_asset() {
    let parent = tempfile::tempdir().unwrap();
    let media = parent.path().join("voice.wav");
    fs::write(&media, WAVE).unwrap();
    let mut value: Value = serde_json::from_str(RECT).unwrap();
    // MP4 and PNG/JPEG preparation are supported; GIF remains unsupported.
    let unsupported_image = parent.path().join("unsupported.gif");
    fs::write(
        &unsupported_image,
        b"GIF89a\x01\0\x01\0\x80\0\0\0\0\0\xff\xff\xff\x21\xf9\x04\x01\0\0\0\0\x2c\0\0\0\0\x01\0\x01\0\0\x02\x01\x44\0\x3b",
    )
    .unwrap();
    let mut image = audio_layer(902, "z-image");
    image["type"] = json!("Image");
    image["activeRange"] = value["composition"]["layers"][0]["activeRange"].clone();
    image["transform"] = value["composition"]["layers"][0]["transform"].clone();
    image["source"]["fit"] = json!("contain");
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .extend([audio_layer(901, "a-voice"), image]);
    let archive = TesseractFileBuilder::try_new(document(value.clone()))
        .unwrap()
        .add_asset("a-voice", &media, AssetKind::Audio)
        .unwrap()
        .add_asset("z-image", &unsupported_image, AssetKind::Image)
        .unwrap()
        .write(parent.path().join("input.tsrct"))
        .unwrap();
    let events = Mutex::new(Vec::new());
    let callback = |event: ConversionProgress| {
        events.lock().unwrap_or_else(|e| e.into_inner()).push(event);
    };
    let staged = AfterEffects
        .stage_document_with_progress(
            &archive,
            archive.project(),
            parent.path(),
            &Default::default(),
            Progress::new(&callback),
        )
        .unwrap();
    assert_eq!(staged.report().artifacts.len(), 2);
    assert!(staged.report().diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains(
            "Asset z-image: image asset does not use the source-backed OpenEXR native profile",
        )
    }));
    let native = read_project(&fs::read(staged.directory().join("project.aep")).unwrap()).unwrap();
    let ItemKind::Composition(comp) = &native
        .item(staged.root_composition().item_id())
        .unwrap()
        .kind
    else {
        panic!("root composition")
    };
    assert!(
        comp.layers
            .iter()
            .any(|layer| layer.name.as_ref() == "Editable identity rectangle")
    );
    assert_eq!(
        events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|event| event.phase == "prepare AEP media")
            .copied()
            .collect::<Vec<_>>(),
        [
            preparation_event(0, 2, true),
            preparation_event(1, 2, false),
            preparation_event(2, 2, false)
        ]
    );
    drop(staged);
    events.lock().unwrap_or_else(|e| e.into_inner()).clear();
    value["composition"]["layers"][2] = audio_layer(902, "z-missing");
    let sentinel = parent.path().join("keep.txt");
    fs::write(&sentinel, b"keep").unwrap();
    let error = AfterEffects
        .stage_document_with_progress(
            &archive,
            &document(value),
            parent.path(),
            &Default::default(),
            Progress::new(&callback),
        )
        .unwrap_err();
    assert!(matches!(error, AepConversionError::Archive(_)));
    assert!(error.to_string().contains("z-missing"));
    let events = events.into_inner().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.phase == "prepare AEP media")
            .copied()
            .collect::<Vec<_>>(),
        [
            preparation_event(0, 2, true),
            preparation_event(1, 2, false)
        ]
    );
    assert!(!events.iter().any(|event| event.phase == "write staged AEP"));
    assert_eq!(fs::read(&sentinel).unwrap(), b"keep");
    assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 4);
}

#[test]
fn staged_media_inventory_has_relative_paths_and_verified_source_bytes() {
    let parent = tempfile::tempdir().unwrap();
    let media = parent.path().join("voice.wav");
    fs::write(&media, WAVE).unwrap();
    let mut value: Value = serde_json::from_str(RECT).unwrap();
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(audio_layer(901, "voice"));
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
        WAVE
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

    // Internal Write preparation binds to the final root, never the private
    // staging directory, and captures the digest after that binding.
    let destination = fs::canonicalize(parent.path()).unwrap().join("published");
    let published = prepare_with_progress(
        &archive,
        archive.project(),
        tempfile::tempdir_in(parent.path()).unwrap(),
        &Default::default(),
        Some(&destination),
        Progress::default(),
    )
    .unwrap();
    let published_bytes = fs::read(published.directory().join("project.aep")).unwrap();
    let digest: [u8; 32] = Sha256::digest(&published_bytes).into();
    assert_eq!(published.generated_project_sha256(), &digest);
    assert_ne!(
        published.generated_project_sha256(),
        staged.generated_project_sha256()
    );
    let published_native = read_project(&published_bytes).unwrap();
    let published_source = published_native
        .items
        .iter()
        .find_map(|item| item.media.as_ref())
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(
        Path::new(&published_source.authored_path),
        destination.join(&asset.path)
    );
    assert_eq!(
        fs::read(published.directory().join(&asset.path)).unwrap(),
        WAVE
    );
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
fn video_exact_mute_gain_edit_preserves_pinned_movie_bytes_and_picture_owner() {
    let parent = tempfile::tempdir().unwrap();
    let media = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio_e2e/movie.mov");
    let original_media = fs::read(&media).unwrap();
    let mut value: Value = serde_json::from_str(RECT).unwrap();
    let transform = value["composition"]["layers"][0]["transform"].clone();
    value["composition"]["layers"] = json!([{
        "type":"Video","id":901,"name":"movie-picture","parent":null,
        "playback":{"type":"windowed","inputRange":{"start":0,"duration":2000},"mapping":{"type":"linear","input":{"start":0,"duration":2000},"output":{"start":0,"duration":2000}},"inputOffsetMs":0},
        "sourceRange":{"start":0,"duration":2000},"sourceIntrinsicDuration":8000,
        "volume":0,"transform":transform,"source":{"assetId":"movie","fit":"contain"}
    }]);
    value["composition"]["dynamics"] = json!({"entries":[]});
    let archive_path = parent.path().join("exact-mute.tsrct");
    let archive = TesseractFileBuilder::try_new(document(value.clone()))
        .unwrap()
        .add_asset("movie", &media, AssetKind::Video)
        .unwrap()
        .write(&archive_path)
        .unwrap();
    let original_archive = fs::read(&archive_path).unwrap();
    for gain in [0.0, 0.5] {
        value["composition"]["layers"][0]["volume"] = json!(gain);
        let edited = document(value.clone());
        let stage = AfterEffects
            .stage_document(&archive, &edited, parent.path(), &Default::default())
            .unwrap();
        let facts = native_layer_facts(&fs::read(stage.directory().join("project.aep")).unwrap());
        let movie = facts.iter().find(|fact| fact.0 == "movie-picture").unwrap();
        assert!(movie.3, "picture stays enabled");
        assert_eq!(movie.4, gain > 0.0);
        let media_artifacts: Vec<_> = stage
            .report()
            .artifacts
            .iter()
            .filter(|artifact| artifact.kind == ArtifactKind::Media)
            .collect();
        assert_eq!(media_artifacts.len(), 1);
        assert_eq!(
            fs::read(stage.directory().join(&media_artifacts[0].path)).unwrap(),
            original_media
        );
    }
    assert_eq!(fs::read(&media).unwrap(), original_media);
    assert_eq!(fs::read(&archive_path).unwrap(), original_archive);
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

#[test]
fn preparation_pre_cancel_creates_no_owned_output() {
    let parent = tempfile::tempdir().unwrap();
    let archive = archive(parent.path());
    let sentinel = parent.path().join("keep.txt");
    fs::write(&sentinel, b"keep").unwrap();
    let cancelled = AtomicBool::new(true);
    let control = AepPreparationControl {
        progress: Progress::default(),
        cancelled: Some(&cancelled),
        ..Default::default()
    };
    let options = AfterEffectsExportOptions::default();
    assert!(matches!(
        AfterEffects.stage_document_with_control(
            &archive,
            archive.project(),
            parent.path(),
            &options,
            control,
        ),
        Err(AepConversionError::Cancelled)
    ));
    assert!(matches!(
        AfterEffects.stage_picture_only_document_with_control(
            &archive,
            archive.project(),
            parent.path(),
            &options,
            control,
        ),
        Err(AepConversionError::Cancelled)
    ));
    assert!(matches!(
        AfterEffects.stage_picture_layers_with_control(
            &archive,
            archive.project(),
            0..1,
            parent.path(),
            &options,
            control,
        ),
        Err(AepConversionError::Cancelled)
    ));
    assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 2);
    assert_eq!(fs::read(sentinel).unwrap(), b"keep");
}

#[test]
fn preparation_callback_cancel_stops_before_second_unique_asset_and_cleans_owned_files() {
    let parent = tempfile::tempdir().unwrap();
    let media = parent.path().join("voice.wav");
    fs::write(&media, WAVE).unwrap();
    let sentinel = parent.path().join("keep.txt");
    fs::write(&sentinel, b"keep").unwrap();
    let mut value: Value = serde_json::from_str(RECT).unwrap();
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .extend([
            audio_layer(901, "a-voice"),
            audio_layer(902, "a-voice"),
            // Deliberately absent: cancellation must win before this asset is opened.
            audio_layer(903, "z-missing"),
        ]);
    let selected = document(value.clone());
    value["composition"]["layers"].as_array_mut().unwrap().pop();
    let archive = TesseractFileBuilder::try_new(document(value))
        .unwrap()
        .add_asset("a-voice", &media, AssetKind::Audio)
        .unwrap()
        .write(parent.path().join("input.tsrct"))
        .unwrap();
    let cancelled = AtomicBool::new(false);
    let events = Mutex::new(Vec::new());
    let callback = |event: ConversionProgress| {
        events.lock().unwrap_or_else(|e| e.into_inner()).push(event);
        if event.phase == "prepare AEP media" && event.completed == Some(1) {
            cancelled.store(true, Ordering::Relaxed);
        }
    };
    let error = AfterEffects
        .stage_document_with_control(
            &archive,
            &selected,
            parent.path(),
            &Default::default(),
            AepPreparationControl {
                progress: Progress::new(&callback),
                cancelled: Some(&cancelled),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(matches!(error, AepConversionError::Cancelled));
    let events = events.into_inner().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.phase == "prepare AEP media")
            .copied()
            .collect::<Vec<_>>(),
        [
            preparation_event(0, 2, true),
            preparation_event(1, 2, false)
        ]
    );
    assert!(!events.iter().any(|event| event.phase == "write staged AEP"));
    assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 3);
    assert_eq!(fs::read(sentinel).unwrap(), b"keep");
}

#[test]
fn selected_scope_writer_cancellation_overrides_omission_fallback_and_cleans_stage() {
    let parent = tempfile::tempdir().unwrap();
    let archive = archive(parent.path());
    let sentinel = parent.path().join("keep.txt");
    fs::write(&sentinel, b"keep").unwrap();
    let mut value: Value = serde_json::from_str(RECT).unwrap();
    let failed = &mut value["composition"]["layers"][0];
    failed["name"] = json!("x".repeat(256));
    failed["type"] = json!("Shape");
    failed.as_object_mut().unwrap().remove("rect");
    failed["shape"] = json!({
        "path":{"commands":[]},
        "ellipse":{"size":[100.0,50.0],"position":[0.0,0.0]},
        "fills":[{"paint":{"type":"solid","color":[1.0,0.0,0.0,1.0]}}]
    });
    let selected = document(value);
    let options = AfterEffectsExportOptions::default();
    let omitted = AfterEffects
        .stage_picture_layers_with_control(
            &archive,
            &selected,
            0..1,
            parent.path(),
            &options,
            AepPreparationControl::default(),
        )
        .unwrap_err();
    assert!(omitted.picture_scope_omissions().is_some());

    let cancelled = AtomicBool::new(false);
    let callback = |event: ConversionProgress| {
        if event.phase == "prepare AE layers" {
            cancelled.store(true, Ordering::Relaxed);
        }
    };
    let error = AfterEffects
        .stage_picture_layers_with_control(
            &archive,
            &selected,
            0..1,
            parent.path(),
            &options,
            AepPreparationControl {
                progress: Progress::new(&callback),
                cancelled: Some(&cancelled),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(cancelled.load(Ordering::Relaxed));
    assert!(matches!(error, AepConversionError::Cancelled), "{error:?}");
    assert!(error.picture_scope_omissions().is_none());
    assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 2);
    assert_eq!(fs::read(sentinel).unwrap(), b"keep");
}

#[test]
fn export_media_preserve_original_control_never_prepares_unsupported_video() {
    let parent = tempfile::tempdir().unwrap();
    let mut value: Value = serde_json::from_str(include_str!(
        "../../../../../premiere_file/tests/fixtures/editable-video.json"
    ))
    .unwrap();
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    value["composition"]["layers"][0]["sourceIntrinsicDuration"] = json!(2000);
    let input = parent.path().join("input.tsrct");
    let archive = TesseractFileBuilder::from_project_json(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../premiere_file/tests/fixtures/feature_video_formats_hevc.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .write(&input)
        .unwrap();
    let original = fs::read(&input).unwrap();
    let error = AfterEffects
        .stage_picture_layers_with_control(
            &archive,
            archive.project(),
            0..1,
            parent.path(),
            &AfterEffectsExportOptions { fps: 30.0 },
            AepPreparationControl {
                preserve_video_assets: true,
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(error.picture_scope_omissions().unwrap().iter().any(|d| {
        d.message
            .contains("original video bytes required, destination preparation disabled")
    }));
    assert_eq!(fs::read(input).unwrap(), original);
    assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 1);
}
