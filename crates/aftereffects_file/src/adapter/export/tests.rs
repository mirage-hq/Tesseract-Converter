use super::*;
use crate::{
    AfterEffectsImportOptions,
    properties::read_transform,
    structure::{ItemKind, read_project},
};
use fx_conv::ImportToTesseract;
use fx_schema::{EditableFxCompositionDocument, LayerData};
use serde_json::json;
use sha2::{Digest, Sha256};

mod audio;
mod ntsc_clock;
mod pr4442_package_cases;
mod review_regressions;

fn native_layer_named<'a>(
    project: &'a crate::structure::StructuralProject,
    layers: &'a [crate::structure::Layer],
    name: &str,
) -> Option<&'a crate::structure::Layer> {
    layers.iter().find_map(|layer| {
        if layer.name.as_ref() == name {
            return Some(layer);
        }
        let item = project.item(layer.record.source_id())?;
        let ItemKind::Composition(composition) = &item.kind else {
            return None;
        };
        native_layer_named(project, &composition.layers, name)
    })
}

fn rect_with_size(layers: &[fx_schema::Layer], size: [f64; 2]) -> Option<&fx_schema::RectLayer> {
    layers.iter().find_map(|layer| match layer.data() {
        LayerData::Rect(rect) if rect.rect.size == size => Some(rect),
        LayerData::Group(group) => rect_with_size(&group.layers, size),
        _ => None,
    })
}

fn import_solid(root: &Path) -> std::path::PathBuf {
    let source = root.join("source.aep");
    let bytes = include_bytes!("../../../tests/fixtures/properties/transform_unseparated.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "1004b3ee82efd5e24b90ff67d8dd8c89a92c9a5ae554d69d96dc8847cdc61537"
    );
    fs::write(&source, bytes).unwrap();
    AfterEffects
        .import_to_tesseract(
            &source,
            &root.join("imported"),
            &AfterEffectsImportOptions {
                composition: Some(1),
                ..Default::default()
            },
            ConversionMode::Write,
        )
        .unwrap();
    fs::remove_file(source).unwrap();
    root.join("imported/project.tsrct")
}

