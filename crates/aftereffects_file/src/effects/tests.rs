//! Pinned, independently Adobe-authored projects and Adobe ExtendScript readbacks.
//! Structural checks do not assert Adobe export acceptance or visual fidelity.
use serde_json::Value;

use super::native;
use crate::{
    structure::{ItemKind, StructuralProject, read_project},
    structure_document::to_structural_fx_document,
};

// Nondefault native feature contracts may expose unfinished converter behavior.
mod coverage;
mod keylight;
mod native_controls;
mod warp;

const CATALOG: &[u8] = include_bytes!("../../tests/fixtures/effects/catalog.aep");
const ANIMATED: &[u8] = include_bytes!("../../tests/fixtures/effects/animated_catalog.aep");
const VIGNETTE_ISOLATED: &[u8] =
    include_bytes!("../../tests/fixtures/effects/vignette_isolated.aep");
const VIGNETTE_STATIC_COVERAGE: &[u8] =
    include_bytes!("../../tests/fixtures/effects_coverage/vignette_static_only.aep");
const VIGNETTE_ANIMATED_COVERAGE: &[u8] =
    include_bytes!("../../tests/fixtures/effects_coverage/vignette_animated_only.aep");
const STATIC_RECEIPT: &str = include_str!("../../tests/fixtures/effects/catalog-receipt.json");
const ANIMATED_RECEIPT: &str = include_str!("../../tests/fixtures/effects/animated-receipt.json");
const CATALOG_TARGET_IDS: [u32; 32] = [
    1, 16, 30, 44, 58, 72, 86, 100, 114, 128, 142, 156, 170, 184, 198, 212, 226, 240, 254, 268,
    282, 296, 310, 324, 338, 352, 366, 380, 394, 408, 422, 436,
];

fn cases(receipt: &str) -> Vec<Value> {
    serde_json::from_str::<Value>(receipt).expect("Adobe readback JSON")["effects"]
        .as_array()
        .expect("Adobe effect cases")
        .clone()
}

fn layer<'a>(project: &'a StructuralProject, case: &Value) -> &'a crate::structure::Layer {
    let id = case["compositionId"]
        .as_u64()
        .expect("pinned composition ID") as u32;
    let ItemKind::Composition(comp) = &project.item(id).expect("pinned composition").kind else {
        panic!("{id} is not a composition")
    };
    assert_eq!(
        (comp.width, comp.height),
        (320, 180),
        "source composition {id}"
    );
    comp.layers.first().expect("Adobe effect layer")
}

fn components(value: &Value) -> Option<Vec<f64>> {
    if let Some(number) = value.as_f64() {
        return Some(vec![number]);
    }
    value.as_array()?.iter().map(Value::as_f64).collect()
}

fn chunk_match_name(chunk: &crate::rifx::Chunk) -> Option<&str> {
    let bytes = chunk.data_payload()?;
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end]).ok()
}

fn named_group_mut<'a>(
    chunks: &'a mut [crate::rifx::Chunk],
    target: &str,
) -> Option<&'a mut Vec<crate::rifx::Chunk>> {
    let group_index = chunks.iter().enumerate().find_map(|(index, chunk)| {
        (chunk.id() == *b"tdmn" && chunk_match_name(chunk) == Some(target)).then(|| {
            let end = chunks[index + 1..]
                .iter()
                .position(|candidate| candidate.id() == *b"tdmn")
                .map_or(chunks.len(), |offset| index + 1 + offset);
            (index + 1..end).find(|candidate| chunks[*candidate].list_kind() == Some(*b"tdgp"))
        })?
    });
    if let Some(index) = group_index {
        return chunks[index].children_mut();
    }
    for chunk in chunks {
        if let Some(children) = chunk.children_mut()
            && let Some(group) = named_group_mut(children, target)
        {
            return Some(group);
        }
    }
    None
}

fn duplicate_then_corrupt_first_effect(parade: &mut Vec<crate::rifx::Chunk>) {
    let starts: Vec<_> = parade
        .iter()
        .enumerate()
        .filter(|(_, chunk)| chunk.id() == *b"tdmn")
        .map(|(index, _)| index)
        .collect();
    let start = *starts.first().expect("native Effect Parade occurrence");
    let end = starts.get(1).copied().unwrap_or(parade.len());
    let duplicate = parade[start..end].to_vec();
    parade.splice(end..end, duplicate);

    let sspc = parade[start + 1..end]
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"sspc"))
        .and_then(crate::rifx::Chunk::children_mut)
        .expect("native effect plugin descriptor");
    let explicit = sspc
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .and_then(crate::rifx::Chunk::children_mut)
        .expect("native explicit effect controls");
    let match_name = explicit
        .iter_mut()
        .find(|chunk| chunk.id() == *b"tdmn")
        .expect("explicit effect control match name");
    *match_name = crate::rifx::Chunk::data(*b"tdmn", vec![b'x'])
        .expect("malformed short match-name chunk is still valid RIFX");
}

