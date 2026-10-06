use super::*;

const CONTROLS: &str = include_str!("../../fixtures/color-matte-motion/native-controls.xml");

fn with_controls(mut xml: String, components: &[u32]) -> String {
    edit_record(
        &mut xml,
        "<VideoComponentChain ObjectID=\"156\"",
        "</VideoComponentChain>",
        |record| {
            let references = components
                .iter()
                .enumerate()
                .map(|(index, id)| format!("<Component Index=\"{index}\" ObjectRef=\"{id}\"/>"))
                .collect::<String>();
            let record = record.replace(
                "<Components><Component Index=\"0\" ObjectRef=\"403\"/></Components>",
                "",
            );
            let mut record = record.replace(
                "<ComponentChain Version=\"3\">",
                &format!("<ComponentChain Version=\"3\"><Components>{references}</Components>"),
            );
            if components.contains(&10212) || components.contains(&10111) {
                record = record.replace("<DefaultMotion>true</DefaultMotion>", "");
            }
            if components.contains(&10211) {
                record = record.replace("<DefaultOpacity>true</DefaultOpacity>", "");
            }
            record
        },
    );
    xml.replace(
        "</PremiereData>",
        &CONTROLS.replace("<PremiereData Version=\"3\">", ""),
    )
}

fn convert(xml: &str) -> (Value, Vec<premiere_file::Omission>) {
    let dir = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let expected_bytes = std::fs::read(fixtures.join("video-30fps-10s.mp4")).unwrap();
    std::fs::write(dir.path().join("video-30fps-10s.mp4"), &expected_bytes).unwrap();
    let source = dir.path().join("project.prproj");
    write_prproj(&source, xml);
    let native = PrProjectFile::load(&source).unwrap().0;
    let sequence = native.sequences().next().unwrap();
    let PrVideoItem::Media(clip) = &sequence.video_tracks().nth(1).unwrap()[0] else {
        panic!("native Color Matte is a media occurrence");
    };
    assert_eq!(clip.timeline_ticks(), 2 * TICKS..5 * TICKS);
    assert_eq!(clip.source_ticks(), 914457600000000..915219648000000);
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(&source, &output, Some(SEQUENCE_UID), false).unwrap();
    let archive = TesseractFile::open(first_project(&output)).unwrap();
    assert_eq!(archive.metadata().assets.len(), 1);
    let asset = archive.metadata().assets.keys().next().unwrap();
    assert_eq!(
        archive
            .asset(asset)
            .unwrap()
            .read_verified_bytes(expected_bytes.len() as u64)
            .unwrap(),
        expected_bytes
    );
    (archive.project_json().unwrap(), omissions)
}

fn red(document: &Value) -> &Value {
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["rect"]["fillColor"] == json!([1.0, 0.0, 0.0, 1.0]))
        .expect("authored red matte must survive as editable content")
}

fn tracks<'a>(document: &'a Value, layer: &Value) -> Vec<&'a Value> {
    document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["target"]["layerId"] == layer["id"])
        .collect()
}

fn key_facts(entry: &Value) -> Vec<Value> {
    entry["animator"]["keyframes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| json!([key["layerTime"], key["value"]["value"]]))
        .collect()
}

#[test]
fn native_color_matte_opacity_keys_keep_clock_blend_and_supported_siblings() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let xml = with_controls(read_xml(&fixtures.join(FIXTURE)), &[10211]);
    let (document, omissions) = convert(&xml);
    assert!(omissions.is_empty(), "{omissions:?}");
    let owner = red(&document);
    assert_eq!(owner["type"], "Rect");
    assert_eq!(
        *crate::test_support::layer_range(owner),
        json!({"start": 2000, "duration": 3000})
    );
    assert_eq!(owner["transform"]["opacity"], 100.0);
    assert_eq!(owner["rect"]["fillEnabled"], true);
    assert_eq!(owner["rect"]["size"], json!([1920.0, 1080.0]));
    assert_eq!(document["duration"], 9.0);
    assert_eq!(
        document["composition"]["layers"].as_array().unwrap().len(),
        4
    );
    let entries = tracks(&document, owner);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["target"]["propertyType"], "opacity");
    assert_eq!(
        key_facts(entries[0]),
        [
            json!([500, 100.0]),
            json!([1000, 50.0]),
            json!([1500, 75.0])
        ]
    );
    assert_eq!(
        entries[0]["animator"]["keyframes"][1]["easing"]["type"],
        "linear"
    );
}

