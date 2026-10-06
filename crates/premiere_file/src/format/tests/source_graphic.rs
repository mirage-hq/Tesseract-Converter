//! Source Graphic placements of the untouched Premiere 26.5.1 save pinned as
//! `premiere_isolated_source_graphic_26_5` (SHA-256
//! fbb324ef1170b1f7982f7ce6f5910ec68b7512a6e82c021db06e1bd63fb6b760): master
//! clip `Graphic` owns the shared Text objects `py` (Scale 150) and an empty
//! Text; placements I1 and I2 of sequence `Source graphic 26.5` keep their
//! own timing and static clip Motion, and the original placement of sequence
//! `Single-style point text` the default Motion. Cases that edit the XML are
//! supplementary controls, not Adobe saves.

use super::graphic::CLIP_OPACITY;
use crate::format::inspect_project_with_omissions;
use crate::schema::{PrBlendMode, PrGraphic, PrStaticTransform, PrVideoItem};
use crate::{Omission, OmissionScope};
use std::io::Read;

const SAVE: &[u8] = include_bytes!("../../../tests/fixtures/feature_source_graphic_26_5.prproj");
/// The sequence of I1 and I2.
const PLACEMENTS: &str = "9eaecdad-b77c-4b0a-8a94-0102641f46dc";
/// The sequence of the original placement.
const ORIGINAL: &str = "c8acf9c1-34b2-4086-9f55-d528950a7059";
const MASTER: &str = "MasterClip:4eba56ac-0cf4-4a57-99cd-fcfabbd21dfd";
const SHARED_EDITING: &str = "Source Graphic shared editing is not converted: each placement imports its own copy of the shared objects, so editing one copy leaves the others unchanged";
/// The master's empty Text object.
const EMPTY_TEXT: &str = "VideoFilterComponent:63";

fn save_xml() -> String {
    let mut xml = String::new();
    flate2::read::GzDecoder::new(SAVE)
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// `xml` with `from`, which occurs once, replaced by `to`.
fn edited(xml: &str, from: &str, to: &str) -> String {
    assert_eq!(xml.matches(from).count(), 1, "{from}");
    xml.replace(from, to)
}

/// `xml` with only the first occurrence of `from` replaced by `to`.
fn edited_first(xml: &str, from: &str, to: &str) -> String {
    assert!(xml.contains(from), "{from}");
    xml.replacen(from, to, 1)
}

/// The graphics of `sequence` in `xml`, bottom track first, and the reports.
fn graphics(xml: &str, sequence: &str) -> (Vec<PrGraphic>, Vec<Omission>) {
    let (project, omissions) = inspect_project_with_omissions(xml, Some(sequence)).unwrap();
    let graphics = project
        .sequences()
        .flat_map(|sequence| sequence.video_items())
        .filter_map(PrVideoItem::graphic)
        .cloned()
        .collect();
    (graphics, omissions)
}

fn motion(position: [f64; 2], scale: f64, rotation: f64) -> PrStaticTransform {
    PrStaticTransform {
        position,
        anchor_point: [0.5, 0.5],
        scale: [scale; 2],
        rotation,
    }
}

/// The Occurrence report of placement `record`, if any.
fn omission<'o>(omissions: &'o [Omission], record: &str) -> Option<&'o str> {
    omissions
        .iter()
        .find(|omission| omission.scope == OmissionScope::Occurrence && omission.record == record)
        .map(|omission| omission.reason.as_str())
}

/// `xml` with a clip Opacity of 40 in Screen (records 9070 to 9073) after
/// I1's Motion, as a media clip keeps one: without `DefaultOpacity`.
fn with_i1_clip_opacity(xml: &str) -> String {
    with_i1_clip_opacities(xml, &[false])
}

