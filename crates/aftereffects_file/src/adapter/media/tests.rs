#[cfg(not(windows))]
mod foreign_collected;
mod foreign_relative;

use super::*;
use sha2::{Digest, Sha256};

fn request(path: &str) -> MediaAssetRequest {
    MediaAssetRequest {
        logical_id: AssetId::new("native-source").unwrap(),
        source_item_id: 0,
        authored_path: path.into(),
        relative_location: None,
        relative_hint_malformed: false,
        kind: MediaAssetKind::Audio,
        photoshop_source: None,
        dimensions: [0, 0],
    }
}

fn media_descriptor(
    authored_path: &str,
    target_is_folder: bool,
) -> crate::structure::MediaDescriptor {
    use crate::structure::{MediaDuration, MediaFrameRate, MediaKind};

    crate::structure::MediaDescriptor {
        source_format: *b"WAVE",
        photoshop_source: None,
        width: 0,
        height: 0,
        duration: MediaDuration {
            numerator: 1,
            denominator: 1,
        },
        native_frame_rate: MediaFrameRate {
            integer: 0,
            fractional: 0,
        },
        conform_frame_rate: MediaFrameRate {
            integer: 0,
            fractional: 0,
        },
        display_frame_rate: MediaFrameRate {
            integer: 0,
            fractional: 0,
        },
        pixel_aspect: (1, 1),
        missing_at_save: false,
        audio_sample_rate: 48_000.0,
        sequence_start_frame: 0,
        sequence_end_frame: 0,
        sequence_frame_padding: 0,
        sequence_frame_range_set: false,
        authored_path: authored_path.into(),
        target_is_folder,
        relative_location: None,
        relative_hint_malformed: false,
        sequence_names: Vec::new(),
        kind: if target_is_folder {
            MediaKind::ImageSequence
        } else {
            MediaKind::Audio
        },
    }
}

fn folder(id: u32, name: &str, parent_folder: Option<u32>) -> crate::structure::ProjectItem {
    use crate::structure::{ItemKind, ProjectItem};

    ProjectItem {
        id,
        name: name.into(),
        parent_folder,
        kind: ItemKind::Folder,
        footage: None,
        solid: None,
        media: None,
        native_media: None,
    }
}

fn media_item(
    id: u32,
    parent_folder: Option<u32>,
    authored_path: &str,
    target_is_folder: bool,
) -> crate::structure::ProjectItem {
    use crate::structure::{ItemKind, ProjectItem};

    let descriptor = media_descriptor(authored_path, target_is_folder);
    ProjectItem {
        id,
        name: Path::new(authored_path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        parent_folder,
        kind: ItemKind::Footage,
        footage: None,
        solid: None,
        media: Some(Ok(descriptor.clone())),
        native_media: Some(Ok(descriptor)),
    }
}

fn project(items: Vec<crate::structure::ProjectItem>) -> crate::structure::StructuralProject {
    crate::structure::StructuralProject {
        format_version: 0,
        items,
    }
}

fn collected_request(id: &str, source_item_id: u32, authored_path: &str) -> MediaAssetRequest {
    MediaAssetRequest {
        logical_id: AssetId::new(id).unwrap(),
        source_item_id,
        authored_path: authored_path.into(),
        ..request(authored_path)
    }
}

#[test]
fn adjacent_collected_media_resolves_root_and_nested_native_folder_paths() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    let root_file = directory.path().join("(Footage)/root.wav");
    let nested_file = directory.path().join("(Footage)/Footage/wav/sound.wav");
    fs::create_dir_all(nested_file.parent().unwrap()).unwrap();
    fs::write(&root_file, b"root").unwrap();
    fs::write(&nested_file, b"nested").unwrap();
    let project = project(vec![
        folder(1, "Footage", None),
        folder(2, "wav", Some(1)),
        media_item(3, Some(2), "/unavailable/sound.wav", false),
        media_item(4, None, "/unavailable/root.wav", false),
    ]);
    let nested = collected_request("nested", 3, "/unavailable/sound.wav");
    let root = collected_request("root", 4, "/unavailable/root.wav");
    let mut resolver = MediaPreflight::with_project(&input, None, &project);

    assert!(resolver.available(&nested));
    assert!(resolver.available(&root));
    assert_eq!(packaged(&resolver, &nested), Some(nested_file.as_path()));
    assert_eq!(packaged(&resolver, &root), Some(root_file.as_path()));
}

