mod collected;
#[cfg(not(windows))]
mod native_relative_foreign;
mod psd;

use std::{fs, io, path::Path};

use fx_conv::{
    ConversionMode, ImportTarget, ImportToTesseract, MediaMap, MediaMapSource, MediaRemediation,
    MediaReplacement, MediaStatus, ValidatedMediaMap,
};
use sha2::{Digest, Sha256};
use tesseract_file::TesseractFile;

use super::{
    AepConversionError, AfterEffects, OUTPUT_NAME, fresh_destination, native_media_inventory,
    publish, read_bytes, read_expression_samples,
};
use crate::{
    AfterEffectsImportOptions, Limitation,
    document::DocumentError,
    expression_samples::ExpressionSamplesError,
    schema::layer_records::LayerRecord,
    structure::{ItemKind, read_project},
};

const SOURCE: &[u8] = include_bytes!("../../tests/fixtures/ae26_one_comp.aep");
const MULTI_SOURCE: &[u8] = include_bytes!("../../tests/fixtures/layers/outPoint_clamp.aep");
const MEDIA_REPLACEMENT_SOURCE: &[u8] =
    include_bytes!("../../tests/fixtures/media-replacement/media_replacement.aep");
const VIDEO_SOURCE: &[u8] =
    include_bytes!("../../tests/fixtures/pr4442_native/sources/media_video.aep");

fn replace_layer_source(layer: &mut crate::structure::Layer, source_id: u32) {
    let mut bytes = layer.record.encode();
    bytes[40..44].copy_from_slice(&source_id.to_be_bytes());
    layer.record = LayerRecord::decode(&bytes).unwrap();
}

fn target(id: &str, name: &str, duration_secs: f64, layer_count: usize) -> ImportTarget {
    ImportTarget {
        id: id.into(),
        name: name.into(),
        width: Some(100),
        height: Some(100),
        fps: Some(24.0),
        duration_secs: Some(duration_secs),
        layer_count: Some(layer_count),
        video_track_count: None,
        audio_track_count: None,
    }
}

#[test]
fn native_media_inventory_ignores_image_metadata_errors_but_keeps_native_errors() {
    let mut psd = read_project(include_bytes!(
        "../../tests/fixtures/psd_import/psd_sources_v2.aep"
    ))
    .unwrap();
    let psd_source = psd.items.iter_mut().find(|item| item.id == 1).unwrap();
    assert!(matches!(
        psd_source
            .native_media
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .kind,
        crate::structure::MediaKind::StillImage
    ));
    psd_source.media = Some(Err(crate::structure::MediaDecodeError::Invalid(
        "Photoshop source selector",
    )));
    assert_eq!(
        psd_source.media.as_ref().unwrap().as_ref().unwrap_err(),
        &crate::structure::MediaDecodeError::Invalid("Photoshop source selector")
    );
    let (references, unassessed) = native_media_inventory(&psd, Some(2)).unwrap();
    assert!(references.is_empty());
    assert!(unassessed.is_empty(), "{unassessed:?}");

    let mut video = read_project(VIDEO_SOURCE).unwrap();
    let video_source = video.items.iter_mut().find(|item| item.id == 16).unwrap();
    video_source.native_media = Some(Err(crate::structure::MediaDecodeError::Invalid(
        "sspc layout",
    )));
    let (references, unassessed) = native_media_inventory(&video, Some(1)).unwrap();
    assert!(references.is_empty());
    assert!(
        unassessed
            .iter()
            .any(|warning| warning.contains("native media descriptor is unreadable")),
        "{unassessed:?}"
    );
}