fn close(actual: &[f64], expected: &[f64], context: &str) {
    assert_eq!(actual.len(), expected.len(), "{context}: dimensions");
    for (a, b) in actual.iter().zip(expected) {
        assert!(
            (a - b).abs() < 0.002,
            "{context}: expected {expected:?}, got {actual:?}"
        );
    }
}

#[test]
fn malformed_explicit_control_table_omits_only_its_effect_occurrence() {
    let project = read_project(CATALOG).expect("pinned Adobe-native effect catalog");
    let case = &cases(STATIC_RECEIPT)[0];
    let expected_name = case["matchName"].as_str().expect("Adobe match name");
    let mut layer = layer(&project, case).clone();
    let parade = named_group_mut(&mut layer.content, "ADBE Effect Parade")
        .expect("pinned native Effect Parade");
    duplicate_then_corrupt_first_effect(parade);

    let (effects, warnings) = native::read_effects(&layer.content, [120.0, 80.0]);
    assert_eq!(effects.len(), 1, "valid sibling occurrence must survive");
    assert_eq!(effects[0].match_name, expected_name);
    assert!(
        warnings.iter().any(|warning| {
            warning.contains(expected_name)
                && warning.contains("explicit control table")
                && warning.contains("malformed")
        }),
        "affected occurrence needs a contextual diagnostic: {warnings:?}"
    );
}

#[test]
fn adobe_native_effect_controls_match_independent_readback() {
    for (bytes, receipt, animated) in [
        (CATALOG, STATIC_RECEIPT, false),
        (ANIMATED, ANIMATED_RECEIPT, true),
    ] {
        let project = read_project(bytes).expect("pinned Adobe-native source");
        let cases = cases(receipt);
        assert_eq!(cases.len(), 32, "all pinned composition targets");
        let mut compared = 0;
        for case in &cases {
            let (effects, warnings) =
                native::read_effects(&layer(&project, case).content, [120.0, 80.0]);
            let name = case["matchName"].as_str().expect("Adobe match name");
            let effect = effects
                .iter()
                .find(|effect| effect.match_name == name)
                .unwrap_or_else(|| panic!("{name}: missing native effect; {warnings:?}"));
            for property in case["properties"].as_array().expect("Adobe controls") {
                let Some(id) = property["matchName"].as_str() else {
                    continue;
                };
                let expected = if animated {
                    &property["values"]
                } else {
                    &property["value"]
                };
                let expected_values = if animated {
                    expected.get(0).and_then(components)
                } else {
                    components(expected)
                };
                let Some(expected_values) = expected_values else {
                    continue;
                };
                let Some(decoded) = effect
                    .parameters
                    .iter()
                    .find(|parameter| parameter.match_name == id)
                else {
                    panic!("{name}/{id}: numeric readback control absent")
                };
                let numeric = match &decoded.numeric {
                    Ok(numeric) => numeric,
                    Err(error) => {
                        // A precise diagnosed unsupported native layout is acceptable here,
                        // but not a silently substituted default or a lost sibling.
                        assert!(
                            warnings.iter().any(|warning| warning.contains(name)
                                && warning.contains(id)
                                && warning.contains(&error.to_string())),
                            "{name}/{id}: undiagnosed {error}"
                        );
                        assert!(
                            matches!(error, crate::properties::PropertyError::Layout(_)),
                            "{name}/{id}: {error}"
                        );
                        continue;
                    }
                };
                let initial = numeric
                    .keyframes
                    .first()
                    .map(|key| key.values.as_slice())
                    .unwrap_or(&numeric.values);
                close(initial, &expected_values, &format!("{name}/{id} initial"));
                if animated {
                    let values = expected.as_array().expect("Adobe keyed values");
                    let times = property["times"].as_array().expect("Adobe keyed times");
                    assert_eq!(numeric.keyframes.len(), values.len(), "{name}/{id}: keys");
                    for ((key, value), time) in numeric.keyframes.iter().zip(values).zip(times) {
                        let expected = components(value).expect("Adobe numeric key");
                        close(&key.values, &expected, &format!("{name}/{id} key"));
                        let time = time.as_f64().expect("Adobe time");
                        assert!(
                            (key.time_secs - time).abs() < 0.002,
                            "{name}/{id}: key time {time} != {}",
                            key.time_secs
                        );
                    }
                }
                compared += 1;
            }
        }
        assert!(
            compared > if animated { 100 } else { 150 },
            "numeric Adobe controls compared: {compared}"
        );
    }
}

fn imported_case(project: &StructuralProject, id: u32) -> (Value, Vec<String>) {
    let imported = to_structural_fx_document(project, Some(id)).expect("fresh Adobe-native import");
    let warnings = imported
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.clone())
        .collect();
    (
        imported.document.to_json_value().expect("editable FX JSON"),
        warnings,
    )
}

