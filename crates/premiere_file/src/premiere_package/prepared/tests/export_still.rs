//! Native OpenEXR package regression; host render proof is kept outside git.
use super::*;
use crate::schema::PrMediaKind;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::io::{Cursor, Read as _};

const EXR: &[u8] = include_bytes!(
    "../../../../../aftereffects_file/tests/fixtures/media_native_panel/media/red.exr"
);

fn image_archive(root: &Path, bytes: &[u8]) -> TesseractFile {
    let (image_width, image_height) = crate::image_media::inspect_image_media(Cursor::new(bytes))
        .map(|image| (image.width, image.height))
        .unwrap_or((64, 48));
    let mut value = crate::test_support::editable_document();
    value["composition"]["layers"][0] = json!({
        "type":"Image", "id":1, "name":"Native PNG sibling",
        "activeRange":{"start":0,"duration":1000},
        "transform":{"anchorPoint":[0,0],"position":[0,0],"scale":[100,100],
            "rotation":0,"opacity":100},
        "source":{"assetId":"native-image","fit":"contain",
            "sourceRect":{"x":0,"y":0,"width":1920,"height":1080}}
    });
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(
            0,
            json!({
                "type":"Image", "id":30, "name":"Native OpenEXR still",
                "activeRange":{"start":0,"duration":1000},
                "transform":{"anchorPoint":[0,0],"position":[0,0],"scale":[100,100],
                    "rotation":0,"opacity":100},
                "source":{"assetId":"source-image","fit":"contain",
                    "sourceRect":{"x":0,"y":0,"width":image_width,"height":image_height}}
            }),
        );
    fs::write(root.join("source.exr"), bytes).unwrap();
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .add_asset("source-image", root.join("source.exr"), AssetKind::Image)
        .unwrap()
        .add_asset(
            "native-image",
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/feature_still_transparent.png"),
            AssetKind::Image,
        )
        .unwrap()
        .write(root.join("input.tsrct"))
        .unwrap()
}

