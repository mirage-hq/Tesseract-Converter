//! Native-derived alias regression; Windows alias mutation is supplementary,
//! not an independently Windows-authored AEP or Adobe-render fidelity proof.

use super::*;
use crate::{aep, rifx::Chunk};

fn foreign_alias(chunks: &mut [Chunk]) {
    for chunk in chunks {
        if chunk.id() == *b"alas" {
            let mut alias: serde_json::Value =
                serde_json::from_slice(chunk.data_payload().unwrap()).unwrap();
            alias["fullpath"] = r"F:\old\Assets\animation.mov".into();
            alias["ascendcount_base"] = 1.into();
            alias["ascendcount_target"] = 2.into();
            alias["platform"] = 1.into();
            *chunk = Chunk::data(*b"alas", serde_json::to_vec(&alias).unwrap()).unwrap();
        } else if let Some(children) = chunk.children_mut() {
            foreign_alias(children);
        }
    }
}

#[test]
fn foreign_drive_native_relative_video_inspection_check_and_write_agree() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    let movie = root.path().join("Assets/animation.mov");
    fs::create_dir(movie.parent().unwrap()).unwrap();
    let video = include_bytes!("../../../tests/fixtures/audio_e2e/movie.mov");
    fs::write(&movie, video).unwrap();
    let mut native = aep::Project::parse(VIDEO_SOURCE).unwrap();
    foreign_alias(&mut native.chunks);
    let source = native.encode().unwrap();
    fs::write(&input, &source).unwrap();
    let options = AfterEffectsImportOptions::default();
    let inspection = AfterEffects.inspect_media(&input, &options, None).unwrap();
    assert!(inspection.is_ready(), "{inspection:?}");
    assert_eq!(inspection.media.len(), 1);
    let canonical = fs::canonicalize(&movie).unwrap();
    assert_eq!(
        inspection.media[0].original.as_deref(),
        Some(canonical.as_path())
    );
    assert_eq!(
        inspection.media[0].selected.as_deref(),
        Some(canonical.as_path())
    );
    let output = root.path().join("converted");
    let check = AfterEffects
        .import_to_tesseract(&input, &output, &options, ConversionMode::Check)
        .unwrap();
    assert!(!output.exists());
    let write = AfterEffects
        .import_to_tesseract(&input, &output, &options, ConversionMode::Write)
        .unwrap();
    assert_eq!(check, write);
    let archive = TesseractFile::open(output.join(OUTPUT_NAME)).unwrap();
    fn videos(value: &serde_json::Value) -> usize {
        match value {
            serde_json::Value::Object(object) => {
                usize::from(object.get("type").and_then(|v| v.as_str()) == Some("Video"))
                    + object.values().map(videos).sum::<usize>()
            }
            serde_json::Value::Array(values) => values.iter().map(videos).sum(),
            _ => 0,
        }
    }
    assert!(videos(&archive.project_json().unwrap()) > 0);
    assert_eq!(archive.metadata().assets.len(), 1);
    assert_eq!(
        archive.metadata().assets.values().next().unwrap().sha256,
        format!("{:x}", Sha256::digest(video))
    );
    assert_eq!(fs::read(&input).unwrap(), source);
    assert_eq!(fs::read(&movie).unwrap(), video);
}
