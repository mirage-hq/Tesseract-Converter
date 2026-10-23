//! Lumetri Color ↔ canonical `primaryGrade`/`vignette` against the pinned
//! native records of `human_lumetri_contrast.xml`.
//!
//! Native human-inputs-20261004.prproj SHA-256
//! e74d088116570ddb7178b127129036755be2f8e553dea80f98aa59b601585b76, effects
//! sequence b3aecac7-c452-48ba-a5e1-737807cb32ee: component 557, parameters
//! 772–901 copied verbatim offline into one-clip.xml's synthetic host. The
//! author edited Exposure 1, Contrast 25 and a keyed Saturation 130 → 60; other
//! values below are supplementary scalar edits of those native records, not
//! independently saved Adobe proof. Nothing here proves Lumetri render
//! fidelity or Premiere's authority of saved scalars over Lumetri blobs.
use super::*;
use crate::schema::{PrLumetriGrade, LUMETRI, LUMETRI_SATURATION, LUMETRI_WHITES};

const NATIVE: &str = include_str!("../../../../tests/fixtures/human_lumetri_contrast.xml");

/// The native values of the pinned grade: Exposure 1, Contrast 25, keyed
/// Saturation starting at 130, every other control neutral.
const PINNED_GRADE: [f64; 10] = [0.0, 0.0, 1.0, 25.0, 0.0, 0.0, 0.0, 0.0, 130.0, 0.0];

/// `NATIVE` with `from` replaced by `to` inside the parameter record whose
/// ObjectID is `object_id`.
fn edit(xml: &str, object_id: &str, from: &str, to: &str) -> String {
    let tag = if xml.contains(&format!("<VideoComponentParam ObjectID=\"{object_id}\"")) {
        "VideoComponentParam"
    } else {
        "ArbVideoComponentParam"
    };
    let start = xml
        .find(&format!("<{tag} ObjectID=\"{object_id}\""))
        .unwrap();
    let end = start + xml[start..].find(&format!("</{tag}>")).unwrap();
    let record = xml[start..end].replace(from, to);
    assert_ne!(record, xml[start..end], "{object_id}: {from}");
    format!("{}{record}{}", &xml[..start], &xml[end..])
}

/// `xml` with the static value of the scalar record `object_id` set to `value`.
fn set_static(xml: &str, object_id: &str, value: &str) -> String {
    let start = xml
        .find(&format!("<VideoComponentParam ObjectID=\"{object_id}\""))
        .unwrap();
    let field = start + xml[start..].find("<StartKeyframe>").unwrap() + "<StartKeyframe>".len();
    let end = field + xml[field..].find("</StartKeyframe>").unwrap();
    let mut fields: Vec<&str> = xml[field..end].split(',').collect();
    fields[1] = value;
    format!("{}{}{}", &xml[..field], fields.join(","), &xml[end..])
}

fn native_effects(xml: &str) -> (Vec<PrEffect>, Vec<Omission>) {
    let (project, omissions) = crate::format::inspect_project_with_omissions(xml, None).unwrap();
    let effects = project.single_sequence().unwrap().video_tracks[0]
        .clip(0)
        .effects
        .clone();
    (effects, omissions)
}

fn import_native(xml: &str) -> (Value, Vec<Omission>) {
    let (project, mut omissions) =
        crate::format::inspect_project_with_omissions(xml, None).unwrap();
    let (wire, losses) =
        imported_with_omissions(project.single_sequence().unwrap(), &project.media);
    omissions.extend(losses);
    (wire, omissions)
}

fn grade(effect: &PrEffect) -> [f64; 10] {
    let PrEffectParams::LumetriGrade(PrLumetriGrade { values, .. }) = effect.params else {
        panic!("expected a Lumetri grade: {effect:?}");
    };
    values
}

fn omitted<'a>(omissions: &'a [Omission], text: &str) -> Option<&'a Omission> {
    omissions
        .iter()
        .find(|omission| omission.reason.contains(text))
}

#[test]
fn human_lumetri_imports_one_primary_grade_with_saturation_keys_and_no_neutral_vignette() {
    use sha2::{Digest, Sha256};
    assert_eq!(
        format!("{:x}", Sha256::digest(NATIVE.as_bytes())),
        "4af7a4075c31e3735461074a18aeb436ff03ec7bfc85c9edd37be08907dac05c"
    );
    let (effects, omissions) = native_effects(NATIVE);
    // The Vignette section is at its defaults (Amount 0): no vignette.
    assert_eq!(effects.len(), 1, "{omissions:?}");
    assert_eq!(grade(&effects[0]), PINNED_GRADE);
    assert_eq!(effects[0].animations.len(), 1);
    assert_eq!(effects[0].animations[0].param, &LUMETRI_SATURATION);
    let keys = effects[0].animations[0].keys.scalar().unwrap();
    assert_eq!(
        keys.iter().map(|key| key.value).collect::<Vec<_>>(),
        [130.0, 60.0]
    );
    // The pinned record is the layout default for every unmapped section,
    // so the only notes are the two declared approximations.
    assert_eq!(omissions.len(), 2, "{omissions:?}");
    assert!(omissions
        .iter()
        .all(|omission| omission.record == "VideoFilterComponent:557"
            && omission.kind == OmissionKind::Approximated));
    assert!(omitted(&omissions, "one editable primaryGrade").is_some());
    assert!(omitted(&omissions, "Contrast/360").is_some());

    let (wire, losses) = import_native(NATIVE);
    assert_eq!(losses.len(), 2, "{losses:?}");
    let effects = wire["composition"]["layers"][0]["effects"]
        .as_array()
        .unwrap();
    assert_eq!(effects.len(), 1);
    assert_eq!(
        effects[0]["effect"],
        json!({
            "type": "primaryGrade", "semanticVersion": 1, "temperature": 0.0, "tint": 0.0,
            "exposure": 0.384615385, "contrast": 0.069444444, "highlights": 0.0, "shadows": 0.0,
            "whites": 0.0, "blacks": 0.0, "saturation": 0.272727273, "vibrance": 0.0
        })
    );
    let entries = wire["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["target"]["paramName"], "saturation");
    assert_eq!(entries[0]["target"]["effectId"], effects[0]["id"]);
    let keys = entries[0]["animator"]["keyframes"].as_array().unwrap();
    assert_eq!(
        keys.iter()
            .map(|key| (
                key["layerTime"].as_i64().unwrap(),
                key["value"]["value"].as_f64().unwrap()
            ))
            .collect::<Vec<_>>(),
        [(0, 0.272727273), (2513, -0.363636364)]
    );
}

/// `ParameterID`s that the 26.5.2 layout has and the Premiere 22/23 Lumetri
/// Color layout (component version 7, body version 5, 103 parameters) does
/// not: the second Input LUT and Look popups, Look Color Space, the HDR
/// tone-mapping toggles, Intensity 123, the hue-curve selectors and the four
/// later hue curves, Auto Tone and the LUT/Look asset slots.
const OLDER_LAYOUT_ABSENT: [usize; 27] = [
    104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122,
    123, 124, 125, 126, 127, 128, 129, 130,
];

/// `xml` rewritten to the Premiere 22/23 Lumetri Color layout, as derived
/// from the parameter names, `ParameterID`s, record versions and defaults of
/// a local Premiere 23 project: the [`OLDER_LAYOUT_ABSENT`] records removed
/// and the remaining ones renumbered in order, record versions 9 (scalars)
/// and 2 (arbitrary data), `Tone` for `Light`, `WB Selector` for `White
/// Balance`, and the only hue curve (ParameterID 42, unnamed) saved as a kind-1
/// curve without points whose first two unused slots still hold the diagonal
/// `(0, 0), (1, 1)`, as Premiere 23 saves it. Synthetic: no Premiere 22/23 save is committed, so
/// this pins the reader's layout independence, not Premiere's upgrade.
fn older_lumetri_layout(xml: &str) -> String {
    let document = roxmltree::Document::parse(xml).unwrap();
    let mut absent = Vec::new();
    for node in document.root_element().children().filter(|node| {
        matches!(
            node.tag_name().name(),
            "VideoComponentParam" | "ArbVideoComponentParam"
        )
    }) {
        let id: usize = node
            .children()
            .find(|child| child.has_tag_name("ParameterID"))
            .and_then(|child| child.text())
            .unwrap()
            .parse()
            .unwrap();
        if OLDER_LAYOUT_ABSENT.contains(&id) {
            absent.push((node.attribute("ObjectID").unwrap().to_owned(), node.range()));
        }
    }
    assert_eq!(absent.len(), OLDER_LAYOUT_ABSENT.len());
    let mut older = xml.to_owned();
    for (_, range) in absent.iter().rev() {
        older.replace_range(range.clone(), "");
    }
    for (object_id, _) in &absent {
        let line = "<Param Index=\"";
        let reference = format!("ObjectRef=\"{object_id}\"/>");
        let end = older.find(&reference).unwrap() + reference.len();
        let start = older[..end].rfind(line).unwrap();
        older.replace_range(start..end, "");
    }
    let mut index = 0;
    let mut renumbered = String::new();
    let mut rest = older.as_str();
    while let Some(at) = rest.find("<Param Index=\"") {
        let value_start = at + "<Param Index=\"".len();
        let value_end = value_start + rest[value_start..].find('"').unwrap();
        renumbered.push_str(&rest[..value_start]);
        renumbered.push_str(&index.to_string());
        index += 1;
        rest = &rest[value_end..];
    }
    renumbered.push_str(rest);
    assert_eq!(index, 103);
    let empty_hue_curve = {
        use base64::{engine::general_purpose::STANDARD, Engine};
        let mut bytes = aftereffects_file::lumetri::LumetriCurve {
            kind: 1,
            points: Vec::new(),
        }
        .encode()
        .unwrap();
        for (slot, value) in [(24, 1.0_f64), (32, 1.0)] {
            bytes[slot..slot + 8].copy_from_slice(&value.to_le_bytes());
        }
        STANDARD.encode(bytes)
    };
    let older = edit(
        &renumbered,
        "830",
        "<Name>Hue vs Sat</Name>",
        "<Name> </Name>",
    );
    let start = older
        .find("f146cafe-eab3-19ed-0ee8-de0e00000214\">")
        .unwrap();
    let end = start + older[start..].find('<').unwrap();
    let older = format!(
        "{}00000001-0000-0000-0000-000000000214\">{empty_hue_curve}\n\t\t{}",
        &older[..start],
        &older[end..]
    );
    older
        .replace("<Name>Light</Name>", "<Name>Tone</Name>")
        .replace("<Name>White Balance</Name>", "<Name>WB Selector</Name>")
        .replace(
            "ClassID=\"313e54d4-6903-49ad-b0bf-8262cdd10f4e\" Version=\"3\"",
            "ClassID=\"313e54d4-6903-49ad-b0bf-8262cdd10f4e\" Version=\"2\"",
        )
        .replace("\" Version=\"10\">", "\" Version=\"9\">")
        .replace(
            "ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"9\"",
            "ClassID=\"d10da199-beea-4dd1-b941-ed3a78766d50\" Version=\"7\"",
        )
        .replace("<Component Version=\"7\">", "<Component Version=\"5\">")
}

#[test]
fn older_lumetri_layout_imports_basic_and_creative_scalars_by_parameter_id() {
    // Temperature 16, Tint -20, Whites 30.5, Vibrance -20 and Creative Sharpen
    // 10 on the older layout, beside the pinned Exposure, Contrast and keyed
    // Saturation.
    let mut xml = older_lumetri_layout(NATIVE);
    assert!(!xml.contains("<ParameterID>130</ParameterID>"));
    for (object_id, value) in [
        ("786", "16."),
        ("787", "-20."),
        ("795", "30.5"),
        ("814", "-20."),
        ("813", "10."),
    ] {
        xml = set_static(&xml, object_id, value);
    }
    let (effects, omissions) = native_effects(&xml);
    assert_eq!(effects.len(), 1, "{omissions:?}");
    assert_eq!(
        grade(&effects[0]),
        [16.0, -20.0, 1.0, 25.0, 0.0, 0.0, 30.5, 0.0, 130.0, -20.0]
    );
    assert_eq!(effects[0].animations.len(), 1);
    assert_eq!(effects[0].animations[0].param, &LUMETRI_SATURATION);
    // The two declared approximations and Creative Sharpen; the older empty
    // Hue vs Sat curve and the absent 26.5.2 parameters are not reported.
    assert_eq!(omissions.len(), 3, "{omissions:?}");
    assert!(omitted(&omissions, "Lumetri Creative Sharpen omitted").is_some());
    assert!(omitted(&omissions, "Hue Saturation Curves omitted").is_none());
    // An older Hue vs Sat curve with a point is still reported.
    let pointed = curve_base64(&aftereffects_file::lumetri::LumetriCurve {
        kind: 1,
        points: vec![[0.5, 0.75]],
    });
    let start = xml.find("00000001-0000-0000-0000-000000000214\">").unwrap();
    let end = start + xml[start..].find('<').unwrap();
    let pointed = format!(
        "{}00000002-0000-0000-0000-000000000214\">{pointed}\n\t\t{}",
        &xml[..start],
        &xml[end..]
    );
    let (_, omissions) = native_effects(&pointed);
    assert!(
        omitted(
            &omissions,
            "Lumetri Hue Saturation Curves omitted: Hue vs Sat has points under saved kind 1"
        )
        .is_some(),
        "{omissions:?}"
    );
}

/// Base64 of a saved Lumetri curve.
fn curve_base64(curve: &aftereffects_file::lumetri::LumetriCurve) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    STANDARD.encode(curve.encode().unwrap())
}

