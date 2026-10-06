//! The static Opacity mask: reader, classifier and writer, on the
//! corpus record forms and on Premiere 26.5.1's saved form (the records of
//! `feature_opacity_masks_26_5_strict.prproj`, verbatim).

use super::{
    animation::animation_fixture::masked_opacity_xml,
    effects::{
        blur, fixture_records, intrinsic, omitted_occurrence_reason, read, top_crop, with_chain,
        with_second_clip, DEFAULT_FLAGS, SOURCE,
    },
};
use crate::{
    format::{inspect_project_with_omissions, writer::project_xml, Graph},
    schema::{native::ArbVideoComponentParam, text::PrPathVertex, MediaId, PrMask, TICKS},
    tests::support::{opacity_mask, video_media, video_sequence},
    Omission, OmissionKind, OmissionScope,
};
use base64::{engine::general_purpose::STANDARD, Engine};

/// The `cinemagraph` mask path (`ArbVideoComponentParam:110`): a pen path of
/// five vertices, the first smooth, in unit-frame fractions.
const PEN_PATH: &str = "MmNpbgIAAAAAAAAABQAAAAEAAADNzEw+LtiCP0kXSz7zkYE/UYJOPmoehD8BAAAAAAAAAFVVtT6Y0B4/VVW1PpjQHj9VVbU+mNAePwEAAAAAAAAAVVU3P/qkzz5VVTc/+qTPPlVVNz/6pM8+AQAAAAAAAABVVWc/XXnAPlVVZz9decA+VVVnP115wD4BAAAAAAAAADMzZT8ofYI/MzNlPyh9gj8zM2U/KH2CPwEAAAA=";
/// The `abstract_slideshow` mask path (`ArbVideoComponentParam:3770`): a
/// rectangle of four corner vertices.
const RECTANGLE_PATH: &str = "MmNpbgIAAAAAAAAABAAAAAAAAABGJ9M+ynbBPkYn0z7KdsE+RifTPsp2wT4BAAAAAAAAABGmFD8gKcE+EaYUPyApwT4RphQ/ICnBPgEAAAAAAAAAsU4UP1hY5D6xThQ/WFjkPrFOFD9YWOQ+AQAAAAAAAADNhNQ+TEHlPs2E1D5MQeU+zYTUPkxB5T4BAAAA";
const PATH_HASH: &str = "895a1cc1-7d8d-7f19-e8fd-f8eb000000bc";
const STATIC: &str = "-91445760000000000";

/// The Premiere 26.3 Opacity of `feature_opacity_screen_strict`
/// (`VideoFilterComponent:200`, Opacity 50, Normal) at ObjectIDs `id..id + 3`, naming
/// the mask `mask_id` in its `SubComponents`, as `cinemagraph`
/// `VideoFilterComponent:97` does.
pub(super) fn masked_opacity(id: u32, mask_id: u32) -> String {
    let mut records = fixture_records(
        "feature_opacity_screen_strict.prproj",
        &["200", "201", "202", "203"],
    )
    .replace(
        "</Component><MatchName>AE.ADBE Opacity</MatchName>",
        &format!("</Component><SubComponents Version=\"1\"><SubComponent Index=\"0\" ObjectRef=\"{mask_id}\"/></SubComponents><MatchName>AE.ADBE Opacity</MatchName>"),
    );
    for offset in 0..4 {
        for attribute in ["ObjectID", "ObjectRef"] {
            records = records.replace(
                &format!("{attribute}=\"{}\"", 200 + offset),
                &format!("{attribute}=\"{}\"", id + offset),
            );
        }
    }
    records
}

/// One `AE.ADBE AEMask` record with the `cinemagraph` values (Feather 30,
/// Opacity 100, Expansion 0, not inverted, the pen path) in the v8 form of
/// `cinemagraph` `VideoFilterComponent:102` (`v8` true: 15 parameters, ObjectIDs
/// `id..id + 15`) or the v7 form of `abstract_slideshow` `:2779` (13
/// parameters, ObjectIDs `id..id + 13`). Parameter ObjectIDs are `id` plus the
/// `ParameterID`.
pub(super) fn mask(id: u32, v8: bool) -> String {
    let (version, body, range, count) = if v8 {
        ("8", "6", "5000", 15)
    } else {
        ("7", "5", "1000", 13)
    };
    let boolean = |parameter_id: u32, control: &str, upper: &str| {
        format!(
            "<VideoComponentParam ObjectID=\"{}\" ClassID=\"cc12343e-f113-4d3b-ae05-b287db77d461\" Version=\"9\"><IsTimeVarying>false</IsTimeVarying><ParameterControlType>{control}</ParameterControlType><StartKeyframe>{STATIC},false,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>{upper}</UpperBound><ParameterID>{parameter_id}</ParameterID></VideoComponentParam>",
            id + parameter_id
        )
    };
    let slider = |parameter_id: u32,
                  name: &str,
                  value: &str,
                  lower: &str,
                  upper: &str,
                  ui: &str| {
        format!(
            "<VideoComponentParam ObjectID=\"{}\" ClassID=\"a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542\" Version=\"9\">{name}<IsTimeVarying>false</IsTimeVarying><ParameterControlType>8</ParameterControlType><StartKeyframe>{STATIC},{value},0,0,0,0,0,0</StartKeyframe><LowerBound>{lower}</LowerBound><UpperBound>{upper}</UpperBound><ParameterID>{parameter_id}</ParameterID>{ui}</VideoComponentParam>",
            id + parameter_id
        )
    };
    let params: String = (0..count)
        .map(|index| {
            let parameter_id = [1, 2, 3, 4, 14, 15, 5, 6, 7, 8, 9, 10, 11, 12, 13][if v8 {
                index
            } else {
                // The v7 record has no parameters 14 and 15.
                index + usize::from(index >= 4) * 2
            }];
            format!(
                "<Param Index=\"{index}\" ObjectRef=\"{}\"/>",
                id + parameter_id
            )
        })
        .collect();
    let tracking_14_15 = if v8 {
        boolean(14, "16", "true") + &boolean(15, "16", "true")
    } else {
        String::new()
    };
    format!(
        "<VideoFilterComponent ObjectID=\"{id}\" ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"{version}\"><Component Version=\"{body}\"><Params Version=\"1\">{params}</Params><ID>0</ID><DisplayName>Mask</DisplayName><InstanceName>1</InstanceName><Bypass>false</Bypass><Intrinsic>false</Intrinsic><ArchivedType>0</ArchivedType></Component><PremiereFilterPrivateData Encoding=\"base64\" BinaryHash=\"9fbd2923-ddd9-cb82-c26f-f75f00000064\">a2NpbgEAAAAAAAAAAAAAAAAAAAAAAAAAq6oIQ6qqkkMAAAhDAICSQ4Dl+f//////AQAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==</PremiereFilterPrivateData><MatchName>AE.ADBE AEMask</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>{}{}{}{}{}\
<ArbVideoComponentParam ObjectID=\"{}\" ClassID=\"313e54d4-6903-49ad-b0bf-8262cdd10f4e\" Version=\"2\"><Name>Mask Path</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>22</ParameterControlType><StartKeyframePosition>{STATIC}</StartKeyframePosition><StartKeyframeValue Encoding=\"base64\" BinaryHash=\"{PATH_HASH}\">{PEN_PATH}</StartKeyframeValue><ParameterID>6</ParameterID></ArbVideoComponentParam>{}{}{}{}{}{}{}{tracking_14_15}",
        boolean(1, "11", "false"),
        boolean(2, "16", "true"),
        boolean(3, "16", "true"),
        boolean(4, "16", "true"),
        boolean(5, "12", "false"),
        id + 6,
        slider(
            7,
            "<Name>Mask Feather</Name>",
            "30.",
            "0",
            range,
            "<UpperUIBound>300</UpperUIBound>"
        ),
        slider(8, "<Name>Mask Opacity</Name>", "100.", "0", "100", ""),
        slider(
            9,
            "<Name>Mask Expansion</Name>",
            "0.",
            &format!("-{range}"),
            range,
            "<LowerUIBound>-300</LowerUIBound><UpperUIBound>300</UpperUIBound>"
        ),
        boolean(10, "4", "true"),
        slider(11, "", "2.", "0", "3", ""),
        slider(12, "", "0.", "0", "4294967296", ""),
        slider(13, "", "0.5", "0", "3.4028234663852886e+38", ""),
    )
}

/// [`mask`] `id` in the v8 form with its Mask Path keyed by `keys`
/// (`ticks,base64;` per key) as the v8 record that Premiere 26.5.1 opened and
/// AME rendered stores them: `Keyframes` and no
/// `IsTimeVarying`, beside the stored [`PEN_PATH`].
pub(super) fn keyed_mask(id: u32, keys: &str) -> String {
    let records = mask(id, true);
    let edited = records.replace(
        &format!("<Name>Mask Path</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>22</ParameterControlType><StartKeyframePosition>{STATIC}</StartKeyframePosition>"),
        &format!("<Name>Mask Path</Name><ParameterControlType>22</ParameterControlType><StartKeyframePosition>{STATIC}</StartKeyframePosition><Keyframes>{keys}</Keyframes>"),
    );
    assert_ne!(edited, records);
    edited
}

/// Mask Path keys at source 0 and 1 s: the pen path, then the rectangle.
pub(super) fn two_path_keys() -> String {
    format!("0,{PEN_PATH};{TICKS},{RECTANGLE_PATH};")
}

/// `one-clip.xml` whose first clip has the Opacity 50 of [`masked_opacity`]
/// with the mask `mask`, and the chain `components` before it.
fn masked_clip(mask: String, components: &[(u32, String)]) -> String {
    let mut components = components.to_vec();
    components.push((200, masked_opacity(200, 300) + &mask));
    with_chain(
        &with_second_clip(SOURCE),
        "<DefaultMotion>true</DefaultMotion><DefaultMotionComponentID>1</DefaultMotionComponentID>",
        &components,
    )
}

fn corner(x: f32, y: f32) -> PrPathVertex {
    PrPathVertex {
        smooth: false,
        point: [x, y],
        in_tangent: [x, y],
        out_tangent: [x, y],
    }
}

