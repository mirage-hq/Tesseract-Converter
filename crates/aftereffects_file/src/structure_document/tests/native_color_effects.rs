//! Fresh imports of the natively authored AE 26.5 colour fixtures
//! (`tests/fixtures/color_ae_*.aep`, provenance in
//! `color_ae_fixtures.provenance.json`). Each asserts the editable FX values
//! produced from the authored native controls; render fidelity is not claimed.
use super::*;

/// Import composition 1 of a pinned fixture and return its JSON document.
fn import(bytes: &[u8], sha256: &str) -> (Value, StructuralConversion) {
    assert_eq!(format!("{:x}", Sha256::digest(bytes)), sha256);
    assert_sanitized_footage(bytes);
    let project = read_project(bytes).unwrap();
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    (converted.document.to_json_value().unwrap(), converted)
}

/// The chart footage alias keeps its `media.png` name under the same-length
/// `/fixture-source/` path recorded in the provenance `path_sanitization`;
/// no host path remains in the fixture.
fn assert_sanitized_footage(bytes: &[u8]) {
    assert!(!bytes.windows(7).any(|window| window == b"/Users/"));
    let start = bytes
        .windows(19)
        .position(|window| window == br#"{"ascendcount_base""#)
        .expect("footage alias");
    let end = start
        + bytes[start..]
            .iter()
            .position(|byte| *byte == b'}')
            .expect("alias JSON end");
    let alias = crate::alias::decode(&bytes[start..=end]).unwrap();
    let path = alias.fullpath.unwrap();
    assert!(
        path.starts_with("/fixture-source/x") && path.ends_with("x/media.png"),
        "{path}"
    );
}

/// Every layer (recursively) of the imported composition.
fn layers(value: &Value) -> Vec<&Value> {
    fn walk<'a>(value: &'a Value, found: &mut Vec<&'a Value>) {
        for layer in value["layers"].as_array().into_iter().flatten() {
            found.push(layer);
            walk(layer, found);
        }
    }
    let mut found = Vec::new();
    walk(&value["composition"], &mut found);
    found
}

/// The single imported effect record of `kind` and the layer hosting it.
fn effect<'a>(value: &'a Value, kind: &str) -> (&'a Value, &'a Value) {
    let found: Vec<_> = layers(value)
        .into_iter()
        .flat_map(|layer| {
            layer["effects"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|record| record["effect"]["type"] == kind)
                .map(move |record| (layer, record))
        })
        .collect();
    assert_eq!(found.len(), 1, "one {kind} effect expected in {value}");
    found[0]
}

fn assert_near(actual: &Value, expected: f64, what: &str) {
    let actual = actual
        .as_f64()
        .unwrap_or_else(|| panic!("{what}: {actual}"));
    assert!(
        (actual - expected).abs() < 1e-9,
        "{what}: {actual} != {expected}"
    );
}

fn assert_fields(payload: &Value, expected: &[(&str, f64)]) {
    for (path, value) in expected {
        let mut slot = payload;
        for part in path.split('.') {
            slot = &slot[part];
        }
        assert_near(slot, *value, path);
    }
}

fn messages(converted: &StructuralConversion) -> Vec<&str> {
    converted
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect()
}