#[test]
fn human_lumetri_grade_controls_map_by_declared_scales() {
    // ObjectIDs of Temperature, Tint, Exposure, Contrast, Highlights, Shadows,
    // Whites, Blacks and Vibrance in the pinned component.
    let mut xml = NATIVE.to_owned();
    for (object_id, value) in [
        ("786", "58."),
        ("787", "20."),
        ("791", "2.6"),
        ("792", "-90."),
        ("793", "120."),
        ("794", "-51."),
        ("795", "10.8"),
        ("796", "-12.4"),
        ("814", "83.5"),
    ] {
        xml = set_static(&xml, object_id, value);
    }
    let (wire, losses) = import_native(&xml);
    assert_eq!(losses.len(), 2, "{losses:?}");
    let effect = &wire["composition"]["layers"][0]["effects"][0]["effect"];
    // The fitted scales: Temperature/116, -Tint/100, Exposure/2.6,
    // Contrast/360, Highlights/240, Shadows/510, Whites/54, Blacks/124,
    // (Saturation-100)/110, Vibrance/167.
    for (name, expected) in [
        ("temperature", 0.5),
        // Lumetri's positive Tint is magenta, FX's green.
        ("tint", -0.2),
        ("exposure", 1.0),
        ("contrast", -0.25),
        ("highlights", 0.5),
        ("shadows", -0.1),
        ("whites", 0.2),
        ("blacks", -0.1),
        ("saturation", 0.272727273),
        ("vibrance", 0.5),
    ] {
        assert_eq!(effect[name], expected, "{name}: {effect}");
    }
}

#[test]
fn human_lumetri_out_of_range_controls_and_keys_clamp_with_diagnostics() {
    // Whites: saved ±150, of which primaryGrade's ±1 represents ±54.
    let keyed = edit(
        &set_static(NATIVE, "795", "-125."),
        "795",
        "<LowerBound>",
        "<IsTimeVarying>true</IsTimeVarying><Keyframes>0,-125.,0,0,0,0,0,0;127008000000,150.,0,0,0,0,0,0;</Keyframes><LowerBound>",
    );
    let (effects, omissions) = native_effects(&keyed);
    let values = grade(&effects[0]);
    assert_eq!(values[6], -54.0);
    let whites = effects[0]
        .animations
        .iter()
        .find(|animation| animation.param == &LUMETRI_WHITES)
        .unwrap();
    assert_eq!(
        whites
            .keys
            .scalar()
            .unwrap()
            .iter()
            .map(|key| key.value)
            .collect::<Vec<_>>(),
        [-54.0, 54.0]
    );
    // The tonal controls' label is their Lumetri name.
    let note = omitted(&omissions, "Whites -125 clamped to -54").unwrap();
    assert_eq!(note.kind, OmissionKind::Approximated);
    assert!(omitted(&omissions, "Whites key values clamped to -54 to 54").is_some());
    let (wire, _) = import_native(&keyed);
    let entry = wire["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["target"]["paramName"] == "whites")
        .unwrap();
    let keys = entry["animator"]["keyframes"].as_array().unwrap();
    assert_eq!(keys[0]["value"]["value"], -1.0);
    assert_eq!(keys[1]["value"]["value"], 1.0);
    assert_eq!(keys[1]["layerTime"], 500);
}

#[test]
fn human_lumetri_creative_saturation_folds_into_keyed_saturation() {
    let xml = set_static(NATIVE, "815", "150.");
    let (effects, omissions) = native_effects(&xml);
    let values = grade(&effects[0]);
    assert_eq!(values[8], 195.0);
    assert_eq!(
        effects[0].animations[0]
            .keys
            .scalar()
            .unwrap()
            .iter()
            .map(|key| key.value)
            .collect::<Vec<_>>(),
        [195.0, 90.0]
    );
    assert!(omitted(
        &omissions,
        "Lumetri Creative Saturation 150 folded multiplicatively"
    )
    .is_some());
    // Beyond what primaryGrade represents, the fold clamps.
    let (effects, omissions) = native_effects(&set_static(NATIVE, "815", "200."));
    assert_eq!(grade(&effects[0])[8], 210.0);
    assert!(omitted(
        &omissions,
        "Lumetri Saturation key values clamped to 0 to 210"
    )
    .is_some());
    assert!(omitted(&omissions, "Lumetri Saturation 260 clamped to 210").is_some());
}

#[test]
fn human_lumetri_off_sections_import_neutral_controls_with_diagnostics() {
    // Basic Correction off: its controls import neutral and keep no keys.
    let basic_off = set_static(&set_static(NATIVE, "775", "false"), "814", "40.");
    let (effects, omissions) = native_effects(&basic_off);
    assert_eq!(effects.len(), 1, "{omissions:?}");
    assert!(effects[0].enabled);
    let values = grade(&effects[0]);
    assert_eq!(values[..9], [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 100.0]);
    assert_eq!(values[9], 40.0);
    assert!(effects[0].animations.is_empty());
    for label in ["Lumetri Exposure", "Lumetri Contrast", "Lumetri Saturation"] {
        let note = omitted(
            &omissions,
            &format!("{label} not imported: its Lumetri section is off"),
        )
        .unwrap();
        assert_eq!(note.kind, OmissionKind::Omitted, "{note:?}");
    }
    // Creative off: Vibrance imports neutral; Basic Correction is kept.
    let creative_off = set_static(&set_static(NATIVE, "803", "false"), "814", "40.");
    let (effects, omissions) = native_effects(&creative_off);
    assert_eq!(grade(&effects[0]), PINNED_GRADE);
    assert!(omitted(&omissions, "Lumetri Vibrance not imported").is_some());
    // With Basic Correction off and Creative neutral, the grade is neutral:
    // no effect, only the diagnostics of the controls it does not import.
    let (effects, omissions) = native_effects(&set_static(NATIVE, "775", "false"));
    assert!(effects.is_empty(), "{effects:?}");
    assert!(omitted(&omissions, "Lumetri Exposure not imported").is_some());
    // A bypassed Lumetri keeps its non-neutral sections editable and bypassed.
    let bypassed = set_static(NATIVE, "882", "-2.5").replace(
        "<DisplayName>Lumetri Color</DisplayName>",
        "<DisplayName>Lumetri Color</DisplayName><Bypass>true</Bypass>",
    );
    let (effects, _) = native_effects(&bypassed);
    assert_eq!(effects.len(), 2);
    assert!(effects.iter().all(|effect| !effect.enabled));
}

#[test]
fn human_lumetri_unreadable_control_keeps_grade_siblings() {
    let renamed = edit(
        NATIVE,
        "792",
        "<Name>Contrast</Name>",
        "<Name>Different</Name>",
    );
    let (effects, omissions) = native_effects(&renamed);
    assert_eq!(effects.len(), 1, "{omissions:?}");
    let mut expected = PINNED_GRADE;
    expected[3] = 0.0;
    assert_eq!(grade(&effects[0]), expected);
    assert_eq!(effects[0].animations.len(), 1);
    let note = omitted(
        &omissions,
        "Lumetri Contrast omitted: unexpected Lumetri Contrast name",
    )
    .unwrap();
    assert_eq!(note.kind, OmissionKind::Omitted);

    // A wrong Basic Correction enable omits the grade, never the vignette.
    let enable = edit(
        &set_static(NATIVE, "882", "-2.5"),
        "775",
        "<Name> </Name>",
        "<Name>Different</Name>",
    );
    let (effects, omissions) = native_effects(&enable);
    assert_eq!(effects.len(), 1);
    assert_eq!(
        effects[0].params,
        PrEffectParams::LumetriVignette([-2.5, 50.0, 50.0])
    );
    assert!(omitted(
        &omissions,
        "unexpected Lumetri Basic Correction enable name"
    )
    .is_some());

    // Shared identity ambiguity omits the whole Lumetri.
    let duplicate = edit(
        NATIVE,
        "792",
        "<ParameterID>12</ParameterID>",
        "<ParameterID>13</ParameterID>",
    );
    let (effects, omissions) = native_effects(&duplicate);
    assert!(effects.is_empty());
    assert!(omitted(&omissions, "duplicate Lumetri ParameterID").is_some());
}

#[test]
fn human_lumetri_unmapped_sections_are_diagnosed_by_parameter_id() {
    // Faded Film, the Creative Look popup, RGB Curves and Color Wheels
    // (Shadows luma 1), which import as a second colorCurves.
    let mut xml = set_static(NATIVE, "812", "20.");
    xml = edit(&xml, "806", "<StartKeyframe>0,0,", "<StartKeyframe>0,5,");
    xml = edit(
        &xml,
        "825",
        "261f3840-e8cf-a8cd-fa5e-bac10000082c\">AAAAAAIA",
        "00000000-e8cf-a8cd-fa5e-bac10000082c\">AAAAAAMA",
    );
    xml = edit(
        &xml,
        "851",
        "0b5436c4-bae0-bb64-a2f6-c8c40000006c\">AAAAAAAAAAAAAAAAAADgPw",
        "00000000-bae0-bb64-a2f6-c8c40000006c\">AAAAAAAAAAAAAAAAAADwPw",
    );
    let (effects, omissions) = native_effects(&xml);
    assert_eq!(effects.len(), 2, "{omissions:?}");
    assert_eq!(grade(&effects[0]), PINNED_GRADE);
    assert_eq!(
        effects[1].params,
        PrEffectParams::LumetriWheels(aftereffects_file::lumetri::Wheels {
            wheels: [
                [0.0, 1.0, 0.0],
                [0.0, 0.5, 0.0],
                [0.0, 0.5, 0.0],
                [0.0, 0.5, 0.0]
            ]
        })
    );
    assert!(
        omitted(&omissions, "Lumetri Color Wheels").is_none(),
        "{omissions:?}"
    );
    for (section, ids) in [("Creative Faded Film", "28"), ("Creative Look", "25")] {
        let note = omitted(
            &omissions,
            &format!("Lumetri {section} omitted: saved ParameterID {ids} differ"),
        )
        .unwrap_or_else(|| panic!("{section}: {omissions:?}"));
        assert_eq!(note.kind, OmissionKind::Omitted);
        assert_eq!(note.record, "VideoFilterComponent:557");
    }
    assert!(omitted(&omissions, "no lookTransform was created").is_some());
    // An RGB Curves value whose Luma curve has three points at x 0 is no
    // curve: omitted with its reason, the grade kept.
    let note = omitted(
        &omissions,
        "Lumetri RGB Curves omitted: RGB curve 0 is not a kind-0 curve",
    )
    .unwrap_or_else(|| panic!("{omissions:?}"));
    assert_eq!(note.record, "VideoFilterComponent:557");
    // An off section is named as such.
    let wheels_off = edit(
        &edit(
            NATIVE,
            "851",
            "0b5436c4-bae0-bb64-a2f6-c8c40000006c\">AAAAAAAAAAAAAAAAAADgPw",
            "00000000-bae0-bb64-a2f6-c8c40000006c\">AAAAAAAAAAAAAAAAAADwPw",
        ),
        "846",
        "-91445760000000000,true,",
        "-91445760000000000,false,",
    );
    let (effects, omissions) = native_effects(&wheels_off);
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert!(
        omitted(
            &omissions,
            "Lumetri Color Wheels not imported: its Lumetri Color Wheels section is off"
        )
        .is_some(),
        "{omissions:?}"
    );
}

/// `lumetri_rgb_curves_probe.prproj`: a Premiere 26.5.2 project of eight 1 s
/// clips of the in-house ramp, each with a native Lumetri Color whose RGB
/// Curves were set offline (`lumetri_rgb_curves_probe.provenance.json`),
/// decompressed.
fn rgb_curves_probe_xml() -> String {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let bytes = include_bytes!("../../../../tests/fixtures/lumetri_rgb_curves_probe.prproj");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "d129dadcef1c137ad8329ceddd26acfed7cfcca45f963570c4fbad1fec6639d8"
    );
    let mut xml = String::new();
    flate2::read::GzDecoder::new(&bytes[..])
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

#[test]
fn rgb_curves_probe_imports_each_clip_curve_in_its_channel_with_the_rendered_shape() {
    let xml = rgb_curves_probe_xml();
    let (project, omissions) = crate::format::inspect_project_with_omissions(&xml, None).unwrap();
    let sequence = project.single_sequence().unwrap();
    let raised = vec![[0.0, 0.0], [0.5, 0.75], [1.0, 1.0]];
    let diagonal = vec![[0.0, 0.0], [1.0, 1.0]];
    // Clip 1 raises Luma, clips 2-4 Red, Green, Blue; 5-7 shape Luma.
    let expected: [Option<usize>; 8] = [
        None,
        Some(0),
        Some(1),
        Some(2),
        Some(3),
        Some(0),
        Some(0),
        Some(0),
    ];
    for (index, channel) in expected.into_iter().enumerate() {
        let effects = &sequence.video_tracks[0].clip(index).effects;
        let Some(channel) = channel else {
            assert!(effects.is_empty(), "clip {index}: {effects:?}");
            continue;
        };
        assert_eq!(effects.len(), 1, "clip {index}: {omissions:?}");
        let PrEffectParams::LumetriCurves(curves) = &effects[0].params else {
            panic!("clip {index}: {:?}", effects[0]);
        };
        for (other, points) in curves.curves.iter().enumerate() {
            if other != channel {
                assert_eq!(points, &diagonal, "clip {index} curve {other}");
            }
        }
        if (1..=4).contains(&index) {
            assert_eq!(curves.curves[channel], raised, "clip {index}");
        }
    }
    let PrEffectParams::LumetriCurves(line) = &sequence.video_tracks[0].clip(6).effects[0].params
    else {
        unreachable!()
    };
    assert_eq!(line.curves[0], [[0.0, 0.2], [1.0, 0.8]]);
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.kind == OmissionKind::Omitted),
        "{omissions:?}"
    );
    // The imported FX Master of clip 1 follows Premiere's render of it
    // (8-bit codes at inputs 16, 48, 64, 96, 128, 176, 208, 240; lossless
    // intermediate of lumetri_rgb_curves_probe.premiere-direct-v1).
    let (wire, _) = imported_with_omissions(sequence, &project.media);
    let master = wire["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["activeRange"]["start"].as_f64() == Some(1000.0))
        .map(|layer| {
            layer["effects"][0]["effect"]["curves"]["master"]
                .as_array()
                .unwrap()
                .clone()
        })
        .unwrap_or_else(|| panic!("{wire}"));
    let value = |x: f64| {
        let pair = master
            .windows(2)
            .find(|pair| pair[0]["x"].as_f64().unwrap() <= x && x <= pair[1]["x"].as_f64().unwrap())
            .unwrap();
        let [x0, y0, x1, y1] = [&pair[0]["x"], &pair[0]["y"], &pair[1]["x"], &pair[1]["y"]]
            .map(|value| value.as_f64().unwrap());
        y0 + (x - x0) / (x1 - x0) * (y1 - y0)
    };
    for (code, rendered) in [16, 48, 64, 96, 128, 176, 208, 240]
        .into_iter()
        .zip([28.0, 82.0, 108.0, 154.0, 192.0, 228.0, 242.0, 251.0])
    {
        let imported = value(f64::from(code) / 255.0) * 255.0;
        assert!(
            (imported - rendered).abs() <= 2.0,
            "{code}: {imported} vs {rendered}"
        );
    }
}