fn effect_payload<'a>(node: &'a Value, kind: &str) -> Option<&'a Value> {
    if let Some(effects) = node.get("effects").and_then(Value::as_array) {
        for effect in effects {
            if effect["effect"]["type"] == kind {
                return Some(effect);
            }
            if effect["type"] == kind {
                return Some(effect);
            }
        }
    }
    if let Some(composition) = node.get("composition") {
        return effect_payload(composition, kind);
    }
    node.get("layers")
        .and_then(Value::as_array)
        .and_then(|layers| layers.iter().find_map(|layer| effect_payload(layer, kind)))
}

// Values below are fixed expectations from catalog-receipt.json (Adobe's own
// readback). Source-local scalar controls stay in native units. Bulge, Ripple
// and Wave Warp UV controls are normalized to the 320x180 destination Group
// plane on which those FX implementations evaluate imported layer effects;
// content-relative specialized effects keep their own contracts. Do not derive
// expectations from the converter's mapping table or FX defaults. Ripple's 20px
// Wave Height at a 20px Wave Width exceeds the documented FX strength limit
// (amplitude * frequency <= 1.25), so its amplitude is
// 1.25 * Wave Width / (2pi * 320).
const STATIC_EDITABLE_CASES: &[(u32, &str, &str)] = &[
    (
        1,
        "gaussianBlur",
        r#"{"blurriness":25,"repeatEdgePixels":true}"#,
    ),
    (
        16,
        "glow",
        r#"{"glowThreshold":60,"glowRadius":10,"glowIntensity":1}"#,
    ),
    (30, "directionalBlur", r#"{"direction":90,"blurLength":10}"#),
    (
        44,
        "pixelMotionBlur",
        r#"{"shutterControl":"manual","shutterAngle":180,"shutterSamples":5,"vectorDetail":20}"#,
    ),
    (
        58,
        "mosaic",
        r#"{"horizontalBlocks":10,"verticalBlocks":10,"sharpColors":false}"#,
    ),
    (72, "brightnessContrast", r#"{"brightness":0,"contrast":0}"#),
    (
        86,
        "shiftChannels",
        r#"{"takeRedFrom":"red","takeGreenFrom":"green","takeBlueFrom":"blue"}"#,
    ),
    (
        100,
        "hueSaturation",
        r#"{"hue":0,"saturation":0,"lightness":0,"colorize":false,"colorizeHue":0,"colorizeSaturation":25,"colorizeLightness":0}"#,
    ),
    (
        114,
        "radialBlur",
        r#"{"amount":10,"centerX":0.5,"centerY":0.5}"#,
    ),
    (
        128,
        "levels",
        r#"{"inputBlack":0,"inputWhite":255,"gamma":1,"outputBlack":0,"outputWhite":255}"#,
    ),
    (
        142,
        "levels",
        r#"{"inputBlack":0,"inputWhite":255,"gamma":1,"outputBlack":0,"outputWhite":255}"#,
    ),
    (
        156,
        "bulge",
        r#"{"horizontalRadius":0.15625,"verticalRadius":0.2777777778,"centerX":0.1875,"centerY":0.2222222222,"bulgeHeight":1,"pinning":false}"#,
    ),
    (
        170,
        "cornerPin",
        r#"{"upperLeftX":0,"upperLeftY":0,"upperRightX":1,"upperRightY":0,"lowerLeftX":0,"lowerLeftY":1,"lowerRightX":1,"lowerRightY":1}"#,
    ),
    (
        184,
        "motionTile",
        r#"{"tileCenterX":0.5,"tileCenterY":0.5,"tileWidth":100,"tileHeight":100,"outputWidth":100,"outputHeight":100,"mirrorEdges":false,"phase":0}"#,
    ),
    (
        198,
        "dropShadow",
        r#"{"color":[0,0,0,0.5],"offset":[3.5355339059,3.5355339059],"blurRadius":0,"spreadRadius":0,"blendMode":"normal"}"#,
    ),
    (212, "posterize", r#"{"levels":7}"#),
    (226, "posterizeTime", r#"{"frameRate":24}"#),
    (254, "findEdges", r#"{"invert":0}"#),
    (
        268,
        "exposure",
        r#"{"exposure":0,"offset":0,"gammaCorrection":1}"#,
    ),
    (282, "vibrance", r#"{"vibrance":0,"saturation":0}"#),
    (
        296,
        "twirl",
        r#"{"angle":0,"radius":0.3,"centerX":0.5,"centerY":0.5}"#,
    ),
    (
        310,
        "ripple",
        r#"{"centerX":0.1875,"centerY":0.2222222222,"amplitude":0.0124339799,"frequency":100.5309649149,"phase":0}"#,
    ),
    (324, "sharpen", r#"{"amount":0}"#),
    (338, "lumaKey", r#"{"threshold":0,"softness":0}"#),
    (352, "simpleChoker", r#"{"choke":0}"#),
    (
        366,
        "grain",
        r#"{"amount":1,"size":1,"softness":1,"aspectRatio":1,"seed":0}"#,
    ),
    (
        380,
        "waveWarp",
        r#"{"waveHeight":0.0555555556,"waveWidth":8,"direction":90,"phase":0}"#,
    ),
    (
        408,
        "tintTritone",
        r#"{"blackR":0,"blackG":0,"blackB":0,"whiteR":1,"whiteG":1,"whiteB":1,"amount":100}"#,
    ),
    (
        422,
        "gradientRamp",
        r#"{"startX":0.5,"startY":0,"endX":0.5,"endY":1,"startR":0,"startG":0,"startB":0,"endR":1,"endG":1,"endB":1,"shape":0,"blend":1}"#,
    ),
    (
        436,
        "turbulentNoise",
        r#"{"noiseType":"softLinear","fractalType":"basic","invert":0,"contrast":100,"brightness":0,"rotation":0,"scale":100,"complexity":6,"subInfluence":70,"evolution":0,"blend":1,"offsetX":0,"offsetY":0}"#,
    ),
];

fn assert_editable_fields(actual: &Value, expected: &Value, context: &str) {
    for (field, value) in expected.as_object().expect("pinned editable fields") {
        let got = &actual[field];
        if let (Some(got), Some(expected)) = (components(got), components(value)) {
            close(&got, &expected, &format!("{context}/{field}"));
        } else {
            assert_eq!(got, value, "{context}/{field}");
        }
    }
}

#[test]
fn adobe_static_shape_point_imports_nondefault_editable_coordinates() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/effects/static_point_controls.aep",
        1,
        || {
            let project = read_project(include_bytes!(
                "../../tests/fixtures/effects/static_point_controls.aep"
            ))
            .unwrap();
            let (document, warnings) = imported_case(&project, 1);
            let record = effect_payload(&document, "twirl").expect("editable native Twirl");
            assert_editable_fields(
                &record["effect"],
                &serde_json::json!({
                    "angle":45.0,"radius":0.3,"centerX":0.09375,"centerY":0.3333333333333333
                }),
                &format!(
                    "nondefault Shape Point in native source coordinates; diagnostics={warnings:?}"
                ),
            );
        },
    );
    cases.finish();
}

#[test]
fn adobe_nondefault_individual_levels_imports_editable_master_controls() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/effects/levels_controls.aep",
        1,
        || {
            let project = read_project(include_bytes!(
                "../../tests/fixtures/effects/levels_controls.aep"
            ))
            .expect("independent Adobe Individual Controls source");
            let (document, _) = imported_case(&project, 1);
            let levels = effect_payload(&document, "levels").expect("editable master controls");
            assert_editable_fields(
                &levels["effect"],
                &serde_json::json!({
                    "inputBlack":51.0,"inputWhite":204.0,"gamma":1.5,"outputBlack":25.5,"outputWhite":229.5
                }),
                "nondefault Individual Controls",
            );
        },
    );
    cases.finish();
}

#[test]
fn adobe_shape_owner_retains_editable_gaussian_control() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/effects/shape_owner_gaussian.aep",
        1,
        || {
            let project = read_project(include_bytes!(
                "../../tests/fixtures/effects/shape_owner_gaussian.aep"
            ))
            .expect("independent Adobe Shape-owner source");
            let (document, _) = imported_case(&project, 1);
            let record =
                effect_payload(&document, "gaussianBlur").expect("editable Shape-owned effect");
            let gaussian = &record["effect"];
            assert_eq!(gaussian["blurriness"].as_f64(), Some(25.0));
            assert!(!document.to_string().contains("JsScript"));
        },
    );
    cases.finish();
}

#[test]
fn adobe_catalog_imports_concrete_editable_controls_without_scripts() {
    let mut batch = crate::adobe_test_support::CaseBatch::new();
    for id in CATALOG_TARGET_IDS {
        batch.run(
            "crates/aftereffects_file/tests/fixtures/effects/catalog.aep",
            id,
            || {
                let source = read_project(CATALOG).expect("Adobe-authored static catalog");
                assert_eq!(
                    STATIC_EDITABLE_CASES.len(),
                    30,
                    "all mapped native occurrences"
                );
                let (document, diagnostics) = imported_case(&source, id);
                let layers = document["composition"]["layers"]
                    .as_array()
                    .expect("editable root layers");
                assert!(!layers.is_empty(), "{id}: native layer owner retained");

                if let Some(&(_, kind, fields)) = STATIC_EDITABLE_CASES
                    .iter()
                    .find(|(case_id, _, _)| *case_id == id)
                {
                    let case = cases(STATIC_RECEIPT)
                        .into_iter()
                        .find(|case| case["compositionId"] == id)
                        .expect("pinned Adobe composition target");
                    assert_eq!(
                        layer(&source, &case).record.id(),
                        case["layerId"].as_u64().unwrap() as u32,
                        "{id}: native layer identity"
                    );
                    let effect = effect_payload(&document, kind)
                        .unwrap_or_else(|| panic!("{id}/{kind}: missing effect; {diagnostics:?}"));
                    assert!(
                        effect["id"].as_u64().is_some(),
                        "{id}: persistent effect identity: {effect}"
                    );
                    assert_eq!(effect["enabled"], true, "{id}: native effect enabled");
                    let payload = effect.get("effect").unwrap_or(effect);
                    assert_editable_fields(
                        payload,
                        &serde_json::from_str::<Value>(fields).expect("pinned expectations"),
                        &format!("{id}/{kind}"),
                    );
                    assert!(
                        !document.to_string().contains("JsScript"),
                        "{id}: no generated scripts"
                    );
                } else {
                    // These have no current FX equivalent. Their absence must be diagnosed,
                    // while the real composition/layer envelope remains editable.
                    let native = match id {
                        240 => "CS Vignette",
                        394 => "ADBE Optics Compensation",
                        _ => panic!("{id}: target has no static effect contract"),
                    };
                    assert!(
                        diagnostics.iter().any(|warning| {
                            warning.contains(native) && warning.contains("omitted")
                        }),
                        "{id}: {diagnostics:?}"
                    );
                }
            },
        );
    }
    batch.finish();
}

#[test]
fn adobe_special_controls_use_contextual_diagnostics() {
    let static_source = read_project(CATALOG).expect("Adobe-authored static catalog");
    for (id, native, kind) in [
        (86, "ADBE Shift Channels", "shiftChannels"),
        (198, "ADBE Drop Shadow", "dropShadow"),
    ] {
        let (document, warnings) = imported_case(&static_source, id);
        assert!(
            effect_payload(&document, kind).is_some(),
            "{id}/{kind}: static special mapping"
        );
        assert!(
            !warnings.iter().any(
                |warning| warning.contains(native) && warning.contains("no mapped FX property")
            ),
            "{id}: recognized static controls received generic omissions: {warnings:?}"
        );
    }

    let animated_source = read_project(ANIMATED).expect("Adobe-authored animated catalog");
    for (id, native, expected) in [
        (86, "ADBE Shift Channels", "animation/expression omitted"),
        (198, "ADBE Drop Shadow", "Shadow Color animation"),
    ] {
        let (_, warnings) = imported_case(&animated_source, id);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains(native) && warning.contains(expected)),
            "{id}: unsupported special animation must remain contextual: {warnings:?}"
        );
        assert!(
            !warnings.iter().any(
                |warning| warning.contains(native) && warning.contains("no mapped FX property")
            ),
            "{id}: handled special controls received generic omissions: {warnings:?}"
        );
    }
}

// Selected keyed Adobe readbacks, in FX units. Times in animated-receipt.json
// are 0 and 1 seconds; key targets must bind the imported occurrence, not a
// similarly named effect elsewhere. These are structure tests, not pixel proof.
const ANIMATED_EDITABLE_KEYS: &[(u32, &str, &str, f64, f64)] = &[
    (1, "gaussianBlur", "blurriness", 25.0, 30.0),
    (16, "glow", "glowThreshold", 60.0, 72.0),
    (30, "directionalBlur", "direction", 90.0, 108.0),
    (44, "pixelMotionBlur", "shutterAngle", 180.0, 216.0),
    (58, "mosaic", "horizontalBlocks", 10.0, 12.0),
    (72, "brightnessContrast", "brightness", 0.0, 0.1),
    (100, "hueSaturation", "colorizeSaturation", 25.0, 30.0),
    (114, "radialBlur", "amount", 10.0, 12.0),
    (198, "dropShadow", "blurRadius", 0.0, 0.05000000074506),
    (142, "levels", "inputWhite", 255.0, 306.0),
    (156, "bulge", "horizontalRadius", 50.0 / 320.0, 60.0 / 320.0),
    (170, "cornerPin", "upperLeftX", 0.0, 12.0 / 120.0),
    (184, "motionTile", "tileWidth", 100.0, 80.0),
    (212, "posterize", "levels", 7.0, 8.4),
    (254, "findEdges", "invert", 0.0, 1.0),
    (268, "exposure", "gammaCorrection", 1.0, 1.2),
    (282, "vibrance", "vibrance", 0.0, 0.1),
    (296, "twirl", "radius", 0.3, 0.36),
    // The 20 -> 24px Wave Height keys exceed the FX strength limit at the
    // initial 20px Wave Width; the 24px peak meets 1.25 * 20 / (2pi * 320).
    (
        310,
        "ripple",
        "amplitude",
        20.0 / 24.0 * 1.25 * 20.0 / (std::f64::consts::TAU * 320.0),
        1.25 * 20.0 / (std::f64::consts::TAU * 320.0),
    ),
    (324, "sharpen", "amount", 0.0, 0.0),
    (338, "lumaKey", "threshold", 0.0, 0.0),
    (352, "simpleChoker", "choke", 0.0, 0.1),
    // The FX payload stores `amount`, but the existing effectProperty animation
    // target is `intensity` (the native -0008 Amount control).
    (366, "grain", "intensity", 1.0, 1.2),
    (380, "waveWarp", "waveHeight", 10.0 / 180.0, 12.0 / 180.0),
    (408, "tintTritone", "amount", 100.0, 80.0),
    (422, "gradientRamp", "blend", 1.0, 0.0),
    (436, "turbulentNoise", "contrast", 100.0, 120.0),
    (16, "glow", "glowRadius", 10.0, 12.0),
    (16, "glow", "glowIntensity", 1.0, 1.2),
    (30, "directionalBlur", "blurLength", 10.0, 12.0),
    (44, "pixelMotionBlur", "shutterSamples", 5.0, 6.0),
    (44, "pixelMotionBlur", "vectorDetail", 20.0, 24.0),
    (58, "mosaic", "verticalBlocks", 10.0, 12.0),
    (72, "brightnessContrast", "contrast", 0.0, 0.1),
    (100, "hueSaturation", "colorizeHue", 0.0, 0.1),
    (100, "hueSaturation", "colorizeLightness", 0.0, 0.1),
    (114, "radialBlur", "centerX", 0.5, 0.6),
    (114, "radialBlur", "centerY", 0.5, 33.0 / 80.0),
    (142, "levels", "inputBlack", 0.0, 25.5),
    (142, "levels", "gamma", 1.0, 1.2),
    (142, "levels", "outputWhite", 255.0, 306.0),
    (156, "bulge", "verticalRadius", 50.0 / 180.0, 60.0 / 180.0),
    (156, "bulge", "centerX", 60.0 / 320.0, 72.0 / 320.0),
    (156, "bulge", "centerY", 40.0 / 180.0, 33.0 / 180.0),
    (156, "bulge", "bulgeHeight", 1.0, 1.2),
    (170, "cornerPin", "upperLeftY", 0.0, -7.0 / 80.0),
    (170, "cornerPin", "upperRightX", 1.0, 132.0 / 120.0),
    (184, "motionTile", "tileCenterX", 0.5, 0.6),
    (184, "motionTile", "tileCenterY", 0.5, 33.0 / 80.0),
    (184, "motionTile", "outputWidth", 100.0, 120.0),
    (268, "exposure", "exposure", 0.0, 0.1),
    (268, "exposure", "offset", 0.0, 0.1),
    (282, "vibrance", "saturation", 0.0, 0.1),
    (296, "twirl", "centerX", 0.5, 0.6),
    (310, "ripple", "centerX", 60.0 / 320.0, 72.0 / 320.0),
    (310, "ripple", "phase", 0.0, std::f64::consts::PI / 1800.0),
    (366, "grain", "size", 1.0, 1.2),
    (366, "grain", "softness", 1.0, 1.2),
    (380, "waveWarp", "direction", 90.0, 108.0),
    (380, "waveWarp", "phase", 0.0, std::f64::consts::PI / 1800.0),
    (408, "tintTritone", "blackR", 0.0, 0.25),
    (408, "tintTritone", "whiteR", 1.0, 0.75),
    (422, "gradientRamp", "startX", 0.5, 0.6),
    (422, "gradientRamp", "endY", 1.0, 73.0 / 80.0),
    (422, "gradientRamp", "shape", 0.0, 1.0),
    (436, "turbulentNoise", "offsetX", 0.0, 10.0),
    (436, "turbulentNoise", "offsetY", 0.0, -35.0 / 6.0),
    (436, "turbulentNoise", "complexity", 6.0, 7.0),
    (142, "levels", "outputBlack", 0.0, 25.5),
    (156, "bulge", "pinning", 0.0, 1.0),
    (170, "cornerPin", "upperRightY", 0.0, -7.0 / 80.0),
    (170, "cornerPin", "lowerLeftX", 0.0, 12.0 / 120.0),
    (170, "cornerPin", "lowerLeftY", 1.0, 73.0 / 80.0),
    (170, "cornerPin", "lowerRightX", 1.0, 132.0 / 120.0),
    (170, "cornerPin", "lowerRightY", 1.0, 73.0 / 80.0),
    (184, "motionTile", "tileHeight", 100.0, 80.0),
    (184, "motionTile", "outputHeight", 100.0, 120.0),
    (184, "motionTile", "mirrorEdges", 0.0, 1.0),
    (184, "motionTile", "phase", 0.0, 0.1),
    (296, "twirl", "angle", 0.0, 0.1),
    (296, "twirl", "centerY", 0.5, 33.0 / 80.0),
    (310, "ripple", "centerY", 40.0 / 180.0, 33.0 / 180.0),
    (338, "lumaKey", "softness", 0.0, 0.0),
    (366, "grain", "aspectRatio", 1.0, 1.2),
    (366, "grain", "seed", 0.0, 0.0),
    (408, "tintTritone", "blackG", 0.0, 0.25),
    (408, "tintTritone", "blackB", 0.0, 0.25),
    (408, "tintTritone", "whiteG", 1.0, 0.75),
    (408, "tintTritone", "whiteB", 1.0, 0.75),
    (422, "gradientRamp", "startY", 0.0, -7.0 / 80.0),
    (422, "gradientRamp", "endX", 0.5, 0.6),
    (422, "gradientRamp", "startR", 0.0, 0.25),
    (422, "gradientRamp", "startG", 0.0, 0.25),
    (422, "gradientRamp", "startB", 0.0, 0.25),
    (422, "gradientRamp", "endR", 1.0, 0.75),
    (422, "gradientRamp", "endG", 1.0, 0.75),
    (422, "gradientRamp", "endB", 1.0, 0.75),
    (436, "turbulentNoise", "brightness", 0.0, 0.1),
    (436, "turbulentNoise", "rotation", 0.0, 0.1),
    (436, "turbulentNoise", "scale", 100.0, 120.0),
    (436, "turbulentNoise", "subInfluence", 70.0, 84.0),
    (436, "turbulentNoise", "evolution", 0.0, 0.1),
];

fn assert_animated_import_target(id: u32) {
    let source = read_project(ANIMATED).expect("Adobe-authored keyed catalog");
    let mut missing = Vec::new();
    let mut contracts = 0;
    for &(_, kind, field, first, last) in ANIMATED_EDITABLE_KEYS
        .iter()
        .filter(|(case_id, _, _, _, _)| *case_id == id)
    {
        contracts += 1;
        let (document, warnings) = imported_case(&source, id);
        let effect = effect_payload(&document, kind)
            .unwrap_or_else(|| panic!("{id}/{kind}: missing: {warnings:?}"));
        let effect_id = effect["id"]
            .as_u64()
            .expect("persistent effect occurrence identity");
        assert_eq!(effect["enabled"], true, "{id}: native effect enabled");
        let entries = document["composition"]["dynamics"]["entries"]
            .as_array()
            .expect("FX animation entries");
        let tracks: Vec<_> = entries
            .iter()
            .filter(|entry| {
                entry["target"]["kind"] == "effectProperty"
                    && entry["target"]["effectId"] == effect_id
                    && entry["target"]["paramName"] == field
            })
            .collect();
        if tracks.len() != 1 {
            let relevant: Vec<_> = warnings
                .iter()
                .filter(|warning| {
                    warning.contains(kind) || warning.contains("invalid numeric type/dimensions")
                })
                .collect();
            missing.push(format!("{id}/{kind}/{field}: expected one keyed target, got {}; relevant diagnostics: {relevant:?}", tracks.len()));
            continue;
        }
        let keys = tracks[0]["animator"]["keyframes"]
            .as_array()
            .expect("editable native keys");
        assert_eq!(keys.len(), 2, "{id}/{field}: selected Adobe key count");
        for (index, expected) in [first, last].into_iter().enumerate() {
            assert_eq!(
                keys[index]["layerTime"],
                (index * 1000) as u64,
                "{id}/{field}: native 0/1s clock"
            );
            assert_eq!(
                keys[index]["value"]["type"], "float",
                "{id}/{field}: editable scalar"
            );
            let actual = keys[index]["value"]["value"]
                .as_f64()
                .expect("FX numeric key");
            close(&[actual], &[expected], &format!("{id}/{field} key {index}"));
        }
        assert!(
            !document.to_string().contains("JsScript"),
            "{id}: no generated scripts"
        );
    }
    // Native selected controls not representable as editable keyed FX fields
    // must remain explicit limitations, never pass on their initial defaults.
    for (_, name, control) in [
        (1, "ADBE Gaussian Blur 2", "ADBE Gaussian Blur 2-0003"),
        (58, "ADBE Mosaic", "ADBE Mosaic-0003"),
        (86, "ADBE Shift Channels", "-0001"),
        (86, "ADBE Shift Channels", "-0002"),
        (86, "ADBE Shift Channels", "-0003"),
        (86, "ADBE Shift Channels", "-0004"),
        (128, "ADBE Easy Levels2", "-0008"),
        (128, "ADBE Easy Levels2", "-0009"),
        (198, "ADBE Drop Shadow", "-0001"),
        (198, "ADBE Drop Shadow", "-0002"),
        (198, "ADBE Drop Shadow", "-0003"),
        (198, "ADBE Drop Shadow", "-0004"),
        (198, "ADBE Drop Shadow", "-0006"),
        (226, "ADBE Posterize Time", "ADBE Posterize Time-0001"),
        (310, "ADBE Ripple", "-0005"),
        (380, "ADBE Wave Warp", "-0003"),
        (436, "ADBE AIF Perlin Noise 3D", "-0001"),
        (436, "ADBE AIF Perlin Noise 3D", "-0002"),
    ]
    .into_iter()
    .filter(|(case_id, _, _)| *case_id == id)
    {
        contracts += 1;
        let (document, warnings) = imported_case(&source, id);
        if !warnings.iter().any(|warning| {
            warning.contains(name)
                && ((warning.contains(control)
                    && (warning.contains("animation")
                        || warning.contains("static")
                        || warning.contains("malformed")))
                    || (id == 198
                        && control == "-0001"
                        && warning.contains("Shadow Color animation")))
        }) {
            missing.push(format!("{id}/{control}: unsupported selected motion not diagnosed; effect diagnostics: {:?}", warnings.iter().filter(|warning| warning.contains(name)).collect::<Vec<_>>()));
        }
        assert!(!document.to_string().contains("JsScript"));
    }
    for (_, name) in [(240, "CS Vignette"), (394, "ADBE Optics Compensation")]
        .into_iter()
        .filter(|(case_id, _)| *case_id == id)
    {
        contracts += 1;
        let (_, warnings) = imported_case(&source, id);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains(name) && warning.contains("omitted")),
            "{id}: {warnings:?}"
        );
    }
    assert!(
        contracts > 0,
        "{id}: registered target has an assertion contract"
    );
    assert!(
        missing.is_empty(),
        "animated native import gaps:\n{}",
        missing.join("\n")
    );
}