/// The vertices of [`PEN_PATH`].
fn pen_path() -> Vec<PrPathVertex> {
    crate::schema::decode_mask_path(&STANDARD.decode(PEN_PATH).unwrap())
        .unwrap()
        .vertices
}

#[test]
fn corpus_opacity_masks_import_in_both_record_forms() {
    // The v8 pen path of `cinemagraph` (the shared runtime fixture, static
    // and with Opacity keys), and the same values in the v7 form with
    // `abstract_slideshow`'s rectangle, Feather 0 and Inverted.
    let pen = PrMask {
        raster: None,
        feather_keys: Vec::new(),
        expansion: 0.0,
        expansion_keys: Vec::new(),
        opacity_keys: Vec::new(),
        path: crate::schema::text::PrShapePath {
            vertices: pen_path(),
            closed: true,
        },
        path_keys: Vec::new(),
        feather: 30.0,
        opacity: 100.0,
        inverted: false,
    };
    let v7 = mask(300, false)
        .replace(PEN_PATH, RECTANGLE_PATH)
        .replace(
            &format!("<StartKeyframe>{STATIC},30.,"),
            &format!("<StartKeyframe>{STATIC},0.,"),
        )
        .replace(
            &format!(
                "<ParameterControlType>4</ParameterControlType><StartKeyframe>{STATIC},false,"
            ),
            &format!("<ParameterControlType>4</ParameterControlType><StartKeyframe>{STATIC},true,"),
        );
    for (form, xml, expected, keyed) in [
        ("v8", masked_opacity_xml(""), pen.clone(), false),
        (
            "v8 with Opacity keys",
            masked_opacity_xml("0,50.,0,0,0,0,0,0;254016000000,100.,0,0,0,0,0,0;"),
            pen,
            true,
        ),
        (
            "v7",
            masked_clip(v7, &[]),
            PrMask {
                raster: None,
                feather_keys: Vec::new(),
                expansion: 0.0,
                expansion_keys: Vec::new(),
                opacity_keys: Vec::new(),
                path: crate::schema::text::PrShapePath {
                    vertices: vec![
                        corner(0.412409, 0.3778594),
                        corner(0.580659, 0.37726688),
                        corner(0.57932574, 0.4459865),
                        corner(0.4150757, 0.4477638),
                    ],
                    closed: true,
                },
                path_keys: Vec::new(),
                feather: 0.0,
                opacity: 100.0,
                inverted: true,
            },
            false,
        ),
    ] {
        let (clip, omissions) = read(&xml);
        assert!(omissions.is_empty(), "{form}: {omissions:?}");
        assert_eq!(clip.opacity, 50.0, "{form}");
        assert_eq!(clip.animations.len(), usize::from(keyed), "{form}");
        assert_eq!(clip.opacity_mask, Some(expected), "{form}");
        assert_eq!(clip.effects_above_mask, 0, "{form}");
    }
}

#[test]
fn opacity_mask_record_versions_and_display_labels_do_not_drop_picture() {
    for (version, body) in [("7", "5"), ("9", "6"), ("42", "19")] {
        let records = mask(300, true)
            .replace(
                "Version=\"8\"><Component Version=\"6\">",
                &format!("Version=\"{version}\"><Component Version=\"{body}\">"),
            )
            .replace(
                "<DisplayName>Mask</DisplayName>",
                "<DisplayName>Outline</DisplayName>",
            )
            .replace(
                "<Name>Mask Feather</Name>",
                "<Name>Saved feather label</Name>",
            );
        let (clip, omissions) = read(&masked_clip(records, &[]));
        assert!(omissions.is_empty(), "{version}/{body}: {omissions:?}");
        assert_eq!(clip.opacity, 50.0);
        let mask = clip.opacity_mask.unwrap();
        assert_eq!(mask.path.vertices, pen_path());
        assert_eq!(
            (mask.feather, mask.opacity, mask.inverted),
            (30.0, 100.0, false)
        );
    }
}

const MASK_RECORDS: &str = include_str!("../../../tests/fixtures/opacity-mask-records.xml");

#[test]
fn native_opacity_mask_selection_defaults_keep_picture_and_path_keys() {
    let (project, omissions) =
        inspect_project_with_omissions(MASK_RECORDS, Some(KEYED_MASK_SEQUENCE)).unwrap();
    assert!(
        omissions
            .iter()
            .all(|omission| omission.scope != OmissionScope::Occurrence),
        "{omissions:?}"
    );
    assert_eq!(project.sequences[0].video_occurrences().count(), 2);
    let clip = project.sequences[0]
        .video_occurrences()
        .find(|clip| clip.id.as_deref() == Some("VideoClipTrackItem:86"))
        .unwrap();
    let mask = clip.opacity_mask.as_ref().unwrap();
    assert_eq!(
        (mask.feather, mask.opacity, mask.expansion, mask.inverted),
        (321.0, 100.0, 0.0, false)
    );
    assert_eq!(
        mask.path
            .vertices
            .iter()
            .map(|vertex| vertex.point)
            .collect::<Vec<_>>(),
        [[0.25, 0.25], [0.5, 0.25], [0.5, 0.75], [0.25, 0.75]]
    );
    assert!(mask.path.closed);
    assert_eq!(
        mask.path_keys
            .iter()
            .map(|key| key.source_ticks)
            .collect::<Vec<_>>(),
        [TICKS / 2, TICKS * 3 / 2, TICKS * 2]
    );
    assert_eq!(mask.path_keys[2].path.vertices.len(), 5);
}

#[test]
fn native_opacity_mask_required_record_and_additional_coverage_fail_closed() {
    for (xml, reason) in [
        (
            edit_start(
                MASK_RECORDS.to_owned(),
                1195,
                "<ParameterID>6</ParameterID>",
                "<ParameterID>99</ParameterID>",
            ),
            "unknown mask parameter",
        ),
        (
            edit_start(
                MASK_RECORDS.to_owned(),
                2102,
                "AAAAAAAAAAAAAAAAAAAAAA==",
                "AQAAAAAAAAAAAAAAAAAAAA==",
            ),
            "mask control 17 (Additional Paths) holds an unknown value",
        ),
    ] {
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some(KEYED_MASK_SEQUENCE)).unwrap();
        assert_eq!(
            project.sequences[0]
                .video_occurrences()
                .map(|clip| clip.id.as_deref())
                .collect::<Vec<_>>(),
            [Some("VideoClipTrackItem:77")]
        );
        assert!(
            omissions
                .iter()
                .any(|omission| omission.scope == OmissionScope::Occurrence
                    && omission.reason.contains(reason)),
            "{omissions:?}"
        );
    }
}

#[test]
fn opacity_mask_counts_every_converted_effect_as_applied_before_it() {
    // A blur at `Index` 0 applies after the Crop that a Crop's classifier
    // would stage it under, but every effect applies before Opacity.
    let (clip, omissions) = read(&masked_clip(mask(300, true), &[(20, blur(20))]));
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(clip.effects.len(), 1);
    assert_eq!(clip.effects_above_mask, 1);
    assert!(clip.opacity_mask.is_some());
}

#[test]
fn second_mask_path_by_binary_hash_reads_the_first_clips_path() {
    // Premiere writes a repeated Mask Path as an empty element naming the
    // earlier blob (16 of the 57 corpus masks).
    let second = mask(400, true).replace(
        &format!("BinaryHash=\"{PATH_HASH}\">{PEN_PATH}</StartKeyframeValue>"),
        &format!("BinaryHash=\"{PATH_HASH}\"></StartKeyframeValue>"),
    );
    let xml = masked_clip(mask(300, true), &[]).replace(
        "<VideoComponentChain ObjectID=\"10\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>",
        &format!("<VideoComponentChain ObjectID=\"10\"><DefaultMotion>true</DefaultMotion><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"500\"/></Components></ComponentChain></VideoComponentChain>{}{second}", masked_opacity(500, 400)),
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let clips: Vec<_> = project.sequences[0].video_occurrences().collect();
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[0].opacity_mask, clips[1].opacity_mask);
    assert_eq!(
        clips[1].opacity_mask.as_ref().unwrap().path.vertices,
        pen_path()
    );
}

#[test]
fn bypassed_opacity_mask_keeps_the_clip_and_is_reported() {
    let bypassed = mask(300, true).replace(
        "<InstanceName>1</InstanceName><Bypass>false</Bypass>",
        "<InstanceName>1</InstanceName><Bypass>true</Bypass>",
    );
    let (clip, omissions) = read(&masked_clip(bypassed, &[]));
    assert_eq!(clip.opacity_mask, None);
    assert_eq!(clip.opacity, 50.0);
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Omitted,
            record: "VideoFilterComponent:300".into(),
            reason: "bypassed mask is not converted; Premiere renders the clip without it".into(),
        }]
    );
}

#[test]
fn opacity_mask_saved_bounds_do_not_change_picture_or_path_coverage() {
    let records = mask(300, true);
    let (expected, notes) = read(&masked_clip(records.clone(), &[]));
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(
        expected.opacity_mask.as_ref().unwrap().path.vertices,
        pen_path()
    );
    let older_bounds = records.replace("5000</", "1000</");
    let without_bounds = older_bounds
        .replace(
            "<LowerBound>0</LowerBound><UpperBound>1000</UpperBound>",
            "",
        )
        .replace(
            "<LowerBound>-1000</LowerBound><UpperBound>1000</UpperBound>",
            "",
        );
    assert_ne!(older_bounds, records);
    assert_ne!(without_bounds, older_bounds);
    for saved in [older_bounds, without_bounds] {
        let (clip, notes) = read(&masked_clip(saved, &[]));
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(clip.id, expected.id);
        assert_eq!(clip.media, expected.media);
        assert_eq!(
            (
                clip.start_ticks,
                clip.end_ticks,
                clip.in_ticks,
                clip.out_ticks
            ),
            (
                expected.start_ticks,
                expected.end_ticks,
                expected.in_ticks,
                expected.out_ticks
            )
        );
        assert_eq!(clip.transform, expected.transform);
        assert_eq!(clip.opacity, expected.opacity);
        assert_eq!(clip.opacity_mask, expected.opacity_mask);
    }
}