/// `lumetri_color_wheels_probe.prproj`: the in-house eight-clip ramp host of
/// `lumetri_rgb_curves_probe` with each clip's Color Wheels set offline
/// (`lumetri_color_wheels_probe.provenance.json`), decompressed.
fn color_wheels_probe_xml() -> String {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let bytes = include_bytes!("../../../../tests/fixtures/lumetri_color_wheels_probe.prproj");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "1e65e56727ef4886074597f2d90b5144212132bfa321d41bd2be46d19a80d6f9"
    );
    let mut xml = String::new();
    flate2::read::GzDecoder::new(&bytes[..])
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// The saved wheels of the probe's clips (Shadows, Midtones, Highlights,
/// fourth wheel; hue degrees, luma, amount).
const WHEELS_PROBE: [[[f64; 3]; 4]; 8] = [
    [
        [0.0, 0.5, 0.5],
        [0.0, 0.5, 0.5],
        [0.0, 0.5, 0.0],
        [0.0, 0.5, 0.0],
    ],
    [
        [0.0, 0.5, 0.0],
        [0.0, 0.5, 0.5],
        [0.0, 0.5, 0.5],
        [0.0, 0.5, 0.0],
    ],
    [
        [0.0, 0.5, 0.5],
        [0.0, 0.5, 0.0],
        [0.0, 0.5, 0.5],
        [0.0, 0.5, 0.0],
    ],
    [
        [120.0, 0.5, 0.5],
        [240.0, 0.5, 0.5],
        [0.0, 0.5, 0.5],
        [0.0, 0.5, 0.0],
    ],
    [
        [0.0, 0.75, 0.0],
        [0.0, 0.75, 0.0],
        [0.0, 0.25, 0.0],
        [0.0, 0.5, 0.0],
    ],
    [
        [200.0, 0.3, 0.6],
        [0.0, 0.5, 0.0],
        [0.0, 0.5, 0.0],
        [0.0, 0.5, 0.0],
    ],
    [
        [0.0, 0.5, 0.0],
        [200.0, 0.3, 0.6],
        [0.0, 0.5, 0.0],
        [0.0, 0.5, 0.0],
    ],
    [
        [30.0, 0.6, 0.4],
        [150.0, 0.4, 0.3],
        [270.0, 0.7, 0.5],
        [60.0, 0.8, 0.9],
    ],
];

/// Premiere 26.5.2's render of the probe (lossless intermediate of
/// `lumetri_color_wheels_probe.premiere-direct-v1`, frame 30k+15 of clip k,
/// grey ramp): 8-bit RGB at input codes 0, 16, 64, 128, 192, 240 and 255.
const WHEELS_RENDERED: [[[f64; 3]; 7]; 8] = [
    [
        [95.0, 0.0, 0.0],
        [109.0, 5.0, 5.0],
        [146.0, 46.0, 46.0],
        [187.0, 112.0, 112.0],
        [223.0, 183.0, 183.0],
        [248.0, 238.0, 238.0],
        [255.0, 255.0, 255.0],
    ],
    [
        [0.0, 0.0, 0.0],
        [53.0, 11.0, 11.0],
        [123.0, 52.0, 52.0],
        [187.0, 112.0, 112.0],
        [240.0, 176.0, 176.0],
        [255.0, 225.0, 225.0],
        [255.0, 240.0, 240.0],
    ],
    [
        [60.0, 0.0, 0.0],
        [75.0, 7.0, 7.0],
        [122.0, 51.0, 51.0],
        [183.0, 115.0, 115.0],
        [242.0, 179.0, 179.0],
        [255.0, 227.0, 227.0],
        [255.0, 241.0, 241.0],
    ],
    [
        [0.0, 16.0, 0.0],
        [1.0, 29.0, 10.0],
        [33.0, 70.0, 78.0],
        [123.0, 127.0, 154.0],
        [215.0, 184.0, 206.0],
        [254.0, 228.0, 238.0],
        [255.0, 241.0, 248.0],
    ],
    [
        [48.0, 48.0, 48.0],
        [64.0, 64.0, 64.0],
        [104.0, 104.0, 104.0],
        [150.0, 150.0, 150.0],
        [192.0, 192.0, 192.0],
        [222.0, 222.0, 222.0],
        [231.0, 231.0, 231.0],
    ],
    [
        [0.0, 0.0, 9.0],
        [0.0, 6.0, 24.0],
        [15.0, 52.0, 71.0],
        [95.0, 120.0, 133.0],
        [176.0, 188.0, 194.0],
        [236.0, 239.0, 241.0],
        [255.0, 255.0, 255.0],
    ],
    [
        [0.0, 0.0, 0.0],
        [4.0, 11.0, 19.0],
        [31.0, 54.0, 71.0],
        [89.0, 117.0, 134.0],
        [165.0, 185.0, 196.0],
        [233.0, 238.0, 241.0],
        [255.0, 255.0, 255.0],
    ],
    [
        [23.0, 9.0, 0.0],
        [35.0, 25.0, 4.0],
        [80.0, 73.0, 54.0],
        [151.0, 138.0, 144.0],
        [230.0, 204.0, 240.0],
        [255.0, 249.0, 255.0],
        [255.0, 255.0, 255.0],
    ],
];