#[test]
fn native_media_walk_carries_enclosing_replacement_to_a_grandchild() {
    let mut project = read_project(MEDIA_REPLACEMENT_SOURCE).unwrap();
    let mut middle = project.item(2).unwrap().clone();
    middle.id = 1_000;
    let ItemKind::Composition(middle_comp) = &mut middle.kind else {
        panic!("fixture item 2 is a composition")
    };
    let middle_layer = middle_comp
        .layers
        .iter_mut()
        .find(|layer| layer.record.id() == 14)
        .unwrap();
    replace_layer_source(middle_layer, 2);

    let ItemKind::Composition(root) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 15)
        .unwrap()
        .kind
    else {
        panic!("fixture item 15 is a composition")
    };
    let occurrence = root
        .layers
        .iter_mut()
        .find(|layer| layer.record.id() == 27)
        .unwrap();
    replace_layer_source(occurrence, middle.id);
    project.items.push(middle);
    let video_project = read_project(VIDEO_SOURCE).unwrap();
    let mut replacement = video_project.item(16).unwrap().clone();
    replacement.id = 30;
    *project.items.iter_mut().find(|item| item.id == 30).unwrap() = replacement;

    let (references, unassessed) = native_media_inventory(&project, Some(15)).unwrap();
    assert!(unassessed.is_empty(), "{unassessed:?}");
    assert_eq!(
        references
            .iter()
            .map(|reference| reference.source_id)
            .collect::<Vec<_>>(),
        [30]
    );
}

#[test]
fn native_media_walk_is_iterative_and_cycle_scoped() {
    const CHAIN_LEN: u32 = 2_048;

    let mut project = read_project(VIDEO_SOURCE).unwrap();
    let template = project.item(1).unwrap().clone();
    let original_source = 16;
    for offset in 0..CHAIN_LEN {
        let mut item = template.clone();
        item.id = 10_000 + offset;
        let ItemKind::Composition(comp) = &mut item.kind else {
            unreachable!()
        };
        let layer = comp
            .layers
            .iter_mut()
            .find(|layer| layer.record.source_id() == original_source)
            .unwrap();
        let next = if offset + 1 == CHAIN_LEN {
            original_source
        } else {
            item.id + 1
        };
        replace_layer_source(layer, next);
        project.items.push(item);
    }

    let ItemKind::Composition(unrelated) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        unreachable!()
    };
    let unrelated_media = unrelated
        .layers
        .iter_mut()
        .find(|layer| layer.record.source_id() == original_source)
        .unwrap();
    replace_layer_source(unrelated_media, 999_999);

    let (references, unassessed) = native_media_inventory(&project, Some(10_000)).unwrap();
    assert!(unassessed.is_empty(), "{unassessed:?}");
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].source_id, original_source);

    let ItemKind::Composition(last) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 10_000 + CHAIN_LEN - 1)
        .unwrap()
        .kind
    else {
        unreachable!()
    };
    let layer = last
        .layers
        .iter_mut()
        .find(|layer| layer.record.source_id() == original_source)
        .unwrap();
    replace_layer_source(layer, 10_000);
    let (references, unassessed) = native_media_inventory(&project, Some(10_000)).unwrap();
    assert!(references.is_empty());
    assert!(unassessed.is_empty(), "{unassessed:?}");
}

