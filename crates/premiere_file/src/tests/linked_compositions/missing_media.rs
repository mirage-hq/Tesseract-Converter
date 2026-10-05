//! Public Premiere admission of absent footage in an already-admitted native AEP.
//! Alias-only fixture derivatives are structural evidence, not render proof.

use super::*;
use aftereffects_file::{aep::Project, rifx::Chunk, AfterEffects};
use fx_conv::{ConversionMode, ImportToTesseract, MediaKind, MediaStatus};

fn native_video_link(root: &Path, authored_path: &str) -> PathBuf {
    native_video_link_with_location(root, authored_path, None)
}

fn native_video_link_with_location(
    root: &Path,
    authored_path: &str,
    location: Option<(Value, Value)>,
) -> PathBuf {
    fn relink(chunks: &mut [Chunk], path: &str, location: Option<&(Value, Value)>) {
        for chunk in chunks {
            if chunk.id() == *b"alas" {
                let mut alias: Value =
                    serde_json::from_slice(chunk.data_payload().unwrap()).unwrap();
                alias["fullpath"] = path.into();
                if let Some((ascend, components)) = location {
                    alias["ascendcount_base"] = ascend.clone();
                    alias["ascendcount_target"] = components.clone();
                }
                *chunk = Chunk::data(*b"alas", serde_json::to_vec(&alias).unwrap()).unwrap();
            } else if let Some(children) = chunk.children_mut() {
                relink(children, path, location);
            }
        }
    }
    let aep = root.join("linked.aep");
    let mut native = Project::parse(
        &fs::read(fixture(
            "../aftereffects_file/tests/fixtures/pr4442_native/sources/media_video.aep",
        ))
        .unwrap(),
    )
    .unwrap();
    relink(&mut native.chunks, authored_path, location.as_ref());
    fs::write(&aep, native.encode().unwrap()).unwrap();
    let targets = AfterEffects.list_import_targets(&aep).unwrap();
    assert_eq!(targets.len(), 1);
    let composition = &targets[0];
    let item: u32 = composition.id.parse().unwrap();
    let guid = format!("{item:08x}-0000-0000-0000-000000000000");
    let xml = linked_av_declaring("linked.aep", &guid, &TICKS.to_string()).replacen(
        "0,0,1920,1080",
        &format!(
            "0,0,{},{}",
            composition.width.unwrap(),
            composition.height.unwrap()
        ),
        1,
    );
    let input = root.join("source.prproj");
    crate::test_support::write_prproj(&input, &xml);
    input
}

#[test]
fn missing_linked_video_preserves_native_solid_through_public_check_and_write() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let input = native_video_link(root, "missing.mov");
    let options = crate::PremiereImportOptions::default();
    let inspection = crate::Premiere
        .inspect_media(&input, &options, None)
        .unwrap();
    assert!(!inspection.is_ready(), "missing content is not readiness");
    let video = inspection
        .media
        .iter()
        .find(|media| media.kind == MediaKind::Video)
        .unwrap();
    assert_eq!(video.owner, root.join("linked.aep").canonicalize().unwrap());
    assert_eq!(video.status, MediaStatus::Missing);
    assert!(video.selected.is_none());

    let mut reports = Vec::new();
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        let output = root.join(format!("missing-{mode:?}"));
        let report = crate::Premiere
            .import_to_tesseract(&input, &output, &options, mode)
            .unwrap();
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.reason.contains("missing.mov") && diagnostic.reason.contains("missing")
            }),
            "{report:?}"
        );
        if mode == ConversionMode::Check {
            assert!(!output.exists());
        } else {
            let archive = TesseractFile::open(output.join("project.tsrct")).unwrap();
            assert!(archive.metadata().assets.is_empty());
            let document = archive.project_json().unwrap();
            let groups = linked_groups(&document);
            assert_eq!(groups.len(), 1, "the linked picture must not be omitted");
            // This native source has one solid sibling, not a missing-media slate.
            assert_eq!(rect_fills(groups[0]).len(), 1);
            assert!(all_layers(&document["composition"]).iter().all(|layer| {
                !matches!(layer["type"].as_str(), Some("Video" | "Image" | "Audio"))
            }));
            assert_unique_identities(&document);
        }
        reports.push(report);
    }
    assert_eq!(reports[0], reports[1]);
}

