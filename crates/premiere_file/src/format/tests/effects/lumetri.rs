//! The native Lumetri Color layout and its writer, against the pinned
//! `human_lumetri_contrast.xml` records and the Premiere 26.5.2 save
//! `color_lumetri_basic.prproj`. Writer output is structural and reread by
//! this reader only: no Premiere reopen or render of a written Lumetri ran.
use super::*;
use crate::schema::{
    LumetriDefault, PrLumetriGrade, LUMETRI_BLOB_SENTINEL, LUMETRI_DEFAULTS, LUMETRI_PARAMS,
    LUMETRI_PRIVATE_DATA, LUMETRI_SATURATION, LUMETRI_TEMPERATURE,
};
use std::io::Read;

const NATIVE: &str = include_str!("../../../../tests/fixtures/human_lumetri_contrast.xml");

fn child<'a>(node: roxmltree::Node<'a, '_>, tag: &str) -> Option<&'a str> {
    node.children()
        .find(|child| child.has_tag_name(tag))
        .and_then(|child| child.text())
}

#[test]
fn lumetri_curve_defaults_decode_and_encode_back() {
    use crate::schema::lumetri_default;
    use aftereffects_file::lumetri::{LumetriCurve, CURVE_BYTES};
    use base64::{engine::general_purpose::STANDARD, Engine};
    // RGB Curves: four diagonal curves of kind 0; Hue vs Sat (kind 3) and
    // Luma vs Sat (kind 2) without points. The saved hue curves keep stale
    // points in their unused slots, so they encode back with them cleared.
    for (id, curves, kind, points, exact) in [
        (39, 4, 0, vec![[0.0, 0.0], [1.0, 1.0]], true),
        (42, 1, 3, Vec::new(), false),
        (110, 1, 2, Vec::new(), false),
    ] {
        let Some(LumetriDefault::Binary { value, .. }) = lumetri_default(id) else {
            panic!("{id}");
        };
        let bytes = STANDARD.decode(value).unwrap();
        assert_eq!(bytes.len(), curves * CURVE_BYTES, "{id}");
        for chunk in bytes.chunks(CURVE_BYTES) {
            let curve = LumetriCurve::decode(chunk).unwrap();
            assert_eq!(curve.kind, kind, "{id}");
            assert_eq!(curve.points, points, "{id}");
            let encoded = curve.encode().unwrap();
            assert_eq!(encoded == chunk, exact, "{id}");
            assert_eq!(LumetriCurve::decode(&encoded).unwrap(), curve, "{id}");
        }
    }
    // Not a curve: a wrong length or 33 points. Slots past the count are
    // not read (Premiere 23 leaves the diagonal in an empty Hue vs Sat curve).
    let mut bytes = LumetriCurve {
        kind: 0,
        points: vec![[0.0, 0.0], [1.0, 1.0]],
    }
    .encode()
    .unwrap();
    assert!(LumetriCurve::decode(&bytes[..519]).is_none());
    bytes[519] = 1;
    assert_eq!(LumetriCurve::decode(&bytes).unwrap().points.len(), 2);
    bytes[519] = 0;
    bytes[4] = 33;
    assert!(LumetriCurve::decode(&bytes).is_none());
}