/// `xml` with one clip Opacity of 40 in Screen per entry of `masked` after
/// I1's Motion, in that order: records 9070 to 9073, then 9080 to 9083, each
/// masked by record 9300 where its entry is true.
fn with_i1_clip_opacities(xml: &str, masked: &[bool]) -> String {
    let chain = |defaults: &str| {
        format!(
            "<VideoComponentChain ObjectID=\"176\" ClassID=\"0970e08a-f58f-4108-b29a-1a717b8e12e2\" Version=\"3\">{defaults}"
        )
    };
    let mut xml = edited(
        xml,
        &chain("\n\t\t<DefaultOpacity>true</DefaultOpacity>\n\t\t<DefaultOpacityComponentID>2</DefaultOpacityComponentID>"),
        &chain(""),
    );
    let mut components = String::from("<Component Index=\"0\" ObjectRef=\"206\"/>");
    let mut records = String::new();
    for (index, &masked) in (1..).zip(masked) {
        let prefix = format!("90{}", 6 + index);
        components.push_str(&format!(
            "<Component Index=\"{index}\" ObjectRef=\"{prefix}0\"/>"
        ));
        let opacity = CLIP_OPACITY
            .replace("ObjectID=\"7", &format!("ObjectID=\"{prefix}"))
            .replace("ObjectRef=\"7", &format!("ObjectRef=\"{prefix}"))
            .replace("KEYED", "")
            .replace("KEYS", "")
            .replace("OPACITY", "40.")
            .replace("PRIMARY", "22")
            .replace("LEGACY", "10");
        records.push_str(&if masked {
            opacity.replace(
                "<MatchName>AE.ADBE Opacity</MatchName>",
                "<SubComponents Version=\"1\"><SubComponent Index=\"0\" ObjectRef=\"9300\"/></SubComponents><MatchName>AE.ADBE Opacity</MatchName>",
            )
        } else {
            opacity
        });
    }
    if masked.contains(&true) {
        records.push_str(&super::mask::mask(9300, true));
    }
    xml = edited(
        &xml,
        "<Component Index=\"0\" ObjectRef=\"206\"/>",
        &components,
    );
    xml.replace("</PremiereData>", &format!("{records}\n</PremiereData>"))
}

#[test]
fn a_source_graphic_placement_reads_its_clip_opacity_and_enable() {
    // Supplementary: I1 keeps a clip Opacity after its Motion, and I2 is
    // disabled.
    let xml = edited(
        &with_i1_clip_opacity(&save_xml()),
        "<SubClip ObjectRef=\"179\"/>",
        "<SubClip ObjectRef=\"179\"/>\n\t\t\t<IsMuted>true</IsMuted>",
    );
    let (placed, _) = graphics(&xml, PLACEMENTS);
    let [i1, i2] = placed.as_slice() else {
        panic!("two Source Graphic placements: {placed:?}");
    };
    assert_eq!(
        (i1.opacity, i1.blend_mode, i1.clip_motion, i1.enabled),
        (
            40.0,
            PrBlendMode::Screen,
            motion([0.35, 0.45], 80.0, 0.0),
            true
        )
    );
    // A disabled placement stays hidden with its Motion.
    assert_eq!(
        (i2.enabled, i2.clip_motion),
        (false, motion([0.7, 0.65], 60.0, 15.0))
    );
}