#[test]
fn missing_linked_video_with_unreachable_native_ancestor_still_imports() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let input = native_video_link_with_location(
        root,
        "/missing/native/movie.mov",
        Some((u32::MAX.into(), 1.into())),
    );
    let options = crate::PremiereImportOptions::default();
    let inspection = crate::Premiere
        .inspect_media(&input, &options, None)
        .unwrap();
    assert!(inspection
        .media
        .iter()
        .any(|media| { media.kind == MediaKind::Video && media.status == MediaStatus::Missing }));
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        let output = root.join(format!("unreachable-{mode:?}"));
        let report = crate::Premiere
            .import_to_tesseract(&input, &output, &options, mode)
            .unwrap();
        assert!(
            report.diagnostics.iter().any(|note| {
                note.reason.contains("movie.mov") && note.reason.contains("does not fit")
            }),
            "{report:?}"
        );
        if mode == ConversionMode::Write {
            let archive = TesseractFile::open(output.join("project.tsrct")).unwrap();
            assert!(archive.metadata().assets.is_empty());
            assert_eq!(linked_groups(&archive.project_json().unwrap()).len(), 1);
        } else {
            assert!(!output.exists());
        }
    }
}

#[test]
fn missing_linked_video_with_malformed_alias_counts_fails_public_check_and_write() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let input = native_video_link_with_location(
        root,
        "/missing/native/movie.mov",
        Some(("6".into(), 1.into())),
    );
    let options = crate::PremiereImportOptions::default();
    let inspection = crate::Premiere
        .inspect_media(&input, &options, None)
        .unwrap();
    let video = inspection
        .media
        .iter()
        .find(|media| media.kind == MediaKind::Video)
        .unwrap();
    assert_eq!(video.status, MediaStatus::Unassessed, "{video:?}");
    assert!(video.selected.is_none());
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        let output = root.join(format!("malformed-{mode:?}"));
        let error = crate::Premiere
            .import_to_tesseract(&input, &output, &options, mode)
            .unwrap_err();
        assert!(error.to_string().contains("failed admission"), "{error}");
        assert!(!output.exists());
    }
}

#[test]
fn unavailable_linked_video_is_not_missing_when_the_path_is_invalid_or_present() {
    for path in [
        "https://example.invalid/movie.mov",
        ".",
        "blocker/movie.mov",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::write(root.join("blocker"), b"not a directory").unwrap();
        let input = native_video_link(root, path);
        let options = crate::PremiereImportOptions::default();
        let inspection = crate::Premiere
            .inspect_media(&input, &options, None)
            .unwrap();
        let video = inspection
            .media
            .iter()
            .find(|media| media.kind == MediaKind::Video)
            .unwrap();
        assert_eq!(video.status, MediaStatus::Unassessed, "{path}: {video:?}");
        for mode in [ConversionMode::Check, ConversionMode::Write] {
            let output = root.join(format!("invalid-{mode:?}"));
            let error = crate::Premiere
                .import_to_tesseract(&input, &output, &options, mode)
                .unwrap_err();
            assert!(error.to_string().contains("failed admission"), "{error}");
            assert!(!output.exists());
        }
    }
}

#[cfg(unix)]
#[test]
fn linked_video_io_failure_remains_fatal() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::os::unix::fs::symlink("loop.mov", root.join("loop.mov")).unwrap();
    let input = native_video_link(root, "loop.mov");
    let options = crate::PremiereImportOptions::default();
    let inspection = crate::Premiere
        .inspect_media(&input, &options, None)
        .unwrap();
    assert!(inspection.media.iter().any(|media| {
        media.kind == MediaKind::Video && media.status == MediaStatus::Unreadable
    }));
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        let output = root.join(format!("io-{mode:?}"));
        assert!(crate::Premiere
            .import_to_tesseract(&input, &output, &options, mode)
            .is_err());
        assert!(!output.exists());
    }
}