#[test]
fn native_media_scope_blocks_only_media_affecting_essential_warnings() {
    use crate::rifx::Chunk;

    fn remove_controller_uuid(chunks: &mut [Chunk]) -> bool {
        for chunk in chunks {
            if chunk.list_kind() == Some(*b"OvG2") {
                let children = chunk.children_mut().unwrap();
                if let Some(position) = children
                    .iter()
                    .position(|child| child.list_kind() == Some(*b"CPrp"))
                {
                    children.remove(position);
                    return true;
                }
            }
            if let Some(children) = chunk.children_mut()
                && remove_controller_uuid(children)
            {
                return true;
            }
        }
        false
    }

    fn corrupt_alternate_source(chunks: &mut [Chunk]) -> bool {
        for chunk in chunks {
            if chunk.id() == *b"blsi" {
                *chunk = Chunk::data(*b"blsi", [0u8; 3]).unwrap();
                return true;
            }
            if let Some(children) = chunk.children_mut()
                && corrupt_alternate_source(children)
            {
                return true;
            }
        }
        false
    }

    let mut numeric = read_project(include_bytes!(
        "../../tests/fixtures/essential/multiple_controllers.aep"
    ))
    .unwrap();
    let (numeric_parent, numeric_occurrence) = numeric
        .items
        .iter_mut()
        .find_map(|item| match &mut item.kind {
            ItemKind::Composition(comp) => comp
                .layers
                .iter_mut()
                .find(|layer| layer.record.id() == 28)
                .map(|layer| (item.id, layer)),
            _ => None,
        })
        .unwrap();
    assert!(remove_controller_uuid(&mut numeric_occurrence.content));
    let (_, unassessed) = native_media_inventory(&numeric, Some(numeric_parent)).unwrap();
    assert!(unassessed.is_empty(), "{unassessed:?}");

    let mut media = read_project(MEDIA_REPLACEMENT_SOURCE).unwrap();
    let ItemKind::Composition(parent) = &media.item(15).unwrap().kind else {
        unreachable!()
    };
    let mut corrupt_content = parent
        .layers
        .iter()
        .find(|layer| layer.record.id() == 27)
        .unwrap()
        .content
        .clone();
    assert!(corrupt_alternate_source(&mut corrupt_content));
    let ItemKind::Composition(parent) = &mut media
        .items
        .iter_mut()
        .find(|item| item.id == 15)
        .unwrap()
        .kind
    else {
        unreachable!()
    };
    parent
        .layers
        .iter_mut()
        .find(|layer| layer.record.id() == 27)
        .unwrap()
        .content = corrupt_content;
    let (_, unassessed) = native_media_inventory(&media, Some(15)).unwrap();
    assert!(
        unassessed
            .iter()
            .any(|warning| warning.contains("Essential media scope")),
        "{unassessed:?}"
    );
}

#[test]
fn target_listing_reads_all_composition_metadata_without_media_or_conversion() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    fs::write(&input, MULTI_SOURCE).unwrap();

    assert_eq!(
        AfterEffects.list_import_targets(&input).unwrap(),
        vec![
            target("13", "outPoint_clamp_precomp", 30.0, 1),
            target("26", "outPoint_clamp_stretch_200", 30.0, 1),
            target("39", "outPoint_clamp_stretch_400", 30.0, 1),
            target("52", "outPoint_clamp_with_startTime", 30.0, 1),
            target("1", "Precomp_5s", 5.0, 0),
        ]
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn ambiguous_import_does_not_publish_and_explicit_nested_target_is_supported() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    let output = root.path().join("out");
    fs::write(&input, MULTI_SOURCE).unwrap();

    let error = AfterEffects
        .import_to_tesseract(
            &input,
            &output,
            &AfterEffectsImportOptions::default(),
            ConversionMode::Write,
        )
        .expect_err("multiple compositions must require an explicit ID");
    assert!(matches!(
        &error,
        AepConversionError::Document(DocumentError::AmbiguousCompositionSelection { count: 5 })
    ));
    assert!(error.to_string().contains("--composition"));
    assert!(error.to_string().contains("tsrct-conv inspect"));
    assert!(!output.exists());

    // Composition 1 is the shared nested precomp in this fixture, so selecting it
    // proves that target inventory is not restricted to inferred roots.
    let report = AfterEffects
        .import_to_tesseract(
            &input,
            &output,
            &AfterEffectsImportOptions {
                composition: Some(1),
                ..AfterEffectsImportOptions::default()
            },
            ConversionMode::Write,
        )
        .unwrap();
    assert_eq!(report.artifacts.len(), 1);
    let archive = TesseractFile::open(output.join(OUTPUT_NAME)).unwrap();
    assert_eq!(archive.project().composition().name(), "Precomp_5s");
}