#[test]
fn opacity_masks_outside_the_supported_form_omit_the_occurrence() {
    let feather = |value: &str| {
        mask(300, true).replace(
            &format!("<StartKeyframe>{STATIC},30.,"),
            &format!("<StartKeyframe>{STATIC},{value},"),
        )
    };
    // A constant control (parameters 11 to 13), told apart by its bound, at 1.
    let control = |upper: &str, value: &str| {
        let records = mask(300, true);
        let edited = records.replace(
            &format!("<StartKeyframe>{STATIC},{value},0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>{upper}"),
            &format!("<StartKeyframe>{STATIC},1.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>{upper}"),
        );
        assert_ne!(edited, records);
        edited
    };
    // `vhs_slideshow` `VideoFilterComponent:2667`: keyed Mask Opacity, saved
    // with `Keyframes` and no `IsTimeVarying` element.
    let keyed_opacity = mask(300, true).replace(
        &format!("<Name>Mask Opacity</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>8</ParameterControlType><StartKeyframe>{STATIC},100.,0,0,0,0,0,0</StartKeyframe>"),
        &format!("<Name>Mask Opacity</Name><ParameterControlType>8</ParameterControlType><StartKeyframe>{STATIC},100.,0,0,0,0,0,0</StartKeyframe><Keyframes>914457600000000,65.,0,0,0,0.16666666666666666,0,0.16666666666666666;914458870080000,42.,0,0,0,0.16666666666666666,0,0.16666666666666666;</Keyframes>"),
    );
    let expansion = mask(300, true).replace(
        &format!("<Name>Mask Expansion</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>8</ParameterControlType><StartKeyframe>{STATIC},0.,"),
        &format!("<Name>Mask Expansion</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>8</ParameterControlType><StartKeyframe>{STATIC},278.,"),
    );
    for records in [keyed_opacity, expansion] {
        let (project, omissions) =
            inspect_project_with_omissions(&masked_clip(records, &[]), Some("sequence-1")).unwrap();
        assert_eq!(
            project.sequences[0].video_occurrences().count(),
            2,
            "{omissions:?}"
        );
    }
    let tracking = mask(300, true).replace(
        &format!("<ParameterControlType>11</ParameterControlType><StartKeyframe>{STATIC},false,"),
        &format!("<ParameterControlType>11</ParameterControlType><StartKeyframe>{STATIC},true,"),
    );
    // `vhsvertical` writes `ParameterID` -1 on its two v8 tracking booleans.
    let unknown_id = mask(300, true).replace(
        "<ParameterID>15</ParameterID>",
        "<ParameterID>-1</ParameterID>",
    );
    let two_masks = masked_clip(mask(300, true) + &mask(400, true), &[]).replace(
        "<SubComponent Index=\"0\" ObjectRef=\"300\"/>",
        "<SubComponent Index=\"0\" ObjectRef=\"300\"/><SubComponent Index=\"1\" ObjectRef=\"400\"/>",
    );
    let missing_blob = mask(300, true).replace(
        &format!("BinaryHash=\"{PATH_HASH}\">{PEN_PATH}</StartKeyframeValue>"),
        "BinaryHash=\"00000000-0000-0000-0000-000000000000\"></StartKeyframeValue>",
    );
    let no_bypass = mask(300, true).replace(
        "<Bypass>false</Bypass><Intrinsic>false</Intrinsic><ArchivedType>0</ArchivedType>",
        "<Intrinsic>false</Intrinsic><ArchivedType>0</ArchivedType>",
    );
    // Mask Path keys are read as Source Text keys are, and fail closed alike.
    let path_keys_off = mask(300, true).replace(
        "<StartKeyframeValue",
        &format!("<Keyframes>0,{PEN_PATH};</Keyframes><StartKeyframeValue"),
    );
    for (case, xml, reason) in [
        (
            "unterminated Mask Path keys",
            masked_clip(keyed_mask(300, &format!("0,{PEN_PATH}")), &[]),
            "ArbVideoComponentParam:306: unterminated Mask Path key list",
        ),
        (
            "Mask Path keys out of order",
            masked_clip(
                keyed_mask(300, &format!("{TICKS},{PEN_PATH};0,{PEN_PATH};")),
                &[],
            ),
            "ArbVideoComponentParam:306: Mask Path keys must have strictly increasing source times",
        ),
        (
            "malformed Mask Path key",
            masked_clip(
                keyed_mask(300, &format!("0,{};", RECTANGLE_PATH.replacen('M', "N", 1))),
                &[],
            ),
            "ArbVideoComponentParam:306: unknown Mask Path magic",
        ),
        (
            "Mask Path keys under IsTimeVarying false",
            masked_clip(path_keys_off, &[]),
            "ArbVideoComponentParam:306: animated or unknown Mask Path is unsupported",
        ),
        (
            "tracking control on",
            masked_clip(tracking, &[]),
            "VideoComponentParam:301: mask control 1 at a value other than false is not converted",
        ),
        (
            "control 11 off its constant",
            masked_clip(control("3", "2."), &[]),
            "VideoComponentParam:311: mask control 11 at a value other than 2 is not converted",
        ),
        (
            "control 13 off its constant",
            masked_clip(control("3.4028234663852886e+38", "0.5"), &[]),
            "VideoComponentParam:313: mask control 13 at a value other than 0.5 is not converted",
        ),
        (
            "feather outside the consumed range",
            masked_clip(feather("3000."), &[]),
            "Feather must be finite and within 0..=1000",
        ),
        (
            "unknown ParameterID",
            masked_clip(unknown_id, &[]),
            "VideoComponentParam:315: unknown mask parameter",
        ),
        (
            "v8 record with the 26.5.1 match name",
            masked_clip(
                mask(300, true).replace("AE.ADBE AEMask<", "AE.ADBE AEMask2<"),
                &[],
            ),
            "VideoFilterComponent:300: unsupported mask parameter layout (MatchName Some(\"AE.ADBE AEMask2\"), 15 parameters)",
        ),
        (
            "nonfinite feather",
            masked_clip(feather("NaN"), &[]),
            "VideoComponentParam:307: nonfinite initial value",
        ),
        (
            "two masks on Opacity",
            two_masks,
            "VideoFilterComponent:200: 2 masks on Opacity are not converted; how Premiere combines them is unverified",
        ),
        (
            "path names a missing blob",
            masked_clip(missing_blob, &[]),
            "ArbVideoComponentParam:306: Mask Path names missing binary 00000000-0000-0000-0000-000000000000",
        ),
        (
            "no Bypass",
            masked_clip(no_bypass, &[]),
            "VideoFilterComponent:300: unsupported mask Bypass",
        ),
        (
            "mask on Motion",
            with_chain(
                &with_second_clip(SOURCE),
                "<DefaultOpacity>true</DefaultOpacity><DefaultOpacityComponentID>2</DefaultOpacityComponentID>",
                &[(
                    20,
                    intrinsic(20, 1, "Motion", "AE.ADBE Motion").replace(
                        "</Component><MatchName>AE.ADBE Motion</MatchName>",
                        "</Component><SubComponents Version=\"1\"><SubComponent Index=\"0\" ObjectRef=\"300\"/></SubComponents><MatchName>AE.ADBE Motion</MatchName>",
                    ) + &mask(300, true),
                )],
            ),
            "VideoFilterComponent:20: a mask on Motion is not converted",
        ),
        (
            "mask on Crop",
            with_chain(
                &with_second_clip(SOURCE),
                DEFAULT_FLAGS,
                &[(
                    20,
                    top_crop(20).replace(
                        "</Component><MatchName>AE.ADBE AECrop</MatchName>",
                        "</Component><SubComponents Version=\"1\"><SubComponent Index=\"0\" ObjectRef=\"300\"/></SubComponents><MatchName>AE.ADBE AECrop</MatchName>",
                    ) + &mask(300, true),
                )],
            ),
            "masked effect has no editable mapping",
        ),
        (
            "Opacity mask with a Crop",
            masked_clip(mask(300, true), &[(20, top_crop(20))]),
            "an Opacity mask with a Crop or Linear Wipe on one clip is not converted",
        ),
    ] {
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        let kept: Vec<_> = project.sequences[0]
            .video_occurrences()
            .map(|clip| clip.id.as_deref())
            .collect();
        assert_eq!(kept, [Some("VideoClipTrackItem:9")], "{case}");
        let occurrence = omissions
            .iter()
            .find(|omission| omission.scope == OmissionScope::Occurrence)
            .unwrap_or_else(|| panic!("{case}: {omissions:?}"));
        assert!(
            occurrence.reason.contains(reason),
            "{case}: {}",
            occurrence.reason
        );
    }
}

/// Unchanged human-authored Opacity/Object Mask records; typed saved-raster references.
const OBJECT_MASK: &str = include_str!("../../../tests/fixtures/object_mask/opacity.xml");

