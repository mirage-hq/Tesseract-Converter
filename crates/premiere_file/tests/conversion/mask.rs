//! Static Opacity masks (JRB-2028) through the public conversion API: an FX
//! video whose one Add mask has a shape guide exports as a Premiere mask on the
//! clip's Opacity, and the written project imports back to the same guide and
//! mask. The Adobe-native import case waits for its pinned fixture.

use super::support::*;
use premiere_file::PrProjectFile;
use serde_json::{json, Value};
use std::path::Path;
use tesseract_file::TesseractFile;

/// The feather approximation, reported once per masked clip in both directions.
const FEATHER_REPORT: &str = "Mask Feather converts one to one to the FX mask feather, an approximation; Premiere feather visuals are not preserved exactly";

/// The flat document: video 1 over 0 to 1 s with a moved transform, an
/// inverted, 12 px feathered mask whose guide (shape 3) is its
/// sibling with the same transform: a pen outline with one smooth vertex, in
/// source pixels at dyadic fractions of the frame, which the native f32
/// fractions hold exactly.
fn masked_document(dir: &Path) -> Value {
    let mut document = document(dir);
    let transform = json!({"anchorPoint": [960, 540], "position": [900, 600], "scale": [80, 80], "rotation": 15, "opacity": 60});
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    layers[0]["transform"] = transform.clone();
    layers[0]["masks"] = json!([{"id": 4, "mode": "add", "inverted": true, "layer": 3, "feather": [12.0, 12.0], "opacity": 1.0}]);
    layers.insert(
        1,
        json!({
            "type": "Shape",
            "id": 3,
            "name": "Outline",
            "activeRange": {"start": 0, "duration": 1000},
            "transform": transform,
            "shape": {"path": {"commands": [
                {"type": "moveTo", "x": 240.0, "y": 945.0, "mirror": "straight"},
                {"type": "cubicTo", "c1x": 300.0, "c1y": 810.0, "c2x": 480.0, "c2y": 540.0, "x": 480.0, "y": 540.0},
                {"type": "lineTo", "x": 1440.0, "y": 270.0},
                {"type": "lineTo", "x": 1680.0, "y": 405.0},
                {"type": "lineTo", "x": 1680.0, "y": 945.0},
                {"type": "close"}
            ]}}
        }),
    );
    document
}

/// The `(MatchName, StartKeyframe or StartKeyframeValue)` of the mask record
/// that the Opacity component of `project` names, in `ParameterID` order.
fn native_mask(project: &Path) -> (String, Vec<(String, String)>) {
    let xml = read_xml(project);
    let document = roxmltree::Document::parse(&xml).unwrap();
    let root = document.root_element();
    let record = |id: &str| {
        root.children()
            .find(|node| node.attribute("ObjectID") == Some(id))
            .unwrap()
    };
    let child_text = |node: roxmltree::Node<'_, '_>, tag: &str| {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .map(str::to_owned)
    };
    let opacity = root
        .children()
        .find(|node| child_text(*node, "MatchName").as_deref() == Some("AE.ADBE Opacity"))
        .expect("an Opacity component");
    let sub_components: Vec<_> = opacity
        .children()
        .find(|node| node.has_tag_name("SubComponents"))
        .expect("SubComponents")
        .children()
        .filter(|node| node.has_tag_name("SubComponent"))
        .map(|node| node.attribute("ObjectRef").unwrap().to_owned())
        .collect();
    let [mask_id] = sub_components.as_slice() else {
        panic!("{sub_components:?}");
    };
    let mask = record(mask_id);
    let version = format!(
        "{}/{}",
        mask.attribute("Version").unwrap(),
        mask.children()
            .find(|node| node.has_tag_name("Component"))
            .and_then(|body| body.attribute("Version"))
            .unwrap()
    );
    let mut params: Vec<(String, String)> = mask
        .descendants()
        .filter(|node| node.has_tag_name("Param"))
        .map(|param| record(param.attribute("ObjectRef").unwrap()))
        .map(|param| {
            let id = child_text(param, "ParameterID").unwrap();
            let value = child_text(param, "StartKeyframe")
                .or_else(|| {
                    param
                        .children()
                        .find(|child| child.has_tag_name("StartKeyframeValue"))
                        .and_then(|value| value.text())
                        .map(str::to_owned)
                })
                .unwrap();
            (id, value)
        })
        .collect();
    params.sort_by_key(|(id, _)| id.parse::<u32>().unwrap());
    assert_eq!(
        child_text(mask, "MatchName").as_deref(),
        Some("AE.ADBE AEMask")
    );
    (version, params)
}

