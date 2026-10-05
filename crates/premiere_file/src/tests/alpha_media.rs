//! Real alpha media through ordinary native import and explicit preparation.

use crate::{Premiere, PremiereImportOptions};
use fx_conv::{
    sha256_file, ConversionMode, ImportToTesseract, MediaMap, MediaMapSource, MediaReplacement,
    ValidatedMediaMap,
};
use std::{fs, io::Cursor, path::Path, sync::atomic::AtomicBool};
use tesseract_file::TesseractFile;

const PRORES: &[u8] = include_bytes!("../../tests/fixtures/alpha-media/prores4444.mov");
const ANIMATION: &[u8] = include_bytes!("../../tests/fixtures/alpha-media/animation.mov");

fn project(root: &Path, bytes: &[u8]) -> std::path::PathBuf {
    let project = root.join("alpha.prproj");
    let xml = include_str!("../../tests/fixtures/one-clip.xml")
        .replace("1270080000000", "254016000000")
        .replace("2540160000000", "254016000000")
        .replace("1920,1080", "16,16")
        .replace("media/source.mp4", "source.mov");
    crate::test_support::write_prproj(&project, &xml);
    fs::write(root.join("source.mov"), bytes).unwrap();
    project
}

fn assert_packaged_picture(output: &Path, expected: &[u8]) {
    let archive = TesseractFile::open(output.join("project.tsrct")).unwrap();
    let document = archive.project_json().unwrap();
    let pictures: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .collect();
    assert_eq!(pictures.len(), 1);
    let picture = pictures[0];
    assert_eq!(
        picture["sourceRange"],
        serde_json::json!({"start": 0, "duration": 1000})
    );
    assert_eq!(picture["volume"], 0.0);
    let asset = picture["source"]["assetId"].as_str().unwrap();
    assert_eq!(asset, "premiere-video-1");
    assert_eq!(archive.metadata().assets.len(), 1);
    assert_eq!(
        archive
            .asset(asset)
            .unwrap()
            .read_verified_bytes(expected.len() as u64)
            .unwrap(),
        expected
    );
}

#[test]
fn alpha_media_prores4444_import_retains_original_packets_and_editable_picture() {
    let directory = tempfile::tempdir().unwrap();
    let input = project(directory.path(), PRORES);
    let output = directory.path().join("converted");
    let report = Premiere
        .import_to_tesseract(
            &input,
            &output,
            &PremiereImportOptions::default(),
            ConversionMode::Write,
        )
        .unwrap();
    assert!(
        report
            .diagnostics
            .iter()
            .any(|note| note.reason.contains("WebCodecs playback is unavailable")),
        "{report:?}"
    );
    assert_packaged_picture(&output, PRORES);
    assert_eq!(
        fs::read(directory.path().join("source.mov")).unwrap(),
        PRORES
    );
}