#[test]
fn authored_and_native_alias_paths_keep_priority_over_collected_media() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("package");
    fs::create_dir_all(package.join("media")).unwrap();
    fs::create_dir_all(package.join("(Footage)")).unwrap();
    let authored = directory.path().join("old/media/sound.wav");
    fs::create_dir_all(authored.parent().unwrap()).unwrap();
    fs::write(&authored, b"authored").unwrap();
    fs::write(package.join("media/sound.wav"), b"alias").unwrap();
    fs::write(package.join("(Footage)/sound.wav"), b"collected").unwrap();
    let input = package.join("project.aep");
    let project = project(vec![media_item(7, None, authored.to_str().unwrap(), false)]);

    let mut authored_request = collected_request("authored", 7, authored.to_str().unwrap());
    authored_request.relative_location = crate::alias::RelativeLocation::new(1, 2);
    let mut resolver = MediaPreflight::with_project(&input, None, &project);
    assert!(resolver.available(&authored_request));
    assert_eq!(
        packaged(&resolver, &authored_request),
        Some(authored.as_path())
    );

    fs::remove_file(&authored).unwrap();
    let mut alias_request = authored_request.clone();
    alias_request.logical_id = AssetId::new("alias").unwrap();
    let mut resolver = MediaPreflight::with_project(&input, None, &project);
    assert!(resolver.available(&alias_request));
    assert_eq!(
        packaged(&resolver, &alias_request),
        Some(
            fs::canonicalize(&package)
                .unwrap()
                .join("media/sound.wav")
                .as_path()
        )
    );

    fs::remove_file(package.join("media/sound.wav")).unwrap();
    let mut resolver = MediaPreflight::with_project(&input, None, &project);
    assert!(resolver.available(&alias_request));
    let assets = resolver
        .used_assets(std::slice::from_ref(&alias_request))
        .unwrap();
    assert_eq!(assets[0].path, package.join("(Footage)/sound.wav"));
    assert_eq!(
        assets[0].source.missing_paths,
        vec![
            authored,
            fs::canonicalize(&package).unwrap().join("media/sound.wav"),
        ]
    );
}

#[test]
fn duplicate_collected_targets_with_different_authored_paths_are_all_ambiguous() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    let collected = directory.path().join("(Footage)/Audio/sound.wav");
    fs::create_dir_all(collected.parent().unwrap()).unwrap();
    fs::write(&collected, b"must not be selected").unwrap();
    let project = project(vec![
        folder(1, "Audio", None),
        media_item(2, Some(1), "/one/sound.wav", false),
        media_item(3, Some(1), "/two/sound.wav", false),
        media_item(4, Some(1), "/two/sound.wav", false),
    ]);
    let mut resolver = MediaPreflight::with_project(&input, None, &project);

    for (id, item_id, authored) in [
        ("one", 2, "/one/sound.wav"),
        ("two", 3, "/two/sound.wav"),
        ("three", 4, "/two/sound.wav"),
    ] {
        let source = collected_request(id, item_id, authored);
        assert!(!resolver.available(&source));
    }
    assert!(resolver.failure.is_none());
    assert!(
        resolver
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.message.contains("conflicting"))
    );
}

#[cfg(unix)]
#[test]
fn unsafe_collected_folders_and_symlink_escapes_are_not_scanned() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    let outside = directory.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("sound.wav"), b"outside").unwrap();
    fs::create_dir_all(directory.path().join("(Footage)")).unwrap();
    std::os::unix::fs::symlink(&outside, directory.path().join("(Footage)/Escape")).unwrap();
    let project = project(vec![
        folder(1, "..", None),
        folder(2, "Escape", None),
        media_item(3, Some(1), "/missing/sound.wav", false),
        media_item(4, Some(2), "/missing/sound.wav", false),
    ]);
    let mut resolver = MediaPreflight::with_project(&input, None, &project);

    for (id, item_id) in [("unsafe", 3), ("escape", 4)] {
        assert!(!resolver.available(&collected_request(id, item_id, "/missing/sound.wav")));
    }
    assert!(resolver.failure.is_none());
    assert!(resolver.resolved.is_empty());
    assert!(
        resolver
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.message.contains("collected"))
    );
}

#[test]
fn folder_alias_sequence_uses_collected_folder_and_requested_frame_name() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    let frame = directory
        .path()
        .join("(Footage)/Stills/Sequence/frame_0001.png");
    fs::create_dir_all(frame.parent().unwrap()).unwrap();
    image::RgbaImage::from_pixel(4, 2, image::Rgba([1, 2, 3, 255]))
        .save(&frame)
        .unwrap();
    let project = project(vec![
        folder(1, "Stills", None),
        media_item(2, Some(1), "/missing/Sequence", true),
    ]);
    let mut source = collected_request("sequence-frame", 2, "/missing/Sequence/frame_0001.png");
    source.kind = MediaAssetKind::SequenceImage;
    source.dimensions = [4, 2];
    let mut resolver = MediaPreflight::with_project(&input, None, &project);

    assert!(matches!(
        resolver.resolve_media(&source),
        MediaResolution::AssetDimensions([4, 2])
    ));
    assert_eq!(packaged(&resolver, &source), Some(frame.as_path()));
}