#[test]
fn an_edited_opacity_mask_exports_as_a_native_mask_and_reimports_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let document = masked_document(root);
    let edited = archive(root, &document, &root.join("source.mp4"));
    let omissions = tesseract_to_premiere(&edited, root.join("native"), false).unwrap();
    assert_eq!(
        omissions
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        [format!("feature layer 1 (\"Source\"): {FEATHER_REPORT}")]
    );
    let exported = root.join("native/project.prproj");
    // The mask is written in the v7 form beside the written Opacity owner:
    // 13 static parameters, the outline as unit-frame fractions.
    let (version, params) = native_mask(&exported);
    assert_eq!(version, "7/5");
    assert_eq!(params.len(), 13);
    let param = |id: &str| params.iter().find(|(key, _)| key == id).unwrap().1.clone();
    assert_eq!(param("7"), "-91445760000000000,12,0,0,0,0,0,0");
    assert_eq!(param("8"), "-91445760000000000,100,0,0,0,0,0,0");
    assert_eq!(param("9"), "-91445760000000000,0,0,0,0,0,0,0");
    assert_eq!(param("10"), "-91445760000000000,true,0,0,0,0,0,0");
    let path = base64_decode(&param("6"));
    assert_eq!(&path[..4], b"2cin");
    assert_eq!(u32::from_le_bytes(path[12..16].try_into().unwrap()), 5);
    // The first vertex, smooth, at (240/1920, 945/1080).
    assert_eq!(u32::from_le_bytes(path[16..20].try_into().unwrap()), 1);
    assert_eq!(f32::from_le_bytes(path[20..24].try_into().unwrap()), 0.125);
    assert_eq!(f32::from_le_bytes(path[24..28].try_into().unwrap()), 0.875);
    let (_, omissions) = PrProjectFile::load(&exported).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");

    // Reimport: the same outline in source pixels on a guide with the video's
    // transform, and the same mask.
    let omissions = premiere_to_tesseract(&exported, root.join("reimported"), None, false).unwrap();
    assert_eq!(
        omissions
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        [format!("feature VideoClipTrackItem:79: {FEATHER_REPORT}")]
    );
    let reimported = TesseractFile::open(first_project(&root.join("reimported")))
        .unwrap()
        .project_json()
        .unwrap();
    let layers = reimported["composition"]["layers"].as_array().unwrap();
    let video = layers
        .iter()
        .find(|layer| layer["type"] == "Video")
        .unwrap();
    let guide = layers
        .iter()
        .find(|layer| layer["type"] == "Shape")
        .unwrap();
    assert_eq!(guide["name"], "Premiere Opacity mask 1");
    assert_eq!(video["masks"][0]["layer"], guide["id"]);
    for field in ["inverted", "feather", "opacity"] {
        assert_eq!(
            video["masks"][0][field], document["composition"]["layers"][0]["masks"][0][field],
            "{field}"
        );
    }
    assert_eq!(
        guide["shape"]["path"],
        document["composition"]["layers"][1]["shape"]["path"]
    );
    for field in ["anchorPoint", "position", "scale", "rotation"] {
        assert_eq!(
            guide["transform"][field], video["transform"][field],
            "{field}"
        );
    }
    assert_eq!(video["transform"]["opacity"], 60.0);
}

fn base64_decode(text: &str) -> Vec<u8> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    STANDARD.decode(text.trim()).unwrap()
}

