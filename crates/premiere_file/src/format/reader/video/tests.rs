use super::*;
use crate::schema::{PrMediaKind, TICKS};

fn native_link_xml() -> String {
    super::super::read_xml(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hybrid/native-linked.prproj"),
    )
    .unwrap()
}

fn linked_source(xml: &str) -> Result<PrMedia> {
    let graph = Graph::parse(xml)?;
    let record = graph.locate_uid("cb0ef24b-d73e-428b-b00e-52eb2abb1038", "pinned native link")?;
    read_media(
        &graph,
        graph.decode_as::<Media>(record, "native link regression")?,
        &mut Vec::new(),
    )
}

#[test]
fn native_after_effects_link_is_not_decoded_video() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hybrid");
    for (name, expected) in [
        (
            "native-linked.prproj",
            "a6a7a7a662bd8f2b5b9392cd11e97dde16344ccb55a589a1d902b6f40ac88154",
        ),
        (
            "native-title.aep",
            "d17ed2fd6a118c69e3be3ae41d327a3877036579715dcc6f5dfa8eb7001c9d31",
        ),
    ] {
        assert_eq!(crate::hash::hash(&fixtures.join(name)).unwrap(), expected);
    }
    let media = linked_source(&native_link_xml()).unwrap();
    let video = media.video.as_ref().unwrap();
    assert_eq!((video.width, video.height), (320, 180));
    assert_eq!(video.frame_rate, FrameRate::Fps30.into());
    assert_eq!(video.intrinsic_ticks, 2 * TICKS);
    assert!(
        !matches!(video.kind, PrMediaKind::Video { .. }),
        "AEP must retain its composition identity, not become an ordinary video source"
    );
    assert_eq!(
        media
            .after_effects_composition()
            .unwrap()
            .dynamic_link_guid(),
        "00000001-0000-0000-0000-000000000000"
    );
    assert_eq!(media.relative_path.as_deref(), Some("./native-title.aep"));
    assert!(crate::media::admitted_container(&media, Path::new("native-title.aep")).is_none());
    let error = crate::media::inspect_media(
        video.kind,
        std::io::Cursor::new([]),
        std::io::Cursor::new([]),
        0,
        None,
    )
    .unwrap_err();
    assert!(error.to_string().contains("not a decoded video asset"));
}

#[test]
fn after_effects_link_rejects_unrepresented_stream_semantics() {
    for (from, to, reason) in [
        (
            "<CodecType>1145854285</CodecType>",
            "<CodecType>0</CodecType>",
            "codec",
        ),
        (
            "<AlphaType>1</AlphaType>",
            "<AlphaType>3</AlphaType>",
            "straight alpha",
        ),
        (
            "<OriginalFieldType>4</OriginalFieldType>",
            "<OriginalFieldType>1</OriginalFieldType>",
            "progressive",
        ),
        ("BT.709 RGB Full", "BT.709 YUV", "color interpretation"),
        (
            "<AlphaType>1</AlphaType>",
            "<AlphaType>1</AlphaType><IgnoreAlpha>true</IgnoreAlpha>",
            "straight alpha",
        ),
        (
            "<AlphaType>1</AlphaType>",
            "<AlphaType>1</AlphaType><IsStill>true</IsStill>",
            "still or continuous",
        ),
        ("native-title.aep", "native-title.mp4", "AEP file"),
        (
            "ec341e53-60c2-4d89-abfc-bdb5c0ff2e0b",
            "00000000-0000-0000-0000-000000000000",
            "without its Dynamic Link importer",
        ),
        (
            "<AlphaType>1</AlphaType>",
            "<AlphaType>1</AlphaType><AlphaInfoIsUncertain>true</AlphaInfoIsUncertain>",
            "uncertain",
        ),
        (
            "<Duration>508032000000</Duration>",
            "<Duration>0</Duration>",
            "duration must be positive",
        ),
    ] {
        let original = native_link_xml();
        assert!(original.contains(from));
        let error = linked_source(&original.replace(from, to)).unwrap_err();
        assert!(error.to_string().contains(reason), "{error}");
    }
}

