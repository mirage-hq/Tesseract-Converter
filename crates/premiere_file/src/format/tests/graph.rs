use crate::format::*;
use crate::schema::native::{Media, Reference};

#[test]
fn rejects_foreign_roots_and_dtd() {
    for xml in [
        "<notPremiere/>",
        "<!DOCTYPE PremiereData [<!ENTITY x 'x'>]><PremiereData>&x;</PremiereData>",
    ] {
        assert!(Graph::parse(xml).is_err(), "{xml}");
    }
}

#[test]
fn rejects_empty_and_duplicate_identities() {
    for attribute in ["ObjectID", "ObjectUID"] {
        for content in [
            format!("<A {attribute}=''/>"),
            format!("<A {attribute}='1'/><B {attribute}='1'/>"),
            format!("<A {attribute}='a&amp;b'/><B {attribute}='a&#38;b'/>"),
        ] {
            let xml = format!("<PremiereData>{content}</PremiereData>");
            assert!(Graph::parse(&xml).is_err(), "{xml}");
        }
    }
}

#[test]
fn indexes_only_root_records_and_resolves_decoded_references() {
    let graph = Graph::parse(
        "<PremiereData><A ObjectID='a&amp;b'><Nested ObjectID='hidden'/></A><B ObjectUID='a&#38;b'/><Link ObjectRef='a&#38;b'/></PremiereData>",
    ).unwrap();
    let records: Vec<_> = graph.records().collect();
    assert_eq!(
        records.iter().map(Record::tag).collect::<Vec<_>>(),
        ["A", "B", "Link"]
    );
    for (id, uid, target) in [(Some("a&b"), None, 0), (None, Some("a&b"), 1)] {
        let reference = Reference {
            id: id.map(str::to_owned),
            uid: uid.map(str::to_owned),
            index: None,
        };
        assert_eq!(graph.locate(&reference, "test").unwrap(), records[target]);
    }
    let nested = Reference {
        id: Some("hidden".into()),
        uid: None,
        index: None,
    };
    assert!(graph.locate(&nested, "test").is_err());
}

#[test]
fn rejects_dangling_and_ambiguous_references() {
    for attributes in [
        "ObjectRef='missing'",
        "ObjectURef='missing'",
        "ObjectRef='1' ObjectURef='one'",
    ] {
        let xml = format!("<PremiereData><A ObjectID='1'/><B ObjectUID='one'/><Link {attributes}/></PremiereData>");
        let graph = Graph::parse(&xml).unwrap();
        assert!(
            graph
                .decode::<serde::de::IgnoredAny>(graph.records().last().unwrap())
                .is_err(),
            "{xml}"
        );
    }
}

#[test]
fn ignores_private_ui_references_but_not_semantic_ones() {
    assert!(Graph::parse("<PremiereData><Project ObjectID='1'><Node><Properties><Properties><Private/></Properties><Columns.List ObjectRef='99'/></Properties></Node></Project></PremiereData>").is_ok());
    let graph = Graph::parse(
        "<PremiereData><Project ObjectID='1'><Source ObjectRef='99'/></Project></PremiereData>",
    )
    .unwrap();
    assert!(graph
        .decode::<serde::de::IgnoredAny>(graph.records().next().unwrap())
        .is_err());
}

#[test]
fn regenerated_settings_do_not_admit_other_dangling_references() {
    assert!(Graph::parse("<PremiereData><ProjectSettings ObjectID='1'><VideoSettings ObjectRef='12'/><AudioSettings ObjectRef='13'/><VideoCompileSettings ObjectRef='14'/><AudioCompileSettings ObjectRef='15'/></ProjectSettings></PremiereData>").is_ok());
    for xml in [
        "<PremiereData><ProjectSettings ObjectID='1'><VideoSettings ObjectRef='13'/></ProjectSettings></PremiereData>",
        "<PremiereData><ProjectSettings ObjectID='1'><Source ObjectRef='12'/></ProjectSettings></PremiereData>",
        "<PremiereData><Project ObjectID='1'><VideoSettings ObjectRef='12'/></Project></PremiereData>",
    ] {
        let graph = Graph::parse(xml).unwrap();
        assert!(graph.decode::<serde::de::IgnoredAny>(graph.records().next().unwrap()).is_err(), "{xml}");
    }
}