#[test]
fn collected_qtrle_is_inspected_as_requiring_transcode() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    let path = directory.path().join("(Footage)/Video/animation.mov");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut qtrle = include_bytes!("../../../tests/fixtures/audio_e2e/movie.mov").to_vec();
    let stsd = qtrle.windows(4).position(|bytes| bytes == b"stsd").unwrap();
    qtrle[stsd + 16..stsd + 20].copy_from_slice(b"rle ");
    fs::write(&path, qtrle).unwrap();
    let project = project(vec![
        folder(1, "Video", None),
        media_item(2, Some(1), "/missing/animation.mov", false),
    ]);
    let mut source = collected_request("video", 2, "/missing/animation.mov");
    source.kind = MediaAssetKind::Video;
    let mut resolver = MediaPreflight::with_project(&input, None, &project);

    let inspected = resolver.inspect(&source, "2".into(), "animation".into(), Vec::new());
    assert_eq!(inspected.original.as_deref(), Some(path.as_path()));
    assert_eq!(inspected.status, MediaStatus::RequiresTranscode);
    assert_eq!(inspected.remediation, MediaRemediation::TranscodeCandidate);
    assert_eq!(inspected.codec.as_deref(), Some("rle "));
}

#[cfg(unix)]
#[test]
fn collected_io_errors_are_not_reported_as_missing() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    fs::create_dir(directory.path().join("(Footage)")).unwrap();
    let path = directory.path().join("(Footage)/sound.wav");
    std::os::unix::fs::symlink("sound.wav", &path).unwrap();
    let project = project(vec![media_item(1, None, "/missing/sound.wav", false)]);
    let source = collected_request("loop", 1, "/missing/sound.wav");
    let mut resolver = MediaPreflight::with_project(&input, None, &project);
    let inspected = resolver.inspect(&source, "1".into(), "sound.wav".into(), vec![]);
    assert_eq!(inspected.status, MediaStatus::Unreadable);
    assert!(matches!(
        resolver.require(&source),
        Err(AepConversionError::Io { .. })
    ));
}

#[test]
fn inspection_requires_preparation_for_unsupported_audio_filename_without_changing_import_policy() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("source.aep");
    // Filename admission only: the bytes intentionally remain the same native fixture WAVE.
    let bytes = include_bytes!("../../../tests/fixtures/audio_e2e/other.wav");
    std::fs::write(directory.path().join("audio.aiff"), bytes).unwrap();
    std::fs::write(directory.path().join("audio.wav"), bytes).unwrap();
    let mut preflight = MediaPreflight::new(&input);
    let inspected = preflight.inspect(&request("audio.aiff"), "1".into(), "audio".into(), vec![]);
    assert_eq!(inspected.status, MediaStatus::RequiresTranscode);
    assert_eq!(inspected.remediation, MediaRemediation::TranscodeCandidate);
    assert!(preflight.require(&request("audio.aiff")).is_ok());
    let mut prepared = MediaPreflight::new(&input);
    assert_eq!(
        prepared
            .inspect(&request("audio.wav"), "1".into(), "audio".into(), vec![])
            .status,
        MediaStatus::Supported
    );
}

#[test]
fn unsafe_paths_are_omitted_without_network_or_platform_guessing() {
    let input = Path::new("fixture/project.aep");
    for path in [
        "",
        "bad\0name",
        "https://example.invalid/media.wav",
        "file:///tmp/media.wav",
        "//host/share.wav",
        "\\\\host\\share.wav",
    ] {
        let mut preflight = MediaPreflight::new(input);
        assert!(!preflight.available(&request(path)));
        assert!(preflight.failure.is_none());
        assert!(preflight.resolved.is_empty());
        assert_eq!(preflight.diagnostics.len(), 1);
        assert!(!preflight.available(&request(path)));
        assert_eq!(
            preflight.diagnostics.len(),
            1,
            "missing source warnings are deduplicated"
        );
    }
    if !cfg!(windows) {
        let mut preflight = MediaPreflight::new(input);
        assert!(!preflight.available(&request("C:\\media.wav")));
        assert!(preflight.failure.is_none());
    }
}

#[test]
fn missing_inspection_distinguishes_absence_from_unassessed_path_omissions() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    fs::write(directory.path().join("blocker"), b"not a directory").unwrap();
    for (path, malformed, expected) in [
        ("missing.mov", false, MediaStatus::Missing),
        ("missing.mov", true, MediaStatus::Unassessed),
        (
            "https://example.invalid/movie.mov",
            false,
            MediaStatus::Unassessed,
        ),
        (".", false, MediaStatus::Unassessed),
        ("blocker/movie.mov", false, MediaStatus::Unassessed),
    ] {
        let mut preflight = MediaPreflight::new(&input);
        let mut source = request(path);
        source.kind = MediaAssetKind::Video;
        source.relative_hint_malformed = malformed;
        for _ in 0..2 {
            let inspected = preflight.inspect(&source, "1".into(), "video".into(), vec![]);
            assert_eq!(inspected.status, expected, "{path}: {inspected:?}");
            assert!(inspected.selected.is_none());
        }
        assert_eq!(preflight.diagnostics.len(), 1);
        assert!(
            preflight.require(&source).is_ok(),
            "standalone omission is unchanged"
        );
    }
}