#[test]
fn local_media_check_write_package_identical_bytes_and_missing_media_stays_unbound() {
    use crate::{aep, rifx::Chunk};
    fn relink(chunks: &mut [Chunk]) {
        for chunk in chunks {
            if chunk.id() == *b"alas" {
                let mut alias: serde_json::Value =
                    serde_json::from_slice(chunk.data_payload().unwrap()).unwrap();
                alias["fullpath"] = "source.wav".into();
                *chunk = Chunk::data(*b"alas", serde_json::to_vec(&alias).unwrap()).unwrap();
            } else if let Some(children) = chunk.children_mut() {
                relink(children);
            }
        }
    }
    let mut native = aep::Project::parse(include_bytes!(
        "../../tests/fixtures/media/audioEnabled.aep"
    ))
    .unwrap();
    relink(&mut native.chunks);
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    fs::write(&input, native.encode().unwrap()).unwrap();
    // Supplemental valid one-sample PCM WAV checks archive byte publication,
    // not agreement with the original Adobe source's duration or audio output.
    let wav: &[u8] = b"RIFF\x26\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x40\x1f\0\0\x80\x3e\0\0\x02\0\x10\0data\x02\0\0\0\0\0";
    fs::write(root.path().join("source.wav"), wav).unwrap();
    let options = AfterEffectsImportOptions {
        composition: Some(1),
        ..AfterEffectsImportOptions::default()
    };
    let output = root.path().join("packaged");
    let checked = AfterEffects
        .import_to_tesseract(&input, &output, &options, ConversionMode::Check)
        .unwrap();
    assert!(!output.exists());
    let written = AfterEffects
        .import_to_tesseract(&input, &output, &options, ConversionMode::Write)
        .unwrap();
    assert_eq!(checked, written);
    let archive = TesseractFile::open(output.join(OUTPUT_NAME)).unwrap();
    assert_eq!(archive.metadata().assets.len(), 1);
    assert_eq!(
        archive
            .asset("aep-local-item-13")
            .unwrap()
            .read_verified_bytes(1024)
            .unwrap(),
        wav
    );
    fs::remove_file(root.path().join("source.wav")).unwrap();
    let missing_output = root.path().join("missing");
    let report = AfterEffects
        .import_to_tesseract(&input, &missing_output, &options, ConversionMode::Write)
        .unwrap();
    assert!(
        report
            .diagnostics
            .iter()
            .any(|warning| warning.message.contains("local source is missing"))
    );
    let missing = TesseractFile::open(missing_output.join(OUTPUT_NAME)).unwrap();
    assert!(missing.metadata().assets.is_empty());
}

