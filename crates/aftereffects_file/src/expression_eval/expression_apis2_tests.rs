//! Second Adobe-native expression-API fixture (AE 26.5x89, managed `run_jsx`):
//! Source Text strings, 3D layer space through AE's default camera and Shape
//! `sourceRectAtTime`, compared with Adobe's evaluated values at all frames.

use super::*;
use crate::expression_samples::PropertyIdentity;
use sha2::{Digest, Sha256};

const SOURCE: &[u8] =
    include_bytes!("../../tests/fixtures/expression_samples/expression_apis2.aep");
const READBACK: &str =
    include_str!("../../tests/fixtures/expression_samples/expression_apis2.readback.json");
const SOURCE_SHA256: &str = "b222051ea8b0aa583e7846383e7a1476f5c9fdd2cd5223f43adfda694befd205";

#[test]
fn expression_apis2_match_adobe_evaluated_values() {
    assert_eq!(format!("{:x}", Sha256::digest(SOURCE)), SOURCE_SHA256);
    let project = crate::structure::read_project(SOURCE).unwrap();
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    let readback: serde_json::Value = serde_json::from_str(READBACK).unwrap();
    for case in readback["cases"].as_array().unwrap() {
        let comp_id = case["composition_id"].as_u64().unwrap() as u32;
        let layer_id = case["layer_id"].as_u64().unwrap() as u32;
        let match_name = case["match_name"].as_str().unwrap();
        let crate::structure::ItemKind::Composition(comp) = &project.item(comp_id).unwrap().kind
        else {
            panic!("composition {comp_id} missing");
        };
        let model = Model::new(&items, comp_id, comp, true);
        let identity = if match_name == "ADBE Text Document" {
            PropertyIdentity::SourceText {}
        } else {
            PropertyIdentity::Transform {
                match_name: match_name.into(),
            }
        };
        let slot = model
            .properties
            .iter()
            .position(|p| p.comp_id == comp_id && p.layer_id == layer_id && p.identity == identity)
            .unwrap_or_else(|| panic!("{} {match_name} not modelled", case["layer"]));
        let samples = case["samples"].as_array().unwrap();
        let times: Vec<f64> = samples
            .iter()
            .map(|s| s["time"].as_f64().unwrap())
            .collect();
        let label = format!("{} {match_name}", case["layer"]);
        if matches!(identity, PropertyIdentity::SourceText {}) {
            let texts = evaluate_text_grid(&mut prepare(&model).unwrap(), slot, &times)
                .unwrap_or_else(|error| panic!("{label}: {error}"));
            for (sample, text) in samples.iter().zip(&texts) {
                assert_eq!(sample["evaluated"].as_str().unwrap(), text, "{label}");
            }
            continue;
        }
        let actual = evaluate_grid(&mut prepare(&model).unwrap(), slot, &times)
            .unwrap_or_else(|error| panic!("{label}: {error}"));
        let mut max_error = 0.0_f64;
        for (sample, actual) in samples.iter().zip(&actual) {
            let expected: Vec<f64> = match &sample["evaluated"] {
                serde_json::Value::Array(values) => {
                    values.iter().map(|v| v.as_f64().unwrap()).collect()
                }
                value => vec![value.as_f64().unwrap()],
            };
            for (a, e) in actual.iter().zip(&expected) {
                max_error = max_error.max((a - e).abs());
            }
        }
        eprintln!("{label}: max error vs Adobe {max_error:e}");
        assert!(max_error <= 1e-3, "{label}: max error {max_error}");
    }
}

#[test]
fn source_text_data_programs_import_editable_tiled_segments() {
    use crate::{aep::Project, rifx::Chunk};

    fn replace(chunks: &mut [Chunk]) -> usize {
        let mut count = 0;
        for chunk in chunks {
            if chunk.id() == *b"Utf8"
                && chunk
                    .data_payload()
                    .is_some_and(|bytes| bytes.starts_with(b"Math.round(linear(time,0,2,0,100))"))
            {
                *chunk = Chunk::data(
                    *b"Utf8",
                    br#"
                    var source=thisComp.layer('Timer').text.sourceText;
                    var repeats=2;
                    try { repeats=effect('Optional Tiles')(1); } catch(error) { repeats=2; }
                    var token=/\S+/.exec(source.value);
                    var record={text:token[0].substr(0),start:token.index};
                    var result='';
                    for(var row=0;row<2;row++) {
                        for(var column=0;column<repeats;column++) result+=record.text+'  ';
                        if(row<1) result+='\r';
                    }
                    source.getStyleAt(record.start).setText(result);
                "#
                    .to_vec(),
                )
                .unwrap();
                count += 1;
            } else if let Some(children) = chunk.children_mut() {
                count += replace(children);
            }
        }
        count
    }

    // Code-mutated public source: editable lowering, not new Adobe-native proof.
    let mut file = Project::parse(SOURCE).unwrap();
    assert_eq!(replace(&mut file.chunks), 1);
    let project = crate::structure::read_project(&file.encode().unwrap()).unwrap();
    let converted =
        crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
    let json = converted.document.to_json_value().unwrap().to_string();
    for text in [
        "0.00s  0.00s  \n0.00s  0.00s  ",
        "0.50s  0.50s  \n0.50s  0.50s  ",
    ] {
        assert!(
            json.contains(&format!(
                "\"text\":{}",
                serde_json::to_string(text).unwrap()
            )),
            "missing editable tile {text:?}: {:?}",
            converted.diagnostics
        );
    }
    assert!(!json.to_ascii_lowercase().contains("jsscript"));
    assert!(
        converted
            .diagnostics
            .iter()
            .any(|d| d.message.contains("sourceText:setText"))
    );
    assert!(
        converted
            .diagnostics
            .iter()
            .any(|d| d.message.contains("setText style not lowered"))
    );
}