#[test]
fn native_import_archive_edit_fresh_export_preserves_current_values() {
    let tmp = tempfile::tempdir().unwrap();
    let input = import_solid(tmp.path());
    let mut archive = TesseractFile::open(&input).unwrap();
    let mut value = archive.project_json().unwrap();
    let occurrence = &mut value["composition"]["layers"][0]["layers"][0];
    occurrence["transform"]["anchorPoint"] = json!([23.0, 31.0]);
    occurrence["transform"]["position"] = json!([230.0, -15.0]);
    occurrence["transform"]["scale"] = json!([-50.0, 125.0]);
    occurrence["transform"]["rotation"] = json!(17.0);
    occurrence["transform"]["opacity"] = json!(62.0);
    // Native constant Solids now import without sampled-source keys. Explicitly
    // edit this supplementary FX fixture's source clock to keep this test on
    // its native Solid hierarchy profile rather than vector normalization.
    occurrence["layers"][0]["playback"]["mapping"] = json!({
        "type": "timeRemap", "property": {
            "keyframes": [
                {"id": "solid-start", "time": 0, "value": 0, "easing": {"type": "linear"}},
                {"id": "solid-end", "time": 30000, "value": 30000, "easing": {"type": "linear"}}
            ],
            "before": "inactive", "after": "inactive"
        }
    });
    let rect = &mut occurrence["layers"][0]["layers"][0];
    rect["name"] = json!("Edited source Ω");
    rect["rect"]["position"] = json!([3.0, 4.0]);
    rect["rect"]["size"] = json!([128.0, 64.0]);
    rect["rect"]["fillColor"] = json!([0.25, 0.5, 0.75, 1.0]);
    archive
        .replace_project(EditableFxCompositionDocument::from_json_value(value).unwrap())
        .unwrap();
    archive.save().unwrap();
    drop(archive);

    let output = tmp.path().join("exported");
    let before = fs::read_dir(tmp.path()).unwrap().count();
    let check = AfterEffects
        .export_from_tesseract(&input, &output, &Default::default(), ConversionMode::Check)
        .unwrap();
    assert!(!output.exists());
    assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), before);
    let written = AfterEffects
        .export_from_tesseract(&input, &output, &Default::default(), ConversionMode::Write)
        .unwrap();
    assert_eq!(check, written);
    let native = read_project(&fs::read(output.join("project.aep")).unwrap()).unwrap();
    let ItemKind::Composition(comp) = &native.item(1).unwrap().kind else {
        panic!("composition")
    };
    assert_eq!(comp.layers.len(), 1, "{:?}", written.diagnostics);
    assert_eq!((comp.width, comp.height), (1920, 1080));
    assert_eq!(comp.duration_secs, 30.0);
    let root = &comp.layers[0];
    assert_eq!(root.name.as_ref(), "Comp 1");
    assert!(matches!(
        native.item(root.record.source_id()).map(|item| &item.kind),
        Some(ItemKind::Composition(_))
    ));
    let layer = native_layer_named(&native, &comp.layers, "Edited source Ω")
        .expect("edited Solid survives in the generated native hierarchy");
    let source = native
        .item(layer.record.source_id())
        .unwrap()
        .solid
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!((source.width, source.height), (128, 64));
    assert_eq!(source.color, [0.25, 0.5, 0.75]);

    let occurrence = native_layer_named(&native, &comp.layers, "Gray Solid 1")
        .expect("edited occurrence Transform survives the generated native hierarchy");
    let properties = read_transform(&occurrence.content).unwrap();
    // Generated precomposition anchors are stored relative to the 128×64 source,
    // not as absolute pixel coordinates.
    for (name, expected) in [
        ("ADBE Anchor Point", vec![20.0 / 128.0, 27.0 / 64.0, 0.0]),
        ("ADBE Position", vec![230.0, -15.0, 0.0]),
        ("ADBE Scale", vec![-0.5, 1.25, 1.0]),
        ("ADBE Rotate Z", vec![17.0]),
        ("ADBE Opacity", vec![0.62]),
    ] {
        assert_eq!(
            properties
                .iter()
                .find(|property| property.match_name == name)
                .unwrap()
                .numeric
                .as_ref()
                .unwrap()
                .values,
            expected
        );
    }

    let root_properties = read_transform(&root.content).unwrap();
    let root_anchor = &root_properties
        .iter()
        .find(|property| property.match_name == "ADBE Anchor Point")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap()
        .values;
    let occurrence_position = &properties
        .iter()
        .find(|property| property.match_name == "ADBE Position")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap()
        .values;
    let Some(ItemKind::Composition(root_source)) =
        native.item(root.record.source_id()).map(|item| &item.kind)
    else {
        panic!("root occurrence must reference a generated precomposition");
    };
    // The untransformed root Group uses the final-root output viewport, so its
    // precomposition is the composition canvas and needs no anchor offset.
    assert_eq!(root_anchor, &[0.0, 0.0, 0.0]);
    // The native anchor uses source-relative units; convert it back to pixels
    // before comparing the edited occurrence position in composition space.
    assert_eq!(root_anchor[2], 0.0);
    assert_eq!(occurrence_position[2], 0.0);
    let anchor_px = [
        root_anchor[0] * f64::from(root_source.width),
        root_anchor[1] * f64::from(root_source.height),
    ];
    assert!((occurrence_position[0] - anchor_px[0] - 230.0).abs() < 1e-9);
    assert!((occurrence_position[1] - anchor_px[1] + 15.0).abs() < 1e-9);

    let reimported =
        crate::structure_document::to_structural_fx_document(&native, Some(1)).unwrap();
    let layers = reimported.document.composition().layers();
    let rect = rect_with_size(layers, [128.0, 64.0]).expect("edited Solid reimports as Rect");
    // Solid geometry is normalized to local zero; the authored [3, 4]
    // offset is retained by the native anchor [23 - 3, 31 - 4] above.
    assert_eq!(rect.rect.position, [0.0, 0.0]);
    assert_eq!(rect.rect.size, [128.0, 64.0]);
    assert_eq!(rect.rect.fill_color, [0.25, 0.5, 0.75, 1.0]);
    assert!(reimported.document.composition().dynamics().is_empty());
    assert!(fs::read_dir(tmp.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".conversion-")
    }));
}

#[test]
fn export_options_apply_fps_in_check_and_write_without_publishing_invalid_rates() {
    let tmp = tempfile::tempdir().unwrap();
    let input = import_solid(tmp.path());
    let output = tmp.path().join("rated");
    let options = AfterEffectsExportOptions { fps: 29.97 };
    let check = AfterEffects
        .export_from_tesseract(&input, &output, &options, ConversionMode::Check)
        .unwrap();
    assert!(!output.exists());
    let write = AfterEffects
        .export_from_tesseract(&input, &output, &options, ConversionMode::Write)
        .unwrap();
    assert_eq!(check, write);
    let native = read_project(&fs::read(output.join("project.aep")).unwrap()).unwrap();
    let ItemKind::Composition(comp) = &native.item(1).unwrap().kind else {
        panic!("composition")
    };
    assert!((comp.frame_rate - 29.97).abs() < 0.00001);
    for fps in [0.0, f64::NAN, 241.0] {
        let invalid = tmp.path().join("invalid");
        assert!(
            AfterEffects
                .export_from_tesseract(
                    &input,
                    &invalid,
                    &AfterEffectsExportOptions { fps },
                    ConversionMode::Write
                )
                .is_err()
        );
        assert!(!invalid.exists());
    }
}