#[test]
fn unsupported_video_fails_before_check_or_write_publication() {
    use crate::{aep, rifx::Chunk};
    fn relink(chunks: &mut [Chunk], filename: &str) {
        for chunk in chunks {
            if chunk.id() == *b"alas" {
                let mut alias: serde_json::Value =
                    serde_json::from_slice(chunk.data_payload().unwrap()).unwrap();
                alias["fullpath"] = filename.into();
                *chunk = Chunk::data(*b"alas", serde_json::to_vec(&alias).unwrap()).unwrap();
            } else if let Some(children) = chunk.children_mut() {
                relink(children, filename);
            }
        }
    }
    fn count(value: &serde_json::Value, key: &str, expected: Option<&str>) -> usize {
        match value {
            serde_json::Value::Object(object) => {
                usize::from(object.get(key).is_some_and(|value| {
                    expected.is_none_or(|expected| value.as_str() == Some(expected))
                })) + object
                    .values()
                    .map(|value| count(value, key, expected))
                    .sum::<usize>()
            }
            serde_json::Value::Array(values) => {
                values.iter().map(|value| count(value, key, expected)).sum()
            }
            _ => 0,
        }
    }
    // Derived native fixture + substitute sample entries exercise preflight and
    // publication, not independent Adobe SWF/QTRLE render-fidelity proof.
    let mut native = aep::Project::parse(include_bytes!(
        "../../tests/fixtures/pr4442_native/sources/media_video.aep"
    ))
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    let options = AfterEffectsImportOptions::default();
    let video = include_bytes!("../../tests/fixtures/audio_e2e/movie.mov");
    let mut qtrle = video.to_vec();
    let stsd = qtrle.windows(4).position(|bytes| bytes == b"stsd").unwrap();
    assert_eq!(&qtrle[stsd + 16..stsd + 20], b"avc1");
    qtrle[stsd + 16..stsd + 20].copy_from_slice(b"rle ");
    let mut selected_target = None;
    for (filename, media, supported) in [
        ("source.mov", video.as_slice(), true),
        (
            "source.swf",
            b"FWS synthetic unsupported footage".as_slice(),
            false,
        ),
        ("unsupported.mov", qtrle.as_slice(), false),
    ] {
        relink(&mut native.chunks, filename);
        fs::write(&input, native.encode().unwrap()).unwrap();
        fs::write(root.path().join(filename), media).unwrap();
        let output = root
            .path()
            .join(if supported { "supported" } else { "omitted" });
        let inspection = AfterEffects
            .inspect_media(&input, &options, None)
            .expect("inspection reports admission failures instead of aborting");
        assert_eq!(inspection.media.len(), 1);
        selected_target = Some(inspection.target.clone());
        let inspected = &inspection.media[0];
        assert_eq!(
            inspected.selected.as_deref(),
            Some(root.path().join(filename).as_path())
        );
        if supported {
            assert_eq!(inspected.status, MediaStatus::Supported);
            assert_eq!(inspected.remediation, MediaRemediation::None);
            assert_eq!(inspected.codec.as_deref(), Some("avc1"));
        } else if filename == "source.swf" {
            assert_eq!(inspected.status, MediaStatus::InvalidMedia);
            assert_eq!(inspected.remediation, MediaRemediation::Unknown);
        } else {
            assert_eq!(inspected.status, MediaStatus::RequiresTranscode);
            assert_eq!(inspected.remediation, MediaRemediation::TranscodeCandidate);
            assert_eq!(inspected.codec.as_deref(), Some("rle "));
        }
        if !supported {
            for mode in [ConversionMode::Check, ConversionMode::Write] {
                let error = AfterEffects
                    .import_to_tesseract(&input, &output, &options, mode)
                    .unwrap_err()
                    .to_string();
                assert!(
                    error.contains(filename) && error.contains("aep-local-item-"),
                    "{error}"
                );
                if filename == "unsupported.mov" {
                    assert!(error.contains("rle "), "{error}");
                }
                assert!(!output.exists(), "failed preflight must not publish");
            }
            continue;
        }
        let checked = AfterEffects
            .import_to_tesseract(&input, &output, &options, ConversionMode::Check)
            .unwrap();
        assert!(!output.exists());
        let written = AfterEffects
            .import_to_tesseract(&input, &output, &options, ConversionMode::Write)
            .unwrap();
        assert_eq!(checked, written);
        let archive = TesseractFile::open(output.join(OUTPUT_NAME)).unwrap();
        let document = archive.project_json().unwrap();
        assert!(count(&document, "type", Some("Video")) > 0);
        assert!(!archive.metadata().assets.is_empty());
    }

    let digest = |path: &Path| format!("{:x}", Sha256::digest(fs::read(path).unwrap()));
    let original = root.path().join("unsupported.mov");
    let replacement = root.path().join("source.mov");
    let map_path = root.path().join("media-map.json");
    fs::write(
        &map_path,
        serde_json::to_vec(&MediaMap {
            version: 1,
            source: MediaMapSource {
                format: "after-effects".into(),
                sha256: digest(&input),
                target: selected_target.unwrap(),
            },
            replacements: vec![MediaReplacement {
                original: original.clone(),
                original_sha256: digest(&original),
                replacement: Path::new("source.mov").to_owned(),
                replacement_sha256: digest(&replacement),
            }],
        })
        .unwrap(),
    )
    .unwrap();
    let media_map = ValidatedMediaMap::load(&map_path).unwrap();
    let inspection = AfterEffects
        .inspect_media(&input, &options, Some(&media_map))
        .unwrap();
    assert!(inspection.is_ready());
    assert_eq!(
        inspection.media[0].original.as_deref(),
        Some(original.as_path())
    );
    assert_eq!(
        inspection.media[0].selected.as_deref(),
        Some(fs::canonicalize(&replacement).unwrap().as_path())
    );
    let mapped_output = root.path().join("mapped");
    AfterEffects
        .import_with_media_map(
            &input,
            &mapped_output,
            &options,
            ConversionMode::Write,
            &media_map,
        )
        .unwrap();
    assert!(mapped_output.join(OUTPUT_NAME).is_file());
}