fn project_xml(path: &Path) -> String {
    let mut xml = String::new();
    flate2::read::GzDecoder::new(fs::File::open(path).unwrap())
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

fn open_exr_importer_prefs(xml: &str) -> Vec<u8> {
    let field = xml
        .split_once("<ImporterPrefs ")
        .unwrap()
        .1
        .split_once("</ImporterPrefs>")
        .unwrap()
        .0;
    STANDARD
        .decode(field.split_once('>').unwrap().1.trim())
        .unwrap()
}

#[test]
fn prepare_export_omitted_track_matte_still_keeps_native_picture_sibling() {
    let root = tempfile::tempdir().unwrap();
    let image =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/feature_still_transparent.png");
    let mut value = crate::test_support::editable_document();
    let transform = json!({
        "anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100],
        "rotation": 0, "opacity": 100
    });
    let sibling = json!({
        "type": "Image", "id": 30, "name": "Native picture sibling",
        "activeRange": {"start": 0, "duration": 1000},
        "transform": transform,
        "source": {"assetId": "sibling-image", "fit": "contain"}
    });
    let matte = json!({
        "type": "Image", "id": 21, "name": "Unsupported matte source", "parent": 24,
        "activeRange": {"start": 0, "duration": 1000},
        "transform": transform,
        "source": {"assetId": "matte-image", "fit": "contain"}
    });
    let consumer = json!({
        "type": "Image", "id": 20, "name": "Unsupported matte consumer", "parent": 24,
        "activeRange": {"start": 0, "duration": 1000},
        "transform": transform,
        "trackMatte": {"layer": 21, "mode": "luma"},
        "source": {"assetId": "consumer-image", "fit": "contain"}
    });
    value["composition"]["layers"] = json!([
        {
            "type": "Group", "id": 24, "name": "Unsupported still matte pair",
            "playback": crate::test_support::linear_playback(
                json!({"start": 0, "duration": 1000}),
                json!({"start": 0, "duration": 1000}),
            ),
            "transform": transform,
            "layers": [consumer, matte]
        },
        sibling
    ]);
    let input = root.path().join("input.tsrct");
    let file = TesseractFileBuilder::from_project_json(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .add_asset("consumer-image", &image, AssetKind::Image)
        .unwrap()
        .add_asset("matte-image", &image, AssetKind::Image)
        .unwrap()
        .add_asset("sibling-image", &image, AssetKind::Image)
        .unwrap()
        .write(&input)
        .unwrap();
    let original = fs::read(&input).unwrap();
    assert!(file.metadata().assets.contains_key("consumer-image"));

    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    assert!(operation.losses().has_native_content);
    for reason in [
        "masks cannot be exported: a still image exports no Linear Wipe or Track Matte Key; occurrence omitted",
        "track matte source of no exported clip was not exported",
    ] {
        assert!(
            operation
                .losses()
                .diagnostics
                .iter()
                .any(|item| item.reason.contains(reason)),
            "{reason}: {:?}",
            operation.losses()
        );
    }
    let (project, _) = operation.into_native().unwrap();
    assert_eq!(project.media.len(), 1);
    assert_eq!(
        project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .count(),
        1
    );
    assert_eq!(fs::read(&input).unwrap(), original);
}

#[test]
fn export_still_exr_keeps_native_picture_sibling_and_original_identity() {
    let root = tempfile::tempdir().unwrap();
    let file = image_archive(root.path(), EXR);
    let original = fs::read(root.path().join("input.tsrct")).unwrap();
    let source = file.project_json().unwrap();
    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    assert!(operation.losses().has_native_content);
    assert!(
        operation.losses().losses.is_empty(),
        "{:?}",
        operation.losses()
    );
    assert!(operation.packing_recipe().is_complete());
    assert!(operation
        .packing_recipe()
        .boundaries()
        .iter()
        .any(|boundary| boundary.layer == 30.into()));
    let stage = operation
        .stage_with_picture_replacements(root.path(), &root.path().join("native-only"), &[])
        .unwrap();
    let project = read_native(&stage.directory().join("project.prproj"));
    assert_eq!(project.media.len(), 2);
    let exr = project
        .media
        .values()
        .find(|media| media.name == "source.exr")
        .expect("native project retains OpenEXR media");
    assert!(matches!(
        exr.video.as_ref().unwrap().kind,
        PrMediaKind::OpenExr {
            alpha: false,
            numbered: false,
            channels: crate::schema::OpenExrChannels::Unspecified,
        }
    ));
    assert_eq!(
        project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .count(),
        2
    );
    assert_eq!(
        fs::read(stage.directory().join("media/source.exr")).unwrap(),
        EXR
    );
    assert_eq!(
        fs::read(
            stage
                .directory()
                .join("media/feature_still_transparent.png")
        )
        .unwrap(),
        include_bytes!("../../../../tests/fixtures/feature_still_transparent.png")
    );
    let xml = project_xml(&stage.directory().join("project.prproj"));
    assert!(xml.contains("<CodecType>1281443650</CodecType>"));
    assert!(!xml.contains(".aep"));
    let prefs = open_exr_importer_prefs(&xml);
    assert_eq!(prefs.len(), 1040);
    assert_eq!(&prefs[..6], b"oEXR\x01\x01");
    assert_eq!(&prefs[16..17], b"R");
    assert_eq!(&prefs[272..273], b"G");
    assert_eq!(&prefs[528..529], b"B");
    assert_eq!(&prefs[784..790], b"(none)");
    assert_eq!(file.project_json().unwrap(), source);
    assert_eq!(fs::read(root.path().join("input.tsrct")).unwrap(), original);
}

#[test]
fn export_still_rgba_exr_writes_native_black_matte_alpha_and_channel_mapping() {
    let root = tempfile::tempdir().unwrap();
    let bytes = crate::test_support::half_rgba_openexr();
    let file = image_archive(root.path(), &bytes);
    let operation = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap();
    assert!(operation.losses().losses.is_empty());
    let stage = operation
        .stage_with_picture_replacements(root.path(), &root.path().join("native-rgba"), &[])
        .unwrap();

    let project = read_native(&stage.directory().join("project.prproj"));
    let exr = project
        .media
        .values()
        .find(|media| media.name == "source.exr")
        .unwrap();
    assert!(matches!(
        exr.video.as_ref().unwrap().kind,
        PrMediaKind::OpenExr {
            alpha: true,
            numbered: false,
            channels: crate::schema::OpenExrChannels::Unspecified,
        }
    ));
    let xml = project_xml(&stage.directory().join("project.prproj"));
    assert!(xml.contains("<AlphaType>2</AlphaType>"));
    let prefs = open_exr_importer_prefs(&xml);
    assert_eq!(prefs.len(), 1040);
    assert_eq!(&prefs[784..785], b"A");
    assert!(prefs[785..1040].iter().all(|byte| *byte == 0));
    assert_eq!(
        fs::read(stage.directory().join("media/source.exr")).unwrap(),
        bytes
    );
}

#[test]
fn export_still_exr_truncated_pixels_remain_fatal() {
    let root = tempfile::tempdir().unwrap();
    let file = image_archive(root.path(), &EXR[..EXR.len() / 2]);
    let error = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap_err();
    assert!(error.to_string().contains("source-image"), "{error}");
    assert!(
        error
            .to_string()
            .contains("OpenEXR structure cannot be read"),
        "{error}"
    );
}

#[test]
fn export_still_exr_corrupt_source_hash_remains_fatal() {
    let root = tempfile::tempdir().unwrap();
    let file = image_archive(root.path(), EXR);
    let path = root.path().join("input.tsrct");
    let mut bytes = fs::read(&path).unwrap();
    let start = bytes
        .windows(EXR.len())
        .position(|part| part == EXR)
        .unwrap();
    bytes[start + EXR.len() - 1] ^= 1;
    fs::write(path, bytes).unwrap();
    let error = Premiere
        .prepare_export(&file, file.project(), &Default::default())
        .unwrap_err();
    assert!(error.to_string().contains("source-image"), "{error}");
}