#[test]
fn typed_edges_reject_wrong_target_type() {
    let graph = Graph::parse("<PremiereData><Clip ObjectID='1'/></PremiereData>").unwrap();
    let reference = Reference {
        id: Some("1".into()),
        uid: None,
        index: None,
    };
    let error = graph
        .follow::<Media>(&reference, "test")
        .unwrap_err()
        .to_string();
    assert!(error.contains("expected Media, found Clip:1"), "{error}");
}

#[test]
fn xml_larger_than_the_former_byte_quota_parses() {
    const FORMER_MAX_XML_BYTES: usize = 32 * 1024 * 1024;
    let xml = format!(
        "<PremiereData>{}</PremiereData>",
        " ".repeat(FORMER_MAX_XML_BYTES)
    );
    assert!(xml.len() > FORMER_MAX_XML_BYTES);
    Graph::parse(&xml).unwrap();
}

#[test]
fn xml_depth_boundary() {
    // Document + PremiereData + 125 elements + text = 128 levels.
    for (elements, accepted) in [(125, true), (126, false)] {
        let xml = format!(
            "<PremiereData>{}text{}</PremiereData>",
            "<A>".repeat(elements),
            "</A>".repeat(elements)
        );
        assert_eq!(Graph::parse(&xml).is_ok(), accepted, "{elements}");
    }
}

#[test]
fn xml_larger_than_the_former_object_quota_parses() {
    const OBJECTS: usize = 100_001;
    let mut xml = String::from("<PremiereData>");
    for id in 0..OBJECTS {
        use std::fmt::Write;
        write!(xml, "<A ObjectID='{id}'/>").unwrap();
    }
    xml.push_str("</PremiereData>");

    let graph = Graph::parse(&xml).unwrap();
    assert_eq!(graph.records().count(), OBJECTS);
    let last = Reference {
        id: Some((OBJECTS - 1).to_string()),
        uid: None,
        index: None,
    };
    assert_eq!(graph.locate(&last, "test").unwrap().tag(), "A");
}

#[test]
fn xml_larger_than_the_former_node_quota_parses() {
    let xml = format!("<PremiereData>{}</PremiereData>", "<A/>".repeat(500_001));
    let graph = Graph::parse(&xml).unwrap();
    assert_eq!(graph.records().count(), 500_001);
}

#[test]
fn gzip_larger_than_the_former_compressed_and_expanded_quotas_decodes() {
    use std::io::Write;
    const FORMER_MAX_XML_BYTES: usize = 32 * 1024 * 1024;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input.prproj");
    let expanded = vec![b' '; FORMER_MAX_XML_BYTES + 1];
    let mut zip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::none());
    zip.write_all(&expanded).unwrap();
    std::fs::write(&path, zip.finish().unwrap()).unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() > FORMER_MAX_XML_BYTES as u64);
    let actual = read_xml(&path).unwrap();
    assert_eq!(actual.len(), expanded.len());
    assert!(actual.as_bytes() == expanded);
}

#[test]
fn gzip_truncation_still_fails() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input.prproj");
    std::fs::write(&path, [0x1f, 0x8b, 8]).unwrap();
    assert!(read_xml(&path).is_err());
}

#[test]
fn decode_errors_carry_the_record_identity() {
    let graph = Graph::parse("<PremiereData><Media ObjectUID='media-1'><RelativePath><Nested/></RelativePath></Media></PremiereData>").unwrap();
    let error = graph
        .decode::<Media>(graph.records().next().unwrap())
        .unwrap_err()
        .to_string();
    assert!(error.contains("Media:media-1"), "{error}");
}
