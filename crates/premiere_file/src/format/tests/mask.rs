//! The static Opacity mask (JRB-2028): reader, classifier and writer, on the
//! corpus record forms and on Premiere 26.5.1's saved form (the records of
//! `feature_opacity_masks_26_5_strict.prproj`, verbatim).

use super::{
    animation::animation_fixture::masked_opacity_xml,
    effects::{
        blur, fixture_records, intrinsic, read, top_crop, with_chain, with_second_clip,
        DEFAULT_FLAGS, SOURCE,
    },
};
use crate::{
    format::{inspect_project_with_omissions, writer::project_xml, Graph},
    schema::{native::ArbVideoComponentParam, text::PrPathVertex, MediaId, PrMask},
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
        slider(7, "<Name>Mask Feather</Name>", "30.", "0", range, "<UpperUIBound>300</UpperUIBound>"),
        slider(8, "<Name>Mask Opacity</Name>", "100.", "0", "100", ""),
        slider(9, "<Name>Mask Expansion</Name>", "0.", &format!("-{range}"), range, "<LowerUIBound>-300</LowerUIBound><UpperUIBound>300</UpperUIBound>"),
        boolean(10, "4", "true"),
        slider(11, "", "2.", "0", "3", ""),
        slider(12, "", "0.", "0", "4294967296", ""),
        slider(13, "", "0.5", "0", "3.4028234663852886e+38", ""),
    )
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
        path: crate::schema::text::PrShapePath {
            vertices: pen_path(),
            closed: true,
        },
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
                path: crate::schema::text::PrShapePath {
                    vertices: vec![
                        corner(0.412409, 0.3778594),
                        corner(0.580659, 0.37726688),
                        corner(0.57932574, 0.4459865),
                        corner(0.4150757, 0.4477638),
                    ],
                    closed: true,
                },
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
    let tracking = mask(300, true).replace(
        &format!("<ParameterControlType>11</ParameterControlType><StartKeyframe>{STATIC},false,"),
        &format!("<ParameterControlType>11</ParameterControlType><StartKeyframe>{STATIC},true,"),
    );
    // `vhsvertical` writes `ParameterID` -1 on its two v8 tracking booleans.
    let unknown_id = mask(300, true).replace(
        "<ParameterID>15</ParameterID>",
        "<ParameterID>-1</ParameterID>",
    );
    // A v7 record with the v8 parameters, and a v8 record with v7 bounds.
    let v7_with_15 = mask(300, true).replace(
        "Version=\"8\"><Component Version=\"6\">",
        "Version=\"7\"><Component Version=\"5\">",
    );
    let v8_v7_bounds = mask(300, true).replace(
        "<UpperBound>5000</UpperBound><ParameterID>7</ParameterID>",
        "<UpperBound>1000</UpperBound><ParameterID>7</ParameterID>",
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
    for (case, xml, reason) in [
        (
            "keyed Mask Opacity",
            masked_clip(keyed_opacity, &[]),
            "VideoComponentParam:308: keyframed Mask Opacity is not supported; only static values convert",
        ),
        (
            "Expansion",
            masked_clip(expansion, &[]),
            "VideoComponentParam:309: Mask Expansion is not converted",
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
            "feather above the written bound",
            masked_clip(feather("3000."), &[]),
            "Mask Feather must be finite and within 0..=1000, the written record's bound",
        ),
        (
            "unknown ParameterID",
            masked_clip(unknown_id, &[]),
            "VideoComponentParam:315: unknown mask parameter",
        ),
        (
            "v7 record with 15 parameters",
            masked_clip(v7_with_15, &[]),
            "VideoFilterComponent:300: unsupported mask parameter layout",
        ),
        (
            "v8 record with the 26.5.1 match name",
            masked_clip(
                mask(300, true).replace("AE.ADBE AEMask<", "AE.ADBE AEMask2<"),
                &[],
            ),
            "VideoFilterComponent:300: unsupported mask record form (MatchName Some(\"AE.ADBE AEMask2\"), VideoFilterComponent Some(\"8\"), Component Some(\"6\"))",
        ),
        (
            "v8 record with v7 bounds",
            masked_clip(v8_v7_bounds, &[]),
            "VideoComponentParam:307: unexpected Mask Feather layout",
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
            "carries a mask (AE.ADBE AEMask sub-component; JRB-2028)",
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

/// The Premiere 26.5.1 fixture (Oracle run 17): five masked clips A to E.
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
fn fixture_mask_26_5(mask_id: u32) -> String {
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
                path: fixture_rectangle(),
                feather: 0.0,
                opacity: 50.0,
                inverted: false,
            },
        ),
        (
            "B",
            masked_clip_26_5(161, |records| records),
            PrMask {
                path: fixture_rectangle(),
                feather: 0.0,
                opacity: 100.0,
                inverted: false,
            },
        ),
        (
            "C",
            masked_clip_26_5(176, |records| records),
            PrMask {
                path: crate::schema::text::PrShapePath {
                    vertices: vec![
                        ellipse(0.5, 0.25, [tangent, 0.25], [far, 0.25]),
                        ellipse(0.75, 0.5, [0.75, tangent], [0.75, far]),
                        ellipse(0.5, 0.75, [far, 0.75], [tangent, 0.75]),
                        ellipse(0.25, 0.5, [0.25, far], [0.25, tangent]),
                    ],
                    closed: true,
                },
                feather: 60.0,
                opacity: 100.0,
                inverted: false,
            },
        ),
        (
            "D not inverted",
            masked_clip_26_5(180, |records| edit_start(records, 320, ",true,", ",false,")),
            PrMask {
                path: pen,
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
    // Keys under `IsTimeVarying` true still import as keys; a bound other
    // than 26 or 27 still fails closed.
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
    let (project, omissions) = inspect_project_with_omissions(
        &owner(&|records| edit_start(records, 155, "<UpperBound>26<", "<UpperBound>25<")),
        Some("sequence-1"),
    )
    .unwrap();
    assert_eq!(project.sequences[0].video_occurrences().count(), 1);
    assert!(
        omissions
            .iter()
            .any(|omission| omission.scope == OmissionScope::Occurrence
                && omission
                    .reason
                    .contains("VideoComponentParam:155: unexpected Opacity parameter layout")),
        "{omissions:?}"
    );
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
            "ArbVideoComponentParam:192: mask control 6 (Tracker) holds an unknown value; only the saved default converts",
        ),
        (
            "Feather above the written bound",
            masked_clip_26_5(176, |records| edit_start(records, 282, ",60.,", ",3000.,")),
            "Mask Feather must be finite and within 0..=1000, the written record's bound",
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
            "VideoFilterComponent:157: unsupported mask record form (MatchName Some(\"AE.ADBE AEMask\"), VideoFilterComponent Some(\"9\"), Component Some(\"7\"))",
        ),
        (
            // Fixture clip E: a masked effect omits its clip in this form as
            // in the corpus form; the clip never imports unblurred.
            "mask on Gaussian Blur",
            with_chain(
                &with_second_clip(SOURCE),
                DEFAULT_FLAGS,
                &[(
                    20,
                    blur(20).replace(
                        "</Component><MatchName>AE.ADBE Gaussian Blur 2</MatchName>",
                        "</Component><SubComponents Version=\"1\"><SubComponent Index=\"0\" ObjectRef=\"184\"/></SubComponents><MatchName>AE.ADBE Gaussian Blur 2</MatchName>",
                    ) + &fixture_mask_26_5(184),
                )],
            ),
            "carries a mask (AE.ADBE AEMask2 sub-component; JRB-2028); the clip is not converted without it",
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
fn a_still_with_an_opacity_mask_is_omitted_through_the_classifier() {
    // The classifier row (`OccurrenceEdit::OpacityMask`) reaches the still
    // reader: `keep_occurrence` names the edit.
    let still = masked_clip(mask(300, true), &[]).replace(
        "<VideoStream ObjectID=\"8\"><Duration>2540160000000</Duration>",
        "<VideoStream ObjectID=\"8\"><IsStill>true</IsStill><Duration>2540160000000</Duration>",
    );
    let (project, omissions) = inspect_project_with_omissions(&still, Some("sequence-1")).unwrap();
    assert_eq!(project.sequences[0].video_occurrences().count(), 1);
    assert!(
        omissions
            .iter()
            .any(|omission| omission.scope == OmissionScope::Occurrence
                && omission
                    .reason
                    .contains("Opacity mask on a still image is unsupported; occurrence omitted")),
        "{omissions:?}"
    );
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