#[test]
fn missing_native_relative_ancestor_is_not_unsafe_alias_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    for (ascend, components, expected) in [
        (u32::MAX, 1, MediaStatus::Missing),
        (1, u32::MAX, MediaStatus::Unassessed),
        // Validate the authored tail even when the current ancestor is unavailable.
        (u32::MAX, u32::MAX, MediaStatus::Unassessed),
    ] {
        let mut source = request("/missing/native/movie.mov");
        source.kind = MediaAssetKind::Video;
        source.relative_location = RelativeLocation::new(ascend, components);
        let mut preflight = MediaPreflight::new(&input);
        let inspected = preflight.inspect(&source, "1".into(), "video".into(), vec![]);
        assert_eq!(inspected.status, expected, "{inspected:?}");
        assert!(
            inspected
                .reason
                .unwrap()
                .contains("does not fit this AEP's location")
        );
        assert!(preflight.require(&source).is_ok());
    }
}

#[test]
fn review_audit_missing_media_keeps_identity_for_conflict_detection() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    fs::write(directory.path().join("present.wav"), b"test bytes").unwrap();
    let mut preflight = MediaPreflight::new(&input);
    let missing = request("missing.wav");
    assert!(!preflight.available(&missing));
    assert!(!preflight.available(&missing));
    assert_eq!(preflight.diagnostics.len(), 1);
    assert!(preflight.failure.is_none());
    assert!(!preflight.available(&request("present.wav")));
    assert!(matches!(
        preflight.failure,
        Some(AepConversionError::Input(
            "conflicting local media identity"
        ))
    ));
}

#[cfg(unix)]
#[test]
fn review_audit_overlong_media_component_is_an_item_local_omission() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    let mut preflight = MediaPreflight::new(&input);
    let source = request(&"x".repeat(300));
    assert!(!preflight.available(&source));
    assert!(preflight.failure.is_none(), "{:?}", preflight.failure);
    assert!(preflight.resolved.is_empty());
    assert_eq!(preflight.diagnostics.len(), 1);
    assert!(preflight.diagnostics[0].message.contains("native-source"));
    assert!(!preflight.available(&source));
    assert_eq!(preflight.diagnostics.len(), 1);
}

#[test]
fn relative_sources_resolve_against_input_and_identity_conflicts_are_fatal() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    fs::write(directory.path().join("media.wav"), b"test bytes").unwrap();
    let mut preflight = MediaPreflight::new(&input);
    let source = request("media.wav");
    assert!(preflight.available(&source));
    assert!(preflight.available(&source));
    assert_eq!(preflight.resolved.len(), 1);
    assert!(!preflight.available(&request("another.wav")));
    assert!(matches!(
        preflight.failure,
        Some(AepConversionError::Input(
            "conflicting local media identity"
        ))
    ));
    assert!(
        !preflight.available(&source),
        "fatal errors stop subsequent resolution"
    );
}

#[test]
fn unsupported_video_extensions_latch_a_fatal_error() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    for (index, name) in ["animation.swf", "animation.SWF", "clip.avi", "clip"]
        .iter()
        .enumerate()
    {
        fs::write(
            directory.path().join(name),
            b"FWS synthetic unsupported footage",
        )
        .unwrap();
        let source = MediaAssetRequest {
            logical_id: AssetId::new(format!("unsupported-{index}")).unwrap(),
            kind: MediaAssetKind::Video,
            ..request(name)
        };
        let mut preflight = MediaPreflight::new(&input);
        assert!(matches!(
            preflight.resolve_media(&source),
            MediaResolution::Unavailable
        ));
        let error = preflight
            .failure
            .as_ref()
            .expect("unsupported video is fatal")
            .to_string();
        assert!(
            error.contains(name) && error.contains(source.logical_id.as_str()),
            "{error}"
        );
        assert!(preflight.resolved.is_empty());
        assert!(
            preflight.diagnostics.is_empty(),
            "not a successful omission"
        );
    }
}

#[test]
fn supported_video_containers_keep_their_original_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    let mut preflight = MediaPreflight::new(&input);
    for (index, name) in ["clip.mp4", "clip.MOV", "clip.m4v"].iter().enumerate() {
        fs::write(
            directory.path().join(name),
            include_bytes!("../../../tests/fixtures/audio_e2e/movie.mov"),
        )
        .unwrap();
        let source = MediaAssetRequest {
            logical_id: AssetId::new(format!("supported-{index}")).unwrap(),
            kind: MediaAssetKind::Video,
            ..request(name)
        };
        assert!(preflight.available(&source));
    }
    assert_eq!(preflight.resolved.len(), 3);
    assert!(preflight.failure.is_none());
}

#[test]
fn unsupported_video_codec_in_mov_and_invalid_container_are_fatal() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    let mut qtrle = include_bytes!("../../../tests/fixtures/audio_e2e/movie.mov").to_vec();
    let stsd = qtrle.windows(4).position(|bytes| bytes == b"stsd").unwrap();
    assert_eq!(&qtrle[stsd + 16..stsd + 20], b"avc1");
    qtrle[stsd + 16..stsd + 20].copy_from_slice(b"rle ");
    for (name, bytes, reason) in [
        ("animation.mov", qtrle.as_slice(), "rle "),
        ("broken.mp4", b"not an MP4".as_slice(), "video"),
    ] {
        fs::write(directory.path().join(name), bytes).unwrap();
        let source = MediaAssetRequest {
            kind: MediaAssetKind::Video,
            ..request(name)
        };
        let mut preflight = MediaPreflight::new(&input);
        assert!(matches!(
            preflight.resolve_media(&source),
            MediaResolution::Unavailable
        ));
        let error = preflight
            .failure
            .as_ref()
            .expect("invalid or unsupported video is fatal")
            .to_string();
        assert!(error.contains(name) && error.contains(reason), "{error}");
        assert!(preflight.resolved.is_empty());
    }
}

