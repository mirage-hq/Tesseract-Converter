//! Adobe-authored source, relinked only to the identical pinned PSD bytes.
//! These CPU assertions establish editable structure and PNG content, not Adobe
//! RGB/alpha equivalence. See the fixture README for independent evidence.

use super::*;
use crate::{aep, media::PhotoshopSource, rifx::Chunk, structure::read_project};
use fx_schema::{GroupLayer, Layer, LayerData, layer::Position};

const NATIVE: &[u8] = include_bytes!("../../../tests/fixtures/psd_import/psd_sources_v2.aep");
const PSD: &[u8] = include_bytes!("../../../tests/fixtures/psd_import/two_layers_v2.psd");

fn group(layer: &Layer) -> &GroupLayer {
    let LayerData::Group(group) = layer.data() else {
        panic!("expected editable AE group")
    };
    group
}

fn relink(chunks: &mut [Chunk]) {
    for chunk in chunks {
        if chunk.id() == *b"alas" {
            let mut alias: serde_json::Value =
                serde_json::from_slice(chunk.data_payload().unwrap()).unwrap();
            assert!(
                alias["fullpath"]
                    .as_str()
                    .unwrap()
                    .ends_with("/two_layers_v2.psd")
            );
            alias["fullpath"] = "two_layers.psd".into();
            *chunk = Chunk::data(*b"alas", serde_json::to_vec(&alias).unwrap()).unwrap();
        } else if let Some(children) = chunk.children_mut() {
            relink(children);
        }
    }
}

fn import(
    composition: u32,
    media: &[u8],
) -> (
    tempfile::TempDir,
    TesseractFile,
    Vec<crate::ImportDiagnostic>,
) {
    let mut native = aep::Project::parse(NATIVE).unwrap();
    relink(&mut native.chunks);
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("input.aep");
    fs::write(&input, native.encode().unwrap()).unwrap();
    fs::write(root.path().join("two_layers.psd"), media).unwrap();
    let output = root.path().join("output");
    let options = AfterEffectsImportOptions {
        composition: Some(composition),
        ..Default::default()
    };
    let checked = AfterEffects
        .import_to_tesseract(&input, &output, &options, ConversionMode::Check)
        .unwrap();
    assert!(!output.exists(), "check never publishes a directory");
    let written = AfterEffects
        .import_to_tesseract(&input, &output, &options, ConversionMode::Write)
        .unwrap();
    assert_eq!(
        checked, written,
        "normalization diagnostics must be deterministic"
    );
    let archive = TesseractFile::open(output.join(OUTPUT_NAME)).unwrap();
    (root, archive, written.diagnostics)
}

fn pixels(archive: &TesseractFile, item: u32) -> image::RgbaImage {
    let asset = archive.asset(&format!("aep-local-item-{item}")).unwrap();
    assert_eq!(asset.descriptor().content_type, "image/png");
    let bytes = asset.read_verified_bytes(1024 * 1024).unwrap();
    image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .unwrap()
        .into_rgba8()
}

#[test]
fn adobe_psd_native_selectors_distinguish_merged_and_cropped_layers() {
    assert_eq!(
        format!("{:x}", Sha256::digest(NATIVE)),
        "d5917214c2a9a5e7d5c741ee1f60ebf6d199a37a7c8311a9eb3e83dc3ac8efdb"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(PSD)),
        "f0f1828ad0f5e50d9c6bc5e8539c1a3b3a137fade695cf389f0155a341f8027f"
    );
    let native = read_project(NATIVE).unwrap();
    for (item, expected, dimensions) in [
        (1, PhotoshopSource::Merged, [64, 48]),
        (28, PhotoshopSource::Layer { id: 101, index: 0 }, [20, 16]),
        (30, PhotoshopSource::Layer { id: 202, index: 1 }, [32, 20]),
    ] {
        let source = native
            .items
            .iter()
            .find(|source| source.id == item)
            .unwrap();
        let descriptor = source.media.as_ref().unwrap().as_ref().unwrap();
        assert_eq!(descriptor.photoshop_source, Some(expected));
        assert_eq!([descriptor.width, descriptor.height], dimensions);
    }
}

