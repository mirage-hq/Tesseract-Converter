//! Native-derived resolver/publication tests, not Adobe render-fidelity proof.

use super::*;
use crate::{aep, rifx::Chunk};

fn relink(chunks: &mut [Chunk], path: &Path) {
    for chunk in chunks {
        if chunk.id() == *b"alas" {
            let mut alias: serde_json::Value =
                serde_json::from_slice(chunk.data_payload().unwrap()).unwrap();
            alias["fullpath"] = path.to_str().unwrap().into();
            alias["ascendcount_base"] = 1.into();
            alias["ascendcount_target"] = 1.into();
            *chunk = Chunk::data(*b"alas", serde_json::to_vec(&alias).unwrap()).unwrap();
        } else if let Some(children) = chunk.children_mut() {
            relink(children, path);
        }
    }
}

fn has_video(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(object) => {
            object.get("type").is_some_and(|value| value == "Video")
                || object.values().any(has_video)
        }
        serde_json::Value::Array(values) => values.iter().any(has_video),
        _ => false,
    }
}

#[test]
fn collected_video_inspection_check_write_and_media_map_agree() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    let original = root.path().join("(Footage)/animation.mov");
    fs::create_dir(original.parent().unwrap()).unwrap();
    let mut native = aep::Project::parse(VIDEO_SOURCE).unwrap();
    relink(&mut native.chunks, &root.path().join("gone/animation.mov"));
    let source_bytes = native.encode().unwrap();
    let project = read_project(&source_bytes).unwrap();
    let footage: Vec<_> = project
        .items
        .iter()
        .filter(|item| item.media.is_some())
        .collect();
    assert_eq!(footage.len(), 1);
    assert!(
        footage[0].parent_folder.is_none(),
        "the pinned fixture has a root footage item"
    );
    fs::write(&input, &source_bytes).unwrap();
    let video = include_bytes!("../../../tests/fixtures/audio_e2e/movie.mov");
    let mut unsupported = video.to_vec();
    let stsd = unsupported
        .windows(4)
        .position(|bytes| bytes == b"stsd")
        .unwrap();
    assert_eq!(&unsupported[stsd + 16..stsd + 20], b"avc1");
    unsupported[stsd + 16..stsd + 20].copy_from_slice(b"rle ");
    fs::write(&original, &unsupported).unwrap();
    let options = AfterEffectsImportOptions::default();
    let inspection = AfterEffects.inspect_media(&input, &options, None).unwrap();
    assert_eq!(inspection.media.len(), 1);
    let media = &inspection.media[0];
    assert_eq!(media.original.as_deref(), Some(original.as_path()));
    assert_eq!(media.selected.as_deref(), Some(original.as_path()));
    assert_eq!(media.status, MediaStatus::RequiresTranscode);
    assert_eq!(media.remediation, MediaRemediation::TranscodeCandidate);
    assert_eq!(media.codec.as_deref(), Some("rle "));
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        let output = root.path().join("blocked");
        let error = AfterEffects
            .import_to_tesseract(&input, &output, &options, mode)
            .unwrap_err();
        assert!(
            matches!(error, AepConversionError::VideoMedia { .. }),
            "{error}"
        );
        assert!(!output.exists());
    }

    let replacement = root.path().join("prepared.mov");
    fs::write(&replacement, video).unwrap();
    let digest = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
    let map_path = root.path().join("media-map.json");
    fs::write(
        &map_path,
        serde_json::to_vec(&MediaMap {
            version: 1,
            source: MediaMapSource {
                format: "after-effects".into(),
                sha256: digest(&source_bytes),
                target: inspection.target,
            },
            replacements: vec![MediaReplacement {
                original: original.clone(),
                original_sha256: digest(&unsupported),
                replacement: Path::new("prepared.mov").to_owned(),
                replacement_sha256: digest(video),
            }],
        })
        .unwrap(),
    )
    .unwrap();
    let map = ValidatedMediaMap::load(&map_path).unwrap();
    let inspection = AfterEffects
        .inspect_media(&input, &options, Some(&map))
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
    let output = root.path().join("mapped");
    let check = AfterEffects
        .import_with_media_map(&input, &output, &options, ConversionMode::Check, &map)
        .unwrap();
    assert!(!output.exists());
    let written = AfterEffects
        .import_with_media_map(&input, &output, &options, ConversionMode::Write, &map)
        .unwrap();
    assert_eq!(check, written);
    let archive = TesseractFile::open(output.join(OUTPUT_NAME)).unwrap();
    assert!(has_video(&archive.project_json().unwrap()));
    assert_eq!(archive.metadata().assets.len(), 1);
    assert_eq!(
        archive.metadata().assets.values().next().unwrap().sha256,
        digest(video)
    );
    assert_eq!(fs::read(&input).unwrap(), source_bytes);
    assert_eq!(fs::read(&original).unwrap(), unsupported);
}