#[test]
fn adobe_animated_import_retains_editable_effect_keys_and_identity() {
    let mut batch = crate::adobe_test_support::CaseBatch::new();
    for id in CATALOG_TARGET_IDS {
        batch.run(
            "crates/aftereffects_file/tests/fixtures/effects/animated_catalog.aep",
            id,
            || assert_animated_import_target(id),
        );
    }
    batch.finish();
}

fn remove_vignette_from_parade(chunks: &mut [crate::rifx::Chunk]) -> usize {
    let Some(parade) = named_group_mut(chunks, "ADBE Effect Parade") else {
        return 0;
    };
    let starts: Vec<_> = parade
        .iter()
        .enumerate()
        .filter(|(_, chunk)| chunk.id() == *b"tdmn")
        .map(|(index, _)| index)
        .collect();
    let ranges: Vec<_> = starts
        .iter()
        .enumerate()
        .filter(|(_, start)| chunk_match_name(&parade[**start]) == Some("CS Vignette"))
        .map(|(index, start)| *start..starts.get(index + 1).copied().unwrap_or(parade.len()))
        .collect();
    let removed = ranges.len();
    for range in ranges.into_iter().rev() {
        parade.drain(range);
    }
    removed
}

fn assert_vignette_omission(source: &StructuralProject, composition_id: u32) -> Value {
    let (document, warnings) = imported_case(source, composition_id);
    assert!(
        effect_payload(&document, "vignette").is_none(),
        "CC Vignette must not become a different FX kernel"
    );
    assert!(
        warnings.iter().any(|warning| {
            warning.contains("CS Vignette")
                && warning.contains("unsupported")
                && warning.contains("omitted")
        }),
        "CC Vignette needs an explicit unsupported diagnostic: {warnings:?}"
    );
    let mut baseline = source.clone();
    // Remove only complete occurrence runs from an in-memory source copy.
    // Renaming the plugin can invalidate its descriptor and swallow siblings.
    let mut removed = 0;
    for item in &mut baseline.items {
        if let crate::structure::ItemKind::Composition(composition) = &mut item.kind {
            for layer in &mut composition.layers {
                removed += remove_vignette_from_parade(&mut layer.content);
            }
        }
    }
    assert!(removed > 0, "independent source must contain CC Vignette");
    let (expected, _) = imported_case(&baseline, composition_id);
    assert_eq!(
        document, expected,
        "only the unsupported effect may be omitted"
    );
    assert!(!document.to_string().contains("JsScript"));
    document
}