#[test]
fn native_saved_object_mask_retains_typed_tracker_for_source_bound_recovery() {
    let xml = with_chain(
        SOURCE,
        "<DefaultMotion>true</DefaultMotion><DefaultMotionComponentID>1</DefaultMotionComponentID>",
        &[(665, OBJECT_MASK.to_owned())],
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let clip = project.sequences[0].video_occurrences().next().unwrap();
    let mask = clip.opacity_mask.as_ref().unwrap();
    let Some(crate::schema::RasterMask::Saved(tracker)) = &mask.raster else {
        panic!("missing saved Tracker")
    };
    assert_eq!(
        tracker.propagation.to_string(),
        "dd06d550-fb83-4fb0-b9e8-6d3d6fcdedf1"
    );
    assert_eq!(tracker.frame_ticks, 8_511_237_907);
    assert_eq!(tracker.frame_count, 204);
}

#[test]
fn object_mask_nondefault_coverage_control_still_omits_safely() {
    let records = edit_start(OBJECT_MASK.to_owned(), 1256, ",0.,", ",10.,");
    let reason = omitted_occurrence_reason(
        "<DefaultMotion>true</DefaultMotion><DefaultMotionComponentID>1</DefaultMotionComponentID>",
        &[(665, records)],
    );
    assert!(
        reason.contains("Object Mask raster requires zero Feather/Expansion"),
        "{reason}"
    );
}

#[test]
fn object_mask_does_not_inherit_vector_tracker_admission() {
    let wrapped = format!("<Records>{OBJECT_MASK}</Records>");
    let dom = roxmltree::Document::parse(&wrapped).unwrap();
    let value = dom
        .descendants()
        .find(|n| n.attribute("ObjectID") == Some("1037"))
        .unwrap()
        .children()
        .find(|n| n.has_tag_name("StartKeyframeValue"))
        .unwrap()
        .text()
        .unwrap()
        .trim();
    let mut bytes = STANDARD.decode(value).unwrap();
    for index in [6, 15] {
        bytes[4 + index * 4..8 + index * 4].copy_from_slice(&0.25_f32.to_le_bytes());
    }
    assert!(crate::schema::decode_mask_tracker(&bytes).is_ok());
    let records = OBJECT_MASK.replace(value, &STANDARD.encode(bytes));
    let reason =
        omitted_occurrence_reason("<DefaultMotion>true</DefaultMotion>", &[(665, records)]);
    assert!(
        reason.contains("mask control 6 (Tracker) holds an unknown value"),
        "{reason}"
    );
}

#[test]
fn native_type_0_aemask2_keeps_the_generic_control_reason() {
    // AEMask2 alone is not evidence of an Object Mask. Type 0 keeps the
    // existing fail-closed nondefault-control reason on the active mask.
    let records = edit_start(OBJECT_MASK.to_owned(), 1262, ",4,", ",0,");
    let reason = omitted_occurrence_reason(
        "<DefaultMotion>true</DefaultMotion><DefaultMotionComponentID>1</DefaultMotionComponentID>",
        &[(665, records)],
    );
    assert!(
        reason.contains("mask control 24 (Tracker) holds an unknown value"),
        "{reason}"
    );
    assert!(!reason.contains("Object Mask"), "{reason}");
}

/// The Premiere 26.5.1 fixture: five masked clips A to E.
const FIXTURE_26_5: &str = "feature_opacity_masks_26_5_strict.prproj";

/// The records with ObjectIDs `roots` of [`FIXTURE_26_5`] and every record
/// they reference, verbatim and in document order.
fn fixture_closure(roots: &[&str]) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(FIXTURE_26_5);
    let source = crate::format::read_xml(&path).unwrap();
    let native = roxmltree::Document::parse(&source).unwrap();
    let mut ids: std::collections::BTreeSet<String> =
        roots.iter().map(|id| (*id).to_owned()).collect();
    loop {
        let referenced: Vec<String> = native
            .root_element()
            .children()
            .filter(|node| {
                node.attribute("ObjectID")
                    .is_some_and(|id| ids.contains(id))
            })
            .flat_map(|node| node.descendants())
            .filter_map(|node| node.attribute("ObjectRef"))
            .filter(|id| !ids.contains(*id))
            .map(str::to_owned)
            .collect();
        if referenced.is_empty() {
            break;
        }
        ids.extend(referenced);
    }
    native
        .root_element()
        .children()
        .filter(|node| {
            node.attribute("ObjectID")
                .is_some_and(|id| ids.contains(id))
        })
        .map(|node| &source[node.range()])
        .collect()
}

/// The 26.5.1 mask record `mask_id` of the fixture (A 157, B 161, C 176, D
/// 180, E 184) and its 35 parameters, with mask A's records, whose tracker,
/// User Interactions and private data values B to E name by `BinaryHash`.
pub(in crate::format) fn fixture_mask_26_5(mask_id: u32) -> String {
    let id = mask_id.to_string();
    let roots: Vec<&str> = if mask_id == 157 {
        vec![&id]
    } else {
        vec![&id, "157"]
    };
    fixture_closure(&roots)
}

/// [`masked_clip`] whose 26.3 Opacity (ObjectIDs 900 to 903, above the
/// fixture's) names the fixture's 26.5.1 mask `mask_id`, after `edit`.
fn masked_clip_26_5(mask_id: u32, edit: impl Fn(String) -> String) -> String {
    with_chain(
        &with_second_clip(SOURCE),
        "<DefaultMotion>true</DefaultMotion><DefaultMotionComponentID>1</DefaultMotionComponentID>",
        &[(
            900,
            masked_opacity(900, mask_id) + &edit(fixture_mask_26_5(mask_id)),
        )],
    )
}

/// The `StartKeyframe` of parameter `object_id` in `records`, whose value
/// `from` becomes `to`.
fn edit_start(records: String, object_id: u32, from: &str, to: &str) -> String {
    let attribute = records
        .find(&format!("ObjectID=\"{object_id}\""))
        .unwrap_or_else(|| panic!("no record {object_id}"));
    let start = records[..attribute].rfind('<').unwrap();
    let tag = &records[start + 1..attribute - 1];
    let end = start + records[start..].find(&format!("</{tag}>")).unwrap();
    let record = &records[start..end];
    let edited = record.replace(from, to);
    assert_ne!(edited, record, "{object_id}: {from} not found");
    format!("{}{edited}{}", &records[..start], &records[end..])
}

/// Fixture mask A's rectangle: the middle half of the frame.
fn fixture_rectangle() -> crate::schema::text::PrShapePath {
    crate::schema::text::PrShapePath {
        vertices: vec![
            corner(0.25, 0.25),
            corner(0.75, 0.25),
            corner(0.75, 0.75),
            corner(0.25, 0.75),
        ],
        closed: true,
    }
}

#[test]
fn premiere_26_5_masks_import_in_the_saved_form() {
    // A: the rectangle at Mask Opacity 50. B: the same rectangle, its Path,
    // trackers and private data named by BinaryHash. C: the ellipse, Feather
    // 60. D with Inverted off: the pen path whose Position and Anchor Point
    // are its off-centre middle (0.5498:0.6489), equal.
    let ellipse = |x: f32, y: f32, [ix, iy]: [f32; 2], [ox, oy]: [f32; 2]| PrPathVertex {
        smooth: true,
        point: [x, y],
        in_tangent: [ix, iy],
        out_tangent: [ox, oy],
    };
    let tangent = 0.361_925_f32;
    let far = 0.638_075_f32;
    let pen = crate::schema::decode_mask_path(
        &STANDARD
            .decode("MmNpbgIAAAAAAAAABQAAAAEAAADNzEw+ZmZmP+xROD7Xo3A/rkdhPvYoXD8BAAAAAAAAADMzsz6amRk/MzOzPpqZGT8zM7M+mpkZPwEAAAAAAAAAMzMzP83MzD4zMzM/zczMPjMzMz/NzMw+AQAAAAAAAABmZmY/XI/CPmZmZj9cj8I+ZmZmP1yPwj4BAAAAAAAAAGZmZj9mZmY/ZmZmP2ZmZj9mZmY/ZmZmPwEAAAA=")
            .unwrap(),
    )
    .unwrap();
    for (clip, xml, expected) in [
        (
            "A",
            masked_clip_26_5(157, |records| records),
            PrMask {
                raster: None,
                feather_keys: Vec::new(),
                expansion: 0.0,
                expansion_keys: Vec::new(),
                opacity_keys: Vec::new(),
                path: fixture_rectangle(),
                path_keys: Vec::new(),
                feather: 0.0,
                opacity: 50.0,
                inverted: false,
            },
        ),
        (
            "B",
            masked_clip_26_5(161, |records| records),
            PrMask {
                raster: None,
                feather_keys: Vec::new(),
                expansion: 0.0,
                expansion_keys: Vec::new(),
                opacity_keys: Vec::new(),
                path: fixture_rectangle(),
                path_keys: Vec::new(),
                feather: 0.0,
                opacity: 100.0,
                inverted: false,
            },
        ),
        (
            "C",
            masked_clip_26_5(176, |records| records),
            PrMask {
                raster: None,
                feather_keys: Vec::new(),
                expansion: 0.0,
                expansion_keys: Vec::new(),
                opacity_keys: Vec::new(),
                path: crate::schema::text::PrShapePath {
                    vertices: vec![
                        ellipse(0.5, 0.25, [tangent, 0.25], [far, 0.25]),
                        ellipse(0.75, 0.5, [0.75, tangent], [0.75, far]),
                        ellipse(0.5, 0.75, [far, 0.75], [tangent, 0.75]),
                        ellipse(0.25, 0.5, [0.25, far], [0.25, tangent]),
                    ],
                    closed: true,
                },
                path_keys: Vec::new(),
                feather: 60.0,
                opacity: 100.0,
                inverted: false,
            },
        ),
        (
            "D not inverted",
            masked_clip_26_5(180, |records| edit_start(records, 320, ",true,", ",false,")),
            PrMask {
                raster: None,
                feather_keys: Vec::new(),
                expansion: 0.0,
                expansion_keys: Vec::new(),
                opacity_keys: Vec::new(),
                path: pen,
                path_keys: Vec::new(),
                feather: 0.0,
                opacity: 50.0,
                inverted: false,
            },
        ),
    ] {
        let (occurrence, omissions) = read(&xml);
        assert!(omissions.is_empty(), "{clip}: {omissions:?}");
        assert_eq!(occurrence.opacity_mask, Some(expected), "{clip}");
        assert_eq!(occurrence.effects_above_mask, 0, "{clip}");
    }
}