#[test]
fn native_color_matte_motion_keys_and_crop_share_motion_not_opacity() {
    let xml = with_controls(cropped_color_matte_xml(), &[10212, 10211, 403]);
    let (document, omissions) = convert(&xml);
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_color_matte_crop(
        &document,
        [1.0, 0.0, 0.0, 1.0],
        json!({"start": 2000, "duration": 3000}),
        [0.0, 0.0],
        [977.1345703124928, 1080.0],
        false,
    );
    let owner = red(&document);
    let guide = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == owner["masks"][0]["layer"])
        .unwrap();
    assert_eq!(owner["transform"], guide["transform"]);
    assert_eq!(owner["transform"]["anchorPoint"], json!([960.0, 540.0]));
    let entries = tracks(&document, owner);
    assert_eq!(entries.len(), 6);
    for (property, expected) in [
        (
            "positionX",
            vec![
                json!([500, 960.0]),
                json!([1000, 1152.0]),
                json!([1500, 1344.0]),
            ],
        ),
        (
            "positionY",
            vec![
                json!([500, 540.0]),
                json!([1000, 486.0]),
                json!([1500, 432.0]),
            ],
        ),
        (
            "scaleX",
            vec![
                json!([500, 100.0]),
                json!([1000, 60.0]),
                json!([1500, 80.0]),
            ],
        ),
        (
            "scaleY",
            vec![
                json!([500, 100.0]),
                json!([1000, 60.0]),
                json!([1500, 80.0]),
            ],
        ),
        (
            "rotation",
            vec![json!([500, 0.0]), json!([1000, 20.0]), json!([1500, -10.0])],
        ),
    ] {
        let entry = entries
            .iter()
            .find(|entry| entry["target"]["propertyType"] == property)
            .unwrap();
        let actual = key_facts(entry);
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(&expected) {
            assert_eq!(actual[0], expected[0]);
            assert!((actual[1].as_f64().unwrap() - expected[1].as_f64().unwrap()).abs() < 1e-8);
        }
        let guide_entry = tracks(&document, guide)
            .into_iter()
            .find(|entry| entry["target"]["propertyType"] == property)
            .unwrap();
        let without_ids = |entry: &Value| {
            entry["animator"]["keyframes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|key| {
                    let mut key = key.clone();
                    key.as_object_mut().unwrap().remove("id");
                    key
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(without_ids(entry), without_ids(guide_entry));
    }
    assert_eq!(tracks(&document, guide).len(), 5);
    assert!(tracks(&document, guide)
        .iter()
        .all(|entry| entry["target"]["propertyType"] != "opacity"));
}

#[test]
fn native_color_matte_position_keeps_saved_point_keys_and_straight_path_easing() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let xml = with_controls(read_xml(&fixtures.join(FIXTURE)), &[10111]);
    let (document, omissions) = convert(&xml);
    assert!(omissions.is_empty(), "{omissions:?}");
    let entries = tracks(&document, red(&document));
    assert_eq!(entries.len(), 3);
    let x = entries
        .iter()
        .find(|entry| entry["target"]["propertyType"] == "positionX")
        .unwrap();
    let keys = &x["animator"]["keyframes"];
    assert_eq!(keys[0]["layerTime"], 0);
    assert_eq!(keys[1]["layerTime"], 3667);
    assert!((keys[0]["value"]["value"].as_f64().unwrap() - -55.0).abs() < 1e-4);
    assert!((keys[1]["value"]["value"].as_f64().unwrap() - 710.0).abs() < 1e-4);
}

// Supplementary curved-path mutation; not a new Adobe-authored oracle.
#[test]
fn color_matte_curved_position_keys_keep_editable_spatial_tangents() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let xml = with_controls(read_xml(&fixtures.join(FIXTURE)), &[10111])
        .replace("-7.0642541261101152e-09", "0.125")
        .replace("7.0642541261101152e-09", "-0.125");
    let (document, omissions) = convert(&xml);
    assert!(omissions.is_empty(), "{omissions:?}");
    let entries = tracks(&document, red(&document));
    for (property, tangent) in [("positionX", 127.4999997615814), ("positionY", 135.0)] {
        let entry = entries
            .iter()
            .find(|entry| entry["target"]["propertyType"] == property)
            .unwrap();
        let keys = &entry["animator"]["keyframes"];
        assert!((keys[0]["spatialOutTangent"].as_f64().unwrap() - tangent).abs() < 1e-8);
        assert!((keys[1]["spatialInTangent"].as_f64().unwrap() + tangent).abs() < 1e-8);
    }
}