#[test]
fn pdf_compatible_ai_resolves_to_cached_vector_content_not_an_archive_asset() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    fs::write(
        directory.path().join("artwork.ai"),
        include_bytes!("../../../tests/fixtures/vector_media/spec_case_1.ai"),
    )
    .unwrap();
    let mut source = request("artwork.ai");
    source.kind = MediaAssetKind::Image;
    let mut preflight = MediaPreflight::new(&input);

    let MediaResolution::Vector(first) = preflight.resolve_media(&source) else {
        panic!("AI must produce vector content");
    };
    let MediaResolution::Vector(second) = preflight.resolve_media(&source) else {
        panic!("cached AI must produce vector content");
    };
    assert!(Arc::ptr_eq(&first, &second));
    assert!(preflight.failure.is_none());
    assert_eq!(preflight.resolved.len(), 1);
    assert!(preflight.diagnostics.is_empty());
    let (_, resolved) = preflight.resolved.get(&source.logical_id).unwrap();
    assert!(matches!(resolved, ResolvedSource::Vector { .. }));
}

#[test]
fn distinct_ai_sources_keep_their_vector_content() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    fs::write(
        directory.path().join("artwork.ai"),
        include_bytes!("../../../tests/fixtures/vector_media/spec_case_1.ai"),
    )
    .unwrap();
    let mut preflight = MediaPreflight::new(&input);
    for index in 0..3 {
        let source = MediaAssetRequest {
            logical_id: AssetId::new(format!("artwork-{index}")).unwrap(),
            kind: MediaAssetKind::Image,
            ..request("artwork.ai")
        };
        assert!(matches!(
            preflight.resolve_media(&source),
            MediaResolution::Vector(_)
        ));
    }
    assert_eq!(preflight.resolved.len(), 3);
    assert!(preflight.missing.is_empty());
}

#[test]
fn postscript_only_ai_is_diagnosed_without_asset_or_raster_fallback() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    fs::write(directory.path().join("legacy.ai"), b"%!PS-Adobe-3.0").unwrap();
    let mut source = request("legacy.ai");
    source.kind = MediaAssetKind::Image;
    let mut preflight = MediaPreflight::new(&input);

    assert!(matches!(
        preflight.resolve_media(&source),
        MediaResolution::Unavailable
    ));
    assert!(preflight.failure.is_none());
    assert!(preflight.resolved.is_empty());
    assert_eq!(preflight.diagnostics.len(), 1);
    assert!(
        preflight.diagnostics[0]
            .message
            .contains("no PNG, raw AI asset or guessed selector fallback")
    );
}

#[test]
fn directories_and_non_directory_parents_are_best_effort_omissions() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    fs::write(directory.path().join("not-directory"), b"test bytes").unwrap();
    for path in [".", "missing.wav", "not-directory/media.wav"] {
        let mut preflight = MediaPreflight::new(&input);
        assert!(!preflight.available(&request(path)));
        assert!(preflight.failure.is_none());
        assert_eq!(preflight.diagnostics.len(), 1);
    }
}

#[test]
fn psd_normalization_keeps_owned_png_until_preflight_drops() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    fs::write(
        directory.path().join("source.psd"),
        include_bytes!("../../../tests/fixtures/psd_import/two_layers_v2.psd"),
    )
    .unwrap();
    let source = MediaAssetRequest {
        kind: MediaAssetKind::Image,
        photoshop_source: Some(PhotoshopSource::Merged),
        dimensions: [64, 48],
        ..request("source.psd")
    };
    let mut preflight = MediaPreflight::new(&input);
    assert!(preflight.available(&source));
    let ResolvedSource::Asset { path: png, .. } = &preflight.resolved[&source.logical_id].1 else {
        panic!("PSD must resolve to a normalized PNG asset");
    };
    let png = png.clone();
    assert_eq!(png.extension().unwrap(), "png");
    assert!(png.exists());
    assert!(preflight.available(&source));
    assert_eq!(preflight.normalized.len(), 1);
    let different_selection = MediaAssetRequest {
        photoshop_source: Some(PhotoshopSource::Layer { id: 101, index: 0 }),
        ..source
    };
    assert!(!preflight.available(&different_selection));
    assert!(preflight.failure.is_some());
    drop(preflight);
    assert!(!png.exists());
}

#[test]
fn existing_png_and_jpeg_sources_are_not_normalized() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    for extension in ["png", "jpg"] {
        let path = directory.path().join(format!("source.{extension}"));
        image::RgbImage::from_pixel(2, 2, image::Rgb([23, 42, 99]))
            .save(&path)
            .unwrap();
        let original = fs::read(&path).unwrap();
        let mut preflight = MediaPreflight::new(&input);
        let source = MediaAssetRequest {
            kind: MediaAssetKind::Image,
            dimensions: [2, 2],
            ..request(path.to_str().unwrap())
        };
        assert!(preflight.available(&source));
        assert!(matches!(&preflight.resolved[&source.logical_id].1,
            ResolvedSource::Asset { path: resolved, .. } if *resolved == path
        ));
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(preflight.normalized.is_empty());
        assert!(preflight.diagnostics.is_empty());
    }
}

