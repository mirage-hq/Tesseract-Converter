use super::{as_matte, COLOR_MATTE_SOURCE_IN_TICKS};
use crate::{schema::TICKS, Premiere, PremiereImportOptions};
use fx_conv::{ConversionMode, ImportToTesseract};
use serde_json::{json, Value};

fn cropped_transform_xml(reversed: bool) -> String {
    use super::super::{
        animation::animation_fixture::SOURCE,
        effects::{top_crop, with_chain, DEFAULT_FLAGS},
    };
    let native = include_str!("../../../tests/fixtures/nested-transform-geometry2.xml");
    let dom = roxmltree::Document::parse(native).unwrap();
    let records: String = dom.root_element().children().filter(|node| node.is_element()).map(|node| {
        let mut record = native[node.range()].to_owned();
        for child in node.children().filter(|child| child.has_tag_name("Keyframes") || child.has_tag_name("IsTimeVarying")) {
            record = record.replace(&native[child.range()], "");
        }
        if node.attribute("ObjectID") == Some("830") {
            record = record.replace("</PointComponentParam>", &format!(
                "<Keyframes>{},0.5:0.5,0,0,0,0,0,0,5,4,0,0,0,0;{},0.5:-0.5,0,0,0,0,0,0,5,4,0,0,0,0;</Keyframes></PointComponentParam>",
                COLOR_MATTE_SOURCE_IN_TICKS + TICKS, COLOR_MATTE_SOURCE_IN_TICKS + 3 * TICKS));
        }
        if node.attribute("ObjectID") == Some("832") {
            record = record.replace(",100.,", ",70.,");
        }
        record.replace("0.3645833432674408:1.6666666269302368", "0.5:0.5")
    }).collect();
    let mut components = vec![(407, records), (900, top_crop(900))];
    if reversed {
        components.reverse();
    }
    as_matte(
        &with_chain(SOURCE, DEFAULT_FLAGS, &components),
        "/wAAAAEAAAA=",
    )
}

fn publish(xml: &str) -> Value {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.prproj");
    std::fs::write(&source, xml).unwrap();
    let output = directory.path().join("converted");
    Premiere
        .import_to_tesseract(
            &source,
            &output,
            &PremiereImportOptions {
                sequence: Some("sequence-1".into()),
            },
            ConversionMode::Write,
        )
        .unwrap();
    tesseract_file::TesseractFile::open(output.join("project.tsrct"))
        .unwrap()
        .project_json()
        .unwrap()
}

#[test]
fn cropped_color_matte_keeps_straight_transform_and_guide_keys() {
    let xml = cropped_transform_xml(false);
    let wire = publish(&xml);
    let layers = wire["composition"]["layers"].as_array().unwrap();
    let solid = layers
        .iter()
        .find(|layer| layer["rect"]["fillColor"] == json!([1.0, 0.0, 0.0, 1.0]))
        .expect("cropped Color Matte must survive import");
    let guide = layers
        .iter()
        .find(|layer| layer["id"] == solid["masks"][0]["layer"])
        .unwrap();
    assert_eq!(solid["activeRange"], json!({"start": 0, "duration": 5000}));
    assert_eq!(solid["transform"]["anchorPoint"], json!([960.0, 540.0]));
    assert_eq!(solid["transform"]["position"], json!([960.0, 540.0]));
    assert_eq!(solid["transform"]["scale"], json!([70.0, 70.0]));
    assert_eq!(guide["transform"], solid["transform"]);
    assert_eq!(guide["rect"]["position"], json!([0.0, 162.0]));
    assert_eq!(guide["rect"]["size"], json!([1920.0, 918.0]));
    assert_eq!(guide["rect"]["fillEnabled"], false);
    for owner in [solid, guide] {
        let track = wire["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| {
                entry["target"]["layerId"] == owner["id"]
                    && entry["target"]["propertyType"] == "positionY"
            })
            .unwrap();
        let keys: Vec<_> = track["animator"]["keyframes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|key| {
                (
                    key["layerTime"].as_i64().unwrap(),
                    key["value"]["value"].as_f64().unwrap(),
                )
            })
            .collect();
        assert_eq!(keys, [(1000, 540.0), (3000, -540.0)]);
    }
}

#[test]
fn cropped_color_matte_transform_rejects_unmapped_controls() {
    let xml = cropped_transform_xml(false);
    let dom = roxmltree::Document::parse(&xml).unwrap();
    let mut rejected = vec![cropped_transform_xml(true)];
    for (id, from, to) in [
        ("832", ",70.,", ",-70.,"),
        ("836", ",0.,", ",30.,"),
        ("837", ",100.,", ",50.,"),
        ("840", ",0,0,0,0,0,0,0", ",1,0,0,0,0,0,0"),
        ("906", ",0,0,0,0,0,0,0", ",1,0,0,0,0,0,0"),
        ("830", ",5,4,0,0,0,0;", ",5,4,0,0,0.2,0;"),
    ] {
        let node = dom
            .root_element()
            .children()
            .find(|node| node.attribute("ObjectID") == Some(id))
            .unwrap();
        let record = &xml[node.range()];
        let changed = record.replace(from, to);
        assert_ne!(changed, record, "{id}");
        rejected.push(xml.replace(record, &changed));
    }
    for xml in rejected {
        let error = crate::format::inspect_project_with_omissions(&xml, Some("sequence-1"))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("nondefault Crop on a Color Matte"),
            "{error}"
        );
    }
}