#[test]
fn premiere_26_5_re_saved_opacity_owner_reads_as_its_static_value() {
    // Fixture clip A's Opacity (`VideoFilterComponent:133`, verbatim): the
    // 26.5 layout of v9/c7, `IsTimeVarying` true with no `Keyframes` and the
    // primary Blend Mode bound 26 that 26.5.1 kept from the older record it
    // re-saved; the native render shows Opacity 100 (alpha 0.500 at Mask
    // Opacity 50). Its mask is A's.
    let owner = |edit: &dyn Fn(String) -> String| {
        with_chain(
            &with_second_clip(SOURCE),
            "<DefaultMotion>true</DefaultMotion><DefaultMotionComponentID>1</DefaultMotionComponentID>",
            &[(133, edit(fixture_closure(&["133"])))],
        )
    };
    let (occurrence, omissions) = read(&owner(&|records| records));
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(occurrence.opacity, 100.0);
    assert!(occurrence.animations.is_empty());
    assert_eq!(occurrence.blend_mode, crate::schema::PrBlendMode::Normal);
    assert_eq!(
        occurrence.opacity_mask.as_ref().map(|mask| mask.opacity),
        Some(50.0)
    );
    // Keys under `IsTimeVarying` true still import as keys. Display bounds
    // do not change the bound Opacity/Blend Mode values or mask coverage.
    let keyed = owner(&|records| {
        records.replacen(
            "<StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe>\n\t\t<LowerBound>0</LowerBound>\n\t\t<UpperBound>100</UpperBound>",
            "<StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><Keyframes>0,100.,0,0,0,0,0,0;254016000000,40.,0,0,0,0,0,0;</Keyframes>\n\t\t<LowerBound>0</LowerBound>\n\t\t<UpperBound>100</UpperBound>",
            1,
        )
    });
    let (occurrence, omissions) = read(&keyed);
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(occurrence.animations.len(), 1);
    let changed_bound =
        owner(&|records| edit_start(records, 155, "<UpperBound>26<", "<UpperBound>25<"));
    let (bounded, omissions) = read(&changed_bound);
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(bounded.opacity, occurrence.opacity);
    assert_eq!(bounded.blend_mode, occurrence.blend_mode);
    assert_eq!(bounded.opacity_mask, occurrence.opacity_mask);
    assert!(bounded.animations.is_empty());
    let (project, omissions) =
        inspect_project_with_omissions(&changed_bound, Some("sequence-1")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        project.sequences[0].video_occurrences().count(),
        2,
        "{project:?}"
    );
    let retained = project.sequences[0]
        .video_occurrences()
        .find(|clip| clip.opacity_mask.is_some())
        .unwrap();
    assert!(retained.enabled);
    assert_eq!(retained.opacity, 100.0);
    assert_eq!(retained.blend_mode, crate::schema::PrBlendMode::Normal);
    assert_eq!(retained.opacity_mask.as_ref().unwrap().opacity, 50.0);
    assert_eq!(
        retained.opacity_mask.as_ref().unwrap().path,
        fixture_rectangle()
    );
    assert_eq!(retained.effects_above_mask, 0);
    assert!(retained.animations.is_empty());
    assert!(project.sequences[0]
        .video_occurrences()
        .any(|clip| clip.opacity_mask.is_none() && clip.enabled));
    assert!(omissions.is_empty(), "{omissions:?}");

    // Consumed alpha still must be representable; only its owner is omitted.
    let (project, omissions) = inspect_project_with_omissions(
        &owner(&|records| edit_start(records, 154, ",100.,", ",101.,")),
        Some("sequence-1"),
    )
    .unwrap();
    assert_eq!(project.sequences[0].video_occurrences().count(), 1);
    assert!(
        omissions.iter().any(|omission| {
            omission.scope == OmissionScope::Occurrence
                && omission.reason.contains("opacity out of range")
        }),
        "{omissions:?}"
    );

    // Actual values still have to be finite integer blend IDs. A malformed
    // value omits its masked occurrence, not its independently valid sibling.
    for value in ["-1", "0.5", "256"] {
        let invalid = owner(&|records| {
            edit_start(
                records,
                155,
                ",18,0,0,0,0,0,0</StartKeyframe>",
                &format!(",{value},0,0,0,0,0,0</StartKeyframe>"),
            )
        });
        let (project, omissions) =
            inspect_project_with_omissions(&invalid, Some("sequence-1")).unwrap();
        assert_eq!(
            project.sequences[0].video_occurrences().count(),
            1,
            "{value}"
        );
        assert!(project.sequences[0]
            .video_occurrences()
            .next()
            .unwrap()
            .opacity_mask
            .is_none());
        assert!(
            omissions
                .iter()
                .any(|omission| omission.scope == OmissionScope::Occurrence
                    && omission
                        .reason
                        .contains("VideoComponentParam:155: invalid blend mode")),
            "{value}: {omissions:?}"
        );
    }
}

#[test]
fn premiere_26_5_masks_off_their_saved_defaults_omit_the_occurrence() {
    let centre = "0.54979544878005981:0.64888888597488403";
    for (case, xml, reason) in [
        (
            // Fixture clip D as saved: Premiere renders 0.5 x (1 - coverage),
            // FX 1 - 0.5 x coverage (G3b).
            "inverted at Mask Opacity 50",
            masked_clip_26_5(180, |records| records),
            "an inverted mask with Mask Opacity below 100 is not converted",
        ),
        (
            "Position off the Anchor Point",
            masked_clip_26_5(180, |records| {
                edit_start(records, 301, centre, "0.5:0.5")
            }),
            "PointComponentParam:307: mask Position and Anchor Point differ; the mask transform is not converted",
        ),
        (
            "Scale Height off 100",
            masked_clip_26_5(157, |records| edit_start(records, 198, ",100.,", ",120.,")),
            "VideoComponentParam:198: mask control 26 at a value other than 100 is not converted",
        ),
        (
            "tracker off its saved value",
            masked_clip_26_5(157, |records| {
                edit_start(records, 192, "AQAAAAAAgD8AAAAAAAAAAAAAAAAAAIA/", "AQAAAAAAgD8AAAAAAAAAAAAAAAAAAIA+")
            }),
            "ArbVideoComponentParam:192: unsupported vector mask Tracker matrix or reference controls",
        ),
        (
            "Feather above the written bound",
            masked_clip_26_5(176, |records| edit_start(records, 282, ",60.,", ",3000.,")),
            "Feather must be finite and within 0..=1000",
        ),
        (
            // Unobserved: a bypassed 26.5.1 mask.
            "Bypass",
            masked_clip_26_5(157, |records| {
                records.replacen(
                    "<DisplayName>Mask2</DisplayName>",
                    "<DisplayName>Mask2</DisplayName><Bypass>true</Bypass>",
                    1,
                )
            }),
            "VideoFilterComponent:157: unsupported mask Bypass",
        ),
        (
            "corpus match name on the 26.5.1 record",
            masked_clip_26_5(157, |records| {
                records.replacen("AE.ADBE AEMask2<", "AE.ADBE AEMask<", 1)
            }),
            "VideoFilterComponent:157: unsupported mask parameter layout (MatchName Some(\"AE.ADBE AEMask\"), 35 parameters)",
        ),

    ] {
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        let kept: Vec<_> = project.sequences[0]
            .video_occurrences()
            .map(|clip| clip.id.as_deref())
            .collect();
        assert_eq!(kept, [Some("VideoClipTrackItem:9")], "{case}");
        let occurrence = omissions
            .iter()
            .find(|omission| omission.scope == OmissionScope::Occurrence)
            .unwrap_or_else(|| panic!("{case}: {omissions:?}"));
        assert!(
            occurrence.reason.contains(reason),
            "{case}: {}",
            occurrence.reason
        );
    }
}

#[test]
fn a_still_with_mask_path_keys_is_omitted_beside_its_sibling_without_a_guide() {
    // Only a video clip's mask converts its Mask Path keys. A keyed mask
    // omits the still, and the second still converts with no guide left
    // behind; static still masks remain supported.
    let still = masked_clip(keyed_mask(300, &two_path_keys()), &[]).replace(
        "<VideoStream ObjectID=\"8\"><Duration>2540160000000</Duration>",
        "<VideoStream ObjectID=\"8\"><IsStill>true</IsStill><Duration>2540160000000</Duration>",
    );
    let (project, mut omissions) =
        inspect_project_with_omissions(&still, Some("sequence-1")).unwrap();
    let sequence = &project.sequences[0];
    let kept: Vec<_> = sequence
        .video_occurrences()
        .map(|clip| clip.id.as_deref())
        .collect();
    assert_eq!(kept, [Some("VideoClipTrackItem:9")]);
    let occurrences: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.scope == OmissionScope::Occurrence)
        .map(|omission| omission.reason.as_str())
        .collect();
    let [reason] = occurrences.as_slice() else {
        panic!("{omissions:?}");
    };
    assert!(
        reason.contains("Mask Path keys on a still are not converted; this host has no admitted keyed mask guide"),
        "{reason}"
    );
    let document = crate::convert::premiere_to_tesseract(
        sequence,
        &project.media,
        &crate::tesseract_output::asset_ids_in_order(sequence, &project.media),
        &mut omissions,
    )
    .unwrap()
    .to_json_value()
    .unwrap();
    let types: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|layer| layer["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["Image", "Rect"]);
}

/// The native Premiere 26.5.1 save, packaged without its absolute
/// media and peak-file paths, and its one sequence.
const KEYED_MASK_26_5: &str = "feature_keyed_mask_path_26_5_saved.prproj";
const KEYED_MASK_SEQUENCE: &str = "5fe2e712-90a9-4044-b93a-7a75b79b1320";