#[test]
fn sequence_png_resolution_preserves_bytes_dimensions_and_missing_siblings() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    let large_path = directory.path().join("shot_0000.png");
    let small_path = directory.path().join("shot_0001.png");
    image::RgbaImage::from_pixel(4, 2, image::Rgba([23, 42, 99, 127]))
        .save(&large_path)
        .unwrap();
    image::RgbaImage::from_pixel(2, 1, image::Rgba([91, 17, 203, 63]))
        .save(&small_path)
        .unwrap();
    let large_bytes = fs::read(&large_path).unwrap();
    let small_bytes = fs::read(&small_path).unwrap();
    let sequence_request = |id: &str, path: &Path| MediaAssetRequest {
        logical_id: AssetId::new(id).unwrap(),
        source_item_id: 0,
        authored_path: path.to_str().unwrap().into(),
        relative_location: None,
        relative_hint_malformed: false,
        kind: MediaAssetKind::SequenceImage,
        photoshop_source: None,
        dimensions: [4, 2],
    };
    let large = sequence_request("sequence-0", &large_path);
    let small = sequence_request("sequence-1", &small_path);
    let wrong_aspect_path = directory.path().join("shot_wrong_aspect.png");
    image::RgbaImage::from_pixel(2, 2, image::Rgba([7, 11, 13, 255]))
        .save(&wrong_aspect_path)
        .unwrap();
    let wrong_aspect = sequence_request("sequence-wrong-aspect", &wrong_aspect_path);
    let missing = sequence_request("sequence-2", &directory.path().join("shot_0002.png"));
    let mut preflight = MediaPreflight::new(&input);

    assert!(matches!(
        preflight.resolve_media(&large),
        MediaResolution::AssetDimensions([4, 2])
    ));
    assert!(matches!(
        preflight.resolve_media(&small),
        MediaResolution::Unavailable
    ));
    assert!(matches!(
        preflight.resolve_media(&wrong_aspect),
        MediaResolution::Unavailable
    ));
    assert!(matches!(
        preflight.resolve_media(&missing),
        MediaResolution::Unavailable
    ));
    assert!(preflight.failure.is_none());
    assert_eq!(fs::read(&large_path).unwrap(), large_bytes);
    assert_eq!(fs::read(&small_path).unwrap(), small_bytes);
    assert!(preflight.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("sequence frame is 2x1")
            && diagnostic.message.contains("fixed footage canvas is 4x2")
            && diagnostic.message.contains("no guessed normalization")
    }));
    assert!(preflight.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("sequence frame is 2x2")
            && diagnostic.message.contains("no guessed normalization")
    }));
    assert!(
        preflight
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("local source is missing"))
    );
    assert!(!preflight.resolved.contains_key(&small.logical_id));
}

#[test]
fn malformed_sequence_png_is_an_item_local_omission() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    fs::write(directory.path().join("broken.png"), b"not a PNG").unwrap();
    let source = MediaAssetRequest {
        logical_id: AssetId::new("broken-sequence-frame").unwrap(),
        source_item_id: 0,
        authored_path: "broken.png".into(),
        relative_location: None,
        relative_hint_malformed: false,
        kind: MediaAssetKind::SequenceImage,
        photoshop_source: None,
        dimensions: [4, 2],
    };
    let mut preflight = MediaPreflight::new(&input);

    assert!(matches!(
        preflight.resolve_media(&source),
        MediaResolution::Unavailable
    ));
    assert!(preflight.failure.is_none());
    assert!(preflight.resolved.is_empty());
    assert!(preflight.diagnostics[0].message.contains("not a PNG image"));
}

#[test]
fn psd_without_selector_is_never_packaged_raw_even_with_a_png_extension() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    fs::write(directory.path().join("source.png"), b"8BPSunknown").unwrap();
    let mut preflight = MediaPreflight::new(&input);
    let source = MediaAssetRequest {
        kind: MediaAssetKind::Image,
        ..request("source.png")
    };
    assert!(!preflight.available(&source));
    assert!(preflight.failure.is_none());
    assert!(preflight.resolved.is_empty());
    assert!(
        preflight.diagnostics[0]
            .message
            .contains("no validated native Photoshop selector")
    );
}

