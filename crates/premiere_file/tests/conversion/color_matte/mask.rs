use super::*;

const MASK_CONTROLS: &str = include_str!("../../fixtures/color-matte-mask/native-controls.xml");

fn masked_matte() -> String {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut xml = read_xml(&fixtures.join(FIXTURE));
    edit_record(
        &mut xml,
        "<VideoComponentChain ObjectID=\"156\"",
        "</VideoComponentChain>",
        |record| {
            record.replace("<Components><Component Index=\"0\" ObjectRef=\"403\"/></Components>", "")
            .replace("<DefaultOpacity>true</DefaultOpacity>", "")
            .replace("<ComponentChain Version=\"3\">", "<ComponentChain Version=\"3\"><Components><Component Index=\"0\" ObjectRef=\"21203\"/></Components>")
        },
    );
    xml.replace(
        "</PremiereData>",
        &MASK_CONTROLS.replace("<PremiereData Version=\"3\">", ""),
    )
}

fn convert_mask(xml: &str) -> (Value, Vec<premiere_file::Omission>) {
    let dir = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let bytes = std::fs::read(fixtures.join("video-30fps-10s.mp4")).unwrap();
    std::fs::write(dir.path().join("video-30fps-10s.mp4"), &bytes).unwrap();
    let source = dir.path().join("project.prproj");
    write_prproj(&source, xml);
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(&source, &output, Some(SEQUENCE_UID), false).unwrap();
    let file = TesseractFile::open(first_project(&output)).unwrap();
    assert_eq!(file.metadata().assets.len(), 1);
    assert_eq!(
        file.asset(file.metadata().assets.keys().next().unwrap())
            .unwrap()
            .read_verified_bytes(bytes.len() as u64)
            .unwrap(),
        bytes
    );
    (file.project_json().unwrap(), omissions)
}

fn red(document: &Value) -> &Value {
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["rect"]["fillColor"] == json!([1.0, 0.0, 0.0, 1.0]))
        .expect("native red matte must remain editable and masked")
}

fn assert_guide<'a>(document: &'a Value, owner: &Value, inverted: bool, feather: f64) -> &'a Value {
    assert_eq!(owner["type"], "Rect");
    assert_eq!(owner["activeRange"], json!({"start":2000,"duration":3000}));
    let masks = owner["masks"].as_array().unwrap();
    assert_eq!(masks.len(), 1);
    let mask = &masks[0];
    assert_eq!(mask["mode"], "add");
    assert_eq!(mask["inverted"], inverted);
    assert_eq!(mask["feather"], json!([feather, feather]));
    assert_eq!(mask["expansion"], 0.0);
    assert_eq!(mask["opacity"], 1.0);
    let guide = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == mask["layer"])
        .unwrap();
    assert_eq!(guide["type"], "Shape");
    assert_eq!(guide["activeRange"], owner["activeRange"]);
    assert!(guide["shape"]["fills"].as_array().is_none_or(Vec::is_empty));
    assert!(guide["shape"]["strokes"]
        .as_array()
        .is_none_or(Vec::is_empty));
    assert!(guide["shape"]["path"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .any(|command| command["type"] == "close"));
    guide
}

