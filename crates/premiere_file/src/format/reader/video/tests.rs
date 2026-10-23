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
fn after_effects_link_unknown_colour_domain_preserves_pinned_source_geometry() {
    let original = native_link_xml();
    let changed = original.replace("BT.709 RGB Full", "Future RGB");
    assert_ne!(changed, original);
    let graph = Graph::parse(&changed).unwrap();
    let record = graph
        .locate_uid("cb0ef24b-d73e-428b-b00e-52eb2abb1038", "pinned native link")
        .unwrap();
    let mut omissions = Vec::new();
    let media = read_media(
        &graph,
        graph
            .decode_as::<Media>(record, "source regression")
            .unwrap(),
        &mut omissions,
    )
    .unwrap();
    assert_eq!(
        media.after_effects_composition(),
        linked_source(&original)
            .unwrap()
            .after_effects_composition()
    );
    let video = media.video.as_ref().unwrap();
    assert_eq!((video.width, video.height), (320, 180));
    assert_eq!(video.intrinsic_ticks, 2 * TICKS);
    assert!(omissions.iter().any(|omission| omission
        .to_string()
        .contains("Unrecognized linked-AEP source colour domain")));
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

fn fixture_xml(name: &str) -> String {
    super::super::read_xml(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

/// Read one media record of `xml`, returning its identity and every report.
fn read_clip_media(xml: &str, uid: &str) -> (String, Result<PrMedia>, Vec<Omission>) {
    let graph = Graph::parse(xml).unwrap();
    let record = graph.locate_uid(uid, "clip colour regression").unwrap();
    let media = graph
        .decode_as::<Media>(record, "clip colour regression")
        .unwrap();
    let identity = media.identity.clone();
    let mut omissions = Vec::new();
    let result = read_media(&graph, media, &mut omissions);
    (identity, result, omissions)
}

fn interpret_footage_reports(omissions: &[Omission]) -> Vec<&Omission> {
    omissions
        .iter()
        .filter(|omission| omission.reason.contains("Interpret Footage"))
        .collect()
}

/// Premiere 25.6.6 saved `InputLUTSpecified=true` on the Version 21 picture
/// stream (`VideoStream:887`) of `Dolby Digital Plus AV Sync.mp4` without any
/// LUT path or ID; the H.264 stream `VideoStream:873` of the same save has `false`.
#[test]
fn pinned_premiere_25_clip_input_lut_is_reported_and_media_retained() {
    let xml = fixture_xml("adobe_native_variable_speed_ramp_ppro25.prproj");
    let (record, media, omissions) = read_clip_media(&xml, "be508f92-2679-48da-8d95-255b2f09bd86");
    assert!(media.unwrap().video.is_some());
    let reports = interpret_footage_reports(&omissions);
    assert_eq!(reports.len(), 1, "{omissions:?}");
    assert_eq!(reports[0].record, record);
    assert_eq!(reports[0].scope, OmissionScope::Feature);
    assert_eq!(reports[0].kind, crate::OmissionKind::Omitted);
    assert!(
        reports[0].reason.starts_with(
            "Interpret Footage Input LUT (InputLUTSpecified, InputLUTID absent) not converted"
        ) && reports[0]
            .reason
            .contains("inputEncoding jerboa:sdr-rec709-display:v1"),
        "{}",
        reports[0].reason
    );

    let (_, other, omissions) = read_clip_media(&xml, "11751c92-8dea-4404-9085-ffecede1decd");
    assert!(other.unwrap().video.is_some());
    assert!(
        interpret_footage_reports(&omissions).is_empty(),
        "{omissions:?}"
    );
}

/// The Version 19 stream of `vhsvertical.prproj` (`VideoStream:275`) saves the
/// four Interpret Footage fields at their unset values.
fn vhs_stream_275(xml: &str, from: &str, to: &str) -> String {
    let start = xml.find("<VideoStream ObjectID=\"275\"").unwrap();
    let end = start + xml[start..].find("</VideoStream>").unwrap();
    let stream = &xml[start..end];
    assert_eq!(stream.matches(from).count(), 1, "{from}");
    format!(
        "{}{}{}",
        &xml[..start],
        stream.replace(from, to),
        &xml[end..]
    )
}

const VHS_MEDIA: &str = "c33ae484-d643-4f7d-9f07-ce27598fbbae";

#[test]
fn pinned_version_19_unset_interpret_footage_fields_report_nothing() {
    let xml = fixture_xml("vhsvertical.prproj");
    assert!(xml.contains(
        "<SelectedColorSpace>{}</SelectedColorSpace>\n\t\t<OverriddenColorSpace>{}</OverriddenColorSpace>\n\t\t<InputLUTSpecified>false</InputLUTSpecified>\n\t\t<InputLUTID>00000000-0000-0000-0000-000000000000</InputLUTID>"
    ));
    let (_, media, omissions) = read_clip_media(&xml, VHS_MEDIA);
    assert!(media.unwrap().video.is_some());
    assert!(
        interpret_footage_reports(&omissions).is_empty(),
        "{omissions:?}"
    );
}

/// Mutations of a pinned Version 19 stream: no committed Premiere project
/// saves a non-empty override, so this is structural evidence of the
/// classification, not of Premiere's saved spelling. A video's override with
/// a registered camera encoding is retained for the document's colour
/// management to map (`cameraInput` in ACES, a report in legacy); every
/// other override is reported here.
#[test]
fn clip_color_space_override_names_the_equal_camera_encoding_or_reports_none() {
    let xml = fixture_xml("vhsvertical.prproj");
    let unset = "<OverriddenColorSpace>{}</OverriddenColorSpace>";
    let profile = |name: &str| {
        format!(
            r#"<OverriddenColorSpace>{{"baseColorProfile":{{"colorProfileData":"AQAAAP////8=","colorProfileName":"{name}"}},"baseProfileType":1}}</OverriddenColorSpace>"#
        )
    };
    for (to, expected) in [
        (
            profile("Sony S-Log3/S-Gamut3.Cine"),
            Some("camera:sony-slog3-sgamut3-cine:v1"),
        ),
        (profile("Apple Log/Rec. 2020"), Some("camera:apple-log:v1")),
        (
            profile("Fuji F-Log/Rec. 2020"),
            Some("no registered Tesseract camera encoding has the same curve"),
        ),
        (profile("BT.709,8-bit,Display-Referred"), None),
        (
            profile("BT.2100 PQ,10-bit,Display-Referred"),
            Some("Color Space Override BT.2100 Pq not converted"),
        ),
        (
            "<OverriddenColorSpace>[]</OverriddenColorSpace>".to_owned(),
            Some("unrecognized Interpret Footage OverriddenColorSpace"),
        ),
        // Premiere 26.5.2 flags an active override; an explicit `false`
        // keeps the saved label inactive.
        (
            format!(
                "{}<IsColorSpacOverridden>true</IsColorSpacOverridden>",
                profile("ARRI LogC4/Wide Gamut4")
            ),
            Some("camera:arri-logc4:v1"),
        ),
        (
            format!(
                "{}<IsColorSpacOverridden>false</IsColorSpacOverridden>",
                profile("ARRI LogC4/Wide Gamut4")
            ),
            None,
        ),
    ] {
        let changed = vhs_stream_275(&xml, unset, &to);
        let (record, media, omissions) = read_clip_media(&changed, VHS_MEDIA);
        let video = media.unwrap().video.unwrap();
        let reports = interpret_footage_reports(&omissions);
        if let Some(camera) = expected.filter(|expected| expected.starts_with("camera:")) {
            let retained = video.camera_override.as_ref().expect(&to);
            assert_eq!(retained.encoding, camera);
            assert_eq!(retained.media_record, record);
            assert!(reports.is_empty(), "{to}: {omissions:?}");
            continue;
        }
        assert_eq!(video.camera_override, None, "{to}");
        match expected {
            None => assert!(reports.is_empty(), "{to}: {omissions:?}"),
            Some(expected) => {
                assert_eq!(reports.len(), 1, "{to}: {omissions:?}");
                assert_eq!(reports[0].record, record);
                assert_eq!(reports[0].scope, OmissionScope::Feature);
                assert!(
                    reports[0].reason.contains(expected),
                    "{}",
                    reports[0].reason
                );
            }
        }
    }
}

/// A BT.709 override is the legacy FX default only for an SDR source: on an
/// HLG or PQ source Premiere shows the HDR bytes as Rec.709 while FX decodes
/// them as BT.2100, so the override is reported. The source profiles are the
/// Premiere 26.5.1 HDR `OriginalColorSpace` texts; the override and stream are
/// mutations of the pinned Version 19 stream.
#[test]
fn clip_rec709_override_on_an_hdr_source_is_reported() {
    let xml = fixture_xml("vhsvertical.prproj");
    let unset = "<OverriddenColorSpace>{}</OverriddenColorSpace>";
    let rec709 = r#"<OverriddenColorSpace>{"baseColorProfile":{"colorProfileData":"AQAAAP////8=","colorProfileName":"BT.709,8-bit,Display-Referred"},"baseProfileType":1}</OverriddenColorSpace>"#;
    let original = |name: &str| {
        format!(
            r#"<OriginalColorSpace>{{"baseColorProfile":{{"colorProfileData":"AQAAAP////8=","colorProfileName":"{name}"}},"baseProfileType":1}}</OriginalColorSpace>"#
        )
    };
    for (source, transfer) in [
        ("BT.2100 HLG,10-bit,Display-Referred", Some("Hlg")),
        ("BT.2100 PQ,10-bit,Display-Referred", Some("Pq")),
        ("BT.709,32f,Display-Referred", None),
    ] {
        for (override_text, overridden) in [(rec709, true), (unset, false)] {
            let changed =
                vhs_stream_275(&xml, unset, &format!("{override_text}{}", original(source)));
            let (record, media, omissions) = read_clip_media(&changed, VHS_MEDIA);
            assert!(media.unwrap().video.is_some(), "{source}");
            let reports = interpret_footage_reports(&omissions);
            match transfer.filter(|_| overridden) {
                None => assert!(reports.is_empty(), "{source}: {omissions:?}"),
                Some(transfer) => {
                    assert_eq!(reports.len(), 1, "{source}: {omissions:?}");
                    assert_eq!(reports[0].record, record);
                    assert_eq!(reports[0].scope, OmissionScope::Feature);
                    assert_eq!(reports[0].kind, crate::OmissionKind::Omitted);
                    assert!(
                        reports[0].reason.starts_with(&format!(
                            "Interpret Footage Color Space Override BT.709 on a BT.2100 {transfer} source not converted"
                        )),
                        "{}",
                        reports[0].reason
                    );
                }
            }
        }
    }
}

#[test]
fn clip_input_lut_with_camera_override_names_the_lut_input_encoding() {
    let xml = fixture_xml("vhsvertical.prproj");
    let changed = vhs_stream_275(
        &xml,
        "<OverriddenColorSpace>{}</OverriddenColorSpace>",
        r#"<OverriddenColorSpace>{"baseColorProfile":{"colorProfileData":"AQAAAP////8=","colorProfileName":"Panasonic V-Log/V-Gamut"},"baseProfileType":1}</OverriddenColorSpace>"#,
    );
    let changed = vhs_stream_275(
        &changed,
        "<InputLUTSpecified>false</InputLUTSpecified>\n\t\t<InputLUTID>00000000-0000-0000-0000-000000000000</InputLUTID>",
        "<InputLUTSpecified>true</InputLUTSpecified>\n\t\t<InputLUTID>6f1c2a3b-0000-4000-8000-000000000001</InputLUTID>",
    );
    let changed = vhs_stream_275(
        &changed,
        "<SelectedColorSpace>{}</SelectedColorSpace>",
        r#"<SelectedColorSpace>{"selected":1}</SelectedColorSpace>"#,
    );
    let (_, media, omissions) = read_clip_media(&changed, VHS_MEDIA);
    // Without decodable LUT samples the camera override is retained for the
    // document's colour management, like an override without a LUT.
    let video = media.unwrap().video.unwrap();
    assert_eq!(
        video.camera_override.map(|camera| camera.encoding),
        Some("camera:panasonic-vlog:v1")
    );
    let reasons: Vec<_> = interpret_footage_reports(&omissions)
        .into_iter()
        .map(|omission| omission.reason.as_str())
        .collect();
    assert_eq!(reasons.len(), 2, "{reasons:?}");
    assert!(reasons[0].starts_with("Interpret Footage SelectedColorSpace"));
    assert!(
        reasons[1].starts_with(
            "Interpret Footage Input LUT (InputLUTSpecified, InputLUTID 6f1c2a3b-0000-4000-8000-000000000001) not converted"
        ) && reasons[1].contains("inputEncoding camera:panasonic-vlog:v1"),
        "{}",
        reasons[1]
    );
}
