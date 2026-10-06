//! Static Crop records extracted unchanged from an Adobe-native source.

use super::support::*;
#[cfg(feature = "ffmpeg-library")]
use serde_json::json;
#[cfg(feature = "ffmpeg-library")]
use std::io::Read;
#[cfg(feature = "ffmpeg-library")]
use tesseract_file::TesseractFile;

const NATIVE_CROP: &str = include_str!("../fixtures/cap2-native-static-crop.xml");
// Native-derived eleven-key Hold schedule for a right-edge reveal.
const HOLD_RIGHT_REVEAL_KEYS: &str = "0,100.,4,0,0,0.16666666666666666,0,0.33333333333333331;16951334400,90.,4,0,-59.940059940059939,0.16666666666666666,0,0.33333333333333331;33902668800,80.,4,0,-59.940059940059939,0.16666666666666666,0,0.33333333333333331;50854003200,70.,4,0,-59.94005994005996,0.16666666666666666,0,0.33333333333333331;67805337600,60.,4,0,-59.940059940059918,0.16666666666666666,0,0.33333333333333331;84756672000,50.,4,0,-59.94005994005996,0.16666666666666666,0,0.33333333333333331;101708006400,40.,4,0,-59.94005994005996,0.16666666666666666,0,0.33333333333333331;118659340800,30.,4,0,-59.940059940059918,0.16666666666666666,0,0.33333333333333331;135610675200,20.,4,0,-59.940059940059918,0.16666666666666666,0,0.33333333333333331;152562009600,10.,4,0,-59.940059940059918,0.16666666666666666,0,0.33333333333333331;169513344000,0.,4,0,-59.940059940060003,0.16666666666666666,0,0.33333333333333331;";

fn crop_xml(component: u32, records: &str) -> String {
    one_second()
        .replace(
            "<DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/>",
            &format!("<DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"{component}\"/></Components></ComponentChain>"),
        )
        .replace(
            "</PremiereData>",
            &records.replace("<PremiereData Version=\"3\">", ""),
        )
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn native_static_crop_uses_authored_start_instead_of_stale_current_value() {
    // Both source placements save Left=0 with a stale CurrentValue=99.
    for component in [403, 405] {
        let dir = tempfile::tempdir().unwrap();
        let source = fixture(dir.path(), &crop_xml(component, NATIVE_CROP));
        let output = dir.path().join("converted");
        let omissions = premiere_to_tesseract(&source, &output, None, false).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let file = TesseractFile::open(first_project(&output)).unwrap();
        let document = file.project_json().unwrap();
        let layers = document["composition"]["layers"].as_array().unwrap();
        let video = layers
            .iter()
            .find(|layer| layer["type"] == "Video")
            .unwrap();
        let guide = layers
            .iter()
            .find(|layer| layer["id"] == video["masks"][0]["layer"])
            .unwrap();
        assert_eq!(
            video["playback"]["inputRange"],
            json!({"start": 0, "duration": 1000})
        );
        assert_eq!(video["sourceRange"], json!({"start": 0, "duration": 1000}));
        assert_eq!(video["masks"][0]["mode"], "add");
        assert_eq!(video["masks"][0]["feather"], json!([0.0, 0.0]));
        assert_eq!(guide["rect"]["position"], json!([0.0, 0.0]));
        let size = &guide["rect"]["size"];
        assert!((size[0].as_f64().unwrap() - 977.1345703124928).abs() < 1e-8);
        assert_eq!(size[1], 1080.0);
        let asset = video["source"]["assetId"].as_str().unwrap();
        let mut packaged = Vec::new();
        file.asset(asset)
            .unwrap()
            .open()
            .unwrap()
            .read_to_end(&mut packaged)
            .unwrap();
        assert_eq!(packaged, MEDIA);
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn native_animated_right_crop_keeps_source_reveal_and_unaffected_canvas_sibling() {
    let records = NATIVE_CROP
        .replace(
            "<StartKeyframe>-91445760000000000,49.107574462891,0,0,0,0,0,0</StartKeyframe>",
            &format!("<StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><Keyframes>{HOLD_RIGHT_REVEAL_KEYS}</Keyframes>"),
        );
    let dir = tempfile::tempdir().unwrap();
    let source = fixture(dir.path(), &crop_xml(403, &records));
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(&source, &output, None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");

    let file = TesseractFile::open(first_project(&output)).unwrap();
    let document = file.project_json().unwrap();
    let layers = document["composition"]["layers"].as_array().unwrap();
    let video = layers
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    let guide = layers
        .iter()
        .find(|layer| layer["id"] == video["masks"][0]["layer"])
        .unwrap();
    let canvas = layers
        .iter()
        .find(|layer| layer["name"] == "Premiere black canvas")
        .unwrap();

    assert_eq!(video["sourceRange"], json!({"start": 0, "duration": 1000}));
    assert_eq!(video["masks"][0]["mode"], "add");
    assert_eq!(guide["transform"]["anchorPoint"], json!([0.0, 0.0]));
    assert_eq!(guide["transform"]["position"], json!([0.0, 0.0]));
    assert_eq!(guide["transform"]["scale"], json!([0.0, 100.0]));
    assert_eq!(canvas["rect"]["size"], json!([1920.0, 1080.0]));

    let entry = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["target"]["layerId"] == guide["id"])
        .unwrap();
    assert_eq!(entry["target"]["propertyType"], "scaleX");
    let keys = entry["animator"]["keyframes"].as_array().unwrap();
    assert_eq!(keys.len(), 11);
    for (index, expected) in [(0, (0, 0.0)), (5, (334, 50.0)), (10, (667, 100.0))] {
        assert_eq!(keys[index]["layerTime"], expected.0);
        assert_eq!(keys[index]["value"]["value"], expected.1);
    }
    assert_eq!(keys[1]["easing"]["type"], "hold");

    let asset = video["source"]["assetId"].as_str().unwrap();
    let mut packaged = Vec::new();
    file.asset(asset)
        .unwrap()
        .open()
        .unwrap()
        .read_to_end(&mut packaged)
        .unwrap();
    assert_eq!(packaged, MEDIA);
}

#[test]
fn native_static_crop_still_rejects_invalid_authored_values_and_animation() {
    for (from, to, reason) in [
        (
            "-91445760000000000,0.,0,0,0,0,0,0",
            "-91445760000000000,99.,0,0,0,0,0,0",
            "Crop",
        ),
        (
            "-91445760000000000,0.,0,0,0,0,0,0",
            "-91445760000000000,NaN,0,0,0,0,0,0",
            "nonfinite initial value",
        ),
        (
            "<ParameterID>1</ParameterID>",
            "<IsTimeVarying>true</IsTimeVarying><ParameterID>1</ParameterID>",
            "animated or malformed Crop Left",
        ),
        (
            "-91445760000000000,false,0,0,0,0,0,0",
            "-91445760000000000,true,0,0,0,0,0,0",
            "Crop Zoom is unsupported",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let records = NATIVE_CROP.replace(from, to);
        let source = fixture(dir.path(), &crop_xml(403, &records));
        let error =
            premiere_to_tesseract(source, dir.path().join("converted"), None, false).unwrap_err();
        assert!(error.to_string().contains(reason), "{error}");
    }
}