#[test]
fn color_wheels_probe_imports_each_clip_as_channel_curves_that_follow_premiere_renders() {
    let xml = color_wheels_probe_xml();
    let (project, omissions) = crate::format::inspect_project_with_omissions(&xml, None).unwrap();
    let sequence = project.single_sequence().unwrap();
    for (index, saved) in WHEELS_PROBE.into_iter().enumerate() {
        let effects = &sequence.video_tracks[0].clip(index).effects;
        assert_eq!(effects.len(), 1, "clip {index}: {omissions:?}");
        assert_eq!(
            effects[0].params,
            PrEffectParams::LumetriWheels(aftereffects_file::lumetri::Wheels { wheels: saved }),
            "clip {index}"
        );
    }
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.kind == OmissionKind::Omitted),
        "{omissions:?}"
    );
    let (wire, omissions) = imported_with_omissions(sequence, &project.media);
    let layers = wire["composition"]["layers"].as_array().unwrap();
    for (index, rendered) in WHEELS_RENDERED.into_iter().enumerate() {
        let layer = layers
            .iter()
            .find(|layer| layer["activeRange"]["start"].as_f64() == Some(1000.0 * index as f64))
            .unwrap_or_else(|| panic!("clip {index}: {wire}"));
        let effects = layer["effects"].as_array().unwrap();
        assert_eq!(effects.len(), 1, "clip {index}: {effects:?}");
        let curves = &effects[0]["effect"]["curves"];
        assert_eq!(
            curves["master"],
            serde_json::json!([{"x": 0.0, "y": 0.0}, {"x": 1.0, "y": 1.0}]),
            "clip {index}"
        );
        for (channel, name) in ["red", "green", "blue"].into_iter().enumerate() {
            let points = curves[name].as_array().unwrap();
            assert!(points.len() <= 32, "clip {index} {name}");
            let value = |x: f64| {
                let pair = points
                    .windows(2)
                    .find(|pair| {
                        pair[0]["x"].as_f64().unwrap() <= x && x <= pair[1]["x"].as_f64().unwrap()
                    })
                    .unwrap();
                let [x0, y0, x1, y1] = [&pair[0]["x"], &pair[0]["y"], &pair[1]["x"], &pair[1]["y"]]
                    .map(|value| value.as_f64().unwrap());
                y0 + (x - x0) / (x1 - x0) * (y1 - y0)
            };
            for (code, expected) in [0, 16, 64, 128, 192, 240, 255].into_iter().zip(rendered) {
                let imported = value(f64::from(code) / 255.0) * 255.0;
                assert!(
                    (imported - expected[channel]).abs() <= 4.0,
                    "clip {index} {name} code {code}: {imported} vs Premiere {}",
                    expected[channel]
                );
            }
        }
    }
    assert!(
        omissions.iter().any(|omission| omission
            .reason
            .contains("Lumetri Color Wheels map to colorCurves")),
        "{omissions:?}"
    );
    // Exported, every clip's wheel curves stay native Lumetri RGB Curves
    // (identity Luma, the same points, Premiere's spline within one code),
    // not a routing loss.
    for layer in layers.iter().filter(|layer| layer["effects"].is_array()) {
        let effects = layer["effects"].clone();
        let report = routing_report(document_with_effects(effects.clone()));
        assert!(
            !report
                .losses
                .iter()
                .any(|loss| loss.omission.reason.contains("colorCurves")),
            "{:?}",
            report.losses
        );
        let (project, _) = export(document_with_effects(effects));
        let written = exported_effects(&project);
        assert_eq!(written.len(), 1);
        let PrEffectParams::LumetriCurves(curves) = &written[0].params else {
            panic!("{:?}", written[0]);
        };
        assert_eq!(curves.curves[0], [[0.0, 0.0], [1.0, 1.0]]);
    }
}

#[test]
fn lumetri_color_wheels_follow_rgb_curves_and_respect_the_wheel_mode() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let wheels: Vec<u8> = [0.0, 0.5, 0.0, 0.0, 0.5, 0.5, 0.0, 0.5, 0.0, 0.0, 0.5, 0.0]
        .into_iter()
        .flat_map(f64::to_le_bytes)
        .collect();
    let with_wheels = |xml: &str| {
        let start = xml.find("0b5436c4-bae0-bb64-a2f6-c8c40000006c\">").unwrap();
        let end = start + xml[start..].find('<').unwrap();
        format!(
            "{}00000047-0000-0000-0000-00000000006c\">{}\n\t\t{}",
            &xml[..start],
            STANDARD.encode(&wheels),
            &xml[end..]
        )
    };
    let xml = with_wheels(&with_rgb_curves([
        DIAGONAL,
        &[[0.0, 0.0], [0.5, 0.75], [1.0, 1.0]],
        DIAGONAL,
        DIAGONAL,
    ]));
    let (effects, omissions) = native_effects(&xml);
    assert_eq!(effects.len(), 3, "{omissions:?}");
    assert!(matches!(
        effects[1].params,
        PrEffectParams::LumetriCurves(_)
    ));
    assert!(matches!(
        effects[2].params,
        PrEffectParams::LumetriWheels(_)
    ));
    // A changed wheel-mode checkbox (ParameterID 101, record 847) keeps the
    // wheels out with a diagnostic.
    let single = edit(
        &xml,
        "847",
        "-91445760000000000,false,",
        "-91445760000000000,true,",
    );
    let (effects, omissions) = native_effects(&single);
    assert_eq!(effects.len(), 2, "{omissions:?}");
    assert!(
        omitted(
            &omissions,
            "Lumetri Color Wheels omitted: the wheel-mode checkbox ParameterID 101 differs"
        )
        .is_some(),
        "{omissions:?}"
    );
}

#[test]
fn hsl_secondary_hue_key_is_omitted_with_the_measured_qualifier_gap() {
    // Record 859 (ParameterID 73): a hue-only key, centre 0.5, range 0.3,
    // softness 0.1, Saturation and Lightness off.
    let xml = edit(
        NATIVE,
        "859",
        "BinaryHash=\"39a419ab-5a1c-7d5c-3db0-76fc00000034\">AAAAAD8AAAAAAAAAAAEAAAA/AAAAAAAAAAABAAAAPwAAAAAAAAAAAQ==",
        "BinaryHash=\"00000073-0000-0000-0000-000000000034\">AAAAAD+amZk+zczMPQEAAAA/AAAAAAAAAAAAAAAAPwAAAAAAAAAAAA==",
    );
    // The key adds no effect: the grade reads as it does without it.
    let (effects, omissions) = native_effects(&xml);
    assert_eq!(effects, native_effects(NATIVE).0);
    let omission = omitted(
        &omissions,
        "Lumetri HSL Secondary omitted: saved ParameterID 73 differ",
    )
    .unwrap_or_else(|| panic!("{omissions:?}"));
    assert!(
        omission
            .reason
            .contains("select HLS saturation and lightness where FX selects HSV saturation"),
        "{omission:?}"
    );
}

/// `NATIVE` with its RGB Curves (record 825) holding the Luma, Red, Green
/// and Blue `curves`.
fn with_rgb_curves(curves: [&[[f64; 2]]; 4]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let bytes: Vec<u8> = curves
        .into_iter()
        .flat_map(|points| {
            aftereffects_file::lumetri::LumetriCurve {
                kind: 0,
                points: points.to_vec(),
            }
            .encode()
            .unwrap()
        })
        .collect();
    let start = NATIVE
        .find("261f3840-e8cf-a8cd-fa5e-bac10000082c\">")
        .unwrap();
    let end = start + NATIVE[start..].find('<').unwrap();
    format!(
        "{}00000003-0000-0000-0000-00000000082c\">{}\n\t\t{}",
        &NATIVE[..start],
        STANDARD.encode(bytes),
        &NATIVE[end..]
    )
}

const DIAGONAL: &[[f64; 2]] = &[[0.0, 0.0], [1.0, 1.0]];

#[test]
fn lumetri_rgb_curves_import_as_fitted_color_curves_in_saved_channel_order() {
    // In-house curves measured in Premiere 26.5.2: a raised Luma midpoint and
    // a Blue line from 0.2 to 0.8.
    let xml = with_rgb_curves([
        &[[0.0, 0.0], [0.5, 0.75], [1.0, 1.0]],
        DIAGONAL,
        DIAGONAL,
        &[[0.0, 0.2], [1.0, 0.8]],
    ]);
    let (effects, omissions) = native_effects(&xml);
    assert_eq!(effects.len(), 2, "{omissions:?}");
    assert_eq!(grade(&effects[0]), PINNED_GRADE);
    let PrEffectParams::LumetriCurves(curves) = &effects[1].params else {
        panic!("{:?}", effects[1]);
    };
    assert_eq!(curves.curves[0], [[0.0, 0.0], [0.5, 0.75], [1.0, 1.0]]);
    assert!(
        omitted(&omissions, "RGB Curves omitted").is_none(),
        "{omissions:?}"
    );
    let (wire, losses) = import_native(&xml);
    let effect = &wire["composition"]["layers"][0]["effects"][1]["effect"];
    assert_eq!(effect["type"], "colorCurves", "{wire}");
    assert_eq!(
        effect["curves"]["red"],
        serde_json::json!([{"x": 0.0, "y": 0.0}, {"x": 1.0, "y": 1.0}])
    );
    assert_eq!(
        effect["curves"]["blue"],
        serde_json::json!([{"x": 0.0, "y": 0.2}, {"x": 1.0, "y": 0.8}])
    );
    let master = effect["curves"]["master"].as_array().unwrap();
    assert!((3..=32).contains(&master.len()), "{master:?}");
    // The spline passes the saved midpoint (0.5, 0.75) within one 8-bit code.
    let at_half = master
        .windows(2)
        .find(|pair| pair[0]["x"].as_f64().unwrap() <= 0.5 && 0.5 <= pair[1]["x"].as_f64().unwrap())
        .map(|pair| {
            let [x0, y0, x1, y1] = [&pair[0]["x"], &pair[0]["y"], &pair[1]["x"], &pair[1]["y"]]
                .map(|value| value.as_f64().unwrap());
            y0 + (0.5 - x0) / (x1 - x0) * (y1 - y0)
        })
        .unwrap();
    assert!((at_half - 0.75).abs() <= 1.0 / 255.0, "{at_half}");
    assert!(
        losses.iter().any(|loss| loss
            .reason
            .contains("Lumetri RGB Curves map to colorCurves")),
        "{losses:?}"
    );
}

#[test]
fn lumetri_rgb_curves_off_identity_or_unrepresentable_create_no_color_curves() {
    let raised: &[[f64; 2]] = &[[0.0, 0.0], [0.5, 0.75], [1.0, 1.0]];
    // Diagonal curves with an extra diagonal point change nothing.
    let (effects, omissions) = native_effects(&with_rgb_curves([
        &[[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]],
        DIAGONAL,
        DIAGONAL,
        DIAGONAL,
    ]));
    assert_eq!(effects.len(), 1, "{omissions:?}");
    // The Curves section off: no colorCurves, diagnosed.
    let off = edit(
        &with_rgb_curves([raised, DIAGONAL, DIAGONAL, DIAGONAL]),
        "821",
        "-91445760000000000,true,",
        "-91445760000000000,false,",
    );
    let (effects, omissions) = native_effects(&off);
    assert_eq!(effects.len(), 1);
    assert!(omitted(
        &omissions,
        "Lumetri RGB Curves not imported: its Lumetri Curves section is off"
    )
    .is_some());
    // An end point moved off x 1 is outside colorCurves' domain.
    let (effects, omissions) = native_effects(&with_rgb_curves([
        DIAGONAL,
        &[[0.0, 0.0], [0.9, 1.0]],
        DIAGONAL,
        DIAGONAL,
    ]));
    assert_eq!(effects.len(), 1);
    assert!(
        omitted(&omissions, "Lumetri RGB Curves omitted: RGB curve 1").is_some(),
        "{omissions:?}"
    );
}