#[test]
fn native_lumetri_basic_fixture_imports_editable_primary_grade() {
    let (value, converted) = import(
        include_bytes!("../../../tests/fixtures/color_ae_lumetri_basic.aep"),
        "2bbdeffc4a93cc0d627bd8b94ce821548db227d12bdad4f1cfb158844dcd4d70",
    );
    let (_, record) = effect(&value, "primaryGrade");
    assert_eq!(record["enabled"], true);
    assert!(record.get("compositingOptions").is_none());
    // Authored Temp 30, Tint -15, Sat 130, Exposure 0.5, Contrast 30,
    // Highlights -25, Shadows 25, Whites 15, Blacks -15 (provenance readback).
    assert_fields(
        &record["effect"],
        &[
            // Fitted scales (`effects::color_grades::LUMETRI_APPROXIMATION`).
            ("temperature", 30.0 / 116.0),
            ("tint", 15.0 / 100.0),
            ("exposure", 0.5 / 2.6),
            ("contrast", 30.0 / 360.0),
            ("highlights", -25.0 / 240.0),
            ("shadows", 25.0 / 510.0),
            ("whites", 15.0 / 54.0),
            ("blacks", -15.0 / 124.0),
            ("saturation", 30.0 / 110.0),
            ("vibrance", 0.0),
        ],
    );
    let messages = messages(&converted);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("Effect ADBE Lumetri: Lumetri Basic/Creative scalars approximated")),
        "{messages:#?}"
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("ADBE Lumetri") && m.contains("omitted, owner")),
        "{messages:#?}"
    );
}

/// Piecewise-linear evaluation of an imported FX curve at eight-bit `code`.
fn curve_code(points: &Value, code: u8) -> f64 {
    let points: Vec<(f64, f64)> = points
        .as_array()
        .unwrap()
        .iter()
        .map(|point| (point["x"].as_f64().unwrap(), point["y"].as_f64().unwrap()))
        .collect();
    let x = f64::from(code) / 255.0;
    let pair = points
        .windows(2)
        .find(|pair| x <= pair[1].0)
        .expect("curve covers 0..=1");
    let fraction = (x - pair[0].0) / (pair[1].0 - pair[0].0);
    (pair[0].1 + fraction * (pair[1].1 - pair[0].1)) * 255.0
}

#[test]
fn native_color_balance_fixture_imports_editable_per_channel_curves() {
    let (value, converted) = import(
        include_bytes!("../../../tests/fixtures/color_ae_color_balance.aep"),
        "dd45bbf3059eb01ca84ec92d6aa9f643fc86c2996ad73b8f8c165ad33f4e3959",
    );
    let (_, record) = effect(&value, "colorCurves");
    let curves = &record["effect"]["curves"];
    assert_eq!(
        curves["master"],
        serde_json::json!([{"x":0.0,"y":0.0},{"x":1.0,"y":1.0}])
    );
    // Authored Shadow Blue 40, Midtone Red 30, Highlight Green -25, Preserve
    // Luminosity on. AE 26.5 renders this look (8 bpc, in-house grey ramp,
    // lane G `cb_mixed` probe) as these per-channel codes at the inputs below.
    let inputs = [1, 4, 8, 16, 32, 64, 96, 128, 160, 192, 224, 250];
    let after_effects: [(&str, [u8; 12]); 3] = [
        ("red", [0, 0, 0, 0, 0, 39, 85, 127, 169, 209, 248, 255]),
        ("green", [0, 0, 0, 0, 0, 22, 57, 94, 133, 173, 214, 248]),
        ("blue", [1, 3, 6, 13, 28, 62, 96, 133, 170, 209, 247, 255]),
    ];
    for (channel, expected) in after_effects {
        for (input, native) in inputs.iter().zip(expected) {
            let imported = curve_code(&curves[channel], *input);
            assert!(
                (imported - f64::from(native)).abs() <= 2.0,
                "{channel} at {input}: FX {imported}, AE {native}"
            );
        }
    }
    let messages = messages(&converted);
    assert!(
        messages.iter().any(|m| m.contains(
            "Effect ADBE Color Balance 2: Color Balance imported as editable per-channel FX colorCurves"
        )),
        "the approximation is stated: {messages:#?}"
    );
    assert!(
        !messages
            .iter()
            .any(|m| m.contains("Preserve Luminosity unreadable")),
        "{messages:#?}"
    );
}