#[test]
fn adobe_cc_vignette_is_unsupported_and_preserves_other_content() {
    for bytes in [
        VIGNETTE_ISOLATED,
        VIGNETTE_STATIC_COVERAGE,
        VIGNETTE_ANIMATED_COVERAGE,
    ] {
        let source = read_project(bytes).expect("independent Adobe-native CC Vignette fixture");
        assert_vignette_omission(&source, 1);
    }
}

#[test]
fn animated_saturation_vibrance_leaf_is_native_and_independently_keyed() {
    let project = read_project(ANIMATED).unwrap();
    let case = cases(ANIMATED_RECEIPT)
        .into_iter()
        .find(|case| case["matchName"] == "ADBE Vibrance")
        .unwrap();
    assert_eq!(case["compositionId"], 282);
    let (effects, warnings) = native::read_effects(&layer(&project, &case).content, [120.0, 80.0]);
    let effect = effects
        .iter()
        .find(|effect| effect.match_name == "ADBE Vibrance")
        .unwrap();
    let property = effect
        .parameters
        .iter()
        .find(|p| p.match_name == "ADBE Vibrance-0002")
        .unwrap();
    let numeric = property.numeric.as_ref().unwrap();
    assert!(numeric.animated, "{warnings:?}");
    assert_eq!(numeric.keyframes.len(), 2);
    for (key, time, value) in [
        (&numeric.keyframes[0], 0.0, 0.0),
        (&numeric.keyframes[1], 1.0, 0.10000000149012),
    ] {
        assert!((key.time_secs - time).abs() < 1e-9);
        assert!((key.values[0] - value).abs() < 1e-9);
    }
    let receipt = cases(STATIC_RECEIPT)
        .into_iter()
        .find(|case| case["matchName"] == "ADBE Vibrance")
        .unwrap();
    let saturation = receipt["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["matchName"] == "ADBE Vibrance-0002")
        .unwrap();
    assert_eq!(saturation["canVaryOverTime"], true);
    assert_eq!(saturation["min"], -100);
    assert_eq!(saturation["max"], 100);
}
