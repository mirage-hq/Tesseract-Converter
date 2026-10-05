use super::*;
use std::io::Write;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

fn aep_member(bytes: &[u8], nested: bool) -> Result<Vec<u8>, CapsuleError> {
    super::aep_member(Cursor::new(bytes), nested)
}

fn zip32_footer(bytes: &[u8]) -> Result<Zip32Footer, CapsuleError> {
    super::zip32_footer(&mut Cursor::new(bytes))
}

fn encoded_json(value: &serde_json::Value) -> String {
    STANDARD.encode(
        serde_json::to_string(value)
            .unwrap()
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
}

fn synthetic_components() -> String {
    let value = serde_json::json!({
        "capPropFontEdit":false,"capPropFontFauxStyleEdit":false,"capPropFontSizeEdit":false,
        "capPropTextRunCount":1,"fontEditValue":["ArialMT"],"fontSizeEditValue":[24.0],
        "fontTextRunLength":[2],"fontFSAllCapsValue":[false],"fontFSBoldValue":[false],
        "fontFSItalicValue":[false],"fontFSSmallCapsValue":[false],"textEditValue":"🦊"
    });
    let mut control = value.clone();
    let fields = control.as_object_mut().unwrap();
    fields.insert("capPropAnimatable".into(), false.into());
    fields.insert("capPropDefault".into(), "default".into());
    fields.insert("capPropMatchName".into(), "synthetic-controller".into());
    fields.insert("capPropType".into(), 0.into());
    fields.insert("capPropUIName".into(), "Caption".into());
    let private = encoded_json(&serde_json::json!({"capsuleparams":{"capParams":[control]},
        "framesize":{"size":{"x":320.0,"y":100.0},"topleft":{"x":0.0,"y":0.0}}}));
    let current = encoded_json(&value);
    format!(
        r#"<PremiereData>
        <VideoFilterComponent ObjectID="1"><Component><Params><Param ObjectRef="3"/></Params></Component><MatchName>AE.ADBE Capsule</MatchName><PremiereFilterPrivateData Encoding="base64" BinaryHash="synthetic-frame">{private}</PremiereFilterPrivateData></VideoFilterComponent>
        <VideoFilterComponent ObjectID="2"><Component><Params><Param ObjectRef="3"/></Params></Component><MatchName>AE.ADBE Capsule</MatchName><PremiereFilterPrivateData Encoding="base64" BinaryHash="synthetic-frame"/></VideoFilterComponent>
        <ArbVideoComponentParam ObjectID="3"><Name>Caption</Name><ParameterControlType>23</ParameterControlType><ParameterID>0</ParameterID><StartKeyframePosition>{}</StartKeyframePosition><StartKeyframeValue Encoding="base64">{current}</StartKeyframeValue></ArbVideoComponentParam>
    </PremiereData>"#,
        records::STATIC_KEYFRAME_TIME
    )
}

// Supplemental wire-only regression: native type 4 stores a UTF-16 string and
// type 8 stores a semicolon-terminated UUID list, not a TextValue JSON object.
// Generic content replaces proprietary template text/identities. This is not
// an independently authored Adobe fixture or a render-fidelity assertion.
fn non_text_components(label: &str, children: &str) -> String {
    let private = encoded_json(&serde_json::json!({
        "capsuleparams":{"capParams":[
            {"capPropAnimatable":false,"capPropDefault":"default label",
             "capPropMatchName":"label-controller","capPropType":4,"capPropUIName":""},
            {"capPropAnimatable":false,"capPropDefault":["label-controller"],
             "capPropGroupExpanded":false,"capPropMatchName":"group-controller",
             "capPropType":8,"capPropUIName":"Settings"}
        ]},
        "framesize":{"size":{"x":320.0,"y":100.0},"topleft":{"x":0.0,"y":0.0}}
    }));
    let utf16 = |text: &str| {
        STANDARD.encode(
            text.encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
    };
    format!(
        r#"<PremiereData>
        <VideoFilterComponent ObjectID="1"><Component><Params><Param ObjectRef="2"/><Param ObjectRef="3"/></Params></Component><MatchName>AE.ADBE Capsule</MatchName><PremiereFilterPrivateData Encoding="base64">{private}</PremiereFilterPrivateData></VideoFilterComponent>
        <ArbVideoComponentParam ObjectID="2"><IsTimeVarying>false</IsTimeVarying><ParameterControlType>24</ParameterControlType><ParameterID>0</ParameterID><StartKeyframePosition>{time}</StartKeyframePosition><StartKeyframeValue Encoding="base64" BinaryHash="label-value">{label}</StartKeyframeValue></ArbVideoComponentParam>
        <ArbVideoComponentParam ObjectID="3"><Name>Settings</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>11</ParameterControlType><ParameterID>1</ParameterID><StartKeyframePosition>{time}</StartKeyframePosition><StartKeyframeValue Encoding="base64">{children}</StartKeyframeValue></ArbVideoComponentParam>
    </PremiereData>"#,
        time = records::STATIC_KEYFRAME_TIME,
        label = utf16(label),
        children = utf16(children)
    )
}

#[test]
fn capsule_saved_type4_type8_decode_without_text_style_fields() {
    let xml = non_text_components("Current 🦊 label", "label-controller;");
    let saved = SavedCapsule::from_xml(&xml, "1").unwrap();
    assert_eq!(saved.size, CapsulePoint { x: 320.0, y: 100.0 });
    assert_eq!(saved.controls.len(), 2);
    assert_eq!(saved.controls[0].controller_uuid, "label-controller");
    assert_eq!(saved.controls[0].parameter, "ArbVideoComponentParam:2");
    assert_eq!(
        saved.controls[0].value,
        CapsuleValue::String("Current 🦊 label".into())
    );
    assert_eq!(saved.controls[1].controller_uuid, "group-controller");
    assert_eq!(saved.controls[1].parameter, "ArbVideoComponentParam:3");
    assert_eq!(
        saved.controls[1].value,
        CapsuleValue::Group {
            children: vec!["label-controller".into()],
            expanded: false,
        }
    );
}

fn change_layout_declaration(xml: &str, change: impl FnOnce(&mut serde_json::Value)) -> String {
    let stored = xml
        .split("<PremiereFilterPrivateData Encoding=\"base64\">")
        .nth(1)
        .unwrap()
        .split('<')
        .next()
        .unwrap();
    let graph = Graph::parse(xml).unwrap();
    let encoded = EncodedValue {
        encoding: "base64".into(),
        binary_hash: None,
        value: stored.into(),
    };
    let mut value: serde_json::Value = decode_json(&graph, &encoded, "test").unwrap();
    change(&mut value);
    xml.replace(stored, &encoded_json(&value))
}

#[test]
fn capsule_optional_layout_fields_do_not_discard_values_but_binding_mismatches_fail() {
    let xml = non_text_components("Current label", "label-controller;");
    let unknown = change_layout_declaration(&xml, |private| {
        private["capsuleparams"]["capParams"][0]["capPropType"] = 99.into();
    });
    let saved = SavedCapsule::from_xml(&unknown, "1").unwrap();
    assert_eq!(saved.controls.len(), 2);
    assert!(matches!(
        saved.controls[0].value,
        CapsuleValue::Unsupported { kind: 99 }
    ));
    assert!(saved
        .diagnostics
        .iter()
        .any(|message| message.contains("99")));
    for supplemental in [
        change_layout_declaration(&xml, |private| {
            private["capsuleparams"]["capParams"][0]["fontSizeEditValue"] = serde_json::json!([24]);
        }),
        change_layout_declaration(&xml, |private| {
            private["capsuleparams"]["capParams"][1]["unknownField"] = true.into();
        }),
        xml.replace("<Name>Settings</Name>", "<Name>Different</Name>"),
    ] {
        let saved = SavedCapsule::from_xml(&supplemental, "1").unwrap();
        assert_eq!(
            saved.controls[0].value,
            CapsuleValue::String("Current label".into())
        );
        assert!(matches!(
            saved.controls[1].value,
            CapsuleValue::Group { .. }
        ));
        assert!(!saved.diagnostics.is_empty());
    }
    for supplemental in [
        xml.replace(
            "<ParameterControlType>24</ParameterControlType>",
            "<ParameterControlType>23</ParameterControlType>",
        ),
        xml.replace(
            "<IsTimeVarying>false</IsTimeVarying>",
            "<IsTimeVarying>true</IsTimeVarying>",
        ),
        xml.replace(
            "<ParameterID>1</ParameterID>",
            "<Keyframes>key</Keyframes><ParameterID>1</ParameterID>",
        ),
    ] {
        let saved = SavedCapsule::from_xml(&supplemental, "1").unwrap();
        assert!(!saved.diagnostics.is_empty());
    }
    for invalid in [
        change_layout_declaration(&xml, |private| {
            private["capsuleparams"]["capParams"][1]["capPropMatchName"] =
                "label-controller".into();
        }),
        xml.replace(
            "<ParameterID>1</ParameterID>",
            "<ParameterID>0</ParameterID>",
        ),
    ] {
        assert!(SavedCapsule::from_xml(&invalid, "1").is_err());
    }
}

#[test]
fn capsule_saved_type8_preserves_effective_members_and_rejects_bad_bindings() {
    // The current value, not the stale declaration's membership, binds the group.
    let xml = non_text_components("Current label", "label-controller;");
    let changed = change_layout_declaration(&xml, |private| {
        private["capsuleparams"]["capParams"][1]["capPropDefault"] = serde_json::json!([]);
    });
    assert_eq!(
        SavedCapsule::from_xml(&changed, "1").unwrap().controls[1].value,
        CapsuleValue::Group {
            children: vec!["label-controller".into()],
            expanded: false
        }
    );
    for value in [
        "label-controller",
        "label-controller;;",
        "label-controller;label-controller;",
        "absent-controller;",
        "group-controller;",
    ] {
        assert!(SavedCapsule::from_xml(&non_text_components("label", value), "1").is_err());
    }
    let cycle = change_layout_declaration(
        &non_text_components("group-controller;", "label-controller;"),
        |private| {
            let control = &mut private["capsuleparams"]["capParams"][0];
            control["capPropType"] = 8.into();
            control["capPropDefault"] = serde_json::json!(["group-controller"]);
            control["capPropGroupExpanded"] = true.into();
        },
    )
    .replace(
        "<ParameterControlType>24</ParameterControlType>",
        "<ParameterControlType>11</ParameterControlType>",
    );
    assert!(SavedCapsule::from_xml(&cycle, "1")
        .unwrap_err()
        .to_string()
        .contains("cyclic"));
}

#[test]
fn capsule_saved_type4_absent_value_is_only_the_empty_saved_profile() {
    let xml = non_text_components("Current label", "label-controller;");
    let start = xml
        .find("<StartKeyframeValue Encoding=\"base64\" BinaryHash=\"label-value\">")
        .unwrap();
    let end =
        start + xml[start..].find("</StartKeyframeValue>").unwrap() + "</StartKeyframeValue>".len();
    let missing = format!("{}{}", &xml[..start], &xml[end..]);
    let saved = SavedCapsule::from_xml(&missing, "1").unwrap();
    assert!(matches!(
        saved.controls[0].value,
        CapsuleValue::Unsupported { kind: 4 }
    ));
    assert!(!saved.diagnostics.is_empty());
    let empty = change_layout_declaration(&missing, |private| {
        private["capsuleparams"]["capParams"][0]["capPropDefault"] = "".into();
    });
    assert_eq!(
        SavedCapsule::from_xml(&empty, "1").unwrap().controls[0].value,
        CapsuleValue::String(String::new())
    );
}

#[test]
fn capsule_older_text_profile_retains_supported_fields_and_diagnoses_optional_styles() {
    let xml = non_text_components("Current 🦊 title", "label-controller;");
    let xml = change_layout_declaration(&xml, |private| {
        let control = &mut private["capsuleparams"]["capParams"][0];
        control["capPropType"] = 0.into();
        control["capPropFontEditInfo"] = serde_json::json!({
            "capPropFontEdit":false,"capPropFontSizeEdit":false,"capPropFontFauxStyleEdit":false,
            "fontEditValue":"ArialMT","fontSizeEditValue":24,"fontFSAllCapsValue":true,
            "fontFSBoldValue":true,"fontFSItalicValue":false,"fontFSSmallCapsValue":false,
            "unknownDecoration":true
        });
    })
    .replace(
        "<ParameterControlType>24</ParameterControlType>",
        "<ParameterControlType>23</ParameterControlType>",
    );
    let saved = SavedCapsule::from_xml(&xml, "1").unwrap();
    let CapsuleValue::Text(value) = &saved.controls[0].value else {
        panic!("expected Text")
    };
    assert_eq!(value.text, "Current 🦊 title");
    assert_eq!(value.font.as_deref(), Some("ArialMT"));
    assert_eq!(value.size, Some(24.0));
    assert_eq!(value.all_caps, Some(true));
    assert!(saved
        .diagnostics
        .iter()
        .any(|message| message.contains("unknownDecoration")));
    assert!(saved
        .diagnostics
        .iter()
        .any(|message| message.contains("faux")));
}

#[test]
fn capsule_saved_numeric_forms_keep_native_values_and_reject_nonfinite_overrides() {
    for (kind, control_type, wire, expected) in [
        (1, "8", "37.", SavedGraphicNumeric::Scalar(37.0)),
        (2, "4", "true", SavedGraphicNumeric::Scalar(1.0)),
        (5, "3", "-25.5", SavedGraphicNumeric::Scalar(-25.5)),
        (
            6,
            "6",
            "0.5:0.25",
            SavedGraphicNumeric::Point([960.0, 270.0]),
        ),
        (
            3,
            "5",
            "72337973781266176",
            SavedGraphicNumeric::ColourRgb([1.0; 3]),
        ),
    ] {
        let xml = format!("<PremiereData><VideoComponentParam ObjectID=\"1\"><ParameterControlType>{control_type}</ParameterControlType><IsTimeVarying>false</IsTimeVarying><StartKeyframe>{},{wire},0,0,0,0,0,0</StartKeyframe></VideoComponentParam></PremiereData>", records::STATIC_KEYFRAME_TIME);
        let graph = Graph::parse(&xml).unwrap();
        let record = graph
            .locate(
                &Reference {
                    id: Some("1".into()),
                    uid: None,
                    index: None,
                },
                "test",
            )
            .unwrap();
        assert_eq!(
            saved_numeric(
                record.element(),
                kind,
                CapsulePoint {
                    x: 1920.0,
                    y: 1080.0
                }
            )
            .unwrap(),
            expected
        );
        let invalid = xml.replace(wire, "NaN");
        let graph = Graph::parse(&invalid).unwrap();
        let record = graph
            .locate(
                &Reference {
                    id: Some("1".into()),
                    uid: None,
                    index: None,
                },
                "test",
            )
            .unwrap();
        assert!(saved_numeric(
            record.element(),
            kind,
            CapsulePoint {
                x: 1920.0,
                y: 1080.0
            }
        )
        .is_err());
    }
}

fn archive(members: &[(&str, &[u8])], compression: CompressionMethod) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in members {
        writer
            .start_file(
                *name,
                SimpleFileOptions::default().compression_method(compression),
            )
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[test]
fn capsule_archive_selection_and_unsafe_members_fail_closed() {
    // Framing controls use arbitrary synthetic bytes, not a native AEP payload.
    let bytes = b"synthetic member";
    let graphic = archive(&[("source.aep", bytes)], CompressionMethod::Stored);
    assert_eq!(aep_member(&graphic, false).unwrap(), bytes);
    let mogrt = archive(
        &[("nested.aegraphic", &graphic)],
        CompressionMethod::Deflated,
    );
    assert_eq!(aep_member(&mogrt, false).unwrap(), bytes);
    for members in [
        vec![("one.aep", bytes.as_slice()), ("two.aep", bytes.as_slice())],
        vec![("../source.aep", bytes.as_slice())],
        vec![("preview.png", bytes.as_slice())],
        vec![("nested.aegraphic", mogrt.as_slice())],
    ] {
        assert!(aep_member(&archive(&members, CompressionMethod::Stored), false).is_err());
    }
    // ZIP writers reject duplicate names; mutate same-length directory names
    // in a synthetic archive to exercise the decoder's duplicate check.
    let duplicate = archive(
        &[("one.aep", bytes), ("two.aep", bytes)],
        CompressionMethod::Stored,
    );
    let mut duplicate = duplicate;
    for offset in 0..duplicate.len() - 6 {
        if &duplicate[offset..offset + 7] == b"two.aep" {
            duplicate[offset..offset + 7].copy_from_slice(b"one.aep");
        }
    }
    assert!(aep_member(&duplicate, false).is_err());
}

fn archive_comment(mut bytes: Vec<u8>, comment: &[u8]) -> Vec<u8> {
    let footer = bytes.len() - ZIP32_FOOTER_SIZE;
    assert_eq!(&bytes[footer..footer + 4], ZIP32_FOOTER_MAGIC);
    bytes[footer + 20..footer + 22]
        .copy_from_slice(&u16::try_from(comment.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(comment);
    bytes
}

#[test]
fn capsule_archive_guard_accepts_comments_and_original_member_bytes() {
    // Footer magic in member data must stay visible to decompression/CRC, but
    // cannot become an alternate metadata footer.
    let payload = b"synthetic member with PK\x05\x06 bytes";
    let graphic = archive(&[("source.aep", payload)], CompressionMethod::Stored);
    for comment in [
        b"ordinary comment".as_slice(),
        &vec![b'x'; usize::from(u16::MAX)],
    ] {
        assert_eq!(
            aep_member(&archive_comment(graphic.clone(), comment), false).unwrap(),
            payload
        );
    }
    let prefixed = [b"prefix".as_slice(), graphic.as_slice()].concat();
    assert_eq!(aep_member(&prefixed, false).unwrap(), payload);
}

#[test]
fn capsule_archive_guard_rejects_malformed_and_ambiguous_footers() {
    let graphic = archive(
        &[("source.aep", b"synthetic member")],
        CompressionMethod::Stored,
    );
    let footer = graphic.len() - ZIP32_FOOTER_SIZE;
    let mut malformed = Vec::new();
    malformed.push(graphic[..graphic.len() - 1].to_vec());
    malformed.push([graphic.as_slice(), b"trailing byte"].concat());
    malformed.push(archive_comment(graphic.clone(), ZIP32_FOOTER_MAGIC));
    for (offset, value) in [(4, 1), (8, 0), (10, u16::MAX), (20, u16::MAX)] {
        let mut bytes = graphic.clone();
        bytes[footer + offset..footer + offset + 2].copy_from_slice(&value.to_le_bytes());
        malformed.push(bytes);
    }
    let mut too_many = graphic.clone();
    for offset in [8, 10] {
        too_many[footer + offset..footer + offset + 2].copy_from_slice(&33u16.to_le_bytes());
    }
    malformed.push(too_many);
    let mut zip64 = graphic[..footer].to_vec();
    zip64.extend_from_slice(b"PK\x06\x07");
    zip64.extend_from_slice(&[0; 16]);
    zip64.extend_from_slice(&graphic[footer..]);
    malformed.push(zip64);
    // A zero declaration must not conceal existing directory records.
    let mut hidden_directory = graphic.clone();
    hidden_directory[footer + 8..footer + 12].fill(0);
    malformed.push(hidden_directory);
    let directory = usize::try_from(u32::from_le_bytes(
        graphic[footer + 16..footer + 20].try_into().unwrap(),
    ))
    .unwrap();
    for (offset, value) in [
        (20, vec![u8::MAX; 4]),
        (28, vec![u8::MAX; 2]),
        (34, vec![1, 0]),
    ] {
        let mut bytes = graphic.clone();
        bytes[directory + offset..directory + offset + value.len()].copy_from_slice(&value);
        malformed.push(bytes);
    }
    for bytes in malformed {
        assert!(aep_member(&bytes, false).is_err());
    }
}

#[test]
fn capsule_archive_guard_does_not_retry_an_earlier_footer() {
    let earlier = archive(&[("old.aep", b"old")], CompressionMethod::Stored);
    let mut later = archive(&[("new.aep", &[0; 128])], CompressionMethod::Stored);
    let footer = later.len() - ZIP32_FOOTER_SIZE;
    let directory = usize::try_from(u32::from_le_bytes(
        later[footer + 16..footer + 20].try_into().unwrap(),
    ))
    .unwrap();
    // Syntactically framed metadata that ZIP rejects: AES without its extra
    // field. The unrestricted constructor retries the earlier valid archive.
    later[directory + 10..directory + 12].copy_from_slice(&99u16.to_le_bytes());
    let bytes = [earlier.as_slice(), later.as_slice()].concat();
    let unguarded = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    assert!(unguarded.central_directory_start() < u64::try_from(earlier.len()).unwrap());
    assert!(zip32_footer(&bytes).is_ok());
    assert!(aep_member(&bytes, false).is_err());
}

// A seekable synthetic ZIP with a virtual unused-media gap. Its selected AEP
// bytes are the unchanged public native fixture. We deliberately never read or
// validate the unused payload; this is admission/editability, not render proof.
struct SparseContainer {
    bytes: Vec<u8>,
    split: usize,
    gap: u64,
    position: u64,
    bytes_read: usize,
}

impl Read for SparseContainer {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        let length = self.bytes.len() as u64 + self.gap;
        let available = usize::try_from(length.saturating_sub(self.position)).unwrap_or(usize::MAX);
        let mut count = output.len().min(available);
        let gap_start = self.split as u64;
        let gap_end = gap_start + self.gap;
        if self.position < gap_start {
            let offset = usize::try_from(self.position).unwrap();
            count = count.min(self.split - offset);
            output[..count].copy_from_slice(&self.bytes[offset..offset + count]);
        } else if self.position < gap_end {
            count = count.min(usize::try_from(gap_end - self.position).unwrap());
            output[..count].fill(0);
        } else {
            let offset = usize::try_from(self.position - self.gap).unwrap();
            output[..count].copy_from_slice(&self.bytes[offset..offset + count]);
        }
        self.position += count as u64;
        self.bytes_read += count;
        Ok(count)
    }
}

impl Seek for SparseContainer {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let position = match position {
            SeekFrom::Start(position) => i128::from(position),
            SeekFrom::End(offset) => {
                self.bytes.len() as i128 + i128::from(self.gap) + i128::from(offset)
            }
            SeekFrom::Current(offset) => i128::from(self.position) + i128::from(offset),
        };
        self.position = u64::try_from(position).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid sparse seek")
        })?;
        Ok(self.position)
    }
}

#[test]
fn capsule_seek_only_large_archive_retains_native_editable_text() {
    let native = include_bytes!(
        "../../../aftereffects_file/tests/fixtures/pr4442_native/sources/text_document_point.aep"
    );
    let names = (0..64)
        .map(|index| format!("preview-{index}.dat"))
        .collect::<Vec<_>>();
    let mut members = names
        .iter()
        .map(|name| (name.as_str(), &[][..]))
        .collect::<Vec<_>>();
    members.push(("source.aep", native));
    let mut bytes = archive(&members, CompressionMethod::Stored);
    let footer = bytes.len() - ZIP32_FOOTER_SIZE;
    let directory = usize::try_from(u32::from_le_bytes(
        bytes[footer + 16..footer + 20].try_into().unwrap(),
    ))
    .unwrap();
    let gap = 400 * 1024 * 1024u32;
    let split = 30
        + usize::from(u16::from_le_bytes(bytes[26..28].try_into().unwrap()))
        + usize::from(u16::from_le_bytes(bytes[28..30].try_into().unwrap()));
    bytes[18..22].copy_from_slice(&gap.to_le_bytes());
    bytes[22..26].copy_from_slice(&gap.to_le_bytes());
    bytes[footer + 16..footer + 20]
        .copy_from_slice(&(u32::try_from(directory).unwrap() + gap).to_le_bytes());
    let mut cursor = directory;
    for index in 0..members.len() {
        if index == 0 {
            bytes[cursor + 20..cursor + 24].copy_from_slice(&gap.to_le_bytes());
            bytes[cursor + 24..cursor + 28].copy_from_slice(&gap.to_le_bytes());
        } else {
            let offset = u32::from_le_bytes(bytes[cursor + 42..cursor + 46].try_into().unwrap());
            bytes[cursor + 42..cursor + 46].copy_from_slice(&(offset + gap).to_le_bytes());
        }
        cursor +=
            46 + usize::from(u16::from_le_bytes(
                bytes[cursor + 28..cursor + 30].try_into().unwrap(),
            )) + usize::from(u16::from_le_bytes(
                bytes[cursor + 30..cursor + 32].try_into().unwrap(),
            )) + usize::from(u16::from_le_bytes(
                bytes[cursor + 32..cursor + 34].try_into().unwrap(),
            ));
    }
    let mut reader = SparseContainer {
        bytes,
        split,
        gap: u64::from(gap),
        position: 0,
        bytes_read: 0,
    };
    let template = decode_template(&mut reader).unwrap();
    assert!(
        reader.bytes_read < 1024 * 1024,
        "unused media was buffered: {} bytes",
        reader.bytes_read
    );
    let mut texts = Vec::new();
    for item in &template.source().items {
        if !matches!(
            item.kind,
            aftereffects_file::structure::ItemKind::Composition(_)
        ) {
            continue;
        }
        let (groups, _) = template
            .editable_layers_in_composition(item.id, &[])
            .unwrap();
        for layer in groups.iter().flat_map(|group| &group.layers) {
            if let fx_schema::LayerData::Text(text) = layer.data() {
                texts.push(text.source_text.clone());
            }
        }
    }
    assert_eq!(texts.len(), 1);
    assert!(!texts[0].text.is_empty());
    assert_eq!(texts[0].font_family.as_ref(), "ArialMT");
}

#[test]
fn capsule_unused_encrypted_media_does_not_block_selected_source() {
    let mut bytes = archive(
        &[("preview.dat", b"unused"), ("source.aep", b"source")],
        CompressionMethod::Stored,
    );
    let footer = bytes.len() - ZIP32_FOOTER_SIZE;
    let directory = usize::try_from(u32::from_le_bytes(
        bytes[footer + 16..footer + 20].try_into().unwrap(),
    ))
    .unwrap();
    // Only the unused member is encrypted. Raw metadata/path inspection does
    // not require its decryption; consumed encryption must still be rejected.
    bytes[6] |= 1;
    bytes[directory + 8] |= 1;
    assert_eq!(aep_member(&bytes, false).unwrap(), b"source");
    let next = directory + 46 + "preview.dat".len();
    bytes[next + 8] |= 1;
    assert!(aep_member(&bytes, false).is_err());
}

#[test]
fn capsule_consumed_member_crc_is_checked() {
    let mut bytes = archive(&[("source.aep", b"source")], CompressionMethod::Stored);
    let data = 30 + "source.aep".len();
    bytes[data] ^= 1;
    assert!(aep_member(&bytes, false).is_err());
}

#[test]
fn capsule_directory_metadata_and_consumed_expansion_remain_bounded() {
    let bytes = archive(&[("source.aep", b"source")], CompressionMethod::Stored);
    let footer = bytes.len() - ZIP32_FOOTER_SIZE;
    let mut oversized_directory = bytes.clone();
    oversized_directory[footer + 12..footer + 16].copy_from_slice(
        &u32::try_from(MAX_DIRECTORY_BYTES + 1)
            .unwrap()
            .to_le_bytes(),
    );
    assert!(zip32_footer(&oversized_directory)
        .unwrap_err()
        .to_string()
        .contains("directory metadata"));
    let directory = usize::try_from(u32::from_le_bytes(
        bytes[footer + 16..footer + 20].try_into().unwrap(),
    ))
    .unwrap();
    let mut oversized_member = bytes;
    oversized_member[directory + 24..directory + 28].copy_from_slice(
        &u32::try_from(MAX_EXPANDED_MEMBER_BYTES + 1)
            .unwrap()
            .to_le_bytes(),
    );
    assert!(aep_member(&oversized_member, false)
        .unwrap_err()
        .to_string()
        .contains("expanded member"));
}

#[test]
fn capsule_synthetic_shared_controls_preserve_effective_values() {
    let components = synthetic_components();
    let first = SavedCapsule::from_xml(&components, "1").unwrap();
    let second = SavedCapsule::from_xml(&components, "2").unwrap();
    assert_eq!(first.controls, second.controls);
    assert_eq!(first.controls[0].controller_uuid, "synthetic-controller");
    let CapsuleValue::Text(value) = &first.controls[0].value else {
        panic!("expected Text")
    };
    assert_eq!(value.text, "🦊");
    assert_eq!(value.font.as_deref(), Some("ArialMT"));
    assert_eq!(value.size, Some(24.0));
    assert_eq!(first.size, CapsulePoint { x: 320.0, y: 100.0 });
}

#[test]
fn capsule_missing_conflicting_hashes_and_keyed_parameters_do_not_choose_defaults() {
    let components = synthetic_components();
    let missing = components.replace("synthetic-frame", "missing");
    // Keep only the empty shared definition; the former inline definition is gone.
    let graph = Graph::parse(&missing).unwrap();
    let stored = graph.binary_value("missing", "test").unwrap().unwrap();
    let missing = missing.replace(stored, "");
    assert!(SavedCapsule::from_xml(&missing, "2").is_err());
    let conflict = components.replace(
        "</PremiereData>",
        "<Other Encoding=\"base64\" BinaryHash=\"synthetic-frame\">e30=</Other></PremiereData>",
    );
    assert!(SavedCapsule::from_xml(&conflict, "1").is_err());
    assert!(SavedCapsule::from_xml(&conflict, "2").is_err());
    let keyed = components.replace(
        "<ParameterID>0</ParameterID>",
        "<IsTimeVarying>true</IsTimeVarying><ParameterID>0</ParameterID>",
    );
    let saved = SavedCapsule::from_xml(&keyed, "1").unwrap();
    assert!(matches!(
        saved.controls[0].value,
        CapsuleValue::Unsupported { kind: 0 }
    ));
    assert!(saved
        .diagnostics
        .iter()
        .any(|message| message.contains("clock")));
}

#[test]
fn capsule_utf16_unicode_and_unknown_fields_remain_discriminating() {
    let mut value = TextValue {
        _font_edit: false,
        _faux_edit: false,
        _size_edit: false,
        run_count: 1,
        fonts: vec!["ArialMT".into()],
        sizes: vec![40.0],
        lengths: vec![2],
        all_caps: vec![false],
        bold: vec![false],
        italic: vec![false],
        small_caps: vec![false],
        text: "🦊".into(),
    };
    assert!(validate_value(&value).is_ok());
    value.lengths[0] = 1;
    assert!(validate_value(&value).is_err());
    let graph = Graph::parse("<PremiereData/>").unwrap();
    let encoded = |bytes: &[u8]| EncodedValue {
        encoding: "base64".into(),
        binary_hash: None,
        value: STANDARD.encode(bytes),
    };
    assert!(decode_json::<TextValue>(&graph, &encoded(&[0]), "test").is_err());
    assert!(decode_json::<TextValue>(&graph, &encoded(&[0x00, 0xd8]), "test").is_err());
    let extra = "{\"unknownController\":true}"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    assert!(decode_json::<TextValue>(&graph, &encoded(&extra), "test").is_err());
}