#[test]
fn native_channel_mixer_fixture_imports_editable_channel_mixer() {
    let (value, _) = import(
        include_bytes!("../../../tests/fixtures/color_ae_channel_mixer.aep"),
        "e7f85e5b1bb8c7cfb831a904ef48ba47472f4a35c9c829fad5ee5cae608c4bd6",
    );
    let (_, record) = effect(&value, "channelMixer");
    // Authored Red-Red 70, Red-Green 30, Blue-Const 10; the rest at defaults.
    assert_fields(
        &record["effect"],
        &[
            ("red.red", 0.7),
            ("red.green", 0.3),
            ("red.blue", 0.0),
            ("red.constant", 0.0),
            ("green.red", 0.0),
            ("green.green", 1.0),
            ("green.blue", 0.0),
            ("green.constant", 0.0),
            ("blue.red", 0.0),
            ("blue.green", 0.0),
            ("blue.blue", 1.0),
            ("blue.constant", 0.1),
        ],
    );
    assert_eq!(record["effect"]["monochrome"], false);
}

#[test]
fn native_selective_color_fixture_imports_editable_selective_color() {
    let (value, converted) = import(
        include_bytes!("../../../tests/fixtures/color_ae_selective_color.aep"),
        "739829f4e4d07e7c1ac328b9e66210d9da4ea3a47de314591615878b67026bbd",
    );
    let (_, record) = effect(&value, "selectiveColor");
    let payload = &record["effect"];
    assert_eq!(payload["method"], "relative");
    // Authored Reds Cyan -60 / Magenta 20 and Blues Yellow -50.
    let authored = [
        ("reds.cyan", -0.6),
        ("reds.magenta", 0.2),
        ("blues.yellow", -0.5),
    ];
    assert_fields(payload, &authored);
    for range in [
        "reds", "yellows", "greens", "cyans", "blues", "magentas", "whites", "neutrals", "blacks",
    ] {
        for ink in ["cyan", "magenta", "yellow", "black"] {
            let path = format!("{range}.{ink}");
            if !authored.iter().any(|(authored, _)| *authored == path) {
                assert_near(&payload[range][ink], 0.0, &path);
            }
        }
    }
    // Scripted authoring sets the Details groups directly, so the visible
    // sliders (mirroring the selected Reds range) stay 0; the Details groups
    // are authoritative and the disagreement is diagnosed, never guessed.
    let messages = messages(&converted);
    for (slider, group) in [
        ("ADBE SelectiveColor-0003", "reds.cyan"),
        ("ADBE SelectiveColor-0004", "reds.magenta"),
    ] {
        assert!(
            messages.iter().any(|m| m.contains(slider)
                && m.contains(group)
                && m.contains("Details group value used")),
            "{messages:#?}"
        );
    }
}

#[test]
fn native_effect_compositing_options_fixture_imports_opacity_and_drops_its_mode_none_mask_reference()
 {
    let (value, converted) = import(
        include_bytes!("../../../tests/fixtures/color_ae_effect_compositing_options.aep"),
        "e2b36293bf74c31ddd0164cc90d759f4f455c9426705bbbcdfeb670062a3b8ba",
    );
    // Levels Gamma 0.5 with Effect Opacity 50 and Mask Reference 1 -> Mask 1
    // in mode None. After Effects 26.5 renders that reference as no
    // restriction (the fixture's reference render darkens both halves; an
    // in-house AE probe pinned it: a mode-None reference leaves the effect on
    // the whole layer, a mode-Add one restricts it), so only the opacity
    // imports and the mask stays an inert FX mask.
    let (owner, record) = effect(&value, "levels");
    let options = &record["compositingOptions"];
    assert_near(&options["effectOpacity"], 0.5, "effectOpacity");
    assert!(
        options["maskReferences"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "{options}"
    );
    let masks = owner["masks"].as_array().unwrap();
    assert_eq!(masks.len(), 1, "{owner}");
    assert_eq!(masks[0]["mode"], "none");
    let messages = messages(&converted);
    assert!(
        messages.iter().any(|m| m.contains(
            "is a mask in mode None, which After Effects renders as no restriction (measured, AE 26.5): reference omitted"
        )),
        "{messages:#?}"
    );
}