#[test]
fn native_color_matte_opacity_mask_keeps_coverage_path_keys_and_siblings() {
    for inverted in [false, true] {
        let mut xml = masked_matte();
        if inverted {
            edit_record(
                &mut xml,
                "<VideoComponentParam ObjectID=\"21199\"",
                "</VideoComponentParam>",
                |record| {
                    record.replace(
                        "-91445760000000000,false,0,0,0,0,0,0",
                        "-91445760000000000,true,0,0,0,0,0,0",
                    )
                },
            );
        }
        let (document, omissions) = convert_mask(&xml);
        assert!(
            !omissions
                .iter()
                .any(|o| o.scope == premiere_file::OmissionScope::Occurrence),
            "{omissions:?}"
        );
        let owner = red(&document);
        let guide = assert_guide(&document, owner, inverted, 321.0);
        assert_eq!(guide["transform"], owner["transform"]);
        let entries = document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap();
        let path = entries
            .iter()
            .find(|entry| {
                entry["target"]["layerId"] == guide["id"]
                    && entry["target"]["propertyType"] == "shapePath"
            })
            .unwrap();
        let keys = path["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(
            keys.iter()
                .map(|key| key["layerTime"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            [500, 1500, 2000]
        );
        assert_eq!(keys[2]["easing"]["type"], "hold");
        assert_eq!(
            document["composition"]["layers"].as_array().unwrap().len(),
            5
        );
        assert!(document["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|layer| layer["type"] == "Video"));
        assert!(document["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|layer| layer["rect"]["fillColor"] == json!([0.0, 0.0, 1.0, 1.0])));
    }
}

// Numeric and Motion mutations supplement the pinned native mask payload.
#[test]
fn color_matte_mask_numeric_keys_and_motion_share_existing_editable_guides() {
    let mut xml = masked_matte();
    edit_record(
        &mut xml,
        "<ArbVideoComponentParam ObjectID=\"21195\"",
        "</ArbVideoComponentParam>",
        |record| {
            let start = record.find("<Keyframes>").unwrap();
            let end = record[start..].find("</Keyframes>").unwrap() + start + "</Keyframes>".len();
            format!("{}{}", &record[..start], &record[end..])
        },
    );
    edit_record(
        &mut xml,
        "<VideoComponentParam ObjectID=\"21196\"",
        "</VideoComponentParam>",
        |record| {
            record.replace("<IsTimeVarying>false</IsTimeVarying>", "<IsTimeVarying>true</IsTimeVarying>").replace("</VideoComponentParam>", "<Keyframes>914584608000000,50.,0,0,0,0,0,0;914838624000000,321.,0,0,0,0,0,0;</Keyframes></VideoComponentParam>")
        },
    );
    edit_record(
        &mut xml,
        "<VideoComponentChain ObjectID=\"156\"",
        "</VideoComponentChain>",
        |record| {
            record.replace("<DefaultMotion>true</DefaultMotion>", "")
            .replace("<Component Index=\"0\" ObjectRef=\"21203\"/>", "<Component Index=\"0\" ObjectRef=\"21203\"/><Component Index=\"1\" ObjectRef=\"10212\"/>")
        },
    );
    let motion = include_str!("../../fixtures/color-matte-motion/native-controls.xml");
    xml = xml.replace(
        "</PremiereData>",
        &motion.replace("<PremiereData Version=\"3\">", ""),
    );
    let (document, omissions) = convert_mask(&xml);
    assert!(
        !omissions
            .iter()
            .any(|o| o.scope == premiere_file::OmissionScope::Occurrence),
        "{omissions:?}"
    );
    let owner = red(&document);
    let guide = assert_guide(&document, owner, false, 50.0);
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    let owner_tracks: Vec<_> = entries
        .iter()
        .filter(|entry| entry["target"]["layerId"] == owner["id"])
        .collect();
    let guide_tracks: Vec<_> = entries
        .iter()
        .filter(|entry| entry["target"]["layerId"] == guide["id"])
        .collect();
    assert_eq!(owner_tracks.len(), 5);
    assert_eq!(guide_tracks.len(), owner_tracks.len());
    for track in owner_tracks {
        let copy = guide_tracks
            .iter()
            .find(|other| other["target"]["propertyType"] == track["target"]["propertyType"])
            .unwrap();
        let facts = |entry: &Value| {
            entry["animator"]["keyframes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|key| json!([key["layerTime"], key["value"], key["easing"]]))
                .collect::<Vec<_>>()
        };
        assert_eq!(facts(copy), facts(track));
    }
    let numeric: Vec<_> = entries
        .iter()
        .filter(|entry| entry["target"]["itemId"] == owner["masks"][0]["id"])
        .collect();
    assert_eq!(numeric.len(), 1);
    for track in numeric {
        assert_eq!(track["target"]["kind"], "fxItemProperty");
        assert_eq!(track["target"]["propertyName"], "feather");
        assert_eq!(track["animator"]["keyframes"][0]["layerTime"], 500);
        assert_eq!(track["animator"]["keyframes"][1]["layerTime"], 1500);
        assert_eq!(
            track["animator"]["keyframes"][0]["value"],
            json!({"type":"vector2","value":[50.0,50.0]})
        );
        assert_eq!(
            track["animator"]["keyframes"][1]["value"],
            json!({"type":"vector2","value":[321.0,321.0]})
        );
    }
}

#[test]
fn color_matte_malformed_mask_never_reappears_unmasked() {
    let mut xml = masked_matte();
    edit_record(
        &mut xml,
        "<ArbVideoComponentParam ObjectID=\"21195\"",
        "</ArbVideoComponentParam>",
        |record| {
            record.replace(
                "<ParameterID>6</ParameterID>",
                "<ParameterID>99</ParameterID>",
            )
        },
    );
    let (document, omissions) = convert_mask(&xml);
    assert!(
        omissions
            .iter()
            .any(|o| o.scope == premiere_file::OmissionScope::Occurrence
                && o.reason.contains("unknown mask parameter")),
        "{omissions:?}"
    );
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert!(!layers
        .iter()
        .any(|layer| layer["rect"]["fillColor"] == json!([1.0, 0.0, 0.0, 1.0])));
    assert!(layers.iter().any(|layer| layer["type"] == "Video"));
    assert!(layers
        .iter()
        .any(|layer| layer["rect"]["fillColor"] == json!([0.0, 0.0, 1.0, 1.0])));
}