#[test]
fn human_lumetri_vignette_imports_independent_editable_radial_controls() {
    for (amount, native) in [("-2.5", -2.5), ("2.5", 2.5)] {
        let xml = set_static(NATIVE, "882", amount);
        let (wire, losses) = import_native(&xml);
        assert_eq!(losses.len(), 2, "{losses:?}");
        let effect = &wire["composition"]["layers"][0]["effects"][1]["effect"];
        assert_eq!(effect["type"], "vignette");
        // The fitted mapping of Amount, Midpoint 50 and Feather 50.
        let expected = aftereffects_file::lumetri::vignette::to_fx([native, 50.0, 50.0]);
        for (field, want) in ["amount", "radius", "feather"].into_iter().zip(expected) {
            let got = effect[field].as_f64().unwrap();
            assert!((got - want).abs() < 1e-6, "{field}: {got} != {want}");
        }
    }
    // Amount 0 darkens nothing, whatever its Midpoint and Feather: no vignette.
    let (wire, _) = import_native(&set_static(NATIVE, "883", "20."));
    let effects = wire["composition"]["layers"][0]["effects"]
        .as_array()
        .unwrap();
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0]["effect"]["type"], "primaryGrade");
    let (_, omissions) = native_effects(&set_static(NATIVE, "884", "30."));
    assert!(omitted(
        &omissions,
        "Lumetri Vignette Roundness omitted: saved ParameterID 53"
    )
    .is_some());
}

#[test]
fn human_lumetri_vignette_enable_and_bad_control_do_not_change_the_grade() {
    // A Vignette section that is off imports no vignette; a nonzero Amount
    // in it is diagnosed.
    let off = set_static(&set_static(NATIVE, "881", "false"), "882", "-2.5");
    let (effects, omissions) = native_effects(&off);
    assert_eq!(effects.len(), 1);
    assert!(effects[0].enabled);
    assert_eq!(grade(&effects[0]), PINNED_GRADE);
    let note = omitted(
        &omissions,
        "Lumetri Vignette Amount -2.5 not imported: its Lumetri section is off",
    )
    .unwrap();
    assert_eq!(note.kind, OmissionKind::Omitted);
    let renamed = edit(NATIVE, "882", "<Name>Amount</Name>", "<Name>Wrong</Name>");
    let (effects, omissions) = native_effects(&renamed);
    assert_eq!(effects.len(), 1);
    assert_eq!(grade(&effects[0]), PINNED_GRADE);
    assert!(omitted(&omissions, "Vignette replacement omitted").is_some());
    let keyed = edit(
        NATIVE,
        "881",
        "<StartKeyframe>",
        "<IsTimeVarying>true</IsTimeVarying><StartKeyframe>",
    );
    let (effects, omissions) = native_effects(&keyed);
    assert_eq!(effects.len(), 1);
    let note = omitted(&omissions, "Vignette replacement omitted").unwrap();
    assert!(
        note.reason
            .contains("requires static Lumetri Vignette enable"),
        "{note:?}"
    );
    assert!(!note.reason.contains("Basic Correction"), "{note:?}");
}

#[test]
fn human_lumetri_keyed_white_balance_imports_affine_tracks() {
    // Temperature ±116 and Tint ±100 are primaryGrade's ±1.
    let mut keyed = NATIVE.to_owned();
    for (id, limit) in [("786", 116), ("787", 100)] {
        keyed = edit(
            &set_static(&keyed, id, &format!("-{limit}.")),
            id,
            "<LowerBound>",
            &format!("<IsTimeVarying>true</IsTimeVarying><Keyframes>0,-{limit}.,0,0,0,0,0,0;127008000000,{limit}.,0,0,0,0,0,0;</Keyframes><LowerBound>"),
        );
    }
    let (wire, losses) = import_native(&keyed);
    assert_eq!(losses.len(), 2, "{losses:?}");
    for (name, expected) in [("temperature", -1.0), ("tint", 1.0)] {
        let entry = wire["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["target"]["paramName"] == name)
            .unwrap();
        let keys = entry["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0]["value"]["value"], expected);
        assert_eq!(keys[1]["value"]["value"], -expected);
        assert_eq!(keys[1]["layerTime"], 500);
    }
}

#[test]
fn edited_primary_grade_and_following_vignette_export_as_one_native_lumetri_color() {
    let wire = document_with_effects(json!([
        {"id": 7, "enabled": true, "effect": {
            "type": "primaryGrade", "semanticVersion": 1, "temperature": 0.25, "tint": 0.1,
            "exposure": 1.0, "contrast": 0.25, "highlights": 0.0, "shadows": 0.0,
            "whites": 0.0, "blacks": 0.0, "saturation": 0.3, "vibrance": 0.0
        }},
        {"id": 8, "enabled": true, "effect": {"type": "vignette", "amount": 0.5}},
        {"id": 9, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 10}}
    ]));
    let (project, omissions) = export(wire);
    let effects = exported_effects(&project);
    // The grade and the vignette directly after it share one Lumetri Color,
    // in the FX order, before the blur.
    assert_eq!(effects.len(), 2);
    assert_eq!(
        grade(&effects[0]),
        [29.0, -10.0, 2.6, 90.0, 0.0, 0.0, 0.0, 0.0, 133.0, 0.0]
    );
    let PrEffectParams::LumetriGrade(PrLumetriGrade {
        vignette: Some(vignette),
        ..
    }) = effects[0].params
    else {
        panic!(
            "one Lumetri Color with grade and vignette: {:?}",
            effects[0].params
        )
    };
    // The fitted inverse of the import for amount 0.5 and FX's default
    // radius 0.75 and feather 0.35.
    assert_vignette(
        vignette,
        aftereffects_file::lumetri::vignette::from_fx([0.5, 0.75, 0.35]),
    );
    assert!(!matches!(
        effects[1].params,
        PrEffectParams::LumetriGrade(_) | PrEffectParams::LumetriVignette(_)
    ));
    assert!(effects[0].enabled);
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.kind == OmissionKind::Omitted),
        "{omissions:?}"
    );
    assert!(omitted(&omissions, "primaryGrade exports as a native Lumetri Color").is_some());
    assert!(omitted(
        &omissions,
        "vignette exports as a native Lumetri Color Vignette"
    )
    .is_some());
}

#[test]
fn edited_color_curves_export_as_native_lumetri_rgb_curves_and_reimport() {
    let curves = |master: Value, blue: Value| {
        json!({"id": 5, "enabled": true, "effect": {
            "type": "colorCurves", "semanticVersion": 1, "curves": {
                "master": master,
                "red": [{"x": 0.0, "y": 0.0}, {"x": 1.0, "y": 1.0}],
                "green": [{"x": 0.0, "y": 0.0}, {"x": 1.0, "y": 1.0}],
                "blue": blue
            }
        }})
    };
    let diagonal = json!([{"x": 0.0, "y": 0.0}, {"x": 1.0, "y": 1.0}]);
    // A two-point Blue line is exactly Premiere's spline: native, retained.
    let blue_line = json!([{"x": 0.0, "y": 0.2}, {"x": 1.0, "y": 0.8}]);
    let (project, omissions) = export(document_with_effects(json!([curves(
        diagonal.clone(),
        blue_line.clone()
    )])));
    let effects = exported_effects(&project);
    assert_eq!(effects.len(), 1, "{omissions:?}");
    let PrEffectParams::LumetriCurves(written) = &effects[0].params else {
        panic!("{:?}", effects[0]);
    };
    assert_eq!(written.curves[3], [[0.0, 0.2], [1.0, 0.8]]);
    let note = omitted(
        &omissions,
        "colorCurves exports as native Lumetri Color RGB Curves",
    )
    .unwrap_or_else(|| panic!("{omissions:?}"));
    assert_eq!(note.kind, OmissionKind::Approximated);
    let report = routing_report(document_with_effects(json!([curves(
        diagonal.clone(),
        blue_line.clone()
    )])));
    assert!(
        !report
            .losses
            .iter()
            .any(|loss| loss.omission.reason.contains("colorCurves")),
        "{:?}",
        report.losses
    );
    let reimported = imported(effects);
    let effect = &reimported["composition"]["layers"][0]["effects"][0]["effect"];
    assert_eq!(effect["type"], "colorCurves");
    assert_eq!(effect["curves"]["blue"], blue_line);
    // A bent Master is folded into the Red, Green and Blue curves (Luma
    // identity): a retained native approximation, never a routing loss.
    let bent = json!([{"x": 0.0, "y": 0.0}, {"x": 0.3, "y": 0.7}, {"x": 1.0, "y": 1.0}]);
    let report = routing_report(document_with_effects(json!([curves(
        bent.clone(),
        blue_line.clone()
    )])));
    assert!(
        !report
            .losses
            .iter()
            .any(|loss| loss.omission.reason.contains("colorCurves")),
        "{:?}",
        report.losses
    );
    let note = omitted(
        &report.diagnostics,
        "colorCurves exports as native Lumetri Color RGB Curves with the Master curve folded into the Red, Green and Blue curves",
    )
    .unwrap_or_else(|| panic!("{:?}", report.diagnostics));
    assert_eq!(note.kind, OmissionKind::Approximated);
    assert!(note.reason.contains("identity Luma curve"), "{note:?}");
    let (project, _) = export(document_with_effects(json!([curves(bent, blue_line)])));
    let effects = exported_effects(&project);
    let PrEffectParams::LumetriCurves(written) = &effects[0].params else {
        panic!("{:?}", effects[0]);
    };
    assert_eq!(written.curves[0], [[0.0, 0.0], [1.0, 1.0]]);
    // Each written channel passes channel(Master(x)) at its points: Red and
    // Green are the Master itself, Blue the line 0.2 + 0.6 * Master(x).
    let master = |x: f64| {
        if x <= 0.3 {
            x / 0.3 * 0.7
        } else {
            0.7 + (x - 0.3) / 0.7 * 0.3
        }
    };
    for (channel, outer) in [(1, (0.0, 1.0)), (2, (0.0, 1.0)), (3, (0.2, 0.8))] {
        let points = &written.curves[channel];
        assert!((3..=32).contains(&points.len()), "{points:?}");
        for [x, y] in points {
            let expected = outer.0 + (outer.1 - outer.0) * master(*x);
            assert!((y - expected).abs() < 1e-12, "{channel}: {x} {y}");
        }
    }
    // Reimport: an identity Master and the folded channel curves.
    let reimported = imported(effects);
    let effect = &reimported["composition"]["layers"][0]["effects"][0]["effect"];
    assert_eq!(effect["curves"]["master"], diagonal);
}

