//! Structural crash regressions, not Adobe-open or render proof.
use super::{ShapeGradientStop, gradient_colors};
use crate::{aep::Project, rifx::Chunk};
use roxmltree::{Document, Node};
use sha2::{Digest, Sha256};

fn native_gradients() -> Vec<Chunk> {
    // Independent py-aep source, Comp 1 (id 1), 1000x1000, 10s, 24fps.
    let bytes = include_bytes!("../../../tests/fixtures/shapes/gradient.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "e1ba81de12b4dc2a5ef29c1f53a37830eec3cd545225d09b3a35ad4e17d98bee"
    );
    fn collect(chunks: &[Chunk], output: &mut Vec<Chunk>) {
        for chunk in chunks {
            if chunk.list_kind() == Some(*b"GCst") {
                output.push(chunk.clone());
            } else if let Some(children) = chunk.children() {
                collect(children, output);
            }
        }
    }
    let mut output = Vec::new();
    collect(&Project::parse(bytes).unwrap().chunks, &mut output);
    assert_eq!(output.len(), 2, "native gradient stroke and fill");
    output
}

fn edited_stops() -> Vec<ShapeGradientStop> {
    vec![
        ShapeGradientStop {
            offset: 0.0,
            color: [0.1, 0.2, 0.3, 0.25],
        },
        ShapeGradientStop {
            offset: 0.37,
            color: [0.8, 0.6, 0.4, 0.75],
        },
        ShapeGradientStop {
            offset: 1.0,
            color: [0.2, 0.7, 0.9, 1.0],
        },
    ]
}

fn xml(chunk: &Chunk) -> &str {
    let keys = chunk
        .children()
        .unwrap()
        .iter()
        .find(|child| child.list_kind() == Some(*b"GCky"))
        .unwrap();
    let bytes = keys.children().unwrap()[0].data_payload().unwrap();
    std::str::from_utf8(bytes).unwrap()
}

fn elements<'a>(node: Node<'a, 'a>) -> Vec<Node<'a, 'a>> {
    node.children().filter(Node::is_element).collect()
}

fn pair<'a>(list: Node<'a, 'a>, key: &str) -> Node<'a, 'a> {
    assert!(list.has_tag_name("prop.list"));
    let pairs = elements(list);
    for item in &pairs {
        assert!(item.has_tag_name("prop.pair"));
        let children = elements(*item);
        assert_eq!(children.len(), 2);
        assert!(children[0].has_tag_name("key"));
    }
    let matches: Vec<_> = pairs
        .into_iter()
        .filter(|item| elements(*item)[0].text() == Some(key))
        .collect();
    assert_eq!(matches.len(), 1, "unique key {key}");
    elements(matches[0])[1]
}

fn read_arrays(document: &Document<'_>, channel: &str, width: usize) -> Vec<Vec<f64>> {
    let root = document.root_element();
    assert!(root.has_tag_name("prop.map"));
    assert_eq!(root.attribute("version"), Some("4"));
    let root_children = elements(root);
    assert_eq!(root_children.len(), 1);
    let list = root_children[0];
    let version = pair(list, "Gradient Colors");
    assert!(version.has_tag_name("string"));
    assert_eq!(version.text(), Some("1.0"));
    let data = pair(list, "Gradient Color Data");
    let channel_list = pair(data, &format!("{channel} Stops"));
    let size = pair(channel_list, "Stops Size");
    assert!(size.has_tag_name("int"));
    assert_eq!(size.attribute("type"), Some("unsigned"));
    assert_eq!(size.attribute("size"), Some("32"));
    let count: usize = size.text().unwrap().parse().unwrap();
    let stops = pair(channel_list, "Stops List");
    assert_eq!(elements(stops).len(), count);
    (0..count)
        .map(|index| {
            let stop = pair(stops, &format!("Stop-{index}"));
            let array = pair(stop, &format!("Stops {channel}"));
            assert!(array.has_tag_name("array"));
            let values = elements(array);
            assert_eq!(values.len(), width + 1);
            assert!(values[0].has_tag_name("array.type"));
            let kind = elements(values[0]);
            assert_eq!(kind.len(), 1);
            assert!(kind[0].has_tag_name("float"));
            values[1..]
                .iter()
                .map(|value| {
                    assert!(value.has_tag_name("float"));
                    let number: f64 = value.text().unwrap().parse().unwrap();
                    assert!(number.is_finite());
                    number
                })
                .collect()
        })
        .collect()
}

#[test]
fn gradient_stream_descriptor_matches_both_pinned_native_paints() {
    let generated = gradient_colors(&edited_stops()).unwrap();
    let children = generated.children().unwrap();
    assert_eq!(
        children.len(),
        2,
        "GCst needs metadata before its key values"
    );
    assert_eq!(children[0].list_kind(), Some(*b"tdbs"));
    assert_eq!(children[1].list_kind(), Some(*b"GCky"));
    for native in native_gradients() {
        assert_eq!(children[0], native.children().unwrap()[0]);
    }
}

#[test]
fn gradient_xml_accepts_payload_beyond_former_one_mib_quota() {
    let stops: Vec<_> = (0..4_096)
        .map(|index| ShapeGradientStop {
            offset: f64::from(index) / 4_095.0,
            color: [0.1, 0.2, 0.3, 1.0],
        })
        .collect();
    let generated = gradient_colors(&stops).unwrap();
    let xml = xml(&generated);
    assert!(xml.len() > 1_048_576);
    assert!(xml.contains("<int type='unsigned' size='32'>4096</int>"));
}

#[test]
fn gradient_xml_matches_native_grammar_and_preserves_edited_stops() {
    for native in native_gradients() {
        let document = Document::parse(xml(&native)).unwrap();
        assert!(!read_arrays(&document, "Color", 6).is_empty());
        assert!(!read_arrays(&document, "Alpha", 3).is_empty());
    }
    let stops = edited_stops();
    let generated = gradient_colors(&stops).unwrap();
    let document = Document::parse(xml(&generated)).unwrap();
    let colors = read_arrays(&document, "Color", 6);
    let alpha = read_arrays(&document, "Alpha", 3);
    assert_eq!(colors.len(), stops.len());
    assert_eq!(alpha.len(), stops.len());
    for ((color, alpha), stop) in colors.iter().zip(&alpha).zip(&stops) {
        assert_eq!(
            *color,
            [
                stop.offset,
                0.5,
                stop.color[0],
                stop.color[1],
                stop.color[2],
                1.0
            ]
        );
        assert_eq!(*alpha, [stop.offset, 0.5, stop.color[3]]);
    }
}