#[test]
fn expression_sample_file_crosses_the_former_byte_quota_and_stays_source_bound() {
    let root = tempfile::tempdir().unwrap();
    let source = b"exact source bytes";
    let samples_path = root.path().join("samples.json");
    let source_sha256 = format!("{:x}", Sha256::digest(source));
    fs::write(
        &samples_path,
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "source_sha256": source_sha256,
            "sample_interval_ms": 1,
            "properties": [],
            "errors": [],
        }))
        .unwrap(),
    )
    .unwrap();
    let samples = read_expression_samples(&samples_path, source).unwrap();
    assert!(samples.properties().is_empty());
    assert!(matches!(
        read_expression_samples(&samples_path, b"different source"),
        Err(AepConversionError::ExpressionSamples(
            ExpressionSamplesError::SourceHashMismatch { .. }
        ))
    ));

    // Valid trailing whitespace crosses the old 128 MiB gate without adding
    // unrelated authored records. This is bounded I/O, not a giant-file proof.
    {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&samples_path)
            .unwrap();
        let whitespace = [b' '; 64 * 1024];
        for _ in 0..2048 {
            file.write_all(&whitespace).unwrap();
        }
    }
    assert!(fs::metadata(&samples_path).unwrap().len() > 128 * 1024 * 1024);
    assert!(
        read_expression_samples(&samples_path, source)
            .unwrap()
            .properties()
            .is_empty()
    );
}

#[test]
fn check_is_read_only_and_write_produces_an_editable_archive() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    let output = root.path().join("out");
    fs::write(&input, SOURCE).unwrap();
    let report = AfterEffects
        .import_to_tesseract(
            &input,
            &output,
            &AfterEffectsImportOptions::default(),
            ConversionMode::Check,
        )
        .unwrap();
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.limitation == Limitation::CompositionSettings)
    );
    assert!(!output.exists());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);

    let written = AfterEffects
        .import_to_tesseract(
            &input,
            &output,
            &AfterEffectsImportOptions::default(),
            ConversionMode::Write,
        )
        .unwrap();
    assert_eq!(written, report);
    let file = TesseractFile::open(output.join(OUTPUT_NAME)).unwrap();
    let project = file.project();
    // Archive size counts JSON whitespace too. Preserve the exact editable
    // model without pretty-printing deeply nested animation tracks.
    assert_eq!(
        file.project_json_bytes(),
        serde_json::to_vec(project).unwrap()
    );
    assert_eq!(project.composition().name(), "classic-3d");
    assert_eq!(
        (project.dimensions().width, project.dimensions().height),
        (1920, 1080)
    );
    assert_eq!(project.duration().as_secs(), 1.0);
    assert_eq!(project.composition().layers().len(), 1);
    let document = file.project_json().unwrap();
    assert_eq!(document["composition"]["layers"][0]["type"], "Group");
    assert_eq!(document["composition"]["layers"][0]["name"], "classic-3d");
    assert_eq!(
        document["composition"]["layers"][0]["layers"],
        serde_json::json!([])
    );
    assert!(file.metadata().assets.is_empty());
    assert_eq!(fs::read_dir(&output).unwrap().count(), 1);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    assert_eq!(fs::read(input).unwrap(), SOURCE);
}

