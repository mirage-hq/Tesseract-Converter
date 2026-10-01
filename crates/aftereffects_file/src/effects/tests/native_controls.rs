//! Explicit native-source tests. Broad feature contracts remain opt-in; focused
//! bug regressions run normally. Failures never justify replacing independently
//! authored nondefault values with defaults.
use super::*;
use serde_json::json;

const STATIC_SOURCE: &[u8] =
    include_bytes!("../../../tests/fixtures/effects_coverage/native_static_controls.aep");
const ANIMATED_SOURCE: &[u8] =
    include_bytes!("../../../tests/fixtures/effects_coverage/native_animated_controls.aep");
const ORACLES: &str = include_str!("../../../tests/fixtures/effects_coverage/imports.json");
const STATIC_SOURCE_PATH: &str =
    "crates/aftereffects_file/tests/fixtures/effects_coverage/native_static_controls.aep";
const ANIMATED_SOURCE_PATH: &str =
    "crates/aftereffects_file/tests/fixtures/effects_coverage/native_animated_controls.aep";

fn native_target_id(name: &str) -> u32 {
    match name {
        "gaussianBlur-static" => 157,
        "glow-static" => 183,
        "directionalBlur-static" => 66,
        "pixelMotionBlur-static" => 391,
        "mosaic-static" => 339,
        "shiftChannels-static" => 521,
        "dropShadow-static" => 79,
        "hueSaturation-static" => 261,
        "radialBlur-static" => 456,
        "levels-static" => 287,
        "bulge-static" => 14,
        "cornerPin-static" => 40,
        "motionTile-static" => 365,
        "posterize-static" => 417,
        "posterizeTime-static" => 430,
        "findEdges-static" => 131,
        "exposure-static" => 105,
        "vibrance-static" => 651,
        "twirl-static" => 625,
        "ripple-static" => 482,
        "sharpen-static" => 508,
        "lumaKey-static" => 313,
        "simpleChoker-static" => 547,
        "grain-static" => 235,
        "waveWarp-static" => 677,
        "tintTritone-static" => 573,
        "gradientRamp-static" => 209,
        "turbulentNoise-static" => 599,
        "gaussianBlur-animated" => 144,
        "glow-animated" => 170,
        "directionalBlur-animated" => 53,
        "pixelMotionBlur-animated" => 378,
        "mosaic-animated" => 326,
        "hueSaturation-animated" => 248,
        "radialBlur-animated" => 443,
        "levels-animated" => 274,
        "bulge-animated" => 1,
        "cornerPin-animated" => 27,
        "motionTile-animated" => 352,
        "posterize-animated" => 404,
        "findEdges-animated" => 118,
        "exposure-animated" => 92,
        "vibrance-animated" => 638,
        "twirl-animated" => 612,
        "ripple-animated" => 469,
        "sharpen-animated" => 495,
        "lumaKey-animated" => 300,
        "simpleChoker-animated" => 534,
        "grain-animated" => 222,
        "waveWarp-animated" => 664,
        "tintTritone-animated" => 560,
        "gradientRamp-animated" => 196,
        "turbulentNoise-animated" => 586,
        _ => panic!("unregistered native effect target {name}"),
    }
}

