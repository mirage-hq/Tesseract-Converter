use super::*;

fn image_request(spelling: &str, ascend: u32, components: u32) -> MediaAssetRequest {
    MediaAssetRequest {
        kind: MediaAssetKind::Image,
        relative_location: crate::alias::RelativeLocation::new(ascend, components),
        dimensions: [2, 2],
        ..request(spelling)
    }
}

fn write_png(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
        .save(path)
        .unwrap();
}

#[test]
#[cfg(not(windows))]
fn foreign_native_relative_images_use_exact_hint_without_host_or_filename_search() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    let original = root.path().join("Assets/source.png");
    write_png(&original);
    for spelling in [r"F:\old\Assets\source.png", "F:/old/Assets/source.png"] {
        let host_spelling = root.path().join(spelling);
        fs::create_dir_all(host_spelling.parent().unwrap()).unwrap();
        fs::write(host_spelling, b"do not probe the foreign spelling").unwrap();
        let request = image_request(spelling, 1, 2);
        let mut resolver = MediaPreflight::new(&input);
        assert!(resolver.available(&request));
        assert_eq!(
            packaged(&resolver, &request),
            Some(fs::canonicalize(&original).unwrap().as_path())
        );
        assert!(resolver.selections[&request.logical_id].authored.is_none());
    }
    for (spelling, ascend, components) in [
        (r"F:\old\Assets\source.png", 1, 1),
        (r"F:\old\Assets\source.png", 1, 8),
        (r"F:\old\Elsewhere\source.png", 1, 2),
        (r"F:\old\..\Assets\source.png", 1, 2),
        (r"F:\old\Assets\source.png:stream", 1, 2),
        (r"F:Assets\source.png", 1, 2),
    ] {
        let mut resolver = MediaPreflight::new(&input);
        assert!(
            !resolver.available(&image_request(spelling, ascend, components)),
            "no invented suffix or traversal: {spelling}"
        );
        assert!(resolver.resolved.is_empty());
    }
}

#[test]
#[cfg(unix)]
fn foreign_native_relative_symlink_escape_is_not_admitted() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let original = outside.path().join("source.png");
    write_png(&original);
    std::os::unix::fs::symlink(outside.path(), root.path().join("Assets")).unwrap();
    let input = root.path().join("input.aep");
    let mut resolver = MediaPreflight::new(&input);
    assert!(!resolver.available(&image_request(r"F:\old\Assets\source.png", 1, 2)));
    assert!(resolver.resolved.is_empty());
    assert!(
        resolver
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("escapes its AEP ancestor"))
    );
}

#[test]
#[cfg(unix)]
fn foreign_native_relative_ai_symlink_preserves_native_format() {
    let root = tempfile::tempdir().unwrap();
    let assets = root.path().join("Assets");
    fs::create_dir(&assets).unwrap();
    let target = assets.join("art.pdf");
    let alias = assets.join("art.ai");
    let bytes = include_bytes!("../../../../tests/fixtures/vector_media/spec_case_1.ai");
    fs::write(&target, bytes).unwrap();
    std::os::unix::fs::symlink(&target, &alias).unwrap();
    let input = root.path().join("input.aep");
    let mut resolver = MediaPreflight::new(&input);
    assert!(matches!(
        resolver.resolve_media(&image_request(r"F:\old\Assets\art.ai", 1, 2)),
        MediaResolution::Vector(_)
    ));
    let source = resolver.vector_sources().next().unwrap();
    assert_eq!(source.path, fs::canonicalize(&target).unwrap());
    assert_eq!(
        source.decoded_sha256.as_deref(),
        Some(format!("{:x}", Sha256::digest(bytes)).as_str())
    );
    assert!(resolver.normalized.is_empty(), "no raster substitution");
}

#[test]
#[cfg(unix)]
fn foreign_native_relative_ai_symlink_rejects_disguised_png() {
    let root = tempfile::tempdir().unwrap();
    let assets = root.path().join("Assets");
    fs::create_dir(&assets).unwrap();
    let target = assets.join("art.png");
    write_png(&target);
    std::os::unix::fs::symlink(&target, assets.join("art.ai")).unwrap();
    let input = root.path().join("input.aep");
    let mut resolver = MediaPreflight::new(&input);
    assert!(!resolver.available(&image_request(r"F:\old\Assets\art.ai", 1, 2)));
    assert!(resolver.resolved.is_empty());
    assert!(resolver.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("AI source is not PDF-compatible")
    }));
}