#[test]
fn source_text_data_programs_copy_static_character_style() {
    use crate::{aep::Project, rifx::Chunk};
    use fx_schema::{Layer, LayerData, TextDocument};

    fn replace(chunks: &mut [Chunk]) {
        for chunk in chunks {
            if let Some(children) = chunk.children_mut() {
                if children.iter().any(|child| {
                    child.id() == *b"Utf8"
                        && child
                            .data_payload()
                            .is_some_and(|bytes| bytes.starts_with(b"time.toFixed(2)"))
                }) {
                    let flags = children
                        .iter_mut()
                        .find(|child| child.id() == *b"tdb4")
                        .unwrap();
                    let mut bytes = flags.data_payload().unwrap().to_vec();
                    bytes[119] |= 1;
                    *flags = Chunk::data(*b"tdb4", bytes).unwrap();
                }
                replace(children);
            } else if chunk.id() == *b"Utf8"
                && chunk
                    .data_payload()
                    .is_some_and(|bytes| bytes.starts_with(b"Math.round(linear(time,0,2,0,100))"))
            {
                *chunk = Chunk::data(
                    *b"Utf8",
                    b"thisComp.layer('Timer').text.sourceText.getStyleAt(0).setText('Styled')"
                        .to_vec(),
                )
                .unwrap();
            }
        }
    }
    fn document(layers: &[Layer], by_text: bool, name: &str) -> Option<TextDocument> {
        layers.iter().find_map(|layer| match layer.data() {
            LayerData::Text(text)
                if (by_text && text.source_text.text == name)
                    || (!by_text && (name.is_empty() || layer.name() == name)) =>
            {
                Some(text.source_text.clone())
            }
            LayerData::Group(group) => document(
                &group.layers,
                by_text,
                if !by_text && layer.name() == name {
                    ""
                } else {
                    name
                },
            ),
            _ => None,
        })
    }
    let mut file = Project::parse(SOURCE).unwrap();
    replace(&mut file.chunks);
    let project = crate::structure::read_project(&file.encode().unwrap()).unwrap();
    let converted =
        crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
    let layers = converted.document.composition().layers();
    let source = document(layers, false, "Timer").unwrap();
    let copied = document(layers, true, "Styled").unwrap_or_else(|| {
        panic!(
            "{:?}",
            converted
                .diagnostics
                .iter()
                .filter(|d| d.message.contains("Source") || d.message.contains("sourceText"))
                .collect::<Vec<_>>()
        )
    });
    assert_eq!(source.font_family, "Helvetica");
    assert_eq!(copied.font_family, source.font_family);
    assert_eq!(copied.font_style, source.font_style);
    assert_eq!(copied.font_size, source.font_size);
    assert_eq!(copied.fill_color, source.fill_color);
    assert!(
        converted
            .diagnostics
            .iter()
            .any(|d| d.message.contains("copies static uniform character style"))
    );
}

/// A fresh import turns the evaluated Source Text strings into held text
/// segments: every distinct Adobe string appears as editable text content.
#[test]
fn source_text_expressions_import_as_held_text_segments() {
    let project = crate::structure::read_project(SOURCE).unwrap();
    let converted =
        crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
    let json = converted.document.to_json_value().unwrap().to_string();
    let readback: serde_json::Value = serde_json::from_str(READBACK).unwrap();
    for case in readback["cases"].as_array().unwrap() {
        if case["match_name"] != "ADBE Text Document" {
            continue;
        }
        for sample in case["samples"].as_array().unwrap() {
            let text = sample["evaluated"].as_str().unwrap();
            assert!(
                json.contains(&format!("\"text\":\"{text}\"")),
                "{}: missing held text {text:?}",
                case["layer"]
            );
        }
    }
    assert!(
        converted.diagnostics.iter().any(|d| d
            .message
            .contains("Source Text expression evaluated by the converter")),
        "missing Source Text evaluation diagnostic"
    );
    assert!(!json.to_ascii_lowercase().contains("jsscript"));
}
