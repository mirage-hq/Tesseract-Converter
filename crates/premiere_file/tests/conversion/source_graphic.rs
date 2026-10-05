//! Source Graphic placements on the untouched Premiere 26.5.1 save of case
//! `premiere_isolated_source_graphic_26_5`: master clip `Graphic` owns the
//! shared Text objects `py` (Scale 150) and an empty Text saved without a
//! font, which imports with Inter Regular. In sequence `Source graphic 26.5`,
//! I1 (0-3 s) and I2 (5-8 s) keep their own static clip Motion; the original
//! placement in `Single-style point text` (0-2 s) keeps the default. Every
//! placement imports its own editable copy, over the grey backdrop still that
//! Premiere keeps at its 5 s source span on a 10 s placement.

use super::support::*;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};

/// The sequence of I1 and I2.
const PLACEMENTS: &str = "9eaecdad-b77c-4b0a-8a94-0102641f46dc";
/// The sequence of the original placement.
const ORIGINAL: &str = "c8acf9c1-34b2-4086-9f55-d528950a7059";
const MASTER: &str = "MasterClip:4eba56ac-0cf4-4a57-99cd-fcfabbd21dfd";
const SHARED_EDITING: &str = "Source Graphic shared editing is not converted: each placement imports its own copy of the shared objects, so editing one copy leaves the others unchanged";
/// The report of the empty Text's import font, once for the master's Text
/// object that every placement shows.
const EMPTY_TEXT: (&str, &str) = (
    "VideoFilterComponent:63",
    "empty Text saved without a font: it imports with Inter Regular, FX's font for new text; Premiere saved no font for it",
);
const FONT_NOT_PACKAGED: &str = "font \"Arial-BoldMT\" is not packaged in this document; import it with tsrct project import-font before preview or export.";
const INTER_NOT_PACKAGED: &str = "font \"Inter Regular\" is not packaged in this document; import it with tsrct project import-font before preview or export.";

fn save() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/feature_source_graphic_26_5.prproj")
}

/// The converted document of `sequence` of `project` and its reports as
/// (record, reason).
fn converted(project: &Path, sequence: &str) -> (Value, Vec<(String, String)>) {
    let output = tempfile::tempdir().unwrap();
    let omissions =
        premiere_to_tesseract(project, output.path().join("out"), Some(sequence), false).unwrap();
    let document = TesseractFile::open(first_project(&output.path().join("out")))
        .unwrap()
        .project_json()
        .unwrap();
    let reports = omissions
        .into_iter()
        .map(|omission| (omission.record, omission.reason))
        .collect();
    (document, reports)
}

fn owned(reports: &[(&str, &str)]) -> Vec<(String, String)> {
    reports
        .iter()
        .map(|&(record, reason)| (record.to_owned(), reason.to_owned()))
        .collect()
}

fn layers(document: &Value) -> &[Value] {
    document["composition"]["layers"].as_array().unwrap()
}

/// Every layer id of `layers` and of the groups in them.
fn layer_ids(layers: &[Value], ids: &mut Vec<u64>) {
    for layer in layers {
        ids.push(layer["id"].as_u64().unwrap());
        if let Some(children) = layer["layers"].as_array() {
            layer_ids(children, ids);
        }
    }
}

/// Checks that the ids of `layers`, at every depth, are distinct.
fn assert_distinct_ids(layers: &[Value]) {
    let mut ids = Vec::new();
    layer_ids(layers, &mut ids);
    assert_eq!(
        ids.iter().collect::<BTreeSet<_>>().len(),
        ids.len(),
        "{ids:?}"
    );
}

/// `layer`'s transform fields that a clip Motion sets.
fn motion(layer: &Value) -> [&Value; 4] {
    ["position", "anchorPoint", "scale", "rotation"].map(|field| &layer["transform"][field])
}