#[test]
#[cfg(not(windows))]
fn foreign_collected_ai_reaches_exact_editable_vector_profile() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    let spelling = r"F:\old\artwork.ai";
    let path = root.path().join("(Footage)/artwork.ai");
    let bytes = include_bytes!("../../../../tests/fixtures/vector_media/spec_case_1.ai");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    let mut item = media_item(2, None, spelling, false);
    let mut descriptor = media_descriptor(spelling, false);
    descriptor.kind = crate::media::MediaKind::StillImage;
    item.media = Some(Ok(descriptor.clone()));
    item.native_media = Some(Ok(descriptor));
    let project = project(vec![item]);
    let source = MediaAssetRequest {
        kind: MediaAssetKind::Image,
        ..collected_request("artwork", 2, spelling)
    };
    let mut resolver = MediaPreflight::with_project(&input, None, &project);
    assert!(matches!(
        resolver.resolve_media(&source),
        MediaResolution::Vector(_)
    ));
    let source = resolver.vector_sources().next().unwrap();
    assert_eq!(source.path, path);
    assert_eq!(
        source.decoded_sha256.as_deref(),
        Some(format!("{:x}", Sha256::digest(bytes)).as_str())
    );
    assert!(resolver.normalized.is_empty(), "no raster substitution");
}

// Reused from w03 PR #4952; executed against the consolidated generic locator.
#[cfg(not(windows))]
#[test]
fn foreign_drive_native_relative_video_uses_exact_alias_tail() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("Assets")).unwrap();
    let video = directory.path().join("Assets/Ink.mp4");
    let bytes = include_bytes!("../../../../tests/fixtures/audio_e2e/movie.mov");
    fs::write(&video, bytes).unwrap();
    let input = directory.path().join("project.aep");
    for spelling in [
        r"F:\old\Powder\Assets\Ink.mp4",
        "F:/old/Powder/Assets/Ink.mp4",
    ] {
        let source = MediaAssetRequest {
            kind: MediaAssetKind::Video,
            ..relinked_request(Path::new(spelling), 1, 2)
        };
        let mut preflight = MediaPreflight::new(&input);
        assert!(preflight.available(&source), "{:?}", preflight.diagnostics);
        assert_eq!(
            packaged(&preflight, &source),
            Some(fs::canonicalize(&video).unwrap().as_path())
        );
        assert_eq!(fs::read(&video).unwrap(), bytes);
        assert!(preflight.failure.is_none());
    }
}

#[cfg(not(windows))]
#[test]
fn foreign_drive_native_relative_video_does_not_guess_or_accept_traversal() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("Assets")).unwrap();
    fs::write(
        directory.path().join("Assets/Ink.mp4"),
        include_bytes!("../../../../tests/fixtures/audio_e2e/movie.mov"),
    )
    .unwrap();
    let input = directory.path().join("project.aep");
    for (spelling, counts) in [
        (r"F:\old\Assets\Ink.mp4", None),
        (
            r"F:Assets\Ink.mp4",
            crate::alias::RelativeLocation::new(1, 2),
        ),
        (
            r"F:\old\..\Assets\Ink.mp4",
            crate::alias::RelativeLocation::new(1, 2),
        ),
        (
            r"\\server\Assets\Ink.mp4",
            crate::alias::RelativeLocation::new(1, 2),
        ),
        (
            r"F:\old\Assets\Ink.mp4",
            crate::alias::RelativeLocation::new(1, 9),
        ),
    ] {
        let source = MediaAssetRequest {
            kind: MediaAssetKind::Video,
            relative_location: counts,
            ..request(spelling)
        };
        let mut preflight = MediaPreflight::new(&input);
        assert!(!preflight.available(&source), "{spelling}");
        assert!(packaged(&preflight, &source).is_none());
    }
}
