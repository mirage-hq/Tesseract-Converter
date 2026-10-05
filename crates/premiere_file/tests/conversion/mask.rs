#![cfg(feature = "ffmpeg-library")]

//! Opacity masks through the public conversion API: an FX video
//! whose one Add mask has a shape guide exports as a Premiere mask on the
//! clip's Opacity, and the written project imports back to the same guide and
//! mask. The Adobe-native cases are Premiere 26.5.1's save of five static
//! masks and the keyed Mask Path in both of its saved forms.

use super::support::*;
use premiere_file::{OmissionScope, PrProjectFile};
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
/// masked clips. Sequence `5fe2e712…`: V1 plays two source
/// clips; V2 plays A, the middle-half rectangle at Mask Opacity 50; B, the
/// same rectangle on the clip at Scale 50; C, an ellipse at Feather 60; D, an
/// inverted pen path at Mask Opacity 50; and E, a rectangle mask on a
/// Gaussian Blur (Legacy).
#[test]
fn adobe_opacity_masks_fixture_imports_a_to_c_and_e_omits_d_and_writes_them_back() {
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
            "feature VideoClipTrackItem:90: effect mask retained on an isolated editable adjustment: FX clips its input before filtering, whereas Premiere masks output computed from the whole input; filter edges can differ".to_owned(),
            "occurrence 89: invalid Premiere project: an inverted mask with Mask Opacity below 100 is not converted: Premiere renders Mask Opacity times the inverted coverage, FX inverts the Mask Opacity-weighted coverage".to_owned(),
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
    // D is omitted. E keeps its effect and mask inside a local scope above
    // the second V1 clip, rather than masking that clip's whole picture.
    assert!(layers
        .iter()
        .all(|layer| (*crate::test_support::layer_range(layer))["start"] != 6000));
    let e = layers
        .iter()
        .find(|layer| {
            layer["type"] == "Group" && (*crate::test_support::layer_range(layer))["start"] == 8000
        })
        .unwrap();
    let children = e["layers"].as_array().unwrap();
    assert_eq!(
        children
            .iter()
            .map(|layer| layer["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["Adjustment", "Shape", "Video"]
    );
    assert_eq!(children[0]["masks"].as_array().unwrap().len(), 1);
    assert_eq!(children[0]["effects"][0]["effect"]["type"], "gaussianBlur");
    assert!(children[2]["masks"].as_array().is_none_or(Vec::is_empty));
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
    // on its first track and A, B, C and E on its second, as in the fixture.
    let rebuilt = root.join("exported");
    let omissions = tesseract_to_premiere(&archive, &rebuilt, false).unwrap();
    let mut reported: Vec<_> = omissions.iter().map(ToString::to_string).collect();
    reported.sort();
    let mut expected = vec![
        format!("feature layer {} (\"Premiere video 5\"): {FEATHER_REPORT}", c["id"]),
        format!("feature layer {}: effect mask retained on an isolated editable adjustment: FX clips its input before filtering, whereas Premiere masks output computed from the whole input; filter edges can differ", e["id"]),
    ];
    expected.sort();
    assert_eq!(reported, expected);
    let exported = rebuilt.join("project.prproj");
    let xml = read_xml(&exported);
    assert_eq!(xml.matches("AE.ADBE AEMask<").count(), 4);
    let graph = roxmltree::Document::parse(&xml).unwrap();
    let blur = graph
        .root_element()
        .children()
        .find(|node| {
            node.children().any(|child| {
                child.has_tag_name("MatchName") && child.text() == Some("AE.Impact_Blur_FX")
            })
        })
        .unwrap();
    let associations = blur
        .children()
        .find(|node| node.has_tag_name("SubComponents"))
        .unwrap();
    let refs: Vec<_> = associations
        .children()
        .filter_map(|node| node.attribute("ObjectRef"))
        .collect();
    assert_eq!(refs.len(), 1);
    let mask = graph
        .root_element()
        .children()
        .find(|node| node.attribute("ObjectID") == Some(refs[0]))
        .unwrap();
    assert!(mask
        .children()
        .any(|node| node.has_tag_name("MatchName") && node.text() == Some("AE.ADBE AEMask")));
    assert!(
        !xml.contains("AE.ADBE Geometry2"),
        "no spurious Transform stage"
    );
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

/// The one sequence of both native Mask Path fixtures.
const KEYED_MASK_SEQUENCE: &str = "5fe2e712-90a9-4044-b93a-7a75b79b1320";

/// Each masked video of `document` by start: its mask's fields, its guide's
/// outline and the guide's outline keys as `[time, easing, outline]`, every
/// coordinate rounded to 1e-3 px, which absorbs the native f32 fractions.
fn keyed_mask_clips(document: &Value) -> Vec<Value> {
    let layers = document["composition"]["layers"].as_array().unwrap();
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let mut clips: Vec<_> = layers
        .iter()
        .filter(|layer| {
            layer["type"] == "Video" && layer["masks"].as_array().is_some_and(|m| !m.is_empty())
        })
        .map(|video| {
            let mask = &video["masks"][0];
            let guide = layers
                .iter()
                .find(|layer| layer["id"] == mask["layer"])
                .unwrap();
            let keys: Vec<_> = entries
                .iter()
                .filter(|entry| {
                    entry["target"]["layerId"] == guide["id"]
                        && entry["target"]["propertyType"] == "shapePath"
                })
                .flat_map(|entry| entry["animator"]["keyframes"].as_array().unwrap())
                .map(|key| {
                    json!([
                        key["layerTime"],
                        key["easing"]["type"],
                        rounded(&key["value"]["value"]["commands"])
                    ])
                })
                .collect();
            json!({
                "start": crate::test_support::layer_range(video)["start"],
                "mask": [mask["mode"], mask["inverted"], mask["feather"], mask["expansion"], mask["opacity"]],
                "guide": [guide["type"], rounded(&guide["shape"]["path"]["commands"])],
                "keys": keys,
            })
        })
        .collect();
    clips.sort_by_key(|clip| clip["start"].as_i64());
    clips
}

/// `commands` with every coordinate rounded to 1e-3 px.
fn rounded(commands: &Value) -> Value {
    let mut commands = commands.clone();
    for command in commands.as_array_mut().into_iter().flatten() {
        for (_, value) in command.as_object_mut().unwrap().iter_mut() {
            if let Some(number) = value.as_f64() {
                *value = json!((number * 1000.0).round() / 1000.0);
            }
        }
    }
    commands
}

/// The commands of the closed rectangle from `left` to `right` px and from
/// 270 to 810 px, top left first, as import draws the native fixture's outlines in
/// the 1920x1080 source frame.
fn mask_rectangle(left: f64, right: f64) -> Vec<Value> {
    vec![
        json!({"type": "moveTo", "x": left, "y": 270.0}),
        json!({"type": "lineTo", "x": right, "y": 270.0}),
        json!({"type": "lineTo", "x": right, "y": 810.0}),
        json!({"type": "lineTo", "x": left, "y": 810.0}),
        json!({"type": "close"}),
    ]
}

/// The native fixture's clip K (V2, 0 to 3 s, source In 0) keys its Mask Path at
/// 0.5 s (rectangle A, x 0.25 to 0.5 and y 0.25 to 0.75 of the frame), 1.5 s
/// (A moved right by a quarter of the frame) and 2 s (A with a fifth vertex
/// at (0.375, 0.15) between its top corners), beside a stored triangle that
/// Premiere does not draw; M13, M12 and M12+13 (3 to 6 s) hold A statically
/// with mask controls 13, 12 or both off their saved defaults.
/// `feature_keyed_mask_path_26_5.prproj` is the v8 form that Premiere 26.5.1
/// opened and AME rendered (keys without `IsTimeVarying`), and `_saved` its
/// 26.5.1 save (`AEMask2`: the keys under `IsTimeVarying` true beside
/// identically keyed mask Position and Anchor Point, the controls back at
/// their defaults); both are packaged without their absolute media and
/// peak-file paths. K imports as its guide's outline keys, Linear between the
/// two rectangles and Hold across the vertex-count change, as AME drew them.
#[test]
fn adobe_keyed_mask_path_imports_as_guide_outline_keys_from_both_saved_forms() {
    let a = mask_rectangle(480.0, 960.0);
    let mut peaked = a.clone();
    peaked.insert(1, json!({"type": "lineTo", "x": 720.0, "y": 162.0}));
    let mask = json!(["add", false, [0.0, 0.0], 0.0, 1.0]);
    let k = json!({
        "start": 0,
        "mask": mask,
        "guide": ["Shape", a],
        "keys": [
            [500, "linear", a],
            [1500, "linear", mask_rectangle(960.0, 1440.0)],
            [2000, "hold", peaked]
        ],
    });
    let static_a =
        |start: i64| json!({"start": start, "mask": mask, "guide": ["Shape", a], "keys": []});
    for (fixture, masked, omitted) in [
        (
            "feature_keyed_mask_path_26_5.prproj",
            vec![k.clone()],
            vec![
                "mask control 12 at a value other than 0 is not converted",
                "mask control 12 at a value other than 0 is not converted",
                "mask control 13 at a value other than 0.5 is not converted",
            ],
        ),
        (
            "feature_keyed_mask_path_26_5_saved.prproj",
            vec![k.clone(), static_a(3000), static_a(4000), static_a(5000)],
            Vec::new(),
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture);
        let output = dir.path().join("converted");
        let omissions =
            premiere_to_tesseract(&source, &output, Some(KEYED_MASK_SEQUENCE), false).unwrap();
        let document = TesseractFile::open(first_project(&output))
            .unwrap()
            .project_json()
            .unwrap();
        assert_eq!(
            keyed_mask_clips(&document),
            masked,
            "{fixture}: {omissions:?}"
        );
        // Each omitted occurrence by the expected reason that it gives.
        let mut occurrences: Vec<_> = omissions
            .iter()
            .filter(|omission| omission.scope == OmissionScope::Occurrence)
            .map(|omission| {
                omitted
                    .iter()
                    .find(|expected| omission.reason.contains(**expected))
                    .copied()
                    .unwrap_or(omission.reason.as_str())
            })
            .collect();
        occurrences.sort_unstable();
        assert_eq!(occurrences, omitted, "{fixture}: {omissions:?}");
    }
}

#[test]
fn adobe_keyed_mask_path_exports_its_edited_outline_keys_and_reads_them_back() {
    // Import the native 26.5.1 Mask Path save, then move K's second key
    // from 1.5 s to 1.25 s and its 5-vertex key's fifth vertex up to y
    // 135 px (0.125 of the frame) in the archive. Export writes K's Mask Path
    // keys at the edited source times with the edited outline, which import
    // reads back as the same guide keys.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_keyed_mask_path_26_5_saved.prproj");
    let output = root.join("converted");
    premiere_to_tesseract(&source, &output, Some(KEYED_MASK_SEQUENCE), false).unwrap();
    let archive = first_project(&output);
    let mut file = TesseractFile::open(&archive).unwrap();
    let checkout = root.join("project.json");
    file.checkout_project_json(&checkout).unwrap();
    let mut document: Value = serde_json::from_slice(&std::fs::read(&checkout).unwrap()).unwrap();
    // K is the masked video at 0 s; the V1 clip there has no mask.
    let guide = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| {
            layer["type"] == "Video"
                && crate::test_support::layer_range(layer)["start"] == 0
                && layer["masks"].is_array()
        })
        .unwrap()["masks"][0]["layer"]
        .clone();
    let entry = document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| {
            entry["target"]["layerId"] == guide && entry["target"]["propertyType"] == "shapePath"
        })
        .unwrap();
    let keys = &mut entry["animator"]["keyframes"];
    keys[1]["layerTime"] = json!(1250);
    keys[2]["value"]["value"]["commands"][1] = json!({"type": "lineTo", "x": 720.0, "y": 135.0});
    std::fs::write(&checkout, serde_json::to_vec(&document).unwrap()).unwrap();
    file.commit_project_json(&checkout).unwrap();
    file.save().unwrap();

    let rebuilt = root.join("exported");
    let omissions = tesseract_to_premiere(&archive, &rebuilt, false).unwrap();
    assert!(
        omissions
            .iter()
            .all(|omission| omission.scope != OmissionScope::Occurrence),
        "{omissions:?}"
    );
    let exported = rebuilt.join("project.prproj");
    // One Mask Path is keyed: under `IsTimeVarying` true, `ticks,base64;` per
    // key on K's source clock, from its In 0.
    let xml = read_xml(&exported);
    let native = roxmltree::Document::parse(&xml).unwrap();
    let child_text = |node: roxmltree::Node<'_, '_>, tag: &str| {
        node.children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .map(str::to_owned)
    };
    let keyed: Vec<_> = native
        .root_element()
        .children()
        .filter(|node| {
            node.has_tag_name("ArbVideoComponentParam")
                && child_text(*node, "Name").as_deref() == Some("Mask Path")
                && child_text(*node, "Keyframes").is_some()
        })
        .collect();
    let [path] = keyed.as_slice() else {
        panic!("one keyed Mask Path: {}", keyed.len());
    };
    assert_eq!(child_text(*path, "IsTimeVarying").as_deref(), Some("true"));
    let wire = child_text(*path, "Keyframes").unwrap();
    let native_keys: Vec<_> = wire
        .split_terminator(';')
        .map(|key| {
            let (ticks, encoded) = key.split_once(',').unwrap();
            let payload = base64_decode(encoded);
            // The written `2cin` value: a 16-byte header, then per vertex a
            // flag, the point, both tangents and a trailer.
            let count = u32::from_le_bytes(payload[12..16].try_into().unwrap()) as usize;
            let points: Vec<_> = (0..count)
                .map(|vertex| {
                    let at = 16 + 32 * vertex + 4;
                    let x = f32::from_le_bytes(payload[at..at + 4].try_into().unwrap());
                    let y = f32::from_le_bytes(payload[at + 4..at + 8].try_into().unwrap());
                    [x, y]
                })
                .collect();
            (ticks.parse::<i64>().unwrap(), points)
        })
        .collect();
    let rectangle = |left: f32| {
        vec![
            [left, 0.25],
            [left + 0.25, 0.25],
            [left + 0.25, 0.75],
            [left, 0.75],
        ]
    };
    let mut peaked = rectangle(0.25);
    peaked.insert(1, [0.375, 0.125]);
    assert_eq!(
        native_keys,
        [
            (TICKS / 2, rectangle(0.25)),
            (5 * TICKS / 4, rectangle(0.5)),
            (2 * TICKS, peaked),
        ]
    );

    // The written project reads back to the edited guide keys.
    let reconverted = root.join("reconverted");
    premiere_to_tesseract(&exported, &reconverted, None, false).unwrap();
    let reimported = TesseractFile::open(first_project(&reconverted))
        .unwrap()
        .project_json()
        .unwrap();
    let edited_k = keyed_mask_clips(&document)
        .into_iter()
        .find(|clip| clip["start"] == 0)
        .unwrap();
    let reread_k = keyed_mask_clips(&reimported)
        .into_iter()
        .find(|clip| clip["start"] == 0)
        .unwrap();
    assert_eq!(reread_k, edited_k);
    assert_eq!(edited_k["keys"][1][0], 1250);
    assert_eq!(
        edited_k["keys"][2][2][1],
        json!({"type": "lineTo", "x": 720.0, "y": 135.0})
    );
}