#[test]
fn sharp_color_curve_exports_through_added_points_and_reimports_within_one_code() {
    // A sharp three-point Red curve: Premiere's spline through FX's own
    // points would overshoot its corner by about 46 codes.
    let sharp = json!([{"x": 0.0, "y": 0.0}, {"x": 0.3, "y": 0.7}, {"x": 1.0, "y": 1.0}]);
    let diagonal = json!([{"x": 0.0, "y": 0.0}, {"x": 1.0, "y": 1.0}]);
    let document = document_with_effects(json!([{"id": 5, "enabled": true, "effect": {
        "type": "colorCurves", "semanticVersion": 1, "curves": {
            "master": diagonal, "red": sharp, "green": diagonal, "blue": diagonal
        }
    }}]));
    let report = routing_report(document.clone());
    assert!(
        !report
            .losses
            .iter()
            .any(|loss| loss.omission.reason.contains("colorCurves")),
        "{:?}",
        report.losses
    );
    let (project, omissions) = export(document);
    let note = omitted(
        &omissions,
        "colorCurves exports as native Lumetri Color RGB Curves (Luma, Red, Green, Blue); Premiere draws a natural cubic spline",
    )
    .unwrap_or_else(|| panic!("{omissions:?}"));
    assert_eq!(note.kind, OmissionKind::Approximated);
    assert!(
        note.reason
            .contains("points were added for rendering fidelity"),
        "{note:?}"
    );
    let effects = exported_effects(&project);
    let PrEffectParams::LumetriCurves(written) = &effects[0].params else {
        panic!("{:?}", effects[0]);
    };
    assert!(written.curves[1].len() > 3, "{:?}", written.curves[1]);
    assert_eq!(written.curves[2], [[0.0, 0.0], [1.0, 1.0]]);
    let polyline = |curve: &Value, x: f64| {
        let curve = curve.as_array().unwrap();
        let pair = curve
            .windows(2)
            .find(|pair| x <= pair[1]["x"].as_f64().unwrap())
            .unwrap();
        let [x0, y0, x1, y1] = [&pair[0]["x"], &pair[0]["y"], &pair[1]["x"], &pair[1]["y"]]
            .map(|value| value.as_f64().unwrap());
        y0 + (x - x0) / (x1 - x0) * (y1 - y0)
    };
    // Reimport: Premiere's spline through the written points, fitted again,
    // follows the original FX polyline within one 8-bit code.
    let reimported = imported(effects);
    let curves = &reimported["composition"]["layers"][0]["effects"][0]["effect"]["curves"];
    assert_eq!(curves["master"], diagonal);
    assert_eq!(curves["green"], diagonal);
    let deviation = (0..=255)
        .map(|code| {
            let x = f64::from(code) / 255.0;
            (polyline(&curves["red"], x) - polyline(&sharp, x)).abs() * 255.0
        })
        .fold(0.0, f64::max);
    assert!(deviation <= 1.0, "{deviation}: {}", curves["red"]);
}

#[test]
fn lumetri_luma_curve_reimports_as_master_and_reexports_folded_into_the_channels() {
    // A raised Luma midpoint (measured in Premiere 26.5.2).
    let raised: &[[f64; 2]] = &[[0.0, 0.0], [0.5, 0.75], [1.0, 1.0]];
    let xml = with_rgb_curves([raised, DIAGONAL, DIAGONAL, DIAGONAL]);
    let (wire, losses) = import_native(&xml);
    let import_note = omitted(&losses, "Lumetri RGB Curves map to colorCurves")
        .unwrap_or_else(|| panic!("{losses:?}"));
    assert_eq!(import_note.kind, OmissionKind::Approximated);
    assert!(
        import_note
            .reason
            .contains("the Luma curve becomes the FX Master curve")
            && import_note
                .reason
                .contains("so a Luma curve does not survive a round trip unchanged"),
        "{import_note:?}"
    );
    let effect = wire["composition"]["layers"][0]["effects"][1].clone();
    assert_eq!(effect["effect"]["type"], "colorCurves", "{wire}");
    let master = effect["effect"]["curves"]["master"].clone();
    assert!(master.as_array().unwrap().len() >= 3, "{master}");
    let report = routing_report(document_with_effects(json!([effect.clone()])));
    assert!(
        !report
            .losses
            .iter()
            .any(|loss| loss.omission.reason.contains("colorCurves")),
        "{:?}",
        report.losses
    );
    assert!(omitted(&report.diagnostics, "with the Master curve folded").is_some());
    let (project, _) = export(document_with_effects(json!([effect])));
    let effects = exported_effects(&project);
    let PrEffectParams::LumetriCurves(written) = &effects[0].params else {
        panic!("{:?}", effects[0]);
    };
    // Luma does not come back: it is identity, and Red, Green and Blue each
    // carry the imported Master.
    assert_eq!(written.curves[0], [[0.0, 0.0], [1.0, 1.0]]);
    assert_eq!(written.curves[1], written.curves[2]);
    assert_eq!(written.curves[1], written.curves[3]);
    assert_ne!(written.curves[1], raised);
    // The written channel spline still passes the saved midpoint within
    // two 8-bit codes (import fit plus export fold, one code each).
    let reimported = imported(effects);
    let curves = &reimported["composition"]["layers"][0]["effects"][0]["effect"]["curves"];
    assert_eq!(
        curves["master"],
        json!([{"x": 0.0, "y": 0.0}, {"x": 1.0, "y": 1.0}])
    );
    let red = curves["red"].as_array().unwrap();
    let at_half = red
        .windows(2)
        .find(|pair| pair[0]["x"].as_f64().unwrap() <= 0.5 && 0.5 <= pair[1]["x"].as_f64().unwrap())
        .map(|pair| {
            let [x0, y0, x1, y1] = [&pair[0]["x"], &pair[0]["y"], &pair[1]["x"], &pair[1]["y"]]
                .map(|value| value.as_f64().unwrap());
            y0 + (0.5 - x0) / (x1 - x0) * (y1 - y0)
        })
        .unwrap();
    assert!((at_half - 0.75).abs() <= 2.0 / 255.0, "{at_half}");
    assert_eq!(curves["green"], curves["red"]);
    assert_eq!(curves["blue"], curves["red"]);
}