#[test]
fn saved_affine_tracking_becomes_editable_path_keys_on_its_native_clock() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_mask_tracker_26_5.prproj");
    let xml = crate::format::read_xml(&fixture).unwrap();
    let (project, omissions) =
        inspect_project_with_omissions(&xml, Some("247c489b-caaf-40c3-a920-a10162ac0240")).unwrap();
    assert!(
        omissions
            .iter()
            .all(|item| item.scope != OmissionScope::Occurrence),
        "{omissions:?}"
    );
    assert!(omissions
        .iter()
        .any(|item| item.reason.contains("Saved affine mask tracking")));
    let sequence = project.single_sequence().unwrap();
    let frame = 8_511_237_907;
    assert_eq!(sequence.frame_rate.ticks_per_frame(), frame);
    let clip = sequence.video_occurrences().next().unwrap();
    let mask = clip.opacity_mask.as_ref().unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let tracker = document
        .root_element()
        .children()
        .find(|node| node.attribute("ObjectID") == Some("1201"))
        .unwrap();
    let raw_keys = tracker
        .children()
        .find(|node| node.has_tag_name("Keyframes"))
        .unwrap()
        .text()
        .unwrap();
    let times: Vec<i64> = raw_keys
        .split_terminator(';')
        .map(|key| key.split_once(',').unwrap().0.parse().unwrap())
        .collect();
    assert_eq!(times.len(), 184);
    assert_eq!((times[0], times[183]), (170_224_758_140, 1_727_781_295_121));
    assert_eq!(mask.path_keys.len(), times.len());
    // Native keys differ by individual ticks from index * rounded frame duration.
    for (key, time) in mask.path_keys.iter().zip(&times) {
        assert_eq!(key.source_ticks, *time);
        assert_eq!(key.path.vertices.len(), 4);
    }
    // The original ellipse centre, then centres saved in the second Tracker
    // matrix. The first matrix must place the outline at those observations.
    for (index, centre) in [
        (0, [0.48159513, 0.45504087]),
        (1, [0.48199415, 0.45453885]),
        (90, [0.47533980, 0.50409293]),
        (183, [0.51308346, 0.49770007]),
    ] {
        for (axis, expected) in centre.into_iter().enumerate() {
            let actual = mask.path_keys[index]
                .path
                .vertices
                .iter()
                .map(|vertex| f64::from(vertex.point[axis]))
                .sum::<f64>()
                / 4.0;
            assert!(
                (actual - expected).abs() < 1e-6,
                "key {index}, axis {axis}: {actual}"
            );
        }
    }
    assert_eq!(mask.path, mask.path_keys[0].path);
    // The native ellipse stores absolute handle positions: both handles share
    // the top vertex's y coordinate, not a zero relative y offset.
    let top = &mask.path.vertices[0];
    assert_eq!(top.point, [0.48159513, 0.19891007]);
    assert_eq!(top.in_tangent, [0.42568898, 0.19891007]);
    assert_eq!(top.out_tangent, [0.5375013, 0.19891007]);
    // The last saved affine matrix has translation [-0.22245486, -0.22462192].
    // These control positions include it, just as the anchor positions do.
    let top = &mask.path_keys[183].path.vertices[0];
    assert_eq!(top.in_tangent, [0.43054572, 0.100648336]);
    assert_eq!(top.out_tangent, [0.6027959, 0.10559781]);
    let mut trimmed = sequence.clone();
    let clip = trimmed.video_tracks[0].clip_mut(0);
    clip.in_ticks += TICKS;
    clip.start_ticks += TICKS;
    let editable = crate::tests::support::project_document_with_media(&trimmed, &project.media);
    let track = editable["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["target"]["propertyType"] == "shapePath")
        .unwrap();
    let keys = track["animator"]["keyframes"].as_array().unwrap();
    assert_eq!(keys.len(), times.len());
    for (key, time) in keys.iter().zip(&times) {
        let expected = ((*time - TICKS) as f64 * 1000.0 / TICKS as f64).round() as i64;
        assert_eq!(key["layerTime"].as_i64(), Some(expected));
        assert_eq!(key["easing"]["type"], "linear");
    }
    assert_ne!(keys[0]["value"], keys[183]["value"]);
    // The editable cubic consumes absolute control positions in the native
    // 1280x720 source frame. Relative vectors would displace both controls.
    let segment = &keys[183]["value"]["value"]["commands"][1];
    assert_eq!(segment["type"], "cubicTo");
    for (field, coordinate, pixels) in [
        ("c1x", 0.6027959_f32, 1280.0),
        ("c1y", 0.10559781, 720.0),
        ("c2x", 0.67100793, 1280.0),
        ("c2y", 0.28426215, 720.0),
    ] {
        assert_eq!(
            segment[field].as_f64(),
            Some(f64::from(coordinate) * pixels)
        );
    }
    // Supplementary writer check on a new catalogue-rate output owner, not a
    // change to the native input clock above. Native sequence rates are import
    // only; ordinary export selects its output profile. File inspection would
    // normally supply the codec. Edited FX export is covered by the mask-path
    // source-In regression in convert/tests/tesseract_to_premiere.rs.
    let expected = mask.clone();
    let mut output = crate::tests::support::video_sequence();
    let owner = output.video_tracks[0].clip_mut(0);
    owner.opacity_mask = Some(expected.clone());
    owner.end_ticks = 10 * TICKS;
    owner.out_ticks = 10 * TICKS;
    output.timeline_end_ticks = 10 * TICKS;
    let mut writable = crate::schema::PrProjectFile {
        sequences: vec![output],
        media: crate::tests::support::video_media(),
    };
    for media in writable.media.values_mut() {
        media.absolute_paths = vec![(
            crate::schema::records::MediaPathField::FilePath,
            std::env::temp_dir().join(&media.name),
        )];
        let relative = format!("./media/{}", media.name);
        media.relative_path = Some(relative.clone());
        media.relative_paths = vec![relative];
        if let Some(video) = &mut media.video {
            video.kind = crate::schema::PrMediaKind::Video {
                codec: Some(crate::schema::VideoCodec::H264),
                hdr_profile: None,
            };
        }
    }
    let written = project_xml(&writable).unwrap();
    let (reopened, _) = inspect_project_with_omissions(&written, None).unwrap();
    let reopened = reopened.single_sequence().unwrap();
    assert_eq!(reopened.frame_rate, crate::schema::FrameRate::Fps30);
    assert_eq!(
        reopened
            .video_occurrences()
            .next()
            .unwrap()
            .opacity_mask
            .as_ref()
            .unwrap(),
        &expected
    );
}

#[test]
fn tracker_samples_reject_truncation_perspective_and_inconsistent_centres() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_mask_tracker_26_5.prproj");
    let xml = crate::format::read_xml(&fixture).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let parameter = document
        .root_element()
        .children()
        .find(|node| node.attribute("ObjectID") == Some("1201"))
        .unwrap();
    let keys = parameter
        .children()
        .find(|node| node.has_tag_name("Keyframes"))
        .unwrap()
        .text()
        .unwrap();
    let (_, encoded) = keys.split(';').nth(1).unwrap().split_once(',').unwrap();
    let payload = STANDARD.decode(encoded).unwrap();
    let mut perspective = payload.clone();
    perspective[12..16].copy_from_slice(&0.25_f32.to_le_bytes());
    let mut different_centre = payload.clone();
    different_centre[64..68].copy_from_slice(&1.0_f32.to_le_bytes());
    for malformed in [payload[..3].to_vec(), perspective, different_centre] {
        assert!(crate::schema::decode_mask_tracker(&malformed).is_err());
    }
}

#[test]
fn a_saved_keyed_mask_path_reads_only_beside_identical_centre_keys() {
    // Clip K (`VideoClipTrackItem:82`) keys its Path at 0.5 s, 1.5 s and 2 s
    // (4, 4 and 5 vertices), and its mask Position (176) and Anchor Point
    // (182) with identical records: the same three keys at the outlines'
    // centres, automatic spatial tangents, and 0:0 as the static start.
    // Position minus Anchor Point stays zero, so the mask transform stays
    // the identity and only the Path converts.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(KEYED_MASK_26_5);
    let saved = crate::format::read_xml(&path).unwrap();
    let read = |xml: &str| inspect_project_with_omissions(xml, Some(KEYED_MASK_SEQUENCE)).unwrap();
    let (project, omissions) = read(&saved);
    assert!(
        omissions
            .iter()
            .all(|omission| omission.scope != OmissionScope::Occurrence),
        "{omissions:?}"
    );
    let k = project.sequences[0]
        .video_occurrences()
        .find(|clip| clip.id.as_deref() == Some("VideoClipTrackItem:82"))
        .unwrap();
    let mask = k.opacity_mask.as_ref().unwrap();
    assert_eq!(
        mask.path_keys
            .iter()
            .map(|key| (key.source_ticks, key.path.vertices.len()))
            .collect::<Vec<_>>(),
        [(TICKS / 2, 4), (3 * TICKS / 2, 4), (2 * TICKS, 5)]
    );
    assert_eq!(mask.path, mask.path_keys[0].path);
    // A centre that moves another way between equal keys, or two identical
    // centres that the point reader rejects, omit K alone.
    let tangent = "-1.5308079514744108e-18,-0.0083333303531010951;";
    let automatic = ",5,4,0,0,0.041666666666666664,0;";
    let unsupported = ",5,3,0,0,0.041666666666666664,0;";
    for (case, xml, reason) in [
        (
            "an Anchor Point tangent of the middle key",
            edit_start(
                saved.clone(),
                182,
                tangent,
                "-1.5308079514744108e-18,-0.0125;",
            ),
            "PointComponentParam:182: mask Position and Anchor Point differ; the mask transform is not converted",
        ),
        (
            "identical unsupported spatial flags",
            edit_start(
                edit_start(saved.clone(), 176, automatic, unsupported),
                182,
                automatic,
                unsupported,
            ),
            "PointComponentParam:176: unsupported spatial interpolation mode 5 with flags 3",
        ),
    ] {
        let (project, omissions) = read(&xml);
        assert_eq!(
            project.sequences[0].video_occurrences().count(),
            7,
            "{case}"
        );
        let occurrences: Vec<_> = omissions
            .iter()
            .filter(|omission| omission.scope == OmissionScope::Occurrence)
            .collect();
        let [omission] = occurrences.as_slice() else {
            panic!("{case}: {omissions:?}");
        };
        assert_eq!(omission.record, "82", "{case}");
        assert!(
            omission.reason.contains(reason),
            "{case}: {}",
            omission.reason
        );
    }
}

/// Whether a 26.5.1 mask parameter is one that Premiere 26.3 does not save:
/// the sharpness and levels controls.
fn sharpness_or_levels(id: u32) -> bool {
    (30..=37).contains(&id)
}