#[test]
fn lumetri_layout_matches_the_pinned_native_records() {
    let document = roxmltree::Document::parse(NATIVE).unwrap();
    let records: Vec<_> = document
        .root_element()
        .children()
        .filter(|node| {
            matches!(
                node.tag_name().name(),
                "VideoComponentParam" | "ArbVideoComponentParam"
            )
        })
        .collect();
    assert_eq!(records.len(), LUMETRI_PARAMS.len());
    for ((record, param), default) in records.iter().zip(LUMETRI_PARAMS).zip(LUMETRI_DEFAULTS) {
        let id = param.id.to_string();
        assert_eq!(child(*record, "ParameterID"), Some(id.as_str()));
        assert_eq!(record.tag_name().name(), param.record.tag, "{id}");
        assert_eq!(
            record.attribute("ClassID"),
            Some(param.record.class_id),
            "{id}"
        );
        assert_eq!(
            record.attribute("Version"),
            Some(param.record.version),
            "{id}"
        );
        assert_eq!(child(*record, "Name").unwrap_or(""), param.name, "{id}");
        assert_eq!(
            child(*record, "ParameterControlType").unwrap_or(""),
            param.control,
            "{id}"
        );
        assert_eq!(
            child(*record, "LowerBound").unwrap_or(""),
            param.lower_bound,
            "{id}"
        );
        assert_eq!(
            child(*record, "UpperBound").unwrap_or(""),
            param.upper_bound,
            "{id}"
        );
        assert_eq!(child(*record, "LowerUIBound"), param.lower_ui_bound, "{id}");
        assert_eq!(child(*record, "UpperUIBound"), param.upper_ui_bound, "{id}");
        assert_eq!(
            child(*record, "DiscontinuousInterpolate") == Some("true"),
            param.discontinuous_interpolate,
            "{id}"
        );
        match default {
            LumetriDefault::Static(start) => {
                // The author's Exposure, Contrast and Saturation edits are
                // reset to Lumetri's neutral values.
                let expected = match param.id {
                    11 | 12 => "-91445760000000000,0.,0,0,0,0,0,0",
                    20 => "-91445760000000000,100.,0,0,0,0,0,0",
                    _ => child(*record, "StartKeyframe").unwrap(),
                };
                assert_eq!(start, expected, "{id}");
            }
            LumetriDefault::Binary { hash, value } => {
                let saved = record
                    .children()
                    .find(|child| child.has_tag_name("StartKeyframeValue"))
                    .unwrap();
                assert_eq!(saved.attribute("BinaryHash"), Some(hash), "{id}");
                assert_eq!(saved.text().unwrap_or("").trim(), value, "{id}");
            }
        }
    }
}

fn grade_effect(enabled: bool) -> PrEffect {
    PrEffect {
        enabled,
        mask: None,
        params: PrEffectParams::LumetriGrade(PrLumetriGrade {
            values: [-90.0, 30.0, 1.5, 25.0, -40.0, 10.0, 5.0, -5.0, 130.0, 20.0],
            vignette: None,
        }),
        // In the grade's control order, as the reader returns them.
        animations: vec![
            PrEffectParamAnimation {
                param: &LUMETRI_TEMPERATURE,
                keys: PrEffectParamKeys::Scalar(vec![key(0, -90.0), key(TICKS / 2, 100.0)]),
            },
            PrEffectParamAnimation {
                param: &LUMETRI_SATURATION,
                keys: PrEffectParamKeys::Scalar(vec![key(0, 130.0), key(TICKS, 60.0)]),
            },
        ],
    }
}

#[test]
fn written_lumetri_grade_and_vignette_reread_with_values_keys_and_defaults() {
    let effects = vec![
        grade_effect(true),
        PrEffect {
            enabled: false,
            mask: None,
            params: PrEffectParams::LumetriVignette([-2.5, 75.0, 20.0]),
            animations: Vec::new(),
        },
    ];
    let xml = project_xml(&project(effects.clone())).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let components: Vec<_> = document
        .root_element()
        .children()
        .filter(|node| {
            node.has_tag_name("VideoFilterComponent")
                && child(*node, "MatchName") == Some("AE.ADBE Lumetri")
        })
        .collect();
    assert_eq!(components.len(), 2);
    for component in &components {
        // The constant native private data, every pinned parameter.
        assert_eq!(
            child(*component, "PremiereFilterPrivateData").map(str::trim),
            Some(LUMETRI_PRIVATE_DATA.1)
        );
        let params = component
            .descendants()
            .filter(|node| node.has_tag_name("Param"))
            .count();
        assert_eq!(params, LUMETRI_PARAMS.len());
    }
    let param = |id: &str| {
        document
            .root_element()
            .children()
            .find(|node| {
                node.has_tag_name("VideoComponentParam") && child(*node, "ParameterID") == Some(id)
            })
            .unwrap()
    };
    assert_eq!(
        child(param("12"), "StartKeyframe"),
        Some("-91445760000000000,25.,0,0,0,0,0,0")
    );
    assert_eq!(child(param("20"), "IsTimeVarying"), Some("true"));
    assert!(child(param("20"), "Keyframes").is_some());
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    // Each written Lumetri carries its replacement note, the grade also its
    // scale approximation, and no unmapped-section difference: the writer's
    // defaults read as defaults.
    assert!(
        omissions
            .iter()
            .all(|omission| omission.kind == OmissionKind::Approximated),
        "{omissions:?}"
    );
    assert_eq!(omissions.len(), 3, "{omissions:?}");
    // Each component rereads as its one mapped section: the neutral section
    // beside it adds no effect.
    let reread = &project.sequences[0].video_tracks[0].clip(0).effects;
    assert_eq!(reread, &effects);
}