/// Checks the graphic group of a placement's copy of the shared objects: an
/// identity group of `py` at the shared Scale 150 and the empty Text, with
/// zero characters in Inter Regular.
fn assert_shared_objects(group: &Value, index: usize) {
    assert_eq!(group["name"], format!("Premiere graphic {index}"));
    assert_eq!(
        motion(group),
        [
            &json!([0.0, 0.0]),
            &json!([0.0, 0.0]),
            &json!([100.0, 100.0]),
            &json!(0.0)
        ]
    );
    let [py, empty] = group["layers"].as_array().unwrap().as_slice() else {
        panic!("the shared objects are two texts: {group}");
    };
    assert_eq!((&py["type"], &py["name"]), (&json!("Text"), &json!("py")));
    assert_eq!(
        (
            &py["sourceText"]["text"],
            &py["sourceText"]["fontFamily"],
            &py["sourceText"]["fontSize"]
        ),
        (&json!("py"), &json!("Arial-BoldMT"), &json!(160.0))
    );
    assert_eq!(
        (&py["transform"]["position"], &py["transform"]["scale"]),
        (&json!([480.0, 540.0]), &json!([150.0, 150.0]))
    );
    assert_eq!(
        (&empty["type"], &empty["name"]),
        (&json!("Text"), &json!(format!("Premiere text {index}")))
    );
    assert_eq!(
        (
            &empty["sourceText"]["text"],
            &empty["sourceText"]["fontFamily"],
            &empty["sourceText"]["fontStyle"]
        ),
        (&json!(""), &json!("Inter"), &json!("Regular"))
    );
    for text in [py, empty] {
        assert_eq!(text["parent"], group["id"]);
        assert_eq!(text.get("isHidden"), None);
    }
}

/// `document` as the archive `name`.tsrct in `directory` with the media
/// `assets` and a face of Inter Regular as `tsrct project import-font`
/// registers one: the converter reads its registry, never its bytes.
fn archive(
    document: &Value,
    directory: &Path,
    name: &str,
    assets: &[(&str, PathBuf, AssetKind)],
) -> PathBuf {
    let mut builder =
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(document).unwrap()).unwrap();
    for &(id, ref path, kind) in assets {
        builder = builder.add_asset(id, path, kind).unwrap();
    }
    let font = directory.join("Inter-Regular.ttf");
    std::fs::write(&font, b"packaged font bytes").unwrap();
    let registry = serde_json::from_value(json!({
        "faces": [{
            "postscriptName": "Inter-Regular",
            "fullName": "Inter Regular",
            "familyName": "Inter",
            "styleName": "Regular",
            "weight": 400,
            "width": 5,
            "selectionNames": ["Inter/Regular"]
        }]
    }))
    .unwrap();
    builder = builder
        .add_font_asset("inter-regular", &font, registry)
        .unwrap();
    let path = directory.join(format!("{name}.tsrct"));
    builder.write(&path).unwrap();
    path
}

/// The export `omissions` as (record, reason).
fn reasons(omissions: Vec<premiere_file::Omission>) -> Vec<(String, String)> {
    omissions
        .into_iter()
        .map(|omission| (omission.record, omission.reason))
        .collect()
}

/// The grey backdrop still's image as the converted document names it.
fn backdrop_asset() -> (&'static str, PathBuf, AssetKind) {
    (
        "premiere-image-1",
        save().with_file_name("tmk_grey128.png"),
        AssetKind::Image,
    )
}