#[test]
fn alpha_media_qtrle_explicit_preparation_imports_without_changing_source_identity() {
    let directory = tempfile::tempdir().unwrap();
    let input = project(directory.path(), ANIMATION);
    let original = directory.path().join("source.mov");
    let prepared = directory.path().join("prepared.mov");
    let result = media_transcode::run(
        media_transcode::TranscodeRequest {
            input: &original,
            output: &prepared,
            backend: media_transcode::Backend::Library,
            cancelled: &AtomicBool::new(false),
        },
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(result.operation, media_transcode::Operation::Transcode);
    let source = result.source.video.as_ref().unwrap();
    let video = result.media.video.as_ref().unwrap();
    assert!(source.has_eight_bit_alpha() && video.alpha);
    assert_eq!(video.codec, "prores");
    assert_eq!((source.frames, video.frames), (30, 30));
    assert_eq!((source.width, source.height), (video.width, video.height));
    assert_eq!(source.frame_rate, video.frame_rate);
    let map_path = directory.path().join("media-map.json");
    fs::write(
        &map_path,
        serde_json::to_vec(&MediaMap {
            version: 1,
            source: MediaMapSource {
                format: "premiere".into(),
                sha256: sha256_file(&input).unwrap(),
                target: "sequence-1".into(),
            },
            replacements: vec![MediaReplacement {
                original: original.canonicalize().unwrap(),
                original_sha256: result.input_sha256,
                replacement: "prepared.mov".into(),
                replacement_sha256: result.output_sha256,
            }],
        })
        .unwrap(),
    )
    .unwrap();
    let map = ValidatedMediaMap::load(&map_path).unwrap();
    let output = directory.path().join("converted");
    Premiere
        .import_with_media_map(
            &input,
            &output,
            &PremiereImportOptions::default(),
            ConversionMode::Write,
            &map,
        )
        .unwrap();
    assert_packaged_picture(&output, &fs::read(&prepared).unwrap());
    assert_eq!(fs::read(&original).unwrap(), ANIMATION);
    fs::write(&original, b"changed source").unwrap();
    assert!(Premiere
        .import_with_media_map(
            &input,
            &directory.path().join("stale"),
            &PremiereImportOptions::default(),
            ConversionMode::Write,
            &map
        )
        .is_err());
    assert!(!directory.path().join("stale").exists());
}

#[test]
fn alpha_media_composition_rechecks_original_prepared_and_project_before_publication() {
    for changed in ["source.mov", "prepared.mov", "alpha.prproj"] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let input = project(root, ANIMATION);
        let authored = r"E:\collected\source.mov";
        let xml = crate::tests::support::prproj_xml(&input).replace(
            "<RelativePath>source.mov</RelativePath>",
            &format!("<FilePath>{authored}</FilePath>"),
        );
        crate::test_support::write_prproj(&input, &xml);
        fs::write(root.join("prepared.mov"), PRORES).unwrap();
        let source = MediaMapSource {
            format: "premiere".into(),
            sha256: sha256_file(&input).unwrap(),
            target: "sequence-1".into(),
        };
        let original = root.join("source.mov").canonicalize().unwrap();
        let hash = sha256_file(&original).unwrap();
        let relink = crate::ValidatedMediaRelink::new(crate::MediaRelink {
            version: 1,
            source: source.clone(),
            bindings: vec![crate::MediaRelinkBinding {
                media_uid: "media-1".into(),
                authored_path: authored.into(),
                local_path: original.clone(),
                sha256: hash.clone(),
            }],
        })
        .unwrap();
        let map_path = root.join("map.json");
        fs::write(
            &map_path,
            serde_json::to_vec(&MediaMap {
                version: 1,
                source,
                replacements: vec![MediaReplacement {
                    original,
                    original_sha256: hash,
                    replacement: "prepared.mov".into(),
                    replacement_sha256: sha256_file(&root.join("prepared.mov")).unwrap(),
                }],
            })
            .unwrap(),
        )
        .unwrap();
        let map = ValidatedMediaMap::load(&map_path).unwrap();
        let output = root.join("converted");
        let pending = crate::tesseract_import::TesseractImport::convert_with_media_relink_and_map(
            &input,
            &output,
            Some("sequence-1"),
            &relink,
            Some(&map),
            fx_conv::Progress::default(),
        )
        .unwrap();
        fs::write(root.join(changed), b"changed after conversion").unwrap();
        assert!(
            pending.write_with_media_map(&map).is_err(),
            "accepted changed {changed}"
        );
        assert!(!output.exists(), "published changed {changed}");
    }
}

#[test]
fn alpha_media_prores4444_keeps_container_and_clock_guards() {
    let inspect = |bytes: &[u8]| {
        crate::media::inspect_video_media(
            Cursor::new(bytes),
            Cursor::new(bytes),
            bytes.len() as u64,
        )
    };
    let media = inspect(PRORES).unwrap();
    assert_eq!(
        media.codec,
        crate::schema::VideoCodec::ProRes {
            profile: crate::schema::video_codec::ProResProfile::P4444,
            alpha: true,
        }
    );
    assert_eq!(media.codec.codec_type(), "1634743400");
    assert_eq!(
        media.timing.supported().unwrap(),
        (crate::format::FrameRate::Fps30, crate::schema::TICKS)
    );
    assert!(inspect(&PRORES[..PRORES.len() - 1]).is_err());
    let directory = tempfile::tempdir().unwrap();
    let input = project(directory.path(), PRORES);
    let xml = crate::tests::support::prproj_xml(&input).replace("16,16", "32,16");
    crate::test_support::write_prproj(&input, &xml);
    let output = directory.path().join("bad-size");
    assert!(Premiere
        .import_to_tesseract(
            &input,
            &output,
            &PremiereImportOptions::default(),
            ConversionMode::Write
        )
        .unwrap_err()
        .to_string()
        .contains("dimensions"));
    assert!(!output.exists());
    assert!(inspect(ANIMATION)
        .unwrap_err()
        .to_string()
        .contains("explicitly prepare the whole source"));
}