#[test]
fn malformed_input_never_publishes_or_leaves_staging() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    let output = root.path().join("out");
    for bytes in [&b"not an AEP"[..], &SOURCE[..SOURCE.len() / 2]] {
        fs::write(&input, bytes).unwrap();
        for mode in [ConversionMode::Check, ConversionMode::Write] {
            assert!(matches!(
                AfterEffects.import_to_tesseract(
                    &input,
                    &output,
                    &AfterEffectsImportOptions::default(),
                    mode,
                ),
                Err(AepConversionError::Read(_))
            ));
            assert!(!output.exists());
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
        }
    }
}

#[test]
fn malformed_structural_records_are_rejected_before_publication() {
    // Mutate independently authored bytes, without coupling this adapter test to
    // the evolving low-level Chunk API. Each mutation targets an exact wire tag.
    let mut layers = SOURCE.to_vec();
    let at = layers
        .windows(12)
        .position(|window| &window[..4] == b"LIST" && &window[8..] == b"DLay")
        .unwrap();
    layers[at + 8..at + 12].copy_from_slice(b"Layr");
    let record = at
        + 12
        + layers[at + 12..]
            .windows(4)
            .position(|tag| tag == b"ldta")
            .unwrap();
    // A view envelope can have a valid timeline layout. Actually remove the
    // required descriptor instead of expecting unsupported semantics to fail.
    layers[record..record + 4].copy_from_slice(b"xxxx");

    let mut multiple = SOURCE.to_vec();
    let at = multiple
        .windows(12)
        .position(|window| &window[..4] == b"LIST" && &window[8..] == b"Fold")
        .unwrap();
    let folder_len = u32::from_be_bytes(multiple[at + 4..at + 8].try_into().unwrap());
    let end = at + 8 + folder_len as usize;
    // An Item without its required idta is malformed, not just a second comp.
    multiple.splice(end..end, b"LIST\0\0\0\x04Item".iter().copied());
    multiple[at + 4..at + 8].copy_from_slice(&(folder_len + 12).to_be_bytes());
    let root_len = u32::from_be_bytes(multiple[4..8].try_into().unwrap());
    multiple[4..8].copy_from_slice(&(root_len + 12).to_be_bytes());

    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    let output = root.path().join("out");
    for bytes in [layers, multiple] {
        fs::write(&input, &bytes).unwrap();
        for mode in [ConversionMode::Check, ConversionMode::Write] {
            assert!(matches!(
                AfterEffects.import_to_tesseract(
                    &input,
                    &output,
                    &AfterEffectsImportOptions::default(),
                    mode,
                ),
                Err(AepConversionError::Read(_))
            ));
            assert!(!output.exists());
        }
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn unsupported_composition_semantics_succeed_with_check_write_diagnostic_parity() {
    let mut fps = SOURCE.to_vec();
    let at = fps
        .windows(8)
        .position(|window| window == b"cdta\0\0\0\xcc")
        .unwrap();
    fps[at + 8 + 156..at + 8 + 158].copy_from_slice(&25_u16.to_be_bytes());

    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    let output = root.path().join("out");
    fs::write(&input, fps).unwrap();
    let checked = AfterEffects
        .import_to_tesseract(
            &input,
            &output,
            &AfterEffectsImportOptions::default(),
            ConversionMode::Check,
        )
        .unwrap();
    assert!(!output.exists());
    assert!(checked.diagnostics.iter().any(|diagnostic| {
        diagnostic.limitation == Limitation::CompositionSettings
            && diagnostic.message.contains("fps=25")
    }));
    let written = AfterEffects
        .import_to_tesseract(
            &input,
            &output,
            &AfterEffectsImportOptions::default(),
            ConversionMode::Write,
        )
        .unwrap();
    assert_eq!(written, checked);
    assert!(output.join(OUTPUT_NAME).is_file());
}

#[test]
fn existing_file_or_directory_is_never_replaced() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    fs::write(&input, SOURCE).unwrap();
    let file = root.path().join("file");
    let directory = root.path().join("directory");
    fs::write(&file, b"original file").unwrap();
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("keep"), b"original directory").unwrap();
    for output in [&file, &directory] {
        for mode in [ConversionMode::Check, ConversionMode::Write] {
            assert!(matches!(
                AfterEffects.import_to_tesseract(
                    &input,
                    output,
                    &AfterEffectsImportOptions::default(),
                    mode,
                ),
                Err(AepConversionError::Output(_))
            ));
        }
    }
    assert_eq!(fs::read(file).unwrap(), b"original file");
    assert_eq!(
        fs::read(directory.join("keep")).unwrap(),
        b"original directory"
    );
    assert_eq!(fs::read_dir(directory).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn dangling_destination_symlink_is_not_followed_or_replaced() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    fs::write(&input, SOURCE).unwrap();
    let target = root.path().join("absent");
    let output = root.path().join("out");
    std::os::unix::fs::symlink(&target, &output).unwrap();
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        assert!(matches!(
            AfterEffects.import_to_tesseract(
                &input,
                &output,
                &AfterEffectsImportOptions::default(),
                mode,
            ),
            Err(AepConversionError::Output(_))
        ));
    }
    assert_eq!(fs::read_link(output).unwrap(), target);
    assert!(!target.exists());
}