#[test]
fn written_lumetri_grade_with_merged_vignette_is_one_component_and_rereads_as_both() {
    let mut grade = grade_effect(true);
    let PrEffectParams::LumetriGrade(params) = &mut grade.params else {
        unreachable!()
    };
    params.vignette = Some([-2.5, 75.0, 20.0]);
    let xml = project_xml(&project(vec![grade.clone()])).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let (_, params) = lumetri_records(&document);
    let start = |id: &str| {
        params
            .iter()
            .find(|node| child(**node, "ParameterID") == Some(id))
            .and_then(|node| child(*node, "StartKeyframe"))
    };
    // Contrast (12) and the Vignette Amount, Midpoint, Feather (51, 52, 54).
    assert_eq!(start("12"), Some("-91445760000000000,25.,0,0,0,0,0,0"));
    assert_eq!(start("51"), Some("-91445760000000000,-2.5,0,0,0,0,0,0"));
    assert_eq!(start("52"), Some("-91445760000000000,75.,0,0,0,0,0,0"));
    assert_eq!(start("54"), Some("-91445760000000000,20.,0,0,0,0,0,0"));
    let (project, _) = inspect_project_with_omissions(&xml, None).unwrap();
    let reread = &project.sequences[0].video_tracks[0].clip(0).effects;
    let mut expected_grade = grade;
    let PrEffectParams::LumetriGrade(params) = &mut expected_grade.params else {
        unreachable!()
    };
    params.vignette = None;
    assert_eq!(
        reread,
        &[
            expected_grade,
            PrEffect {
                enabled: true,
                mask: None,
                params: PrEffectParams::LumetriVignette([-2.5, 75.0, 20.0]),
                animations: Vec::new(),
            },
        ]
    );
}

#[test]
fn written_lumetri_rgb_curves_are_their_own_component_and_reread_exactly() {
    use aftereffects_file::lumetri::RgbCurves;
    let curves = PrEffect {
        enabled: true,
        mask: None,
        params: PrEffectParams::LumetriCurves(RgbCurves {
            curves: [
                vec![
                    [0.0, 0.0],
                    [0.25, 0.15],
                    [0.5, 0.6],
                    [0.75, 0.7],
                    [1.0, 1.0],
                ],
                vec![[0.0, 0.0], [1.0, 1.0]],
                vec![[0.0, 0.0], [0.5, 0.25], [1.0, 1.0]],
                vec![[0.0, 0.2], [1.0, 0.8]],
            ],
        }),
        animations: Vec::new(),
    };
    let effects = vec![grade_effect(true), curves];
    let xml = project_xml(&project(effects.clone())).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let curve_values: Vec<_> = document
        .root_element()
        .children()
        .filter(|node| {
            node.has_tag_name("ArbVideoComponentParam") && child(*node, "ParameterID") == Some("39")
        })
        .filter_map(|node| {
            node.children()
                .find(|value| value.has_tag_name("StartKeyframeValue"))
                .and_then(|value| value.attribute("BinaryHash"))
        })
        .collect();
    // The grade's default diagonal and the written curves, whose hash ends
    // with the byte length plus 12, like Premiere's.
    assert_eq!(curve_values.len(), 2, "{curve_values:?}");
    assert_eq!(curve_values[0], "261f3840-e8cf-a8cd-fa5e-bac10000082c");
    assert!(curve_values[1].ends_with("0000082c"), "{curve_values:?}");
    assert_ne!(curve_values[0], curve_values[1]);
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(
        omissions
            .iter()
            .all(|omission| omission.kind == OmissionKind::Approximated),
        "{omissions:?}"
    );
    let reread = &project.sequences[0].video_tracks[0].clip(0).effects;
    assert_eq!(reread, &effects);
}