#[test]
fn psd_large_source_is_normalized_without_an_input_byte_ceiling() {
    use std::io::{SeekFrom, Write};

    // Supplementary resource padding; selected native fixture pixels are unchanged.
    let fixture = include_bytes!("../../../tests/fixtures/psd_import/two_layers_v2.psd");
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    let mut file = File::create(directory.path().join("large.psd")).unwrap();
    let payload_len = 128 * 1024 * 1024_u32;
    file.write_all(&fixture[..30]).unwrap();
    file.write_all(&(payload_len + 12).to_be_bytes()).unwrap();
    file.write_all(b"8BIM\x04\x0f\0\0").unwrap(); // ICC resource, empty Pascal name.
    file.write_all(&payload_len.to_be_bytes()).unwrap();
    file.seek(SeekFrom::Current(i64::from(payload_len)))
        .unwrap();
    file.write_all(&fixture[34..]).unwrap();
    drop(file);

    let mut preflight = MediaPreflight::new(&input);
    let source = MediaAssetRequest {
        kind: MediaAssetKind::Image,
        photoshop_source: Some(PhotoshopSource::Merged),
        dimensions: [64, 48],
        ..request("large.psd")
    };
    assert!(preflight.available(&source), "{:?}", preflight.diagnostics);
    let ResolvedSource::Asset { path, .. } = &preflight.resolved[&source.logical_id].1 else {
        panic!("PSD must produce a PNG");
    };
    let png = image::open(path).unwrap().into_rgba8();
    assert_eq!(png.get_pixel(10, 8).0, [255, 32, 16, 255]);
}

#[test]
fn psd_failed_attempts_do_not_exhaust_later_sources() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    let broken = directory.path().join("broken.psd");
    fs::write(&broken, b"8BPSbroken").unwrap();
    File::options()
        .write(true)
        .open(&broken)
        .unwrap()
        .set_len(128 * 1024 * 1024)
        .unwrap();
    fs::write(
        directory.path().join("valid.psd"),
        include_bytes!("../../../tests/fixtures/psd_import/two_layers_v2.psd"),
    )
    .unwrap();
    let mut preflight = MediaPreflight::new(&input);
    // Cross both the former 512 MiB read and 64 Mi-pixel attempted-output quotas.
    for index in 0..5 {
        let source = MediaAssetRequest {
            logical_id: AssetId::new(format!("broken-{index}")).unwrap(),
            kind: MediaAssetKind::Image,
            photoshop_source: Some(PhotoshopSource::Merged),
            dimensions: [4096, 4096],
            ..request("broken.psd")
        };
        assert!(!preflight.available(&source));
        assert!(
            preflight
                .diagnostics
                .last()
                .unwrap()
                .message
                .contains("cannot be normalized")
        );
    }
    let valid = MediaAssetRequest {
        kind: MediaAssetKind::Image,
        photoshop_source: Some(PhotoshopSource::Merged),
        dimensions: [64, 48],
        ..request("valid.psd")
    };
    assert!(preflight.available(&valid), "{:?}", preflight.diagnostics);
}

#[cfg(unix)]
#[test]
fn operational_filesystem_errors_are_not_downgraded_to_missing_media() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    std::os::unix::fs::symlink("loop.wav", directory.path().join("loop.wav")).unwrap();
    let mut preflight = MediaPreflight::new(&input);
    assert!(!preflight.available(&request("loop.wav")));
    assert!(preflight.failure.is_some());
    assert!(preflight.diagnostics.is_empty());
    assert!(preflight.resolved.is_empty());
}

/// A request for `authored`, which AE relinks from the moved project by
/// ascending `ascend` levels and following its last `components` components.
fn relinked_request(authored: &Path, ascend: u32, components: u32) -> MediaAssetRequest {
    MediaAssetRequest {
        relative_location: crate::alias::RelativeLocation::new(ascend, components),
        ..request(authored.to_str().unwrap())
    }
}

/// The file that `preflight` packages for `source`, if any.
fn packaged<'a>(preflight: &'a MediaPreflight<'_>, source: &MediaAssetRequest) -> Option<&'a Path> {
    let used = preflight.used_assets(std::slice::from_ref(source)).ok()?;
    used.first().map(|asset| asset.path)
}

#[test]
fn missing_absolute_media_relinks_at_its_native_relative_location() {
    // The native layout: `<package>/project.aep` authored `<old package>/media/a.wav`
    // with AE's counts (1, 2), after the whole package moved.
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("moved package");
    fs::create_dir_all(package.join("media")).unwrap();
    fs::write(package.join("media/a.wav"), b"moved bytes").unwrap();
    let input = package.join("project.aep");
    let authored = directory.path().join("old package/media/a.wav");
    let source = relinked_request(&authored, 1, 2);
    let mut preflight = MediaPreflight::new(&input);
    assert!(preflight.available(&source));
    assert!(
        preflight.diagnostics.is_empty(),
        "{:?}",
        preflight.diagnostics
    );
    assert_eq!(
        packaged(&preflight, &source),
        Some(
            fs::canonicalize(&package)
                .unwrap()
                .join("media/a.wav")
                .as_path()
        )
    );
    // Two ascents reach the common ancestor of a sibling media directory.
    let nested = package.join("projects/project.aep");
    let source = relinked_request(&directory.path().join("old/projects/x/media/a.wav"), 2, 2);
    let mut preflight = MediaPreflight::new(&nested);
    fs::create_dir_all(package.join("projects")).unwrap();
    assert!(preflight.available(&source));
}