#[test]
fn missing_parent_and_nonregular_input_do_not_create_output() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    fs::write(&input, SOURCE).unwrap();
    let missing = root.path().join("missing");
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        assert!(
            AfterEffects
                .import_to_tesseract(
                    &input,
                    &missing.join("out"),
                    &AfterEffectsImportOptions::default(),
                    mode,
                )
                .is_err()
        );
        assert!(matches!(
            AfterEffects.import_to_tesseract(
                root.path(),
                &root.path().join("out"),
                &AfterEffectsImportOptions::default(),
                mode,
            ),
            Err(AepConversionError::Input(_))
        ));
    }
    assert!(!missing.exists());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn input_reader_preserves_the_complete_envelope_and_trailing_bytes() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("source.aep");
    let mut bytes = SOURCE.to_vec();
    bytes.extend_from_slice(b"trailing metadata");
    fs::write(&input, &bytes).unwrap();
    assert_eq!(super::read_input(&input).unwrap(), bytes);
}

#[test]
fn input_reader_reports_io_errors_without_truncating() {
    let input = Path::new("source.aep");
    assert_eq!(read_bytes(&b"12345"[..], input).unwrap(), b"12345");
    struct BrokenReader;
    impl io::Read for BrokenReader {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("read failure"))
        }
    }
    assert!(
        matches!(read_bytes(BrokenReader, input), Err(AepConversionError::Io { source, .. }) if source.to_string() == "read failure")
    );
}

#[test]
fn late_destination_preserves_concurrent_files() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("out");
    let destination = fresh_destination(&output).unwrap();
    fs::create_dir(&output).unwrap();
    let target = output.join(OUTPUT_NAME);
    fs::write(&target, b"concurrent writer").unwrap();
    assert!(publish(&root.path().join("unused-staged-file"), &destination).is_err());
    assert_eq!(fs::read(target).unwrap(), b"concurrent writer");
}

#[test]
fn failed_link_removes_only_the_new_empty_output_directory() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("out");
    let unrelated = root.path().join("keep");
    fs::write(&unrelated, b"unrelated").unwrap();
    assert!(publish(&root.path().join("missing-staged-file"), &output).is_err());
    assert!(!output.exists());
    assert_eq!(fs::read(unrelated).unwrap(), b"unrelated");
}