/// The routing report of exporting `wire` at 30 fps.
fn routing_report(wire: Value) -> crate::export_loss::ExportLossReport {
    let document = EditableFxCompositionDocument::from_json_value(wire).unwrap();
    let mut collector = crate::export_loss::LossCollector::default();
    let lowered = crate::convert::lower_document(
        &document,
        &source_facts(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        FrameRate::Fps30,
        &mut collector,
    )
    .unwrap();
    collector.finish(lowered.project.is_some())
}

#[test]
fn written_lumetri_scale_approximations_are_diagnosed_but_are_not_routing_losses() {
    let grade = json!({"id": 7, "enabled": true, "effect": {
        "type": "primaryGrade", "semanticVersion": 1, "temperature": 0.25, "tint": 0.1,
        "exposure": 1.0, "contrast": 0.25, "highlights": 0.0, "shadows": 0.0,
        "whites": 0.0, "blacks": 0.0, "saturation": 0.3, "vibrance": 0.0
    }});
    let vignette = json!({"id": 8, "enabled": true, "effect": {"type": "vignette", "amount": 0.5}});
    let report = routing_report(document_with_effects(json!([grade, vignette])));
    assert!(report.has_native_content);
    // Only the declared scale approximations, which the linked After Effects
    // route would share: diagnosed, never a picture loss.
    assert!(
        !report
            .losses
            .iter()
            .any(|loss| loss.omission.reason.contains("Lumetri")),
        "{:?}",
        report.losses
    );
    for declared in [
        "primaryGrade exports as a native Lumetri Color",
        "vignette exports as a native Lumetri Color Vignette",
    ] {
        let note = omitted(&report.diagnostics, declared).unwrap();
        assert_eq!(note.kind, OmissionKind::Approximated);
    }
    // A clamp is a real loss of the edited value and still routes.
    let mut clamped = grade.clone();
    clamped["effect"]["exposure"] = json!(5.0);
    let report = routing_report(document_with_effects(json!([clamped])));
    let clamp = report
        .losses
        .iter()
        .find(|loss| {
            loss.omission
                .reason
                .contains("primaryGrade exposure beyond")
        })
        .unwrap_or_else(|| panic!("{:?}", report.losses));
    // Owned by the graded layer, so the hybrid seeds a linked scope from it.
    assert_ne!(clamp.source, crate::export_loss::ExportLossSource::Document);
    assert!(
        !report.losses.iter().any(|loss| loss
            .omission
            .reason
            .contains("exports as a native Lumetri Color")),
        "{:?}",
        report.losses
    );
}

/// The keyed parameters of effect `id` in `document`, as export gathers them.
fn lumetri_keys(
    document: &EditableFxCompositionDocument,
    id: u64,
) -> Vec<(
    &'static crate::schema::EffectParamSpec,
    &fx_schema::animator::AnimationGraphEntry,
)> {
    document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .filter_map(|entry| {
            let PropertyTarget::EffectProperty(target) = &entry.target else {
                return None;
            };
            (target.effect_id().value() == id)
                .then(|| (LUMETRI.bound_param(target.param_name()).unwrap(), entry))
        })
        .collect()
}

fn layer_effect_payload(wire: &Value, index: usize) -> LayerEffect {
    serde_json::from_value(wire["composition"]["layers"][0]["effects"][index]["effect"].clone())
        .unwrap()
}

#[test]
fn native_lumetri_export_inverts_import_scales_and_round_trips_the_pinned_grade() {
    // A nonzero Vignette Amount, so that the vignette imports too.
    let native = set_static(NATIVE, "882", "-2.5");
    let (effects, _) = native_effects(&native);
    let (wire, _) = import_native(&native);
    let document = EditableFxCompositionDocument::from_json_value(wire.clone()).unwrap();
    let id = wire["composition"]["layers"][0]["effects"][0]["id"]
        .as_u64()
        .unwrap();
    let mut notes = Vec::new();
    let exported = super::super::export_lumetri(
        true,
        &layer_effect_payload(&wire, 0),
        &lumetri_keys(&document, id),
        0,
        "layer 1",
        &mut notes,
    )
    .unwrap();
    // Import stores FX values to 1e-9 (Exposure 1 as 0.384615385); export
    // rounds saved values to 1e-6, so the saved values return exactly.
    assert_eq!(grade(&exported), PINNED_GRADE);
    assert_eq!(exported.animations.len(), 1);
    assert_eq!(exported.animations[0].param, &LUMETRI_SATURATION);
    assert_eq!(
        exported.animations[0]
            .keys
            .scalar()
            .unwrap()
            .iter()
            .map(|key| key.value)
            .collect::<Vec<_>>(),
        effects[0].animations[0]
            .keys
            .scalar()
            .unwrap()
            .iter()
            .map(|key| key.value)
            .collect::<Vec<_>>()
    );
    assert!(omitted(&notes, "exports as a native Lumetri Color").is_some());
    let vignette = super::super::export_lumetri(
        true,
        &layer_effect_payload(&wire, 1),
        &[],
        0,
        "layer 1",
        &mut notes,
    )
    .unwrap();
    assert_eq!(vignette.params, effects[1].params);
    assert!(vignette.enabled);
}

#[test]
fn native_lumetri_export_writes_edited_values_and_clamps_exposure() {
    let wire = document_with_effects(json!([
        {"id": 7, "enabled": false, "effect": {
            "type": "primaryGrade", "semanticVersion": 1, "temperature": 0.25, "tint": 0.1,
            "exposure": 5.0, "contrast": 1.0, "highlights": -0.5, "shadows": 0.25,
            "whites": 1.0, "blacks": -1.0, "saturation": -1.0, "vibrance": 0.75
        }},
        {"id": 8, "enabled": true, "effect": {"type": "vignette", "amount": 1.5, "feather": 0.2}}
    ]));
    let document = EditableFxCompositionDocument::from_json_value(wire.clone()).unwrap();
    let mut notes = Vec::new();
    let exported = super::super::export_lumetri(
        false,
        &layer_effect_payload(&wire, 0),
        &lumetri_keys(&document, 7),
        0,
        "layer 1",
        &mut notes,
    )
    .unwrap();
    assert!(!exported.enabled);
    // Inverse fitted scales; Exposure, Contrast, Saturation and Vibrance
    // clamp to their saved ranges.
    assert_eq!(
        grade(&exported),
        [29.0, -10.0, 7.0, 150.0, -120.0, 127.5, 54.0, -124.0, 0.0, 100.0]
    );
    for clamped in ["exposure", "contrast", "saturation", "vibrance"] {
        assert!(
            omitted(&notes, &format!("primaryGrade {clamped} beyond")).is_some(),
            "{clamped}: {notes:?}"
        );
    }
    let vignette = super::super::export_lumetri(
        true,
        &layer_effect_payload(&wire, 1),
        &[],
        0,
        "layer 1",
        &mut notes,
    )
    .unwrap();
    // The fitted Amount for FX 1.5 is below Lumetri's -5 and clamps; the
    // absent radius is FX's 0.75.
    let PrEffectParams::LumetriVignette(values) = vignette.params else {
        panic!("Lumetri vignette expected: {:?}", vignette.params)
    };
    let [_, midpoint, feather] = aftereffects_file::lumetri::vignette::from_fx([1.5, 0.75, 0.2]);
    assert_vignette(values, [-5.0, midpoint, feather]);
    assert!(notes.iter().any(
        |note| note.reason.contains("vignette Lumetri Vignette Amount")
            && note.reason.contains("clamped to -5")
    ));
    // Imported again, the written native values are the edited FX controls,
    // or the clamped ones (Exposure through Lumetri's ±7-stop limit).
    let reimported = imported(vec![exported]);
    let effect = &reimported["composition"]["layers"][0]["effects"][0]["effect"];
    assert_eq!(effect["temperature"], 0.25);
    assert_eq!(effect["tint"], 0.1);
    assert_eq!(effect["exposure"], 2.692307692);
    assert_eq!(effect["contrast"], 0.416666667);
    assert_eq!(effect["saturation"], -0.909090909);
    assert_eq!(effect["vibrance"], 0.598802395);
}

#[test]
fn static_vibrance_and_temperature_tint_export_as_lumetri_color() {
    let wire = document_with_effects(json!([
        {"id": 4, "enabled": true, "effect": {"type": "vibrance", "vibrance": 50.0, "saturation": -20.0}},
        {"id": 5, "enabled": true, "effect": {"type": "temperatureTint", "temperature": 50.0, "tint": -30.0}}
    ]));
    let (project, omissions) = export(wire);
    let effects = exported_effects(&project);
    assert_eq!(effects.len(), 2, "{omissions:?}");
    // Saved Lumetri values: [temperature, tint, exposure, contrast, highlights,
    // shadows, whites, blacks, saturation, vibrance] by the fitted scales.
    let saved: Vec<_> = effects.iter().map(grade).collect();
    assert_eq!(saved[0][9], 83.5, "vibrance 0.5 x167");
    assert_eq!(saved[0][8], 78.0, "saturation 100 + 110 x -0.2");
    assert!((saved[1][0] - 0.31 * 116.0).abs() < 1e-6, "{:?}", saved[1]);
    assert!((saved[1][1] - 18.6).abs() < 1e-6, "{:?}", saved[1]);
    for declared in [
        "vibrance exports as native Lumetri Color Creative Vibrance",
        "temperatureTint exports as native Lumetri Color Temperature/Tint",
    ] {
        assert!(
            omitted(&omissions, declared).is_some(),
            "{declared}: {omissions:?}"
        );
    }
    // Declared approximations do not route the picture elsewhere.
    let report = routing_report(document_with_effects(json!([
        {"id": 4, "enabled": true, "effect": {"type": "vibrance", "vibrance": 50.0, "saturation": -20.0}}
    ])));
    assert!(report.losses.is_empty(), "{:?}", report.losses);
}

#[test]
fn static_exposure_exports_as_proc_amp_and_gamma_correction_and_reimports() {
    use crate::schema::PrProcAmp;
    let proc_amp = |effect: &PrEffect| match effect.params {
        PrEffectParams::ProcAmp(proc_amp) => proc_amp,
        _ => panic!("expected ProcAmp: {effect:?}"),
    };
    // One stop and an offset: one ProcAmp, exact.
    let (project, omissions) = export(document_with_effects(json!([
        {"id": 3, "enabled": true, "effect": {"type": "exposure", "exposure": 1.0, "offset": 0.1, "gammaCorrection": 1.0}}
    ])));
    let effects = exported_effects(&project);
    assert_eq!(effects.len(), 1, "{omissions:?}");
    assert_eq!(
        proc_amp(&effects[0]),
        PrProcAmp {
            brightness: 10.0,
            contrast: 200.0,
            hue: 0.0,
            saturation: 200.0
        }
    );
    assert!(omitted(&omissions, "exposure exports as 1 native ProcAmp").is_some());
    let reimported = imported(effects);
    let effect = &reimported["composition"]["layers"][0]["effects"][0]["effect"];
    assert_eq!(effect["type"], "exposure", "{reimported}");
    assert_eq!(effect["exposure"], 1.0);
    assert_eq!(effect["offset"], 0.1);
    // 2.5 stops need three ProcAmps (Contrast stops at 200); gamma 2.2 is
    // written as the nearest whole Gamma Correction, 5 (FX gamma 2).
    let (project, omissions) = export(document_with_effects(json!([
        {"id": 3, "enabled": true, "effect": {"type": "exposure", "exposure": 2.5, "offset": -0.05, "gammaCorrection": 2.2}}
    ])));
    let effects = exported_effects(&project);
    assert_eq!(effects.len(), 4, "{omissions:?}");
    let gain = 100.0 * (2.5_f64 / 3.0).exp2();
    for (index, effect) in effects[..3].iter().enumerate() {
        let written = proc_amp(effect);
        assert!((written.contrast - gain).abs() < 1e-9 && written.saturation == written.contrast);
        assert_eq!(written.brightness, if index == 2 { -5.0 } else { 0.0 });
    }
    assert_eq!(effects[3].params, PrEffectParams::GammaCorrection(5.0));
    assert!(
        omitted(&omissions, "FX gamma 2.2 written as 2").is_some(),
        "{omissions:?}"
    );
    // ProcAmp and Gamma Correction are native and exact: no routing loss.
    let report = routing_report(document_with_effects(json!([
        {"id": 3, "enabled": true, "effect": {"type": "exposure", "exposure": 1.0, "offset": 0.1}}
    ])));
    assert!(report.losses.is_empty(), "{:?}", report.losses);
    let reimported = imported(effects);
    let layers = &reimported["composition"]["layers"][0]["effects"];
    assert_eq!(layers.as_array().unwrap().len(), 4, "{reimported}");
    assert_eq!(layers[3]["effect"]["gammaCorrection"], 2.0);
}

#[test]
fn static_hue_saturation_exports_as_proc_amp_and_reimports() {
    let (project, omissions) = export(document_with_effects(json!([
        {"id": 3, "enabled": true, "effect": {"type": "hueSaturation", "hue": 30.0, "saturation": -40.0, "lightness": 10.0}}
    ])));
    let effects = exported_effects(&project);
    assert_eq!(effects.len(), 1, "{omissions:?}");
    assert_eq!(
        effects[0].params,
        PrEffectParams::ProcAmp(crate::schema::PrProcAmp {
            brightness: 10.0,
            contrast: 100.0,
            hue: 30.0,
            saturation: 60.0
        })
    );
    assert!(omitted(
        &omissions,
        "hueSaturation exports as ProcAmp at Contrast 100"
    )
    .is_some());
    // The written ProcAmp reimports exactly, as the channelMixer of its
    // affine transform, and that mixer exports back as the same ProcAmp.
    let reimported = imported(effects.clone());
    let record = &reimported["composition"]["layers"][0]["effects"][0];
    assert_eq!(record["effect"]["type"], "channelMixer", "{reimported}");
    let (project, omissions) = export(document_with_effects(json!([record])));
    let again = exported_effects(&project);
    assert_eq!(again.len(), 1, "{omissions:?}");
    let PrEffectParams::ProcAmp(round_trip) = again[0].params else {
        panic!("{:?}", again[0]);
    };
    for (a, b) in round_trip.values().iter().zip([10.0, 100.0, 30.0, 60.0]) {
        assert!((a - b).abs() < 1e-6, "{round_trip:?}");
    }
    // Colorize has no ProcAmp form.
    let (_, omissions) = export(document_with_effects(json!([
        {"id": 3, "enabled": true, "effect": {"type": "hueSaturation", "hue": 0.0, "saturation": 0.0, "lightness": 0.0, "colorize": true, "colorizeHue": 120.0, "colorizeSaturation": 50.0}}
    ])));
    assert!(
        omitted(&omissions, "Colorize has no ProcAmp equivalent").is_some(),
        "{omissions:?}"
    );
}

#[test]
fn hue_curves_and_tonal_color_export_as_lumetri_sections_and_reimport() {
    let tonal = json!({"type": "tonalColor", "semanticVersion": 1,
        "globalRed": 0.0, "globalGreen": 0.0, "globalBlue": 0.0,
        "shadowsRed": 0.08, "shadowsGreen": 0.0, "shadowsBlue": -0.08,
        "midtonesRed": 0.0, "midtonesGreen": 0.03, "midtonesBlue": 0.0,
        "highlightsRed": -0.06, "highlightsGreen": 0.0, "highlightsBlue": 0.06});
    let wire = document_with_effects(json!([
        {"id": 3, "enabled": true, "effect": {"type": "hueCurves", "semanticVersion": 1,
            "hueVsSaturation": [{"x": 0.0, "y": 0.5}, {"x": 0.3, "y": 0.2}, {"x": 0.6, "y": 0.5}, {"x": 1.0, "y": 0.5}],
            "luminanceVsSaturation": [{"x": 0.0, "y": 0.5}, {"x": 1.0, "y": 0.3}]}},
        {"id": 4, "enabled": true, "effect": tonal}
    ]));
    let (project, omissions) = export(wire.clone());
    let effects = exported_effects(&project);
    assert_eq!(effects.len(), 2, "{omissions:?}");
    let PrEffectParams::LumetriHueCurves(curves) = &effects[0].params else {
        panic!("{:?}", effects[0]);
    };
    assert!(!curves.curves[0].is_empty() && !curves.curves[3].is_empty());
    assert!(
        curves.curves[1].is_empty() && curves.curves[2].is_empty() && curves.curves[4].is_empty()
    );
    let PrEffectParams::LumetriWheels(wheels) = &effects[1].params else {
        panic!("{:?}", effects[1]);
    };
    assert!(!wheels.is_neutral());
    for declared in [
        "hueCurves exports as native Lumetri Color Hue Saturation Curves",
        "tonalColor exports as Lumetri Color Wheels",
    ] {
        assert!(
            omitted(&omissions, declared).is_some(),
            "{declared}: {omissions:?}"
        );
    }
    assert!(routing_report(wire).losses.is_empty());
    let reimported = imported(effects);
    let layer = &reimported["composition"]["layers"][0]["effects"];
    assert_eq!(layer[0]["effect"]["type"], "hueCurves", "{reimported}");
    let points = layer[0]["effect"]["hueVsSaturation"].as_array().unwrap();
    let at = |x: f64| {
        points
            .windows(2)
            .find(|pair| pair[0]["x"].as_f64().unwrap() <= x && x <= pair[1]["x"].as_f64().unwrap())
            .map(|pair| {
                let [x0, y0, x1, y1] = [&pair[0]["x"], &pair[0]["y"], &pair[1]["x"], &pair[1]["y"]]
                    .map(|value| value.as_f64().unwrap());
                y0 + (x - x0) / (x1 - x0) * (y1 - y0)
            })
            .unwrap()
    };
    assert!(
        (at(0.3) - 0.2).abs() < 0.03 && (at(0.8) - 0.5).abs() < 0.03,
        "{points:?}"
    );
    // The wheels reimport as their per-channel colorCurves.
    assert_eq!(layer[1]["effect"]["type"], "colorCurves", "{reimported}");
}

#[test]
fn canonical_color_effects_without_native_encoding_are_reported_not_exported() {
    let wire = document_with_effects(json!([
        {"id": 3, "enabled": true, "effect": {"type": "lookTransform", "semanticVersion": 1, "transform": null, "mix": 1.0}},
        {"id": 4, "enabled": true, "effect": {"type": "channelMixer", "semanticVersion": 1,
            "red": {"red": 0.5, "green": 0.5, "blue": 0.0, "constant": 0.0},
            "green": {"red": 0.0, "green": 1.0, "blue": 0.0, "constant": 0.0},
            "blue": {"red": 0.0, "green": 0.0, "blue": 1.0, "constant": 0.0},
            "monochrome": false}},
        {"id": 9, "enabled": true, "effect": {"type": "gaussianBlur", "blurriness": 10}}
    ]));
    let (project, omissions) = export(wire);
    assert_eq!(exported_effects(&project).len(), 1);
    let mixer = omitted(&omissions, "channelMixer effect 4 was not exported").unwrap();
    assert!(
        mixer
            .reason
            .contains("no native Premiere effect mixes channels"),
        "{mixer:?}"
    );
    // A Look without a LUT has no Lumetri Creative Look form.
    let look = omitted(&omissions, "lookTransform effect 3 was not exported").unwrap();
    assert!(
        look.reason.contains(
            "a lookTransform without a lut3d transform has no Lumetri Creative Look form"
        ),
        "{look:?}"
    );
}

fn lut_look(id: u64, input: &str, mix: f64) -> Value {
    json!({"id": id, "enabled": true, "effect": {
        "type": "lookTransform",
        "semanticVersion": 1,
        "transform": {
            "type": "lut3d",
            "assetId": "warm-lut",
            "inputEncoding": input,
            "outputEncoding": "jerboa:sdr-rec709-display:v1",
            "interpolation": "trilinear",
        },
        "mix": mix,
    }})
}

#[test]
fn a_rec709_lut_look_transform_exports_as_a_lumetri_creative_look_at_its_mix() {
    let (project, omissions) = export(document_with_effects(json!([
        lut_look(3, "jerboa:sdr-rec709-display:v1", 0.3),
        lut_look(4, "camera:sony-slog3-sgamut3-cine:v1", 1.0),
    ])));
    let effects = exported_effects(&project);
    assert_eq!(effects.len(), 1, "{omissions:?}");
    let PrEffectParams::LumetriLook(look) = &effects[0].params else {
        panic!("{effects:?}");
    };
    assert_eq!(look.intensity, 30.0);
    // The package file and its absolute path are bound at publication.
    assert_eq!(
        look.lut.file,
        crate::schema::LutFile::Packaged(fx_schema::AssetId::from_trusted("warm-lut"))
    );
    let approximation = omissions
        .iter()
        .find(|omission| {
            omission
                .reason
                .contains("Lumetri Color > Creative > Look (Intensity = mix*100)")
        })
        .unwrap_or_else(|| panic!("{omissions:?}"));
    assert_eq!(approximation.kind, OmissionKind::Approximated);
    let camera = omitted(&omissions, "lookTransform effect 4 was not exported").unwrap();
    assert!(
        camera.reason.contains(
            "a lut3d camera:sony-slog3-sgamut3-cine:v1 -> jerboa:sdr-rec709-display:v1 has no native form"
        ),
        "{camera:?}"
    );
}

#[test]
fn human_lumetri_look_and_input_lut_diagnostics_name_the_premiere_controls() {
    // A non-None Creative Look popup and Basic Correction Input LUT popup.
    let mut xml = edit(NATIVE, "806", "<StartKeyframe>0,0,", "<StartKeyframe>0,5,");
    xml = edit(&xml, "778", "<StartKeyframe>0,0,", "<StartKeyframe>0,3,");
    let (effects, omissions) = native_effects(&xml);
    assert_eq!(effects.len(), 1, "{omissions:?}");
    for (section, control, id) in [
        ("Creative Look", "Lumetri Color > Creative > Look", "25"),
        (
            "Basic Correction Input LUT",
            "Lumetri Color > Basic Correction > Input LUT",
            "5",
        ),
    ] {
        let note = omitted(
            &omissions,
            &format!("Lumetri {section} omitted: saved ParameterID {id} differ"),
        )
        .unwrap_or_else(|| panic!("{section}: {omissions:?}"));
        assert_eq!(note.kind, OmissionKind::Omitted);
        assert!(note.reason.contains(control), "{note:?}");
    }
}

/// `color_lumetri_basic.prproj` (Premiere 26.5.2): one Lumetri Color
/// authored by `color_lumetri_basic.author.jsx` with Temperature 30, Tint -15,
/// Saturation 130, Exposure 0.5, Contrast 30, Highlights -25, Shadows 25,
/// Whites 15, Blacks -15, every other control at its default.
const COLOR_LUMETRI_BASIC: &str = "tests/fixtures/color_lumetri_basic.prproj";
/// Its SHA-256 as recorded in `color_fixtures.provenance.json`.
const COLOR_LUMETRI_BASIC_SHA256: &str =
    "8b78978e027e9b1af0df8a5c06009edd7a8b9bce0c4a6948b94a8d48195ff4a1";
const COLOR_LUMETRI_BASIC_SEQUENCE: &str = "5c919df6-dea9-4620-9eb3-611b6fc20602";
/// Its saved grade in primaryGrade order (Vibrance 0).
const COLOR_LUMETRI_BASIC_GRADE: [f64; 10] =
    [30.0, -15.0, 0.5, 30.0, -25.0, 25.0, 15.0, -15.0, 130.0, 0.0];

/// The editable effects of the one Lumetri clip of `color_lumetri_basic`,
/// imported fresh through the project reader, and the import's reports.
fn color_lumetri_basic_effects() -> (Value, Vec<Omission>) {
    use sha2::{Digest, Sha256};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(COLOR_LUMETRI_BASIC);
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap())),
        COLOR_LUMETRI_BASIC_SHA256
    );
    let (project, mut omissions) =
        PrProjectFile::load_selected(&path, Some(COLOR_LUMETRI_BASIC_SEQUENCE)).unwrap();
    let (wire, losses) =
        imported_with_omissions(project.single_sequence().unwrap(), &project.media);
    omissions.extend(losses);
    let layers = wire["composition"]["layers"].as_array().unwrap();
    let mut graded = layers.iter().filter(|layer| {
        layer["effects"]
            .as_array()
            .is_some_and(|effects| !effects.is_empty())
    });
    let effects = graded.next().unwrap()["effects"].clone();
    assert!(graded.next().is_none());
    assert!(wire["composition"]["dynamics"]["entries"]
        .as_array()
        .is_none_or(Vec::is_empty));
    (effects, omissions)
}