#[test]
fn written_lumetri_hue_curves_and_wheels_are_their_own_components_and_reread_exactly() {
    use aftereffects_file::lumetri::{hue_curves::HueSatCurves, Wheels};
    let hue_curves = PrEffect {
        enabled: true,
        mask: None,
        params: PrEffectParams::LumetriHueCurves(HueSatCurves {
            curves: [
                vec![[0.0, 0.0], [0.25, -0.6], [0.5, 0.0]],
                Vec::new(),
                vec![[0.1, 0.2], [0.6, -0.1]],
                vec![[0.0, -0.5], [1.0, 0.0]],
                Vec::new(),
            ],
        }),
        animations: Vec::new(),
    };
    let wheels = PrEffect {
        enabled: true,
        mask: None,
        params: PrEffectParams::LumetriWheels(Wheels {
            wheels: [
                [240.0, 0.5, 0.3],
                [120.0, 0.55, 0.4],
                [30.0, 0.5, 0.3],
                [0.0, 0.5, 0.0],
            ],
        }),
        animations: Vec::new(),
    };
    let effects = vec![grade_effect(true), hue_curves, wheels];
    let xml = project_xml(&project(effects.clone())).unwrap();
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(
        omissions
            .iter()
            .all(|omission| omission.kind == OmissionKind::Approximated),
        "{omissions:?}"
    );
    let reread = &project.sequences[0].video_tracks[0].clip(0).effects;
    assert_eq!(reread, &effects);
}

/// `color_lumetri_basic.prproj` (Premiere 26.5.2, sequence "Lumetri
/// Basic" 5c919df6-dea9-4620-9eb3-611b6fc20602, SHA-256 pinned as in
/// `color_fixtures.provenance.json`), decompressed.
fn color_lumetri_basic_xml() -> String {
    use sha2::{Digest, Sha256};
    let bytes = include_bytes!("../../../../tests/fixtures/color_lumetri_basic.prproj");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "8b78978e027e9b1af0df8a5c06009edd7a8b9bce0c4a6948b94a8d48195ff4a1"
    );
    let mut xml = String::new();
    flate2::read::GzDecoder::new(&bytes[..])
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// The saved values of `color_lumetri_basic`'s grade, in primaryGrade order:
/// Temperature 30, Tint -15, Exposure 0.5, Contrast 30, Highlights -25,
/// Shadows 25, Whites 15, Blacks -15, Saturation 130, Vibrance 0.
const COLOR_LUMETRI_BASIC: [f64; 10] =
    [30.0, -15.0, 0.5, 30.0, -25.0, 25.0, 15.0, -15.0, 130.0, 0.0];

/// The only Lumetri Color in `document`, and its parameter records in its
/// `Params` order.
fn lumetri_records<'a, 'input>(
    document: &'a roxmltree::Document<'input>,
) -> (
    roxmltree::Node<'a, 'input>,
    Vec<roxmltree::Node<'a, 'input>>,
) {
    let root = document.root_element();
    let mut components = root.children().filter(|node| {
        node.has_tag_name("VideoFilterComponent")
            && child(*node, "MatchName") == Some("AE.ADBE Lumetri")
    });
    let component = components.next().unwrap();
    assert!(components.next().is_none());
    let params = component
        .descendants()
        .filter(|node| node.has_tag_name("Param"))
        .map(|reference| {
            let id = reference.attribute("ObjectRef").unwrap();
            root.children()
                .find(|node| node.is_element() && node.attribute("ObjectID") == Some(id))
                .unwrap()
        })
        .collect();
    (component, params)
}