#[test]
fn adobe_source_graphic_placements_import_independent_editable_copies() {
    let (document, reports) = converted(&save(), PLACEMENTS);
    // The sharing and the empty Text's import font are reported once.
    for report in owned(&[EMPTY_TEXT, (MASTER, SHARED_EDITING)]) {
        assert_eq!(
            reports.iter().filter(|&found| *found == report).count(),
            1,
            "{reports:?}"
        );
    }
    let placed = layers(&document);
    let [i1, i2, backdrop, canvas] = placed else {
        panic!("two placements, the backdrop and the canvas: {placed:?}");
    };
    assert_eq!(
        (&backdrop["name"], &canvas["name"]),
        (&json!("Premiere still 1"), &json!("Premiere black canvas"))
    );
    // Each placement's own clip Motion moves a group of its own copy, over its
    // own range: I1 at Position 0.35:0.45 and Scale 80, I2 at 0.7:0.65, Scale
    // 60 and Rotation 15, both about the frame centre.
    for (placement, index, range, expected) in [
        (
            i1,
            2,
            json!({"start": 0, "duration": 3000}),
            [
                json!([672.0, 486.0]),
                json!([960.0, 540.0]),
                json!([80.0, 80.0]),
                json!(0.0),
            ],
        ),
        (
            i2,
            3,
            json!({"start": 5000, "duration": 3000}),
            [
                json!([1344.0, 702.0]),
                json!([960.0, 540.0]),
                json!([60.0, 60.0]),
                json!(15.0),
            ],
        ),
    ] {
        assert_eq!(
            (&placement["type"], &placement["name"]),
            (
                &json!("Group"),
                &json!(format!("Premiere graphic Motion {index}"))
            )
        );
        assert_eq!(*crate::test_support::layer_range(placement), range);
        assert_eq!(motion(placement), expected.each_ref());
        assert_eq!(placement["transform"]["opacity"], json!(100.0));
        assert_eq!(placement.get("isHidden"), None);
        let [content] = placement["layers"].as_array().unwrap().as_slice() else {
            panic!("a Motion group holds one root: {placement}");
        };
        assert_eq!(content["parent"], placement["id"]);
        assert_eq!(
            *crate::test_support::layer_range(content),
            json!({"start": 0, "duration": 3000})
        );
        assert_shared_objects(content, index);
    }
    // Only assets could be shared: every layer has an id of its own, and no
    // placement has keys.
    let mut ids = Vec::new();
    layer_ids(placed, &mut ids);
    assert_eq!(ids.len(), 10);
    assert_distinct_ids(placed);
    assert_eq!(document["composition"]["dynamics"]["entries"], json!([]));

    // Edit I1's copy only: new text and colour for `py`, and text alone in
    // the empty Text, which keeps its import font. I2's copy is unchanged.
    let mut edited = document.clone();
    let copy = &mut edited["composition"]["layers"][0]["layers"][0]["layers"];
    copy[0]["sourceText"]["text"] = json!("pq");
    copy[0]["sourceText"]["fillColor"] = json!([1.0, 0.0, 0.0, 1.0]);
    copy[1]["sourceText"]["text"] = json!("NEW");
    let edited_layers = layers(&edited);
    assert_eq!(edited_layers[1], *i2);
    let edited_copy = edited_layers[0]["layers"][0]["layers"].as_array().unwrap();
    assert_eq!(
        (
            &edited_copy[0]["sourceText"]["text"],
            &edited_copy[1]["sourceText"]["text"],
            &edited_copy[1]["sourceText"]["fontFamily"]
        ),
        (&json!("pq"), &json!("NEW"), &json!("Inter"))
    );

    // Existing export writes each Motion group as a nested sequence whose
    // placement carries that Motion once the document packages Inter Regular,
    // and the backdrop as a still.
    let root = tempfile::tempdir().unwrap();
    let native = root.path().join("native");
    let omissions = tesseract_to_premiere(
        archive(&edited, root.path(), "edited", &[backdrop_asset()]),
        &native,
        false,
    )
    .unwrap();
    assert_eq!(reasons(omissions), owned(&[]));
    let xml = read_xml(&native.join("project.prproj"));
    let root_sequence = sequence_uid(&xml, "Source graphic 26.5");
    for nest in ["Premiere graphic Motion 2", "Premiere graphic Motion 3"] {
        assert_ne!(sequence_uid(&xml, nest), root_sequence);
    }
    // Each nest reads back as a group that its static Motion moves once,
    // over the placement's range, around its own editable copy of the static
    // text: I1's with the red `pq` and the empty Text's `NEW`, I2's
    // unchanged, both over the whole backdrop. Nothing is omitted.
    let omissions = premiere_to_tesseract(
        native.join("project.prproj"),
        root.path().join("reimported"),
        Some(&root_sequence),
        false,
    )
    .unwrap();
    assert_eq!(reasons(omissions), owned(&[]));
    let reimported = TesseractFile::open(first_project(&root.path().join("reimported")))
        .unwrap()
        .project_json()
        .unwrap();
    let placed = layers(&reimported);
    let [nest_i1, nest_i2, backdrop, canvas] = placed else {
        panic!("two nests, the backdrop and the canvas: {reimported}");
    };
    assert_eq!(
        (&backdrop["type"], &backdrop["activeRange"], &canvas["name"]),
        (
            &json!("Image"),
            &json!({"start": 0, "duration": 10000}),
            &json!("Premiere black canvas")
        )
    );
    let white = json!([1.0, 1.0, 1.0, 1.0]);
    for (nest, placement, texts) in [
        (
            nest_i1,
            &edited_layers[0],
            [("pq", json!([1.0, 0.0, 0.0, 1.0])), ("NEW", white.clone())],
        ),
        (
            nest_i2,
            &edited_layers[1],
            [("py", white.clone()), ("", white.clone())],
        ),
    ] {
        assert_reimported_copy(nest, placement, texts);
    }
    assert_distinct_ids(placed);
    assert_eq!(reimported["composition"]["dynamics"]["entries"], json!([]));
}