#[test]
fn unmeasured_source_graphic_forms_omit_only_their_placements() {
    let xml = save_xml();
    // Supplementary edits of I1 (record 145); I2 (146) still converts.
    for (xml, reason) in [
        // Keyed clip Motion on a graphic is unmeasured.
        (
            edited(
                &xml,
                "<Name>Scale</Name>\n\t\t<ParameterID>2</ParameterID>\n\t\t<UpperUIBound>200</UpperUIBound>\n\t\t<StartKeyframe>-91445760000000000,80.,0,0,0,0,0,0</StartKeyframe>",
                "<Name>Scale</Name>\n\t\t<IsTimeVarying>true</IsTimeVarying>\n\t\t<ParameterID>2</ParameterID>\n\t\t<UpperUIBound>200</UpperUIBound>\n\t\t<StartKeyframe>-91445760000000000,80.,0,0,0,0,0,0</StartKeyframe>\n\t\t<Keyframes>914457600000000,80.,0,0,0,0.16666666666666666,0,0.16666666666666666;914711616000000,90.,0,0,0,0.16666666666666666,0,0.16666666666666666;</Keyframes>",
            ),
            "VideoComponentChain:176: graphic clip Motion keys are not converted",
        ),
        // A Source Graphic has no crop guide, so exposing its cropped-away
        // content would be a silent concealment failure.
        (
            edited_first(
                &xml,
                "<Name>Crop Left</Name>\n\t\t<ParameterID>8</ParameterID>\n\t\t<StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe>",
                "<Name>Crop Left</Name>\n\t\t<ParameterID>8</ParameterID>\n\t\t<StartKeyframe>-91445760000000000,20.25,0,0,0,0,0,0</StartKeyframe>",
            ),
            "VideoComponentChain:176: Motion Crop, Linear Wipe or Track Matte Key on a Source Graphic placement is not converted",
        ),
        (
            edited_first(
                &xml,
                "<Name>Crop Left</Name>\n\t\t<ParameterID>8</ParameterID>\n\t\t<StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe>",
                "<Name>Crop Left</Name>\n\t\t<IsTimeVarying>true</IsTimeVarying>\n\t\t<ParameterID>8</ParameterID>\n\t\t<StartKeyframe>-91445760000000000,20.25,0,0,0,0,0,0</StartKeyframe>",
            ),
            "VideoComponentChain:176: Motion Crop, Linear Wipe or Track Matte Key on a Source Graphic placement is not converted",
        ),
        // A placement's own chain holds only its clip Motion and Opacity; the
        // master holds the objects.
        (
            edited(
                &xml,
                "<Component Index=\"0\" ObjectRef=\"206\"/>",
                "<Component Index=\"0\" ObjectRef=\"206\"/><Component Index=\"1\" ObjectRef=\"62\"/>",
            ),
            "VideoComponentChain:176: a Source Graphic placement's chain holds only its clip Motion and Opacity, not Some(\"AE.ADBE Text\")",
        ),
        // A mask on its clip Opacity has an unmeasured frame on a Source Graphic.
        (
            with_i1_clip_opacities(&xml, &[true]),
            "VideoComponentChain:176: a clip Opacity mask on a Source Graphic is not converted: its mask frame is unmeasured",
        ),
        // The compositing reader checks one clip Opacity: a second one, masked
        // or not, before or after it, would pass unchecked.
        (
            with_i1_clip_opacities(&xml, &[true, false]),
            "VideoComponentChain:176: duplicate intrinsic Opacity",
        ),
        (
            with_i1_clip_opacities(&xml, &[false, true]),
            "VideoComponentChain:176: duplicate intrinsic Opacity",
        ),
        (
            with_i1_clip_opacities(&xml, &[false, false]),
            "VideoComponentChain:176: duplicate intrinsic Opacity",
        ),
        // A component reference to a record of another type keeps its cause.
        (
            edited(
                &xml,
                "<Component Index=\"0\" ObjectRef=\"206\"/>",
                "<Component Index=\"0\" ObjectRef=\"206\"/><Component Index=\"1\" ObjectRef=\"177\"/>",
            ),
            "VideoComponentChain:176: expected VideoFilterComponent, found SubClip:177 (unsupported or cyclic edge)",
        ),
    ] {
        let (placed, omissions) = graphics(&xml, PLACEMENTS);
        assert_eq!(
            placed
                .iter()
                .map(|graphic| (graphic.id().unwrap(), graphic.clip_motion))
                .collect::<Vec<_>>(),
            [("VideoClipTrackItem:146", motion([0.7, 0.65], 60.0, 15.0))],
            "{reason}"
        );
        assert!(
            omission(&omissions, "145").is_some_and(|omitted| omitted.contains(reason)),
            "{reason}: {omissions:?}"
        );
    }

    // Edits of the shared master omit every placement of it, whatever hides
    // or masks the content: none shows the object without its control.
    for (from, to, reason) in [
        // Keys in shared content have no measured clock.
        (
            "<Name>Scale</Name>\n\t\t<ParameterID>4</ParameterID>\n\t\t<StartKeyframe>-91445760000000000,150.,0,0,0,0,0,0</StartKeyframe>",
            "<Name>Scale</Name>\n\t\t<IsTimeVarying>true</IsTimeVarying>\n\t\t<ParameterID>4</ParameterID>\n\t\t<StartKeyframe>-91445760000000000,150.,0,0,0,0,0,0</StartKeyframe>\n\t\t<Keyframes>0,150.,0,0,0,0.16666666666666666,0,0.16666666666666666;254016000000,120.,0,0,0,0.16666666666666666,0,0.16666666666666666;</Keyframes>",
            "VideoComponentChain:46: keys in Source Graphic shared content are not converted: their clock is unmeasured",
        ),
        (
            "<ID>1</ID>\n\t\t\t<DisplayName>Text</DisplayName>\n\t\t\t<InstanceName>py</InstanceName>",
            "<ID>1</ID>\n\t\t\t<DisplayName>Text</DisplayName>\n\t\t\t<InstanceName>py</InstanceName>\n\t\t\t<Bypass>true</Bypass>",
            "VideoFilterComponent:62: a bypassed graphic object is unsupported",
        ),
        (
            "<PremiereFilterPrivateData Encoding=\"base64\" BinaryHash=\"c40c6399-6b26-8c2c-feaf-d01b0000000d\"/>",
            "<PremiereFilterPrivateData Encoding=\"base64\" BinaryHash=\"c40c6399-6b26-8c2c-feaf-d01b0000000d\"/>\n\t\t<SubComponents Version=\"1\"><SubComponent Index=\"0\" ObjectRef=\"62\"/></SubComponents>",
            "VideoFilterComponent:63: a mask on a graphic object is not converted",
        ),
        (
            "<VideoComponentChain ObjectID=\"46\" ClassID=\"0970e08a-f58f-4108-b29a-1a717b8e12e2\" Version=\"3\">",
            "<VideoComponentChain ObjectID=\"46\" ClassID=\"0970e08a-f58f-4108-b29a-1a717b8e12e2\" Version=\"3\">\n\t\t<DefaultMotion>true</DefaultMotion>",
            "VideoComponentChain:46: a Source Graphic chain with its own Motion or Opacity defaults is unsupported",
        ),
        // The master must play the placements' generator source.
        (
            "<Source ObjectRef=\"64\"/>\n\t\t\t<ClipID>95b59568",
            "<Source ObjectRef=\"71\"/>\n\t\t\t<ClipID>95b59568",
            "graphic source identity mismatch",
        ),
    ] {
        let (placed, omissions) = graphics(&edited(&xml, from, to), PLACEMENTS);
        assert!(placed.is_empty(), "{reason}: {placed:?}");
        for record in ["145", "146"] {
            assert!(
                omission(&omissions, record).is_some_and(|omitted| omitted.contains(reason)),
                "{reason}: {record}: {omissions:?}"
            );
        }
        // With no placement shown, neither the empty Text's import font nor
        // the shared editing is reported.
        assert!(
            omissions
                .iter()
                .all(|omission| omission.record != EMPTY_TEXT && omission.record != MASTER),
            "{reason}: {omissions:?}"
        );
    }
}

#[test]
fn a_graphic_without_a_master_chain_keeps_the_ordinary_graphic_rules() {
    // Supplementary: without the master's own chain the placements are
    // ordinary graphics again, whatever master they share: I1 and I2 keep
    // the clip Motion that ordinary graphics reject, and the original, whose
    // own chain holds no object, has nothing to show.
    let xml = edited(&save_xml(), "<VideoComponentChain ObjectRef=\"46\"/>", "");
    let (placed, omissions) = graphics(&xml, PLACEMENTS);
    assert!(placed.is_empty(), "{placed:?}");
    for record in ["145", "146"] {
        assert!(
            omission(&omissions, record).is_some_and(|omitted| omitted
                .contains("graphic clip Motion and Opacity must keep their defaults")),
            "{record}: {omissions:?}"
        );
    }
    assert!(
        omissions.iter().all(|omission| omission.record != MASTER),
        "{omissions:?}"
    );
    let error = inspect_project_with_omissions(&xml, Some(ORIGINAL))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(
            "VideoComponentChain:158: a graphic without text or shape objects is unsupported"
        ),
        "{error}"
    );
    assert!(!error.contains(SHARED_EDITING), "{error}");
}
