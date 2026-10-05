//! Intrinsic Opacity metadata variants; licensed originals remain outside Git.

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

#[test]
fn opacity_legacy_metadata_admission_keeps_invalid_bounds_and_values_closed() {
    let control = legacy_control();
    let typed = full_range_bounds(&control);
    for xml in [
        control.replace("<UpperBound>26</UpperBound>", "<UpperBound>25</UpperBound>"),
        typed.replace(
            "<UpperBound>2147483647</UpperBound>",
            "<UpperBound>26</UpperBound>",
        ),
        typed.replace(
            "<LowerBound>-3.4028234663852886e+38</LowerBound>",
            "<LowerBound>0</LowerBound>",
        ),
        typed.replace(",50.,0,0,0,0,0,0", ",101.,0,0,0,0,0,0"),
        typed.replace(
            "<ParameterControlType>2</ParameterControlType>",
            "<ParameterControlType>8</ParameterControlType>",
        ),
    ] {
        assert_ne!(xml, typed);
        let dir = tempfile::tempdir().unwrap();
        let source = fixture(dir.path(), &xml);
        let error = PrProjectFile::load(&source).unwrap_err().to_string();
        assert!(
            error.contains("Opacity parameter layout") || error.contains("opacity out of range"),
            "{error}"
        );
    }
}