#[test]
fn source_graphic_copy_authored_text_keys_reach_the_motion_nest() {
    let (mut document, _) = converted(&save(), PLACEMENTS);
    let copy = &document["composition"]["layers"][0]["layers"][0]["layers"][0];
    let owner = copy["id"].clone();
    let untouched = document["composition"]["layers"][1].clone();
    document["composition"]["dynamics"]["entries"] = json!([{
        "target": {"kind": "layer", "layerId": owner, "propertyType": "opacity"},
        "animator": {"type": "keyframes", "enabled": true, "keyframes": [
            {"id": "copy-opacity-0", "layerTime": 0, "value": {"type": "float", "value": 100.0}, "easing": {"type": "linear"}},
            {"id": "copy-opacity-1", "layerTime": 2000, "value": {"type": "float", "value": 40.0}, "easing": {"type": "linear"}}
        ]}
    }]);
    assert_eq!(document["composition"]["layers"][1], untouched);
    let root = tempfile::tempdir().unwrap();
    let native = root.path().join("native");
    let omissions = tesseract_to_premiere(
        archive(&document, root.path(), "keyed", &[backdrop_asset()]),
        &native,
        false,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let xml = read_xml(&native.join("project.prproj"));
    let sequence = sequence_uid(&xml, "Source graphic 26.5");
    let (restored, reports) = converted(&native.join("project.prproj"), &sequence);
    assert!(reports.is_empty(), "{reports:?}");
    let placed = layers(&restored);
    for (nest, original) in placed.iter().take(2).zip(layers(&document)) {
        assert_reimported_copy(
            nest,
            original,
            [
                ("py", json!([1.0, 1.0, 1.0, 1.0])),
                ("", json!([1.0, 1.0, 1.0, 1.0])),
            ],
        );
    }
    let entries = restored["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 1);
    let keyed_owner = &placed[0]["layers"][0]["layers"][0];
    assert_eq!(entries[0]["target"]["layerId"], keyed_owner["id"]);
    assert_eq!(entries[0]["target"]["propertyType"], "opacity");
    for (key, time, value) in [(0, 0, 100.0), (1, 2000, 40.0)] {
        assert_eq!(entries[0]["animator"]["keyframes"][key]["layerTime"], time);
        assert_eq!(
            entries[0]["animator"]["keyframes"][key]["value"]["value"],
            value
        );
    }
}

/// Checks `nest`, the reimported nest of the exported Motion group
/// `placement`: a group with the placement's name, range and Motion, not
/// hidden, around one identity graphic group on the group clock whose two
/// texts are `py`'s copy at its shared Scale 150, then the empty Text's, each
/// with its (text, fill) of `texts` and its PostScript font. Beside it, the
/// guide that clips the moved nest to its frame.
fn assert_reimported_copy(nest: &Value, placement: &Value, texts: [(&str, Value); 2]) {
    assert_eq!(
        (
            &nest["type"],
            &nest["name"],
            crate::test_support::layer_range(nest)
        ),
        (
            &json!("Group"),
            &placement["name"],
            crate::test_support::layer_range(placement)
        )
    );
    assert_eq!(motion(nest), motion(placement));
    assert_eq!(nest["transform"]["opacity"], json!(100.0));
    assert_eq!(nest.get("isHidden"), None);
    let graphics: Vec<_> = nest["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["name"] != "Nested sequence frame")
        .collect();
    let [graphic] = graphics.as_slice() else {
        panic!("a nest holds its one graphic: {nest}");
    };
    assert_eq!(
        (
            &graphic["type"],
            &graphic["parent"],
            crate::test_support::layer_range(graphic)
        ),
        (
            &json!("Group"),
            &nest["id"],
            &json!({"start": 0, "duration": 3000})
        )
    );
    assert_eq!(
        motion(graphic),
        [
            &json!([0.0, 0.0]),
            &json!([0.0, 0.0]),
            &json!([100.0, 100.0]),
            &json!(0.0)
        ]
    );
    let copy = graphic["layers"].as_array().unwrap();
    assert_eq!(copy.len(), 2, "{graphic}");
    for ((layer, (text, fill)), font) in copy
        .iter()
        .zip(texts)
        .zip(["Arial-BoldMT", "Inter-Regular"])
    {
        assert_eq!(
            (
                &layer["type"],
                &layer["parent"],
                &layer["activeRange"],
                layer.get("isHidden")
            ),
            (
                &json!("Text"),
                &graphic["id"],
                &json!({"start": 0, "duration": 3000}),
                None
            )
        );
        assert_eq!(
            (
                &layer["sourceText"]["text"],
                &layer["sourceText"]["fillColor"],
                &layer["sourceText"]["fontFamily"],
                &layer["sourceText"]["fontStyle"]
            ),
            (&json!(text), &fill, &json!(font), &json!(""))
        );
    }
    assert_eq!(
        (
            &copy[0]["transform"]["position"],
            &copy[0]["transform"]["scale"]
        ),
        (&json!([480.0, 540.0]), &json!([150.0, 150.0]))
    );
}

/// The `ObjectUID` of the one sequence named `name` in a project's XML.
fn sequence_uid(xml: &str, name: &str) -> String {
    let document = roxmltree::Document::parse(xml).unwrap();
    let uids: Vec<_> = document
        .root_element()
        .children()
        .filter(|record| {
            record.has_tag_name("Sequence")
                && record
                    .children()
                    .any(|node| node.has_tag_name("Name") && node.text() == Some(name))
        })
        .filter_map(|record| record.attribute("ObjectUID"))
        .collect();
    let [uid] = uids.as_slice() else {
        panic!("one sequence {name:?}: {uids:?}");
    };
    (*uid).to_owned()
}

#[test]
fn adobe_source_graphic_original_placement_at_the_default_motion_needs_no_group() {
    let (document, reports) = converted(&save(), ORIGINAL);
    assert_eq!(
        reports,
        owned(&[
            EMPTY_TEXT,
            (MASTER, SHARED_EDITING),
            ("VideoClipTrackItem:132 (\"py\")", FONT_NOT_PACKAGED),
            (
                "VideoClipTrackItem:132 (\"Premiere text 1\")",
                INTER_NOT_PACKAGED
            ),
        ])
    );
    // The default Motion needs no group: the graphic group of the shared
    // objects is the root, over the original 0-2 s.
    let [graphic, _canvas] = layers(&document) else {
        panic!("the graphic and the canvas: {document}");
    };
    assert_eq!(
        *crate::test_support::layer_range(graphic),
        json!({"start": 0, "duration": 2000})
    );
    assert_shared_objects(graphic, 1);
}

#[test]
fn a_disabled_source_graphic_placement_imports_hidden() {
    // Supplementary: the save with I2 disabled.
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("disabled.prproj");
    let xml = read_xml(&save());
    let item = "<SubClip ObjectRef=\"179\"/>";
    assert_eq!(xml.matches(item).count(), 1);
    write_prproj(
        &project,
        &xml.replace(item, &format!("{item}\n\t\t\t<IsMuted>true</IsMuted>")),
    );
    std::fs::copy(
        save().with_file_name("tmk_grey128.png"),
        directory.path().join("tmk_grey128.png"),
    )
    .unwrap();
    let (document, _) = converted(&project, PLACEMENTS);
    let [i1, i2, _backdrop, _canvas] = layers(&document) else {
        panic!("two placements, the backdrop and the canvas: {document}");
    };
    // The Motion group hides I2 whole; its content and I1 stay visible.
    assert_eq!((i1.get("isHidden"), &i2["isHidden"]), (None, &json!(true)));
    let content = &i2["layers"][0];
    assert_eq!(content.get("isHidden"), None);
    assert_shared_objects(content, 3);
    assert_eq!(motion(i2)[3], &json!(15.0));
}

/// `xml` with `from`, which occurs once, replaced by `to`.
#[cfg(feature = "ffmpeg-library")]
fn edited(xml: &str, from: &str, to: &str) -> String {
    assert_eq!(xml.matches(from).count(), 1, "{from}");
    xml.replace(from, to)
}

/// The Track Matte Key of `track_matte_key_xml` in `tests/support/animation.rs`
/// (Premiere 14.4's `AE.ADBE Legacy Key Track Matte` and its three
/// parameters) as records 9010 to 9013, with Matte 2, the `Track/ID` of the
/// track of I1 and I2, Composite Using 0 (Matte Alpha) and Reverse false.
#[cfg(feature = "ffmpeg-library")]
const MATTE_ALPHA_KEY: &str = "<VideoFilterComponent ObjectID=\"9010\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"8\"><Component Version=\"6\"><Params Version=\"1\"><Param Index=\"0\" ObjectRef=\"9011\"/><Param Index=\"1\" ObjectRef=\"9012\"/><Param Index=\"2\" ObjectRef=\"9013\"/></Params><ID>3</ID><DisplayName>Track Matte Key</DisplayName><Bypass>false</Bypass><Intrinsic>false</Intrinsic><ArchivedType>0</ArchivedType></Component><MatchName>AE.ADBE Legacy Key Track Matte</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>
<VideoComponentParam ObjectID=\"9011\" ClassID=\"2f2eb0a3-318c-4a93-99fc-f1d319edc864\" Version=\"9\"><Name>Matte:</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>13</ParameterControlType><StartKeyframe>-91445760000000000,2,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>4294967295</UpperBound><ParameterID>1</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID=\"9012\" ClassID=\"6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8\" Version=\"9\"><Name>Composite Using:</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>7</ParameterControlType><StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>1</UpperBound><ParameterID>2</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID=\"9013\" ClassID=\"cc12343e-f113-4d3b-ae05-b287db77d461\" Version=\"9\"><Name>Reverse</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>4</ParameterControlType><StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound><ParameterID>3</ParameterID></VideoComponentParam>";

/// Supplementary: the save in `directory` with I2 at the default Motion as
/// the Matte Alpha of a 1920x1080 video placement (records 9001 to 9006 and
/// [`MATTE_ALPHA_KEY`]) over I2's 5-8 s on the track below, which takes I2's
/// saved Motion (component 208) when `moved`. The grey still there ends at
/// 5 s, before the video.
#[cfg(feature = "ffmpeg-library")]
fn keyed_by_i2(directory: &Path, moved: bool) -> PathBuf {
    let chain = "<VideoComponentChain ObjectID=\"178\" ClassID=\"0970e08a-f58f-4108-b29a-1a717b8e12e2\" Version=\"3\">";
    let xml = edited(
        &read_xml(&save()),
        chain,
        &format!("{chain}\n\t\t<DefaultMotion>true</DefaultMotion>"),
    );
    let xml = edited(&xml, "<Component Index=\"0\" ObjectRef=\"208\"/>", "");
    let xml = edited(&xml, "<End>2540160000000</End>", "<End>1270080000000</End>");
    let item = "<TrackItem Index=\"0\" ObjectRef=\"144\"/>";
    let xml = edited(
        &xml,
        item,
        &format!("{item}<TrackItem Index=\"1\" ObjectRef=\"9001\"/>"),
    );
    let (defaults, components) = if moved {
        (
            "",
            "<Component Index=\"0\" ObjectRef=\"208\"/><Component Index=\"1\" ObjectRef=\"9010\"/>",
        )
    } else {
        (
            "<DefaultMotion>true</DefaultMotion>",
            "<Component Index=\"0\" ObjectRef=\"9010\"/>",
        )
    };
    let records = format!(
        "<VideoClipTrackItem ObjectID=\"9001\"><ClipTrackItem><ComponentOwner><Components ObjectRef=\"9002\"/></ComponentOwner><TrackItem><Start>1270080000000</Start><End>2032128000000</End></TrackItem><SubClip ObjectRef=\"9003\"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio></VideoClipTrackItem>
<VideoComponentChain ObjectID=\"9002\">{defaults}<DefaultOpacity>true</DefaultOpacity><ComponentChain><Components>{components}</Components></ComponentChain></VideoComponentChain>
<SubClip ObjectID=\"9003\"><Clip ObjectRef=\"9004\"/><Name>Keyed</Name></SubClip>
<VideoClip ObjectID=\"9004\"><Clip><Source ObjectRef=\"9005\"/><InPoint>0</InPoint><OutPoint>762048000000</OutPoint></Clip></VideoClip>
<VideoMediaSource ObjectID=\"9005\"><MediaSource><Media ObjectURef=\"keyed-media\"/></MediaSource><OriginalDuration>2540160000000</OriginalDuration></VideoMediaSource>
<Media ObjectUID=\"keyed-media\"><VideoStream ObjectRef=\"9006\"/><RelativePath>keyed.mp4</RelativePath></Media>
<VideoStream ObjectID=\"9006\"><Duration>2540160000000</Duration><FrameRate>8467200000</FrameRate><FrameRect>0,0,1920,1080</FrameRect></VideoStream>
{MATTE_ALPHA_KEY}
</PremiereData>"
    );
    let project = directory.join("keyed.prproj");
    write_prproj(&project, &edited(&xml, "</PremiereData>", &records));
    for (fixture, name) in [
        ("tmk_grey128.png", "tmk_grey128.png"),
        ("video-30fps-10s.mp4", "keyed.mp4"),
    ] {
        std::fs::copy(save().with_file_name(fixture), directory.join(name)).unwrap();
    }
    project
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn a_default_motion_source_graphic_matte_keys_its_clip_as_its_whole_root() {
    for moved in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let (document, _) = converted(&keyed_by_i2(directory.path(), moved), PLACEMENTS);
        let placed = layers(&document);
        assert_distinct_ids(placed);
        // I2's matte is the group of both its objects, which the keyed video
        // consumes whole: under the stage group that its Motion needs, or
        // beside a flat one.
        let (i1, root, consumer) = if moved {
            let [i1, _backdrop, stage, _canvas] = placed else {
                panic!("I1, the backdrop, the stage and the canvas: {document}");
            };
            assert_eq!(
                (
                    crate::test_support::layer_range(stage),
                    motion(stage)[0],
                    motion(stage)[3]
                ),
                (
                    &json!({"start": 5000, "duration": 3000}),
                    &json!([1344.0, 702.0]),
                    &json!(15.0)
                )
            );
            let [video, root] = stage["layers"].as_array().unwrap().as_slice() else {
                panic!("the video and its matte: {stage}");
            };
            assert_eq!(
                (&video["type"], &video["parent"]),
                (&json!("Video"), &stage["id"])
            );
            assert_eq!(
                (&root["parent"], crate::test_support::layer_range(root)),
                (&stage["id"], &json!({"start": 0, "duration": 3000}))
            );
            (i1, root, stage)
        } else {
            let [i1, root, _backdrop, video, _canvas] = placed else {
                panic!("I1, I2's root, the backdrop, the video and the canvas: {document}");
            };
            assert_eq!(
                (root.get("parent"), crate::test_support::layer_range(root)),
                (None, &json!({"start": 5000, "duration": 3000}))
            );
            (i1, root, video)
        };
        assert_eq!(
            consumer["trackMatte"],
            json!({"mode": "alpha", "layer": root["id"]}),
            "moved {moved}"
        );
        assert_eq!(
            (&root["name"], root["layers"].as_array().map(Vec::len)),
            (&json!("Premiere graphic 4"), Some(2))
        );
        // I1, unrelated to the key, keeps its own Motion group.
        assert_eq!(motion(i1)[0], &json!([672.0, 486.0]));

        // Existing export does not write a graphic matte, so it omits the
        // keyed video with its matte and writes I1 and the still.
        let root = tempfile::tempdir().unwrap();
        let assets = [
            (
                "premiere-image-1",
                directory.path().join("tmk_grey128.png"),
                AssetKind::Image,
            ),
            (
                "premiere-video-2",
                directory.path().join("keyed.mp4"),
                AssetKind::Video,
            ),
        ];
        let omissions = tesseract_to_premiere(
            archive(&document, root.path(), "keyed", &assets),
            root.path().join("native"),
            false,
        )
        .unwrap();
        let unexported_matte = "the nested track matte source must be an unkeyed, neutral group of one full-frame video without effects, masks or sound";
        let duration = (
            "document.duration",
            "duration differs from the last occurrence; exported duration is 1270080000000 ticks",
        );
        let expected = if moved {
            vec![
                (
                    format!("layer {} (\"Premiere stage 2\")", consumer["id"]),
                    format!("stage group was not exported as one clip: {unexported_matte}"),
                ),
                (duration.0.to_owned(), duration.1.to_owned()),
            ]
        } else {
            vec![
                (
                    format!("layer {} (\"Premiere video 2\")", consumer["id"]),
                    format!("masks cannot be exported: {unexported_matte}; occurrence omitted"),
                ),
                (
                    format!("layer {} (\"Premiere graphic 4\")", placed[1]["id"]),
                    "track matte source of no exported clip was not exported; FX draws it only through the clips that it keys".to_owned(),
                ),
                (duration.0.to_owned(), duration.1.to_owned()),
            ]
        };
        assert_eq!(reasons(omissions), expected, "moved {moved}");
    }
}

/// The records of the measured A4 Transform of
/// `feature_transform_track_matte_26_5_strict` (component 114: Linear
/// Position keys at source 0.5 and 1.5 s, every other value the default, and
/// its parameters 145 to 156) as records 9020 to 9032.
#[cfg(feature = "ffmpeg-library")]
fn a4_transform() -> String {
    let source =
        read_xml(&save().with_file_name("feature_transform_track_matte_26_5_strict.prproj"));
    let document = roxmltree::Document::parse(&source).unwrap();
    let mut records = String::new();
    for (from, to) in (145..=156)
        .map(|id| (id, id - 145 + 9021))
        .chain([(114, 9020)])
    {
        let node = document
            .root_element()
            .children()
            .find(|node| node.attribute("ObjectID") == Some(&from.to_string()))
            .unwrap();
        records.push_str(&source[node.range()].replace(
            &format!("ObjectID=\"{from}\""),
            &format!("ObjectID=\"{to}\""),
        ));
    }
    for id in 145..=156 {
        records = records.replace(
            &format!("ObjectRef=\"{id}\""),
            &format!("ObjectRef=\"{}\"", id - 145 + 9021),
        );
    }
    records
}

/// Supplementary: [`keyed_by_i2`] at the default Motion, whose keyed video
/// also carries the measured A4 Transform after its Alpha key, as the fixture
/// saves the pair. A4 is measured over a static media matte, and I2 is a
/// graphic: only the Transform is omitted, so the video keeps its key on I2's
/// whole root exactly as without the Transform, and I1 is unchanged.
#[cfg(feature = "ffmpeg-library")]
#[test]
fn a_transform_on_a_clip_keyed_by_a_source_graphic_is_omitted_and_its_key_kept() {
    let directory = tempfile::tempdir().unwrap();
    let project = keyed_by_i2(directory.path(), false);
    let (plain, plain_reports) = converted(&project, PLACEMENTS);
    let xml = edited(
        &read_xml(&project),
        "<Component Index=\"0\" ObjectRef=\"9010\"/></Components>",
        "<Component Index=\"0\" ObjectRef=\"9010\"/><Component Index=\"1\" ObjectRef=\"9020\"/></Components>",
    );
    write_prproj(
        &project,
        &edited(
            &xml,
            "</PremiereData>",
            &format!("{}</PremiereData>", a4_transform()),
        ),
    );
    let (document, reports) = converted(&project, PLACEMENTS);
    assert_eq!(document, plain);
    let omitted = (
        "VideoClipTrackItem:9001".to_owned(),
        "Transform with Track Matte Key requires a static matte with no active native effects, including effects without a mapping (measured A4); Transform omitted, existing Track Matte Key retained".to_owned(),
    );
    let mut expected = plain_reports;
    let before_fonts = expected
        .iter()
        .position(|(_, reason)| reason == FONT_NOT_PACKAGED)
        .unwrap();
    expected.insert(before_fonts, omitted);
    assert_eq!(reports, expected);
    let placed = layers(&document);
    let [i1, root, _backdrop, video, _canvas] = placed else {
        panic!("I1, I2's root, the backdrop, the video and the canvas: {document}");
    };
    assert_eq!(
        (&video["trackMatte"], &video["transform"]["position"]),
        (
            &json!({"mode": "alpha", "layer": root["id"]}),
            &json!([960.0, 540.0])
        )
    );
    assert_shared_objects(root, 4);
    assert_eq!(motion(i1)[0], &json!([672.0, 486.0]));
    assert_eq!(document["composition"]["dynamics"]["entries"], json!([]));
}