/// [`fixture_mask_26_5`] `mask_id` without its parameters whose
/// `ParameterID` is `dropped`, their records and their `Params` entries, the
/// others renumbered in their saved order: with [`sharpness_or_levels`], the
/// 26.3 form of both masks of the observed 26.3 project. The kept records are
/// the fixture's.
fn fixture_mask_without(mask_id: u32, dropped: impl Fn(u32) -> bool) -> String {
    let records = fixture_mask_26_5(mask_id);
    let wrapped = format!("<Records>{records}</Records>");
    let document = roxmltree::Document::parse(&wrapped).unwrap();
    let parameter_id = |node: roxmltree::Node<'_, '_>| {
        node.children()
            .find(|child| child.has_tag_name("ParameterID"))
            .and_then(|id| id.text()?.parse::<u32>().ok())
    };
    let dropped: std::collections::BTreeSet<&str> = document
        .root_element()
        .children()
        .filter(|node| {
            parameter_id(*node).is_some_and(&dropped) && node.has_tag_name("VideoComponentParam")
        })
        .filter_map(|node| node.attribute("ObjectID"))
        .collect();
    let mut output = String::new();
    for node in document
        .root_element()
        .children()
        .filter(|node| node.is_element())
    {
        if node
            .attribute("ObjectID")
            .is_some_and(|id| dropped.contains(id))
        {
            continue;
        }
        let text = &wrapped[node.range()];
        let params = node
            .children()
            .find(|child| child.has_tag_name("Component"))
            .and_then(|component| {
                component
                    .children()
                    .find(|child| child.has_tag_name("Params"))
            });
        let Some(params) = params else {
            output.push_str(text);
            continue;
        };
        let kept: String = params
            .children()
            .filter_map(|param| param.attribute("ObjectRef"))
            .filter(|id| !dropped.contains(id))
            .enumerate()
            .map(|(index, id)| format!("<Param Index=\"{index}\" ObjectRef=\"{id}\"/>"))
            .collect();
        let offset = node.range().start;
        output.push_str(&text[..params.range().start - offset]);
        output.push_str(&format!("<Params Version=\"1\">{kept}</Params>"));
        output.push_str(&text[params.range().end - offset..]);
    }
    output
}

/// The tracker state of every fixture mask (`ArbVideoComponentParam:193`,
/// which masks B to E name by `BinaryHash`).
const FIXTURE_TRACKER_STATE: &str = "DAAAAAgADAAEAAgACAAAABgAAABMAAAAEAAMAAAAAAAAAAcAAAAIABAAAAAAAAAABAAAACQAAAA0NTA1NGJlOC0xODg5LTQyNGYtOTdlYi1mN2Y1MDJkZGIxOTgAAAAAAAAAAA==";

/// `records` whose saved tracker state is `edit`ed.
fn with_tracker_state(records: String, edit: impl Fn(&mut Vec<u8>)) -> String {
    let mut state = STANDARD.decode(FIXTURE_TRACKER_STATE).unwrap();
    edit(&mut state);
    let edited = records.replace(FIXTURE_TRACKER_STATE, &STANDARD.encode(state));
    assert_ne!(edited, records, "the records name no edited tracker state");
    edited
}

/// Name the tracker `uuid`: the 36 bytes from offset 56 of the state.
fn naming_tracker(uuid: &'static str) -> impl Fn(&mut Vec<u8>) {
    move |state| state[56..92].copy_from_slice(uuid.as_bytes())
}

/// [`masked_clip_26_5`] with the fixture mask `mask_id` without its
/// parameters `dropped` ([`fixture_mask_without`]), after `edit`.
fn masked_clip_without(
    mask_id: u32,
    dropped: impl Fn(u32) -> bool,
    edit: impl Fn(String) -> String,
) -> String {
    with_chain(
        &with_second_clip(SOURCE),
        "<DefaultMotion>true</DefaultMotion><DefaultMotionComponentID>1</DefaultMotionComponentID>",
        &[(
            900,
            masked_opacity(900, mask_id) + &edit(fixture_mask_without(mask_id, dropped)),
        )],
    )
}

/// [`masked_clip_26_5`] with the 26.3 form of the fixture mask `mask_id`,
/// after `edit`.
fn masked_clip_26_3(mask_id: u32, edit: impl Fn(String) -> String) -> String {
    masked_clip_without(mask_id, sharpness_or_levels, edit)
}

#[test]
fn premiere_26_3_masks_import_naming_any_tracker() {
    // Fixture clip C's feathered ellipse in the 27-parameter form, its saved
    // tracker state naming another tracker, as a 26.3 save names its own;
    // then inverted at Mask Opacity 100, the shape of an expanded background
    // that shows the clip below through a feathered hole.
    let ellipse = |x: f32, y: f32, [ix, iy]: [f32; 2], [ox, oy]: [f32; 2]| PrPathVertex {
        smooth: true,
        point: [x, y],
        in_tangent: [ix, iy],
        out_tangent: [ox, oy],
    };
    let (tangent, far) = (0.361_925_f32, 0.638_075_f32);
    let expected = PrMask {
        raster: None,
        feather_keys: Vec::new(),
        expansion: 0.0,
        expansion_keys: Vec::new(),
        opacity_keys: Vec::new(),
        path: crate::schema::text::PrShapePath {
            vertices: vec![
                ellipse(0.5, 0.25, [tangent, 0.25], [far, 0.25]),
                ellipse(0.75, 0.5, [0.75, tangent], [0.75, far]),
                ellipse(0.5, 0.75, [far, 0.75], [tangent, 0.75]),
                ellipse(0.25, 0.5, [0.25, far], [0.25, tangent]),
            ],
            closed: true,
        },
        path_keys: Vec::new(),
        feather: 60.0,
        opacity: 100.0,
        inverted: false,
    };
    let another = naming_tracker("0f8e3c55-2b1d-4a6e-9c7f-3d2a1b0c9e8d");
    for (case, xml, expected) in [
        (
            "saved tracker",
            masked_clip_26_3(176, |records| records),
            expected.clone(),
        ),
        (
            "another tracker",
            masked_clip_26_3(176, |records| with_tracker_state(records, &another)),
            expected.clone(),
        ),
        (
            "inverted",
            masked_clip_26_3(176, |records| {
                edit_start(
                    with_tracker_state(records, &another),
                    285,
                    ",false,",
                    ",true,",
                )
            }),
            PrMask {
                raster: None,
                inverted: true,
                ..expected.clone()
            },
        ),
    ] {
        let (occurrence, omissions) = read(&xml);
        assert!(omissions.is_empty(), "{case}: {omissions:?}");
        assert_eq!(occurrence.opacity_mask, Some(expected), "{case}");
    }
    // The 26.5.1 form names any tracker too.
    let (occurrence, omissions) = read(&masked_clip_26_5(176, |records| {
        with_tracker_state(records, &another)
    }));
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(occurrence.opacity_mask, Some(expected));
}

#[test]
fn premiere_26_3_masks_outside_the_saved_profile_omit_the_occurrence() {
    let unknown_tracker = "mask control 24 (Tracker) holds an unknown value; only the saved default, naming any tracker, converts";
    for (case, xml, reason) in [
        (
            "one control fewer",
            masked_clip_without(
                176,
                |id| sharpness_or_levels(id) || id == 25,
                |records| records,
            ),
            "VideoFilterComponent:176: unsupported mask parameter layout",
        ),
        (
            "one sharpness control kept",
            masked_clip_without(
                176,
                |id| id != 30 && sharpness_or_levels(id),
                |records| records,
            ),
            "VideoFilterComponent:176: unsupported mask parameter layout",
        ),
        (
            "a tracker UUID in capitals",
            masked_clip_26_3(176, |records| {
                with_tracker_state(
                    records,
                    naming_tracker("0F8E3C55-2B1D-4A6E-9C7F-3D2A1B0C9E8D"),
                )
            }),
            unknown_tracker,
        ),
        (
            "tracker text that is no UUID",
            masked_clip_26_3(176, |records| {
                with_tracker_state(
                    records,
                    naming_tracker("0f8e3c55-2b1d-4a6e-9c7f-3d2a1b0c9e8z"),
                )
            }),
            unknown_tracker,
        ),
        (
            "tracker state holding other data",
            masked_clip_26_3(176, |records| {
                with_tracker_state(records, |state| state[32] = 1)
            }),
            unknown_tracker,
        ),
        (
            "tracker state one byte longer",
            masked_clip_26_3(176, |records| {
                with_tracker_state(records, |state| state.push(0))
            }),
            unknown_tracker,
        ),
        (
            "a tracked mask transform",
            masked_clip_26_3(176, |records| {
                edit_start(
                    records,
                    192,
                    "AQAAAAAAgD8AAAAAAAAAAAAAAAAAAIA/",
                    "AQAAAAAAgD8AAAAAAAAAAAAAAAAAAIA+",
                )
            }),
            "unsupported vector mask Tracker matrix or reference controls",
        ),
        (
            "tracked Path keys",
            masked_clip_26_3(176, |records| {
                edit_start(
                    records,
                    264,
                    "<Name>Path</Name>",
                    "<Name>Path</Name><IsTimeVarying>true</IsTimeVarying>",
                )
            }),
            "ArbVideoComponentParam:264: animated or unknown Path is unsupported",
        ),
        (
            "Feather with unmeasured velocity",
            masked_clip_26_3(176, |records| {
                edit_start(
                    records,
                    282,
                    "</StartKeyframe>",
                    "</StartKeyframe><Keyframes>0,60.,5,0,0,0,2,0.3;254016000000,30.,0,0,0,0,0,0;</Keyframes>",
                )
            }),
            "VideoComponentParam:282: Feather supports only all-Linear keys or zero-speed handles, with temporal flags 0",
        ),
        (
            "Feather with the wrong scalar record class",
            masked_clip_26_3(176, |records| {
                edit_start(
                    records,
                    282,
                    "ClassID=\"a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542\"",
                    "ClassID=\"cc12343e-f113-4d3b-ae05-b287db77d461\"",
                )
            }),
            "VideoComponentParam:282: unexpected Feather layout",
        ),
    ] {
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        let kept: Vec<_> = project.sequences[0]
            .video_occurrences()
            .map(|clip| clip.id.as_deref())
            .collect();
        assert_eq!(kept, [Some("VideoClipTrackItem:9")], "{case}");
        let occurrence = omissions
            .iter()
            .find(|omission| omission.scope == OmissionScope::Occurrence)
            .unwrap_or_else(|| panic!("{case}: {omissions:?}"));
        assert!(
            occurrence.reason.contains(reason),
            "{case}: {}",
            occurrence.reason
        );
    }
}