/// A record's attributes other than its ObjectID, and its child elements
/// with their attributes and trimmed text, in name order.
fn record_shape(node: roxmltree::Node<'_, '_>) -> Vec<String> {
    let mut shape: Vec<String> = node
        .attributes()
        .filter(|attribute| attribute.name() != "ObjectID")
        .map(|attribute| format!("@{}={}", attribute.name(), attribute.value()))
        .collect();
    let mut children: Vec<String> = node
        .children()
        .filter(roxmltree::Node::is_element)
        .map(|element| {
            let attributes: Vec<String> = element
                .attributes()
                .map(|attribute| format!("{}={}", attribute.name(), attribute.value()))
                .collect();
            format!(
                "{}[{}]={}",
                element.tag_name().name(),
                attributes.join(","),
                element.text().unwrap_or("").trim()
            )
        })
        .collect();
    children.sort();
    shape.extend(children);
    shape
}

#[test]
fn written_lumetri_grade_matches_the_native_color_lumetri_basic_record_for_record() {
    let native_xml = color_lumetri_basic_xml();
    let native = roxmltree::Document::parse(&native_xml).unwrap();
    let (native_component, native_params) = lumetri_records(&native);
    let effect = PrEffect {
        enabled: true,
        mask: None,
        params: PrEffectParams::LumetriGrade(PrLumetriGrade {
            values: COLOR_LUMETRI_BASIC,
            vignette: None,
        }),
        animations: Vec::new(),
    };
    let xml = project_xml(&project(vec![effect.clone()])).unwrap();
    let written = roxmltree::Document::parse(&xml).unwrap();
    let (component, params) = lumetri_records(&written);
    // Component: versions, display name, filter type, match name and the
    // constant private data as saved; neither carries Bypass or Intrinsic.
    // Only its chain-local `ID` differs (3 saved, 4 written).
    assert_eq!(
        component.attribute("Version"),
        native_component.attribute("Version")
    );
    assert_eq!(
        component.attribute("ClassID"),
        native_component.attribute("ClassID")
    );
    let body = |node: roxmltree::Node<'_, '_>| {
        let body = node
            .children()
            .find(|child| child.has_tag_name("Component"))
            .unwrap();
        (
            body.attribute("Version").map(str::to_owned),
            child(body, "DisplayName").map(str::to_owned),
            child(body, "Bypass").map(str::to_owned),
            child(body, "Intrinsic").map(str::to_owned),
        )
    };
    assert_eq!(body(component), body(native_component));
    for tag in ["MatchName", "VideoFilterType"] {
        assert_eq!(child(component, tag), child(native_component, tag), "{tag}");
    }
    let private_data = |node: roxmltree::Node<'_, '_>| {
        let data = node
            .children()
            .find(|child| child.has_tag_name("PremiereFilterPrivateData"))
            .unwrap();
        (
            data.attribute("Encoding").map(str::to_owned),
            data.attribute("BinaryHash").map(str::to_owned),
            data.text().map(|text| text.trim().to_owned()),
        )
    };
    assert_eq!(private_data(component), private_data(native_component));
    assert_eq!(
        private_data(component).1.as_deref(),
        Some(LUMETRI_PRIVATE_DATA.0)
    );
    // Every parameter record, in native order, with the same class, version,
    // fields and values, including the Blob sentinel and the empty-valued
    // repeats of each binary hash.
    assert_eq!(params.len(), native_params.len());
    assert_eq!(params.len(), LUMETRI_PARAMS.len());
    for (written, native) in params.iter().zip(&native_params) {
        assert_eq!(
            record_shape(*written),
            record_shape(*native),
            "ParameterID {:?}",
            child(*native, "ParameterID")
        );
    }
    let blob = params[0]
        .children()
        .find(|child| child.has_tag_name("StartKeyframeValue"))
        .unwrap();
    let LumetriDefault::Binary { hash, value } = LUMETRI_BLOB_SENTINEL else {
        panic!("the Blob is binary");
    };
    assert_eq!(child(params[0], "ParameterID"), Some("1"));
    assert_eq!(child(params[0], "Name"), Some("Blob"));
    assert_eq!(blob.attribute("BinaryHash"), Some(hash));
    assert_eq!(blob.text().map(str::trim), Some("/v4="));
    assert_eq!(STANDARD.decode(value).unwrap(), [0xFE, 0xFE]);
    // The writer's output rereads as the saved grade.
    let (reread, _) = inspect_project_with_omissions(&xml, None).unwrap();
    assert_eq!(
        reread.sequences[0].video_tracks[0].clip(0).effects[0],
        effect
    );
}