#[test]
fn after_effects_link_requires_exact_non_nil_guid_payload() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let original = native_link_xml();
    let graph = Graph::parse(&original).unwrap();
    let payload = graph
        .records()
        .find(|record| {
            record.tag() == "Media"
                && record
                    .element()
                    .child("ImplementationID")
                    .and_then(|value| value.text())
                    == Some(crate::schema::after_effects::IMPORTER_ID)
        })
        .unwrap()
        .element()
        .child("ImporterPrefs")
        .unwrap()
        .text()
        .unwrap();
    for value in [
        "00000000-0000-0000-0000-000000000000",
        "00000001000000000000000000000000",
        "00000001-0000-0000-0000-00000000000z",
        "00000001-0000-0000-0000-000000000000\0",
    ] {
        let encoded = STANDARD.encode(
            value
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        );
        let error = linked_source(&original.replace(payload, &encoded)).unwrap_err();
        assert!(error.to_string().contains("GUID"), "{error}");
    }
    assert!(linked_source(&original.replace(payload, &"!".repeat(96))).is_err());
}

#[test]
fn after_effects_link_resolves_shared_importer_binary_payload() {
    let original = native_link_xml();
    let graph = Graph::parse(&original).unwrap();
    let media = graph
        .records()
        .find(|record| {
            record.tag() == "Media"
                && record
                    .element()
                    .child("ImplementationID")
                    .and_then(|value| value.text())
                    == Some(crate::schema::after_effects::IMPORTER_ID)
        })
        .unwrap();
    let prefs = media.element().child("ImporterPrefs").unwrap();
    let payload = prefs.text().unwrap();
    let binary_hash = prefs.attribute("BinaryHash").unwrap();
    let shared = format!("<SharedLinkPreferences Encoding=\"base64\" BinaryHash=\"{binary_hash}\">{payload}</SharedLinkPreferences>");
    let xml = original
        .replace(payload, "")
        .replace("</PremiereData>", &format!("{shared}</PremiereData>"));
    assert!(xml.contains(&shared));
    assert_eq!(
        linked_source(&xml)
            .unwrap()
            .after_effects_composition()
            .unwrap()
            .dynamic_link_guid(),
        "00000001-0000-0000-0000-000000000000"
    );
}

#[test]
fn object_mask_sampling_trigger_excludes_nonphysical_owners() {
    let source = include_str!("../../../../tests/fixtures/one-clip.xml");
    let nest = source
        .replace(
            "<VideoMediaSource ObjectID=\"7\">",
            "<VideoSequenceSource ObjectID=\"7\">",
        )
        .replace("</VideoMediaSource>", "</VideoSequenceSource>");
    let adjustment = source.replace(
        "</Clip></VideoClip>",
        "</Clip><AdjustmentLayer>true</AdjustmentLayer></VideoClip>",
    );
    for (name, xml, expected) in [
        ("physical", source.to_owned(), true),
        (
            "still",
            crate::format::tests::still::still_xml(false),
            false,
        ),
        (
            "Color Matte",
            crate::format::tests::color_matte::matte_xml("ZEGlAAEAAAA="),
            false,
        ),
        ("adjustment", adjustment, false),
        ("nest", nest, false),
        ("After Effects", native_link_xml(), false),
    ] {
        let dom = roxmltree::Document::parse(&xml).unwrap();
        let item = dom
            .descendants()
            .find(|n| {
                n.has_tag_name("VideoClipTrackItem")
                // Pinned AE placement69; placement68 is the ordinary-video control.
                && (name != "After Effects" || n.attribute("ObjectID") == Some("69"))
            })
            .unwrap();
        let id = item.attribute("ObjectID").unwrap().to_owned();
        let chain = item
            .descendants()
            .find(|n| n.has_tag_name("Components"))
            .unwrap()
            .attribute("ObjectRef")
            .unwrap();
        let node = dom
            .root_element()
            .children()
            .find(|n| {
                n.has_tag_name("VideoComponentChain") && n.attribute("ObjectID") == Some(chain)
            })
            .unwrap();
        let replacement=format!("<VideoComponentChain ObjectID=\"{chain}\"><DefaultMotion>true</DefaultMotion><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"665\"/></Components></ComponentChain></VideoComponentChain>");
        let mut masked = xml.clone();
        masked.replace_range(node.range(), &replacement);
        masked = masked.replace(
            "</PremiereData>",
            &format!(
                "{}</PremiereData>",
                include_str!("../../../../tests/fixtures/object_mask/opacity.xml")
            ),
        );
        let graph = Graph::parse(&masked).unwrap();
        let reference = Reference {
            id: Some(id),
            uid: None,
            index: None,
        };
        assert_eq!(
            has_saved_raster(&graph, &reference, "test owner").unwrap_or(false),
            expected,
            "{name}"
        );
    }
}