fn check_native(name: &str) {
    let id = native_target_id(name);
    let (source_path, source) = if name.ends_with("-animated") {
        (ANIMATED_SOURCE_PATH, ANIMATED_SOURCE)
    } else {
        (STATIC_SOURCE_PATH, STATIC_SOURCE)
    };
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(source_path, id, || {
        let all: Value = serde_json::from_str(ORACLES).unwrap();
        let case = all["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap();
        let oracle_id = u32::try_from(case["composition_id"].as_u64().unwrap()).unwrap();
        assert_eq!(oracle_id, id, "{name}: registry and oracle target identity");
        let project = read_project(source).expect("independently Adobe-authored source");
        let (document, diagnostics) = imported_case(&project, id);
        let kind = case["effect"]["type"].as_str().unwrap();
        let record = effect_payload(&document, kind)
            .unwrap_or_else(|| panic!("{name}: missing editable effect: {diagnostics:?}"));
        assert_eq!(record["enabled"], true, "{name}: enabled native occurrence");
        assert_editable_fields(&record["effect"], &case["effect"], name);
        let entries = document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap();
        let expected_tracks = case["tracks"].as_array().unwrap();
        for track in expected_tracks {
            let param = track["param"].as_str().unwrap();
            let matches: Vec<_> = entries
                .iter()
                .filter(|e| {
                    e["target"]["kind"] == "effectProperty"
                        && e["target"]["effectId"] == record["id"]
                        && e["target"]["paramName"] == param
                })
                .collect();
            assert_eq!(
                matches.len(),
                1,
                "{name}/{param}: one stable editable target; {diagnostics:?}"
            );
            let keys = matches[0]["animator"]["keyframes"].as_array().unwrap();
            assert_eq!(
                keys.len(),
                2,
                "{name}/{param}: retain source knots, no frame baking"
            );
            for (index, key) in keys.iter().enumerate() {
                assert_eq!(
                    key["layerTime"],
                    index * 1000,
                    "{name}/{param}: source clock"
                );
                assert_eq!(
                    key["value"]["type"], "float",
                    "{name}/{param}: scalar component"
                );
                close(
                    &[key["value"]["value"].as_f64().unwrap()],
                    &[track["values"][index].as_f64().unwrap()],
                    &format!("{name}/{param}/{index}"),
                );
                if index > 0 {
                    assert_eq!(
                        key["easing"]["type"], track["easing"],
                        "{name}/{param}: native interpolation"
                    );
                }
            }
        }
        if expected_tracks.is_empty() {
            assert!(
                !entries
                    .iter()
                    .any(|e| e["target"]["kind"] == "effectProperty"
                        && e["target"]["effectId"] == record["id"]),
                "{name}: static native effect must not invent motion"
            );
        }
        assert!(
            !document.to_string().contains("JsScript"),
            "{name}: no generated script"
        );
    });
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_gaussian_blur_static() {
    check_native("gaussianBlur-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_gaussian_blur_animated() {
    check_native("gaussianBlur-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_glow_static() {
    check_native("glow-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_glow_animated() {
    check_native("glow-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_directional_blur_static() {
    check_native("directionalBlur-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_directional_blur_animated() {
    check_native("directionalBlur-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_pixel_motion_blur_static() {
    check_native("pixelMotionBlur-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_pixel_motion_blur_animated() {
    check_native("pixelMotionBlur-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_mosaic_static() {
    check_native("mosaic-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_mosaic_animated() {
    check_native("mosaic-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_shift_channels_static() {
    check_native("shiftChannels-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_drop_shadow_static() {
    check_native("dropShadow-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_hue_saturation_static() {
    check_native("hueSaturation-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_hue_saturation_animated() {
    check_native("hueSaturation-animated");
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adobe_authored_hue_master_static_imports_editable_values() {
    const SOURCE: &[u8] =
        include_bytes!("../../../tests/fixtures/effects_coverage/hue_master_static_adobe.aep");
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/effects_coverage/hue_master_static_adobe.aep",
        1,
        || {
            let project = read_project(SOURCE).expect("Adobe-authored Master-only source");
            let (document, diagnostics) = imported_case(&project, 1);
            let record = effect_payload(&document, "hueSaturation")
                .unwrap_or_else(|| panic!("editable Hue/Saturation missing: {diagnostics:?}"));
            let effect = &record["effect"];
            assert_eq!(effect["hue"], json!(50.0));
            assert_eq!(effect["saturation"], json!(-60.0));
            assert_eq!(effect["lightness"], json!(20.0));
            assert_eq!(effect["colorize"], false);
            assert_eq!(record["enabled"], true);
            assert!(
                !document.to_string().contains("JsScript"),
                "native source must not become generated script"
            );
        },
    );
    cases.finish();
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_radial_blur_static() {
    check_native("radialBlur-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_radial_blur_animated() {
    check_native("radialBlur-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_levels_static() {
    check_native("levels-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_levels_animated() {
    check_native("levels-animated");
}
/// The documented FX strength limit (`amplitude * frequency <= 1.25`, a visual
/// approximation beyond the fold-over threshold of 1) changes only a Ripple
/// whose retained or emitted Wave Height exceeds it.
/// Expectations use each pinned source's native Wave Height and Wave Width in
/// pixels on its 320px destination plane: the strong catalog ripples are
/// limited, while the weaker coverage ripples keep their pixel units.
#[test]
fn native_ripple_amplitude_is_limited_only_beyond_the_fx_strength_limit() {
    const PLANE: f64 = 320.0;
    let tau = std::f64::consts::TAU;
    let limited = 1.25 * 20.0 / (tau * PLANE);
    for (name, source, id, wavelength, initial, keys, scaled) in [
        ("catalog static", CATALOG, 310, 20.0, limited, vec![], true),
        (
            "catalog animated",
            ANIMATED,
            310,
            20.0,
            20.0 / 24.0 * limited,
            vec![20.0 / 24.0 * limited, limited],
            true,
        ),
        (
            "coverage static",
            STATIC_SOURCE,
            482,
            40.0,
            3.0 / PLANE,
            vec![],
            false,
        ),
        (
            "coverage animated",
            ANIMATED_SOURCE,
            469,
            40.0,
            3.0 / PLANE,
            vec![3.0 / PLANE, 6.0 / PLANE],
            false,
        ),
    ] {
        let project = read_project(source).expect("independently Adobe-authored source");
        let (document, diagnostics) = imported_case(&project, id);
        let record = effect_payload(&document, "ripple")
            .unwrap_or_else(|| panic!("{name}: missing editable ripple: {diagnostics:?}"));
        assert_eq!(record["enabled"], true, "{name}: enabled native occurrence");
        let frequency = record["effect"]["frequency"].as_f64().unwrap();
        assert!(
            (frequency - tau * PLANE / wavelength).abs() < 1e-9,
            "{name}: static reciprocal wavelength is unchanged, got {frequency}"
        );
        let amplitude = record["effect"]["amplitude"].as_f64().unwrap();
        assert!(
            (amplitude - initial).abs() < 1e-12 && amplitude * frequency <= 1.25 + 1e-12,
            "{name}: initial amplitude {amplitude}, expected {initial}"
        );
        let entries = document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap();
        let tracks: Vec<_> = entries
            .iter()
            .filter(|e| {
                e["target"]["kind"] == "effectProperty"
                    && e["target"]["effectId"] == record["id"]
                    && e["target"]["paramName"] == "amplitude"
            })
            .collect();
        assert_eq!(tracks.len(), usize::from(!keys.is_empty()), "{name}");
        if let Some(track) = tracks.first() {
            let actual = track["animator"]["keyframes"].as_array().unwrap();
            assert_eq!(actual.len(), keys.len(), "{name}: authored knots retained");
            for (index, (key, expected)) in actual.iter().zip(&keys).enumerate() {
                assert_eq!(key["layerTime"], index * 1000, "{name}: source clock");
                let value = key["value"]["value"].as_f64().unwrap();
                assert!(
                    (value - expected).abs() < 1e-12 && value * frequency <= 1.25 + 1e-12,
                    "{name}/{index}: amplitude key {value}, expected {expected}"
                );
                if index > 0 {
                    assert_eq!(key["easing"]["type"], "linear", "{name}: native easing");
                }
            }
        }
        let limit_diagnosed = diagnostics.iter().any(|diagnostic| {
            diagnostic.contains("ADBE Ripple / ADBE Ripple-0006")
                && diagnostic.contains("strength limit")
        });
        assert_eq!(limit_diagnosed, scaled, "{name}: {diagnostics:?}");
        assert!(
            !document.to_string().contains("JsScript"),
            "{name}: no generated script"
        );
    }
}

#[test]
fn native_bulge_pinning_edge_semantics_are_diagnosed_without_retuning_controls() {
    let oracles: Value = serde_json::from_str(ORACLES).unwrap();
    for (name, id, source) in [
        ("bulge-static", 14, STATIC_SOURCE),
        ("bulge-animated", 1, ANIMATED_SOURCE),
    ] {
        let project = read_project(source).expect("independently Adobe-authored source");
        let (document, diagnostics) = imported_case(&project, id);
        let expected = oracles["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["name"] == name)
            .unwrap();
        let record = effect_payload(&document, "bulge")
            .unwrap_or_else(|| panic!("{name}: missing editable Bulge: {diagnostics:?}"));

        assert_editable_fields(&record["effect"], &expected["effect"], name);
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.contains("ADBE Bulge / ADBE Bulge-0007")
                    && diagnostic.contains("Pin All Edges")
                    && diagnostic.contains("occupied boundary pixels")
            }),
            "{name}: the retained pinning control needs a contextual edge-semantics limitation: {diagnostics:?}"
        );
        let serialized = document.to_string();
        assert!(
            serialized.contains("\"type\":\"Rect\""),
            "{name}: convertible shape owner must survive"
        );
        assert!(
            !serialized.contains("JsScript"),
            "{name}: diagnosis must not generate scripts"
        );
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_bulge_static() {
    check_native("bulge-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_bulge_animated() {
    check_native("bulge-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_corner_pin_static() {
    check_native("cornerPin-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_corner_pin_animated() {
    check_native("cornerPin-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_motion_tile_static() {
    check_native("motionTile-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_motion_tile_animated() {
    check_native("motionTile-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_posterize_static() {
    check_native("posterize-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_posterize_animated() {
    check_native("posterize-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_posterize_time_static() {
    check_native("posterizeTime-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_find_edges_static() {
    check_native("findEdges-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_find_edges_animated() {
    check_native("findEdges-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_exposure_static() {
    check_native("exposure-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_exposure_animated() {
    check_native("exposure-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_vibrance_static() {
    check_native("vibrance-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_vibrance_animated() {
    check_native("vibrance-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_twirl_static() {
    check_native("twirl-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_twirl_animated() {
    check_native("twirl-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_ripple_static() {
    check_native("ripple-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_ripple_animated() {
    check_native("ripple-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_sharpen_static() {
    check_native("sharpen-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_sharpen_animated() {
    check_native("sharpen-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_luma_key_static() {
    check_native("lumaKey-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_luma_key_animated() {
    check_native("lumaKey-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_simple_choker_static() {
    check_native("simpleChoker-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_simple_choker_animated() {
    check_native("simpleChoker-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_grain_static() {
    check_native("grain-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_grain_animated() {
    check_native("grain-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_wave_warp_static() {
    check_native("waveWarp-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_wave_warp_animated() {
    check_native("waveWarp-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_tint_tritone_static() {
    check_native("tintTritone-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_tint_tritone_animated() {
    check_native("tintTritone-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_gradient_ramp_static() {
    check_native("gradientRamp-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_gradient_ramp_animated() {
    check_native("gradientRamp-animated");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_turbulent_noise_static() {
    check_native("turbulentNoise-static");
}
#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_turbulent_noise_animated() {
    check_native("turbulentNoise-animated");
}
