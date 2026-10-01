//! Static Crop records extracted unchanged from an Adobe-native source.

use super::support::*;
use serde_json::json;
use std::io::Read;
use tesseract_file::TesseractFile;

const NATIVE_CROP: &str = include_str!("../fixtures/cap2-native-static-crop.xml");

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
