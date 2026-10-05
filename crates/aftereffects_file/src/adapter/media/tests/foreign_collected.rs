//! Supplemental synthetic admission/security cases; native proof is in adapter tests.

use super::*;

fn raster_item(id: u32, parent: Option<u32>, spelling: &str) -> crate::structure::ProjectItem {
    let mut item = media_item(id, parent, spelling, false);
    let mut descriptor = media_descriptor(spelling, false);
    descriptor.kind = crate::media::MediaKind::StillImage;
    item.media = Some(Ok(descriptor.clone()));
    item.native_media = Some(Ok(descriptor));
    item
}

fn raster_request(id: &str, item: u32, spelling: &str) -> MediaAssetRequest {
    MediaAssetRequest {
        kind: MediaAssetKind::Image,
        dimensions: [2, 2],
        ..collected_request(id, item, spelling)
    }
}

fn png(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
        .save(path)
        .unwrap();
}

#[test]
fn foreign_collected_disguised_psd_is_not_normalized_or_packaged() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    let path = root.path().join("(Footage)/source.png");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        include_bytes!("../../../../tests/fixtures/psd_import/two_layers_v2.psd"),
    )
    .unwrap();
    let spelling = r"C:\old\source.png";
    let project = project(vec![raster_item(2, None, spelling)]);
    let mut request = raster_request("image", 2, spelling);
    request.photoshop_source = Some(PhotoshopSource::Merged);
    request.dimensions = [64, 48];
    let mut resolver = MediaPreflight::with_project(&input, None, &project);
    assert!(
        !resolver.available(&request),
        "foreign PSD must not become a substitute PNG"
    );
    assert!(resolver.normalized.is_empty());
    assert!(resolver.resolved.is_empty());
}

#[test]
#[cfg(not(windows))]
fn foreign_collected_raster_uses_only_native_hierarchy_and_not_host_spelling() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    let spelling = r"C:\old\asset.png";
    let collected = root.path().join("(Footage)/Images/asset.png");
    png(&collected);
    fs::write(
        root.path().join(spelling),
        b"must not read this host filename",
    )
    .unwrap();
    let project = project(vec![
        folder(1, "Images", None),
        raster_item(2, Some(1), spelling),
    ]);
    let request = raster_request("image", 2, spelling);
    let mut resolver = MediaPreflight::with_project(&input, None, &project);
    assert!(resolver.available(&request));
    assert!(resolver.failure.is_none());
    assert_eq!(packaged(&resolver, &request), Some(collected.as_path()));
    assert!(resolver.selections[&request.logical_id].authored.is_none());
    assert!(!resolver.available(&raster_request("mismatch", 2, r"D:\other\asset.png")));
    let empty = super::project(vec![raster_item(2, None, spelling)]);
    let mut resolver = MediaPreflight::with_project(&input, None, &empty);
    assert!(
        !resolver.available(&request),
        "must not search Images or a host spelling"
    );
}

#[test]
#[cfg(not(windows))]
fn foreign_collected_rejects_unsafe_spellings_and_non_raster_admission() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    png(&root.path().join("(Footage)/asset.png"));
    for spelling in [
        r"C:asset.png",
        r"\\server\asset.png",
        "//server/asset.png",
        "https://host/asset.png",
        "C:\\old\\asset.png\0",
        "C:\\old\\",
        r"C:\old\..",
        r"C:\old\bad:name.png",
        r"C:\old\asset.wav",
        r"C:\old\asset.mov",
        r"C:\old\asset.eps",
    ] {
        let project = project(vec![raster_item(2, None, spelling)]);
        let mut resolver = MediaPreflight::with_project(&input, None, &project);
        assert!(
            !resolver.available(&raster_request("image", 2, spelling)),
            "{spelling:?}"
        );
    }
    let spelling = r"C:\old\asset.png";
    let project = project(vec![raster_item(2, None, spelling)]);
    for kind in [
        MediaAssetKind::Audio,
        MediaAssetKind::Video,
        MediaAssetKind::SequenceImage,
    ] {
        let mut resolver = MediaPreflight::with_project(&input, None, &project);
        let mut request = raster_request("image", 2, spelling);
        request.kind = kind;
        assert!(!resolver.available(&request));
    }
}

#[test]
#[cfg(not(windows))]
fn foreign_collected_rejects_unsafe_missing_cyclic_and_ambiguous_native_folders() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    png(&root.path().join("(Footage)/Images/asset.png"));
    let spelling = r"C:\old\asset.png";
    for folder in [
        folder(1, "../Images", None),
        folder(1, "Images/", None),
        folder(1, r"Images\", None),
        folder(1, "Images", Some(1)),
        folder(1, "Images", Some(99)),
    ] {
        let project = project(vec![folder, raster_item(2, Some(1), spelling)]);
        let mut resolver = MediaPreflight::with_project(&input, None, &project);
        assert!(!resolver.available(&raster_request("image", 2, spelling)));
    }
    let project = project(vec![
        folder(1, "Images", None),
        raster_item(2, Some(1), spelling),
        raster_item(3, Some(1), r"D:\other\asset.png"),
    ]);
    let mut resolver = MediaPreflight::with_project(&input, None, &project);
    assert!(!resolver.available(&raster_request("first", 2, spelling)));
    assert!(!resolver.available(&raster_request("second", 3, r"D:\other\asset.png")));
}

#[test]
#[cfg(unix)]
fn foreign_collected_rejects_root_and_descendant_symlink_escapes() {
    use std::os::unix::fs::symlink;
    let outside = tempfile::tempdir().unwrap();
    png(&outside.path().join("asset.png"));
    for root_link in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("input.aep");
        let collected = root.path().join("(Footage)");
        if root_link {
            symlink(outside.path(), &collected).unwrap();
        } else {
            fs::create_dir(&collected).unwrap();
            symlink(
                outside.path().join("asset.png"),
                collected.join("asset.png"),
            )
            .unwrap();
        }
        let spelling = r"C:\old\asset.png";
        let project = project(vec![raster_item(2, None, spelling)]);
        let mut resolver = MediaPreflight::with_project(&input, None, &project);
        assert!(!resolver.available(&raster_request("image", 2, spelling)));
    }
}