fn assert_close(actual: &Value, expected: f64, name: &str) {
    let actual = actual
        .as_f64()
        .unwrap_or_else(|| panic!("{name}: {actual}"));
    assert!(
        (actual - expected).abs() < 1e-9,
        "{name}: {actual} != {expected}"
    );
}

#[test]
fn native_color_lumetri_basic_imports_its_authored_grade_by_declared_scales() {
    let (effects, omissions) = color_lumetri_basic_effects();
    let effects = effects.as_array().unwrap();
    // The Vignette section is saved at its defaults: no FX vignette.
    assert_eq!(effects.len(), 1);
    let grade = &effects[0]["effect"];
    assert_eq!(grade["type"], "primaryGrade");
    assert_eq!(grade["semanticVersion"], 1);
    assert_eq!(effects[0]["enabled"], true);
    // The fitted scales: Temperature/116, -Tint/100, Exposure/2.6,
    // Contrast/360, Highlights/240, Shadows/510, Whites/54, Blacks/124,
    // (Saturation-100)/110, Vibrance/167.
    for (name, expected) in [
        ("temperature", 30.0 / 116.0),
        ("tint", 0.15),
        ("exposure", 0.5 / 2.6),
        ("contrast", 30.0 / 360.0),
        ("highlights", -25.0 / 240.0),
        ("shadows", 25.0 / 510.0),
        ("whites", 15.0 / 54.0),
        ("blacks", -15.0 / 124.0),
        ("saturation", 30.0 / 110.0),
        ("vibrance", 0.0),
    ] {
        assert_close(&grade[name], expected, name);
    }
    // Only the declared approximations on the Lumetri: no unmapped section
    // differs. (The sequence's colour-management reports are not Lumetri's.)
    assert!(
        omissions
            .iter()
            .filter(|omission| omission.record.starts_with("VideoFilterComponent"))
            .all(|omission| omission.kind == OmissionKind::Approximated),
        "{omissions:?}"
    );
    assert!(omitted(&omissions, "one editable primaryGrade").is_some());
}

#[test]
fn native_color_lumetri_basic_grade_exports_back_to_its_saved_values_and_reimports() {
    let (effects, _) = color_lumetri_basic_effects();
    let (project, omissions) = export(document_with_effects(effects.clone()));
    let exported = exported_effects(&project);
    assert_eq!(exported.len(), 1);
    // Exactly the saved values, Exposure 0.5 included (FX 0.192307692).
    assert_eq!(grade(&exported[0]), COLOR_LUMETRI_BASIC_GRADE);
    assert!(
        !omissions
            .iter()
            .any(|omission| omission.kind == OmissionKind::Omitted),
        "{omissions:?}"
    );
    // Imported again (model level; the CLI test rereads the written XML),
    // the grade is the imported one.
    let reimported = imported(exported);
    let again = reimported["composition"]["layers"][0]["effects"]
        .as_array()
        .unwrap();
    assert_eq!(again.len(), 1);
    for (again, effect) in again.iter().zip(effects.as_array().unwrap()) {
        assert_eq!(again["effect"]["type"], effect["effect"]["type"]);
        for (name, value) in effect["effect"].as_object().unwrap() {
            if let Some(value) = value.as_f64() {
                assert_close(&again["effect"][name], value, name);
            }
        }
    }
}

/// The written Lumetri vignette equals `expected` up to the writer's rounding.
fn assert_vignette(written: [f64; 3], expected: [f64; 3]) {
    for (got, want) in written.iter().zip(expected) {
        assert!((got - want).abs() < 1e-5, "{written:?} != {expected:?}");
    }
}
