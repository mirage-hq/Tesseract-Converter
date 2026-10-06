//! Intrinsic control metadata and initial values; licensed originals stay outside Git.

use super::support::*;
use premiere_file::PrProjectFile;
#[cfg(feature = "ffmpeg-library")]
use serde_json::{json, Value};
use std::path::Path;
#[cfg(feature = "ffmpeg-library")]
use tesseract_file::TesseractFile;

/// Isolate the original component and its parameter records verbatim, on the
/// existing one-second media control. This is offline derived structure, not a
/// newly authored native project or an independent render oracle.
fn isolated_opacity(source: &Path, component_id: &str) -> String {
    let xml = read_xml(source);
    let document = roxmltree::Document::parse(&xml).unwrap();
    let record = |id: &str| {
        document
            .root_element()
            .children()
            .find(|node| node.attribute("ObjectID") == Some(id))
            .unwrap()
    };
    let component = record(component_id);
    assert!(!component
        .children()
        .any(|node| node.has_tag_name("SubComponents")));
    let mut records = xml[component.range()].to_owned();
    for parameter in component
        .descendants()
        .filter(|node| node.has_tag_name("Param"))
    {
        records.push_str(&xml[record(parameter.attribute("ObjectRef").unwrap()).range()]);
    }
    one_second()
        .replace(
            "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>",
            &format!("<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><ComponentChain><Components><Component ObjectRef=\"{component_id}\"/></Components></ComponentChain></VideoComponentChain>"),
        )
        .replace("</PremiereData>", &format!("{records}</PremiereData>"))
}

#[cfg(feature = "ffmpeg-library")]
fn convert_opacity(xml: &str) -> Value {
    let dir = tempfile::tempdir().unwrap();
    let source = fixture(dir.path(), xml);
    let output = dir.path().join("converted");
    let omissions = premiere_to_tesseract(&source, &output, None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    TesseractFile::open(first_project(&output))
        .unwrap()
        .project_json()
        .unwrap()
}

/// Mutations of the public native fixture cover metadata admission in CI.
fn legacy_control() -> String {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_opacity_screen_strict.prproj");
    isolated_opacity(&source, "200")
}

fn full_range_bounds(xml: &str) -> String {
    xml.replace("<LowerBound>0</LowerBound><UpperBound>100</UpperBound>", "<LowerBound>-3.4028234663852886e+38</LowerBound><UpperBound>3.4028234663852886e+38</UpperBound>")
        .replace("<LowerBound>0</LowerBound><UpperBound>26</UpperBound>", "<LowerBound>-2147483648</LowerBound><UpperBound>2147483647</UpperBound>")
        .replace("<LowerBound>0</LowerBound><UpperBound>31</UpperBound>", "<LowerBound>-2147483648</LowerBound><UpperBound>2147483647</UpperBound>")
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn opacity_legacy_controls_accept_saved_27_and_typed_bounds() {
    let control = legacy_control();
    let variants = [
        control.clone(),
        control.replace("<UpperBound>26</UpperBound>", "<UpperBound>27</UpperBound>"),
        full_range_bounds(&control),
    ];
    for xml in variants {
        let document = convert_opacity(&xml);
        let videos = video_layers(&document);
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0]["transform"]["opacity"], json!(50.0));
        assert_eq!(videos[0]["blendMode"], "normal");
    }
}

/// Derived from the pinned 26.5 save on the portable one-second media fixture.
/// Only parameter control metadata and static-value framing are varied below.
#[cfg(feature = "ffmpeg-library")]
fn animation_value_control() -> String {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_motion_opacity_26_5_strict.prproj");
    let xml = read_xml(&source);
    let document = roxmltree::Document::parse(&xml).unwrap();
    let mut records = String::new();
    for id in ["181", "182"] {
        let component = document
            .root_element()
            .children()
            .find(|node| node.attribute("ObjectID") == Some(id))
            .unwrap();
        assert!(!component
            .children()
            .any(|node| node.has_tag_name("SubComponents")));
        records.push_str(&xml[component.range()]);
        for parameter in component
            .descendants()
            .filter(|node| node.has_tag_name("Param"))
        {
            let record = document
                .root_element()
                .children()
                .find(|node| node.attribute("ObjectID") == parameter.attribute("ObjectRef"))
                .unwrap();
            let mut wire = xml[record.range()].to_owned();
            if record.attribute("ObjectID") == Some("228") {
                wire = wire.replace(",100.,", ",108.953247070312,");
            }
            if record.attribute("ObjectID") == Some("230") {
                // A singleton boolean key must not disturb numeric Motion keys.
                wire = wire.replace(
                    "</StartKeyframe>",
                    "</StartKeyframe><Keyframes>0,true,0,0,0,0,0,0;</Keyframes>",
                );
            }
            records.push_str(&wire);
        }
    }
    one_second()
        .replace(
            "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>",
            "<VideoComponentChain ObjectID=\"4\"><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"181\"/><Component Index=\"1\" ObjectRef=\"182\"/></Components></ComponentChain></VideoComponentChain>",
        )
        .replace("</PremiereData>", &format!("{records}</PremiereData>"))
}

