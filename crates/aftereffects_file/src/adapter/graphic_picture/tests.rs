//! Offline native-source structure/publication coverage, not Adobe pixel proof.

use super::*;
use crate::{aep, graphic_template::SavedGraphicTemplate, rifx::Chunk};
use fx_schema::{Layer, LayerData};

fn descendants<'a>(layers: &'a [Layer], output: &mut Vec<&'a Layer>) {
    for layer in layers {
        output.push(layer);
        if let LayerData::Group(group) = layer.data() {
            descendants(&group.layers, output);
        }
    }
}

#[test]
fn capsule_picture_native_precomps_keep_nested_editable_solids_without_guids() {
    let template = SavedGraphicTemplate::decode(include_bytes!(
        "../../../tests/fixtures/layers/import_precomp_structure.aep"
    ))
    .unwrap();
    let mut picture = template
        .import_editable_picture(Path::new("native.aep"), 31, 500, "capsule-native", &[])
        .unwrap();
    let document = picture.take_document().unwrap();
    let mut layers = Vec::new();
    descendants(document.composition().layers(), &mut layers);
    assert_eq!(
        layers
            .iter()
            .filter(|layer| matches!(layer.data(), LayerData::Group(group)
        if group.name == "SHARED_SOURCE"))
            .count(),
        2
    );
    assert!(
        layers
            .iter()
            .any(|layer| matches!(layer.data(), LayerData::Rect(rect)
        if rect.rect.fill_color.iter().zip([0.9, 0.2, 0.1, 1.0])
            .all(|(actual, expected)| (actual - expected).abs() < 1e-6)))
    );
    assert!(layers.iter().all(|layer| layer.id().value() >= 500));
    assert!(picture.next_id > 500);
    assert!(picture.take_document().is_err());
}

#[test]
fn capsule_picture_native_timing_retains_editable_animation_tracks() {
    let template = SavedGraphicTemplate::decode(include_bytes!(
        "../../../tests/fixtures/layers/import_timing_controls.aep"
    ))
    .unwrap();
    let mut picture = template
        .import_editable_picture(Path::new("native.aep"), 50, 200, "capsule-keys", &[])
        .unwrap();
    let document = picture.take_document().unwrap();
    assert!(!document.composition().dynamics().entries().is_empty());
    let mut layers = Vec::new();
    descendants(document.composition().layers(), &mut layers);
    assert!(
        layers
            .iter()
            .any(|layer| matches!(layer.data(), LayerData::Rect(rect)
        if rect.rect.size == [480.0, 270.0]))
    );
}

#[test]
fn capsule_picture_normalized_psd_lives_through_publication_and_detects_source_drift() {
    fn relink(chunks: &mut [Chunk]) {
        for chunk in chunks {
            if chunk.id() == *b"alas" {
                let mut alias: serde_json::Value =
                    serde_json::from_slice(chunk.data_payload().unwrap()).unwrap();
                alias["fullpath"] = "two_layers.psd".into();
                *chunk = Chunk::data(*b"alas", serde_json::to_vec(&alias).unwrap()).unwrap();
            } else if let Some(children) = chunk.children_mut() {
                relink(children);
            }
        }
    }
    // Supplementary relink only: native PSD selectors and layer content are unchanged.
    let mut native = aep::Project::parse(include_bytes!(
        "../../../tests/fixtures/psd_import/psd_sources_v2.aep"
    ))
    .unwrap();
    relink(&mut native.chunks);
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("native.aep");
    let bytes = native.encode().unwrap();
    std::fs::write(&input, &bytes).unwrap();
    let psd = directory.path().join("(Footage)/two_layers.psd");
    let mut requested = Vec::new();
    let mut stage = |relative: &Path| {
        requested.push(relative.to_owned());
        let destination = directory.path().join(relative);
        std::fs::create_dir_all(destination.parent().unwrap())?;
        std::fs::write(
            destination,
            include_bytes!("../../../tests/fixtures/psd_import/two_layers_v2.psd"),
        )
    };
    let template = SavedGraphicTemplate::decode(&bytes).unwrap();
    let mut picture = template
        .import_editable_picture_with_collected_media(
            &input,
            2,
            100,
            "capsule-psd",
            &[],
            &mut stage,
        )
        .unwrap();
    assert_eq!(requested, [PathBuf::from("(Footage)/two_layers.psd")]);
    assert!(!picture.assets.is_empty());
    let normalized = picture.assets[0].1.clone();
    assert!(normalized.exists());
    assert!(!normalized.starts_with(directory.path()));
    let document = picture.take_document().unwrap();
    let mut builder = tesseract_file::TesseractFileBuilder::from_project_json(
        &serde_json::to_vec(&document).unwrap(),
    )
    .unwrap();
    for (id, path, kind) in &picture.assets {
        builder = builder.add_asset(id.as_str(), path, *kind).unwrap();
    }
    let archive = builder
        .write(directory.path().join("picture.tsrct"))
        .unwrap();
    picture.verify_sources().unwrap();
    picture.verify_packaged(&archive).unwrap();
    let mut changed = std::fs::read(&psd).unwrap();
    *changed.last_mut().unwrap() ^= 1;
    std::fs::write(&psd, changed).unwrap();
    assert!(matches!(
        picture.verify_sources(),
        Err(AepConversionError::MediaChanged(_))
    ));
    drop(picture);
    assert!(!normalized.exists());
}