#[test]
fn native_color_lumetri_basic_reads_its_authored_grade_without_its_neutral_vignette() {
    let (project, omissions) = inspect_project_with_omissions(
        &color_lumetri_basic_xml(),
        Some("5c919df6-dea9-4620-9eb3-611b6fc20602"),
    )
    .unwrap();
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.name, "Lumetri Basic");
    let effects = &sequence.video_tracks[0].clip(0).effects;
    assert_eq!(
        effects
            .iter()
            .map(|effect| &effect.params)
            .collect::<Vec<_>>(),
        [
            // The Vignette section is at its defaults (Amount 0): no effect.
            &PrEffectParams::LumetriGrade(PrLumetriGrade {
                values: COLOR_LUMETRI_BASIC,
                vignette: None,
            }),
        ]
    );
    assert!(effects
        .iter()
        .all(|effect| effect.enabled && effect.animations.is_empty()));
    // No unmapped section differs from Lumetri's defaults. (The sequence's
    // colour-management reports are not Lumetri's.)
    assert!(
        omissions
            .iter()
            .filter(|omission| omission.record.starts_with("VideoFilterComponent"))
            .all(|omission| omission.kind == OmissionKind::Approximated),
        "{omissions:?}"
    );
}

#[test]
fn native_26_5_sdr_sequence_default_colour_settings_are_not_reported() {
    // `color_lumetri_basic`'s SDR sequence saves Premiere 26.5.2's defaults:
    // graphics white 203 in ColorManagementSettings and ToneMappingDesaturation
    // 0.5. Neither is reported.
    let xml = color_lumetri_basic_xml();
    let sequence_reports = |xml: &str| -> Vec<String> {
        let (_, omissions) =
            inspect_project_with_omissions(xml, Some("5c919df6-dea9-4620-9eb3-611b6fc20602"))
                .unwrap();
        omissions
            .into_iter()
            .filter(|omission| omission.record.starts_with("VideoTrackGroup"))
            .map(|omission| omission.reason)
            .collect()
    };
    assert_eq!(sequence_reports(&xml), Vec::<String>::new());
    // Other values are still reported as unconverted controls.
    let sequence_settings = r#"<ColorManagementSettings>{"autoToneMapEnabled":true,"enableLogColorManagement":2,"graphicsWhiteLuminance":203,"lutInterpolationMethod":1}</ColorManagementSettings>"#;
    assert_eq!(xml.matches(sequence_settings).count(), 1);
    let edited = xml
        .replace(sequence_settings, &sequence_settings.replace("203", "300"))
        .replace(
            "<ToneMappingDesaturation>0.5</ToneMappingDesaturation>",
            "<ToneMappingDesaturation>0.7</ToneMappingDesaturation>",
        );
    assert_eq!(
        sequence_reports(&edited),
        [
            "nondefault ColorManagementSettings not converted",
            "nondefault ToneMappingDesaturation not converted",
        ]
    );
}