#[test]
fn an_existing_absolute_media_file_wins_over_its_native_relative_location() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("package");
    fs::create_dir_all(package.join("media")).unwrap();
    fs::write(package.join("media/a.wav"), b"relative bytes").unwrap();
    let authored = directory.path().join("original/media/a.wav");
    fs::create_dir_all(authored.parent().unwrap()).unwrap();
    fs::write(&authored, b"absolute bytes").unwrap();
    let source = relinked_request(&authored, 1, 2);
    let input = package.join("project.aep");
    let mut preflight = MediaPreflight::new(&input);
    assert!(preflight.available(&source));
    assert_eq!(packaged(&preflight, &source), Some(authored.as_path()));
}

#[test]
fn unfit_native_relative_counts_are_diagnosed_without_a_search() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("package");
    fs::create_dir_all(package.join("media")).unwrap();
    // A same-named file where a guessed search would find it.
    fs::write(package.join("media/a.wav"), b"bytes").unwrap();
    let input = package.join("project.aep");
    let depth = u32::try_from(fs::canonicalize(&package).unwrap().components().count()).unwrap();
    for (authored, ascend, components) in [
        // Above the filesystem root.
        (directory.path().join("gone/media/a.wav"), depth + 1, 2),
        // More components than the authored path has.
        (PathBuf::from("/a.wav"), 1, 3),
        // A parent component is never followed.
        (PathBuf::from("/gone/../media/a.wav"), 1, 3),
    ] {
        let source = relinked_request(&authored, ascend, components);
        let mut preflight = MediaPreflight::new(&input);
        assert!(!preflight.available(&source), "{authored:?}");
        assert!(preflight.failure.is_none());
        assert_eq!(preflight.diagnostics.len(), 1);
        let message = &preflight.diagnostics[0].message;
        assert!(
            message.contains(&format!(
                "native relative location (ascend {ascend}, {components} trailing components) does not fit"
            )),
            "{message}"
        );
    }
}

#[test]
fn interrupted_ai_conversion_latches_fatal_input_error() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("project.aep");
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << /Fm 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        pdf_stream("", b"/Fm Do"),
    ];
    for id in 5..=22 {
        let resources = if id < 22 {
            format!("/Resources << /XObject << /Fm {} 0 R >> >>", id + 1)
        } else {
            String::new()
        };
        let content = if id < 22 {
            b"/Fm Do".as_slice()
        } else {
            b"10 10 20 20 re f".as_slice()
        };
        objects.push(pdf_stream(
            &format!("/Type /XObject /Subtype /Form /BBox [0 0 100 100] {resources}"),
            content,
        ));
    }
    fs::write(directory.path().join("nested.ai"), build_pdf(objects)).unwrap();
    let mut source = request("nested.ai");
    source.kind = MediaAssetKind::Image;
    let mut preflight = MediaPreflight::new(&input);

    assert!(matches!(
        preflight.resolve_media(&source),
        MediaResolution::Unavailable
    ));
    assert!(matches!(
        preflight.failure,
        Some(AepConversionError::Input("Form XObject recursion depth"))
    ));
    assert!(preflight.diagnostics.is_empty());
    assert!(matches!(
        preflight.resolve_media(&source),
        MediaResolution::Unavailable
    ));
}

#[test]
fn media_missing_at_both_locations_names_the_native_relative_location() {
    let directory = tempfile::tempdir().unwrap();
    let package = directory.path().join("package");
    fs::create_dir_all(&package).unwrap();
    let source = relinked_request(&directory.path().join("old/media/a.wav"), 1, 2);
    let input = package.join("project.aep");
    let mut preflight = MediaPreflight::new(&input);
    assert!(!preflight.available(&source));
    assert!(preflight.failure.is_none());
    let expected = fs::canonicalize(&package).unwrap().join("media/a.wav");
    assert_eq!(preflight.diagnostics.len(), 1);
    assert!(
        preflight.diagnostics[0].message.contains(&format!(
            "missing at its authored path and at its native relative location {expected:?}"
        )),
        "{}",
        preflight.diagnostics[0].message
    );
    // A relative authored path already resolves against the AEP: no relink.
    let mut preflight = MediaPreflight::new(&input);
    let relative = MediaAssetRequest {
        relative_location: crate::alias::RelativeLocation::new(1, 2),
        ..request("media/a.wav")
    };
    assert!(!preflight.available(&relative));
    assert_eq!(
        preflight.diagnostics[0].message,
        "media native-source at \"media/a.wav\": local source is missing; media content omitted"
    );
}

fn pdf_stream(dictionary: &str, content: &[u8]) -> Vec<u8> {
    let mut output = format!("<< {dictionary} /Length {} >>\nstream\n", content.len()).into_bytes();
    output.extend_from_slice(content);
    output.extend_from_slice(b"\nendstream");
    output
}

fn build_pdf(objects: Vec<Vec<u8>>) -> Vec<u8> {
    let object_count = objects.len();
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::with_capacity(object_count);
    for object in objects {
        offsets.push(pdf.len());
        let id = offsets.len();
        pdf.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        pdf.extend_from_slice(&object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes(),
    );
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            object_count + 1
        )
        .as_bytes(),
    );
    pdf
}