#[test]
fn a_still_keeps_its_opacity_mask_through_the_classifier() {
    // The classifier row (`OccurrenceEdit::OpacityMask`) reaches the still
    // reader: `keep_occurrence` keeps a still's one mask, as a flat video's.
    let still = masked_clip(mask(300, true), &[]).replace(
        "<VideoStream ObjectID=\"8\"><Duration>2540160000000</Duration>",
        "<VideoStream ObjectID=\"8\"><IsStill>true</IsStill><Duration>2540160000000</Duration>",
    );
    let (project, omissions) = inspect_project_with_omissions(&still, Some("sequence-1")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let occurrences: Vec<_> = project.sequences[0].video_occurrences().collect();
    assert_eq!(occurrences.len(), 2);
    assert!(occurrences[0].opacity_mask.is_some());
    assert!(occurrences[1].opacity_mask.is_none());
}

#[test]
fn written_opacity_mask_records_read_back_in_the_v7_form() {
    let mut sequence = video_sequence();
    let clip = sequence.video_tracks[0].clip_mut(0);
    clip.opacity_mask = Some(opacity_mask());
    let mut media = video_media();
    let source = media.get_mut(&MediaId("source".into())).unwrap();
    source.video.as_mut().unwrap().kind = crate::schema::PrMediaKind::Video {
        codec: Some(crate::schema::VideoCodec::H264),
        hdr_profile: None,
    };
    source.name = "source.mp4".into();
    source.relative_path = Some("./media/source.mp4".into());
    source.relative_paths = vec!["./media/source.mp4".into()];
    source.absolute_paths = vec![(
        crate::schema::records::MediaPathField::FilePath,
        "/media/source.mp4".into(),
    )];
    let project = crate::schema::PrProjectFile::from_sequences(vec![sequence], media);
    let xml = project_xml(&project).unwrap();
    // The Opacity component is materialized for the mask alone and names it.
    let graph = Graph::parse(&xml).unwrap();
    let opacity = graph
        .records()
        .find(|record| {
            record
                .element()
                .child("MatchName")
                .and_then(crate::format::graph::Element::text)
                == Some("AE.ADBE Opacity")
        })
        .expect("an Opacity component");
    let sub_components: Vec<_> = opacity
        .element()
        .child("SubComponents")
        .expect("SubComponents")
        .children()
        .map(|child| child.reference())
        .collect();
    let [reference] = sub_components.as_slice() else {
        panic!("{sub_components:?}");
    };
    let mask = graph.locate(reference, "test").unwrap();
    assert_eq!(mask.element().attribute("Version"), Some("7"));
    assert_eq!(
        mask.element()
            .child("Component")
            .and_then(|body| body.attribute("Version")),
        Some("5")
    );
    let params: Vec<_> = mask
        .element()
        .child("Component")
        .and_then(|body| body.child("Params"))
        .expect("Params")
        .children()
        .map(|param| param.reference())
        .collect();
    assert_eq!(params.len(), 13);
    let path = graph
        .follow::<ArbVideoComponentParam>(&params[5], "test")
        .unwrap();
    assert_eq!(path.value.parameter_id, "6");
    let encoded = path.value.start_keyframe_value.unwrap();
    assert_ne!(
        encoded.binary_hash,
        mask.element()
            .child("PremiereFilterPrivateData")
            .and_then(|data| data.attribute("BinaryHash"))
            .map(str::to_owned)
    );
    let payload = STANDARD.decode(encoded.value).unwrap();
    assert_eq!(&payload[..16], b"2cin\x02\0\0\0\0\0\0\0\x04\0\0\0");
    // The crate reader reads the written form back.
    let (reread, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let clip = reread.sequences[0].video_tracks[0].clip(0);
    assert_eq!(clip.opacity_mask, Some(opacity_mask()));
    assert_eq!(clip.opacity, 100.0);
}

/// Source-derived numeric mutation; not an Adobe-authored numeric-key oracle.
pub(super) fn numeric_mask_record_keys(records: String, name: &str, keys: &str) -> String {
    let wrapped = format!("<Records>{records}</Records>");
    let doc = roxmltree::Document::parse(&wrapped).unwrap();
    let node = doc
        .descendants()
        .find(|node| {
            node.has_tag_name("VideoComponentParam")
                && node
                    .children()
                    .any(|child| child.has_tag_name("Name") && child.text() == Some(name))
        })
        .unwrap();
    let original = &wrapped[node.range()];
    let edited = original.replace("<IsTimeVarying>false</IsTimeVarying>", "");
    let edited = edited.replace(
        "</StartKeyframe>",
        &format!(
            "</StartKeyframe><IsTimeVarying>true</IsTimeVarying><Keyframes>{keys}</Keyframes>"
        ),
    );
    assert_ne!(original, edited);
    records.replacen(original, &edited, 1)
}

#[test]
fn numeric_mask_native_scalar_layouts_read_signed_controls_and_zero_speed_curves() {
    use crate::schema::PrKeyframeEasing;
    for (modern, records) in [
        (false, mask(300, false)),
        (false, mask(300, true)),
        (true, fixture_mask_26_5(157)),
        (true, fixture_mask_without(157, sharpness_or_levels)),
    ] {
        let prefix = if modern { "" } else { "Mask " };
        let records = numeric_mask_record_keys(
            records,
            &format!("{prefix}Feather"),
            &format!("0,0,5,0,0,0,0,0.3;{TICKS},20,0,0,0,0.4,0,0;"),
        );
        let records = numeric_mask_record_keys(
            records,
            &format!("{prefix}Expansion"),
            &format!("0,-12,0,0,0,0,0,0;{TICKS},24,0,0,0,0,0,0;"),
        );
        let records = numeric_mask_record_keys(
            records,
            &format!("{prefix}Opacity"),
            &format!("0,100,4,0,0,0,0,0;{TICKS},35,0,0,0,0,0,0;"),
        );
        let xml = if modern {
            masked_clip_26_5(157, |_| records.clone())
        } else {
            masked_clip(records, &[])
        };
        let still = xml.replace(
            "<VideoStream ObjectID=\"8\"><Duration>2540160000000</Duration>",
            "<VideoStream ObjectID=\"8\"><IsStill>true</IsStill><Duration>2540160000000</Duration>",
        );
        assert_ne!(still, xml);
        let (stills, omissions) =
            inspect_project_with_omissions(&still, Some("sequence-1")).unwrap();
        assert!(stills.sequences[0]
            .video_occurrences()
            .all(|clip| clip.opacity_mask.is_none()));
        let reason = "numeric Opacity mask keys on a still are not converted";
        assert!(
            omissions
                .iter()
                .any(|omission| omission.reason.contains(reason)),
            "{omissions:?}"
        );
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        let mask = project.sequences[0]
            .video_occurrences()
            .find_map(|clip| clip.opacity_mask.as_ref())
            .unwrap_or_else(|| panic!("{omissions:?}"));
        assert_eq!(mask.expansion, -12.0);
        assert_eq!(
            mask.expansion_keys
                .iter()
                .map(|key| (key.source_ticks, key.value))
                .collect::<Vec<_>>(),
            [(0, -12.0), (TICKS, 24.0)]
        );
        assert_eq!(mask.opacity_keys[1].easing, PrKeyframeEasing::Hold);
        assert_eq!(
            mask.feather_keys[1].easing,
            PrKeyframeEasing::CubicBezier {
                x1: 0.3,
                y1: 0.0,
                x2: 0.6,
                y2: 1.0
            }
        );
    }
}

#[test]
fn numeric_mask_invalid_native_keys_omit_only_the_masked_occurrence() {
    for (name, keys, reason) in [
        ("Mask Feather", "0,-1,0,0,0,0,0,0;", "Mask Feather key"),
        ("Mask Expansion", "0,1001,0,0,0,0,0,0;", "Mask Expansion"),
        ("Mask Opacity", "0,101,0,0,0,0,0,0;", "Mask Opacity key"),
        ("Mask Expansion", "0,NaN,0,0,0,0,0,0;", "nonfinite"),
        ("Mask Expansion", "0,1,2,0,0,0,0,0;", "interpolation mode"),
        ("Mask Expansion", "0,1,5,0,0,0,2,0.3;", "zero-speed"),
        (
            "Mask Expansion",
            "0,0,0,0,0,0.16666666666666666,36,0.16666666666666666;169344000000,24,5,0,0,0.16666666666666666,0,0.16666666666666666;",
            "zero-speed",
        ),
        ("Mask Expansion", "0,1,0,1,0,0,0,0;", "temporal flags"),
        (
            "Mask Expansion",
            "0,1,0,0,0,0,0,0;0,2,0,0,0,0,0,0;",
            "increasing",
        ),
    ] {
        let xml = masked_clip(numeric_mask_record_keys(mask(300, false), name, keys), &[]);
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        assert_eq!(
            project.sequences[0].video_occurrences().count(),
            1,
            "{name}: {omissions:?}"
        );
        assert!(
            omissions.iter().any(|o| o.reason.contains(reason)),
            "{reason}: {omissions:?}"
        );
    }
    let inverted = edit_start(mask(300, false), 310, ",false,", ",true,");
    let xml = masked_clip(
        numeric_mask_record_keys(
            inverted,
            "Mask Opacity",
            &format!("0,100,0,0,0,0,0,0;{TICKS},50,0,0,0,0,0,0;"),
        ),
        &[],
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    assert_eq!(project.sequences[0].video_occurrences().count(), 1);
    assert!(
        omissions.iter().any(|o| o
            .reason
            .contains("inverted mask with Mask Opacity below 100")),
        "{omissions:?}"
    );
}