#[test]
fn adobe_psd_merged_import_packages_png_and_preserves_editable_image() {
    let (_root, archive, diagnostics) = import(2, PSD);
    assert_eq!(archive.metadata().assets.len(), 1);
    let image = pixels(&archive, 1);
    assert_eq!(image.dimensions(), (64, 48));
    assert_eq!(image.get_pixel(0, 0).0, [0, 0, 0, 0]);
    assert_eq!(image.get_pixel(12, 12).0, [255, 32, 16, 255]);
    assert_eq!(image.get_pixel(40, 30).0, [16, 64, 255, 128]);
    let root = group(&archive.project().composition().layers()[0]);
    let occurrence = group(&root.layers[0]);
    assert_eq!(occurrence.transform.position, Position::TwoD([32.0, 24.0]));
    assert_eq!(occurrence.transform.anchor_point, [32.0, 24.0]);
    let content = group(&occurrence.layers[0]);
    assert!(matches!(content.layers[0].data(), LayerData::Image(_)));
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("PSD Merged rasterized to PNG"))
    );
}

#[test]
fn adobe_psd_selected_layers_import_distinct_cropped_pngs_and_native_placement() {
    let (_root, archive, diagnostics) = import(16, PSD);
    assert_eq!(archive.metadata().assets.len(), 2);
    let blue = pixels(&archive, 30);
    let red = pixels(&archive, 28);
    assert_eq!(blue.dimensions(), (32, 20));
    assert_eq!(red.dimensions(), (20, 16));
    assert!(blue.pixels().all(|p| p.0 == [16, 64, 255, 128]));
    assert!(red.pixels().all(|p| p.0 == [255, 32, 16, 255]));
    let root = group(&archive.project().composition().layers()[0]);
    assert_eq!(root.layers.len(), 2);
    for (layer, name, position, anchor) in [
        (&root.layers[0], "Blue", [40.0, 30.0], [16.0, 10.0]),
        (&root.layers[1], "Red", [20.0, 16.0], [10.0, 8.0]),
    ] {
        let occurrence = group(layer);
        assert_eq!(occurrence.name, name);
        assert_eq!(occurrence.transform.position, Position::TwoD(position));
        assert_eq!(occurrence.transform.anchor_point, anchor);
        assert_eq!(occurrence.transform.scale, [100.0, 100.0]);
        assert_eq!(occurrence.transform.opacity.value(), 100.0);
        let content = group(&occurrence.layers[0]);
        assert!(matches!(content.layers[0].data(), LayerData::Image(_)));
    }
    assert_eq!(
        diagnostics
            .iter()
            .filter(|d| d.message.contains("rasterized to PNG"))
            .count(),
        2
    );
}

#[test]
fn psd_unknown_selected_layer_preserves_convertible_sibling_without_composite_fallback() {
    // Supplementary corruption test: only Red's persistent ID is changed in
    // the media. The independently authored AEP is unchanged except relinking.
    let mut modified = PSD.to_vec();
    let position = modified.windows(4).position(|b| b == b"lyid").unwrap();
    modified[position + 8..position + 12].copy_from_slice(&999_u32.to_be_bytes());
    let (_root, archive, diagnostics) = import(16, &modified);
    assert_eq!(archive.metadata().assets.len(), 1);
    assert_eq!(pixels(&archive, 30).dimensions(), (32, 20));
    assert!(archive.asset("aep-local-item-28").is_err());
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("no whole-image substitution"))
    );
}

#[test]
fn psd_invalid_bytes_leave_diagnosed_unbound_content_not_an_unreadable_asset() {
    let (_root, archive, diagnostics) = import(2, b"8BPS broken");
    assert!(archive.metadata().assets.is_empty());
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("cannot be normalized"))
    );
}