#[test]
fn rejected_shape_has_check_write_parity_and_keeps_sibling() {
    let tmp = tempfile::tempdir().unwrap();
    let input = import_solid(tmp.path());
    let mut archive = TesseractFile::open(&input).unwrap();
    let mut value = archive.project_json().unwrap();
    let mut solid =
        value["composition"]["layers"][0]["layers"][0]["layers"][0]["layers"][0].clone();
    solid["parent"] = serde_json::Value::Null;
    let mut shape = solid.clone();
    shape["id"] = json!(100);
    shape["type"] = json!("Shape");
    shape["name"] = json!("x".repeat(256));
    shape.as_object_mut().unwrap().remove("rect");
    shape["shape"] = json!({"path":{"commands":[]}, "ellipse":{"size":[100.0,50.0],"position":[0.0,0.0]},
        "fills":[{"paint":{"type":"solid","color":[1.0,0.0,0.0,1.0]}}]});
    value["composition"]["layers"] = json!([shape, solid]);
    archive
        .replace_project(EditableFxCompositionDocument::from_json_value(value).unwrap())
        .unwrap();
    archive.save().unwrap();
    let output = tmp.path().join("exported");
    let check = AfterEffects
        .export_from_tesseract(&input, &output, &Default::default(), ConversionMode::Check)
        .unwrap();
    assert!(!output.exists());
    let write = AfterEffects
        .export_from_tesseract(&input, &output, &Default::default(), ConversionMode::Write)
        .unwrap();
    assert_eq!(check, write);
    assert!(write.diagnostics.iter().any(
        |d| d.layer_id == Some(fx_schema::LayerId::new(100)) && d.message.contains("name must")
    ));
    let native = read_project(&fs::read(output.join("project.aep")).unwrap()).unwrap();
    let ItemKind::Composition(comp) = &native.item(1).unwrap().kind else {
        panic!("composition")
    };
    assert_eq!(comp.layers.len(), 1);
}

#[test]
fn malformed_archive_and_existing_destination_are_never_published_or_replaced() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("bad.tsrct");
    fs::write(&input, b"not a zip").unwrap();
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        let output = tmp.path().join("output");
        assert!(
            AfterEffects
                .export_from_tesseract(&input, &output, &Default::default(), mode)
                .is_err()
        );
        assert!(!output.exists());
    }
    let input = import_solid(tmp.path());
    let output = tmp.path().join("output");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("project.aep"), b"keep").unwrap();
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        assert!(
            AfterEffects
                .export_from_tesseract(&input, &output, &Default::default(), mode)
                .is_err()
        );
        assert_eq!(fs::read(output.join("project.aep")).unwrap(), b"keep");
    }
}

// Supplementary publication regressions, authored without execution. These do not
// establish Adobe acceptance of any native media records.
#[test]
fn export_package_publishes_current_project_and_owned_media() {
    use crate::writer::footage::RelativeMediaPath;

    let tmp = tempfile::tempdir().unwrap();
    let staged = tmp.path().join("staged");
    fs::create_dir_all(staged.join("media")).unwrap();
    fs::write(staged.join("project.aep"), b"fresh project").unwrap();
    fs::write(staged.join("media/asset.wav"), b"current asset").unwrap();
    let media = RelativeMediaPath::new("media/asset.wav").unwrap();
    let output = tmp.path().join("output");

    package::publish_package(&staged, &output, &[media], &[]).unwrap();

    assert_eq!(
        fs::read(output.join("project.aep")).unwrap(),
        b"fresh project"
    );
    assert_eq!(
        fs::read(output.join("media/asset.wav")).unwrap(),
        b"current asset"
    );
}

#[test]
fn export_package_rolls_back_owned_files_when_a_later_media_file_is_missing() {
    use crate::writer::footage::RelativeMediaPath;

    let tmp = tempfile::tempdir().unwrap();
    let staged = tmp.path().join("staged");
    fs::create_dir_all(staged.join("media")).unwrap();
    fs::write(staged.join("project.aep"), b"fresh project").unwrap();
    fs::write(staged.join("media/present.wav"), b"current asset").unwrap();
    let files = [
        RelativeMediaPath::new("media/present.wav").unwrap(),
        RelativeMediaPath::new("media/missing.wav").unwrap(),
    ];
    let output = tmp.path().join("output");

    assert!(package::publish_package(&staged, &output, &files, &[]).is_err());
    assert!(!output.exists());
    assert_eq!(
        fs::read(staged.join("media/present.wav")).unwrap(),
        b"current asset"
    );
}

#[test]
fn export_publisher_preserves_late_destination_and_cleans_failed_owned_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("output");
    let destination = fresh_destination(&output).unwrap();
    fs::create_dir(&output).unwrap();
    fs::write(output.join("user-file"), b"keep").unwrap();
    assert!(package::publish_package(&tmp.path().join("missing"), &destination, &[], &[]).is_err());
    assert_eq!(fs::read(output.join("user-file")).unwrap(), b"keep");
    let fresh = tmp.path().join("fresh");
    assert!(package::publish_package(&tmp.path().join("missing"), &fresh, &[], &[]).is_err());
    assert!(!fresh.exists());
}

#[cfg(unix)]
#[test]
fn export_does_not_follow_dangling_destination_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    let input = import_solid(tmp.path());
    let output = tmp.path().join("link");
    let target = tmp.path().join("missing");
    std::os::unix::fs::symlink(&target, &output).unwrap();
    for mode in [ConversionMode::Check, ConversionMode::Write] {
        assert!(
            AfterEffects
                .export_from_tesseract(&input, &output, &Default::default(), mode)
                .is_err()
        );
        assert!(
            fs::symlink_metadata(&output)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(!target.exists());
    }
}