/// `premiere_isolated_opacity_masks_26_5`: Premiere 26.5.1's own save of five
/// masked clips (Oracle run 17). Sequence `5fe2e712…`: V1 plays two source
/// clips; V2 plays A, the middle-half rectangle at Mask Opacity 50; B, the
/// same rectangle on the clip at Scale 50; C, an ellipse at Feather 60; D, an
/// inverted pen path at Mask Opacity 50; and E, a rectangle mask on a
/// Gaussian Blur (Legacy).
#[test]
fn adobe_opacity_masks_fixture_imports_a_to_c_omits_d_and_e_and_writes_them_back() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_opacity_masks_26_5_strict.prproj");
    let output = root.join("converted");
    let omissions = premiere_to_tesseract(
        &source,
        &output,
        Some("5fe2e712-90a9-4044-b93a-7a75b79b1320"),
        false,
    )
    .unwrap();
    // The first V1 clip imports despite its master clip's ID-only Node; UI
    // nodes and the master clip's DefMappingID are feature reports.
    let mut reported: Vec<_> = omissions
        .iter()
        .filter(|omission| {
            !omission.reason.contains("ClipTrackItem/TrackItem/Node")
                && !omission.reason.contains("DefMappingID")
        })
        .map(ToString::to_string)
        .collect();
    reported.sort();
    assert_eq!(
        reported,
        [
            format!("feature VideoClipTrackItem:88: {FEATHER_REPORT}"),
            "occurrence 89: invalid Premiere project: an inverted mask with Mask Opacity below 100 is not converted: Premiere renders Mask Opacity times the inverted coverage, FX inverts the Mask Opacity-weighted coverage".to_owned(),
            "occurrence 90: unsupported conversion: active effect \"Gaussian Blur (Legacy)\" (match name \"AE.ADBE Gaussian Blur 2\", VideoFilterComponent version 9, Component version 7) at stack position 1 carries a mask (AE.ADBE AEMask2 sub-component; JRB-2028); the clip is not converted without it".to_owned(),
        ]
    );
    let archive = first_project(&output);
    let document = TesseractFile::open(&archive)
        .unwrap()
        .project_json()
        .unwrap();
    let layers = document["composition"]["layers"].as_array().unwrap();
    let masked = |start: i64| {
        layers
            .iter()
            .find(|layer| {
                layer["type"] == "Video"
                    && (*crate::test_support::layer_range(layer))["start"] == json!(start)
            })
            .unwrap_or_else(|| panic!("no video at {start} ms"))
    };
    let guide_of = |video: &Value| {
        layers
            .iter()
            .find(|layer| layer["id"] == video["masks"][0]["layer"])
            .unwrap()
    };
    let rectangle = json!({"commands": [
        {"type": "moveTo", "x": 480.0, "y": 270.0},
        {"type": "lineTo", "x": 1440.0, "y": 270.0},
        {"type": "lineTo", "x": 1440.0, "y": 810.0},
        {"type": "lineTo", "x": 480.0, "y": 810.0},
        {"type": "close"}
    ]});
    // A: the rectangle in source pixels, at Mask Opacity 0.5, not inverted.
    let a = masked(0);
    let a_guide = guide_of(a);
    assert_eq!(a["masks"].as_array().unwrap().len(), 1);
    for (field, value) in [
        ("mode", json!("add")),
        ("inverted", json!(false)),
        ("feather", json!([0.0, 0.0])),
        ("opacity", json!(0.5)),
    ] {
        assert_eq!(a["masks"][0][field], value, "A {field}");
    }
    assert_eq!(a_guide["type"], "Shape");
    assert_eq!(a_guide["shape"]["path"], rectangle);
    assert_eq!(a_guide["transform"], a["transform"]);
    assert_eq!(a["transform"]["scale"], json!([100.0, 100.0]));
    assert_eq!(a["transform"]["opacity"], 100.0);
    // B: the same source-frame rectangle on the clip at Scale 50, whose guide
    // shares the transform (G1: the mask follows the clip's source frame).
    let b = masked(2000);
    let b_guide = guide_of(b);
    assert_eq!(b["masks"][0]["opacity"], 1.0);
    assert_eq!(b_guide["shape"]["path"], rectangle);
    assert_eq!(b["transform"]["scale"], json!([50.0, 50.0]));
    assert_eq!(b_guide["transform"], b["transform"]);
    // C: the four-segment ellipse, Feather 60 on both axes.
    let c = masked(4000);
    let c_guide = guide_of(c);
    assert_eq!(c["masks"][0]["feather"], json!([60.0, 60.0]));
    let commands = c_guide["shape"]["path"]["commands"].as_array().unwrap();
    assert_eq!(commands.len(), 6);
    assert_eq!(commands[0]["type"], "moveTo");
    assert!(commands[1..5]
        .iter()
        .all(|command| command["type"] == "cubicTo"));
    assert_eq!(commands[0]["x"], 960.0);
    assert_eq!(commands[0]["y"], 270.0);
    assert_eq!(commands[2]["x"], 960.0);
    assert_eq!(commands[2]["y"], 810.0);
    // D and E are omitted; the second V1 clip plays under them, the first
    // under A, B and C.
    assert!(layers.iter().all(|layer| !matches!(
        (*crate::test_support::layer_range(layer))["start"].as_i64(),
        Some(6000 | 8000)
    )));
    assert_eq!(
        layers
            .iter()
            .filter(|layer| layer["type"] == "Video")
            .count(),
        5
    );
    assert!(layers
        .iter()
        .all(|layer| layer["effects"].as_array().is_none_or(Vec::is_empty)));

    // Export writes each mask back as a native mask record. The reimport
    // holds the same clips, masks and guides; export places the two V1 clips
    // on its first track and A, B and C on its second, as in the fixture.
    let rebuilt = root.join("exported");
    let omissions = tesseract_to_premiere(&archive, &rebuilt, false).unwrap();
    assert_eq!(
        omissions
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        [format!(
            "feature layer {} (\"Premiere video 5\"): {FEATHER_REPORT}",
            c["id"]
        )]
    );
    let exported = rebuilt.join("project.prproj");
    assert_eq!(read_xml(&exported).matches("AE.ADBE AEMask<").count(), 3);
    let reconverted = root.join("reconverted");
    premiere_to_tesseract(&exported, &reconverted, None, false).unwrap();
    let reimported = TesseractFile::open(first_project(&reconverted))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(masked_clips(&reimported), masked_clips(&document));
    assert_eq!(
        reimported["composition"]["layers"]
            .as_array()
            .unwrap()
            .len(),
        layers.len()
    );
}

/// Each video layer of `document` by start: its transform, its masks without
/// their ids, and each mask guide's type, transform and path.
fn masked_clips(document: &Value) -> Vec<(i64, Value, Vec<Value>, Vec<Value>)> {
    let layers = document["composition"]["layers"].as_array().unwrap();
    let mut clips: Vec<_> = layers
        .iter()
        .filter(|layer| layer["type"] == "Video")
        .map(|video| {
            let masks: Vec<Value> = video["masks"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|mask| {
                    let mut mask = mask.clone();
                    mask.as_object_mut().unwrap().remove("id");
                    mask.as_object_mut().unwrap().remove("layer");
                    mask
                })
                .collect();
            let guides: Vec<Value> = video["masks"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|mask| {
                    let guide = layers
                        .iter()
                        .find(|layer| layer["id"] == mask["layer"])
                        .unwrap();
                    json!([guide["type"], guide["transform"], guide["shape"]["path"]])
                })
                .collect();
            (
                (*crate::test_support::layer_range(video))["start"]
                    .as_i64()
                    .unwrap(),
                video["transform"].clone(),
                masks,
                guides,
            )
        })
        .collect();
    clips.sort_by_key(|(start, ..)| *start);
    clips
}