#[cfg(feature = "ffmpeg-library")]
fn without_control_metadata(xml: &str) -> String {
    let document = roxmltree::Document::parse(xml).unwrap();
    let mut ranges = document
        .descendants()
        .filter(|node| {
            node.is_element()
                && matches!(
                    node.tag_name().name(),
                    "ParameterControlType"
                        | "LowerBound"
                        | "UpperBound"
                        | "LowerUIBound"
                        | "UpperUIBound"
                )
        })
        .map(|node| node.range())
        .collect::<Vec<_>>();
    let mut result = xml.to_owned();
    ranges.sort_by_key(|range| range.start);
    for range in ranges.into_iter().rev() {
        result.replace_range(range, "");
    }
    result
}

#[cfg(feature = "ffmpeg-library")]
fn assert_animation_values(document: &Value) {
    let videos = video_layers(document);
    assert_eq!(videos.len(), 1, "meaningful editable picture survives");
    let video = videos[0];
    assert_eq!(
        video["transform"]["scale"],
        json!([108.953247070312, 108.953247070312])
    );
    assert_eq!(video["transform"]["opacity"], json!(100.0));
    assert_eq!(video["blendMode"], "normal");
    let dynamics = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(dynamics.len(), 2);
    for (property, values, easing) in [
        ("rotation", [0.0, 25.0], "hold"),
        ("opacity", [100.0, 40.0], "linear"),
    ] {
        let track = dynamics
            .iter()
            .find(|track| track["target"]["propertyType"] == property)
            .unwrap();
        assert_eq!(track["target"]["layerId"], video["id"]);
        let keys = track["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0]["layerTime"], json!(300));
        assert_eq!(keys[1]["layerTime"], json!(700));
        assert_eq!(keys[0]["value"]["value"], json!(values[0]));
        assert_eq!(keys[1]["value"]["value"], json!(values[1]));
        assert_eq!(keys[1]["easing"]["type"], easing);
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn animation_values_keep_picture_and_native_keys_without_optional_control_metadata() {
    let control = animation_value_control();
    let variants = [
        without_control_metadata(&control),
        control.replace(
            "<Keyframes>0,true,0,0,0,0,0,0;</Keyframes>",
            "<IsTimeVarying>true</IsTimeVarying>",
        ),
        control
            .replace(
                "<UpperUIBound>200</UpperUIBound>",
                "<LowerUIBound>7</LowerUIBound><UpperUIBound>100</UpperUIBound>",
            )
            .replace(
                "<ParameterControlType>3</ParameterControlType>",
                "<ParameterControlType>unknown</ParameterControlType>",
            )
            .replace(
                "<UpperBound>10000</UpperBound>",
                "<UpperBound>unavailable</UpperBound>",
            ),
    ];
    for xml in variants {
        assert_ne!(xml, control);
        // Optional UI metadata has no mapped semantics to approximate or omit.
        // convert_opacity asserts that there are no content-loss diagnostics.
        assert_animation_values(&convert_opacity(&xml));
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn animation_values_keep_initial_values_when_unused_static_curve_fields_differ() {
    let control = animation_value_control();
    let document = roxmltree::Document::parse(&control).unwrap();
    let mut edits = document
        .descendants()
        .filter(|node| node.has_tag_name("StartKeyframe"))
        .map(|node| {
            let text = node.text().unwrap();
            let prefix = text.split(',').take(2).collect::<Vec<_>>().join(",");
            (
                node.range(),
                format!("<StartKeyframe>{prefix}</StartKeyframe>"),
            )
        })
        .collect::<Vec<_>>();
    let mut xml = control.clone();
    edits.sort_by_key(|(range, _)| range.start);
    for (range, replacement) in edits.into_iter().rev() {
        xml.replace_range(range, &replacement);
    }
    assert_ne!(xml, control);
    assert_animation_values(&convert_opacity(&xml));
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn animation_values_keep_legacy_opacity_when_control_metadata_differs() {
    let control = legacy_control();
    let typed = full_range_bounds(&control);
    for xml in [
        without_control_metadata(&control),
        control.replace("<UpperBound>26</UpperBound>", "<UpperBound>25</UpperBound>"),
        typed.replace(
            "<UpperBound>2147483647</UpperBound>",
            "<UpperBound>26</UpperBound>",
        ),
        typed.replace(
            "<LowerBound>-3.4028234663852886e+38</LowerBound>",
            "<LowerBound>0</LowerBound>",
        ),
        typed.replace(
            "<ParameterControlType>2</ParameterControlType>",
            "<ParameterControlType>8</ParameterControlType>",
        ),
    ] {
        assert_ne!(xml, typed);
        let document = convert_opacity(&xml);
        let videos = video_layers(&document);
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0]["transform"]["opacity"], json!(50.0));
        assert_eq!(videos[0]["blendMode"], "normal");
    }
}

#[test]
fn animation_values_keep_required_opacity_values_and_identity_checked() {
    let control = legacy_control();
    for (xml, diagnostic) in [
        (
            control.replace(",50.,0,0,0,0,0,0", ",101.,0,0,0,0,0,0"),
            "opacity out of range",
        ),
        (
            control.replace(",50.,0,0,0,0,0,0", ",NaN,0,0,0,0,0,0"),
            "nonfinite initial value",
        ),
        (
            control.replace("<Name>Opacity</Name>", "<Name>Not Opacity</Name>"),
            "unexpected Opacity parameter name",
        ),
        (
            control.replace(
                "<ParameterID>3</ParameterID>",
                "<ParameterID>2</ParameterID>",
            ),
            "duplicate Opacity ParameterID",
        ),
    ] {
        assert_ne!(xml, control);
        let dir = tempfile::tempdir().unwrap();
        let source = fixture(dir.path(), &xml);
        let error = PrProjectFile::load(&source).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{error}");
    }
}
