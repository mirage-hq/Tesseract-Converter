//! Independent Adobe-native wiggle-targets fixture (AE 26.5x89, managed
//! `run_jsx`): Shape contents on a layer starting at 0.5s, Mask Feather/
//! Opacity/Expansion and Text Animator properties.

use super::*;
use crate::expression_samples::{PropertyIdentity, ShapePathSegment};
use sha2::{Digest, Sha256};

const SOURCE: &[u8] = include_bytes!("../../tests/fixtures/expression_samples/wiggle_targets.aep");
const READBACK: &str =
    include_str!("../../tests/fixtures/expression_samples/wiggle_targets.readback.json");
const SOURCE_SHA256: &str = "d67b8862fd9346013407ae311ccf84f552182d17ee3552dc212f79acf76c003c";

fn project() -> crate::structure::StructuralProject {
    assert_eq!(format!("{:x}", Sha256::digest(SOURCE)), SOURCE_SHA256);
    crate::structure::read_project(SOURCE).unwrap()
}

/// Adobe's own property path, translated into the converter identity.
fn identity(case: &serde_json::Value) -> PropertyIdentity {
    let path = case["path"].as_array().unwrap();
    let segment = |value: &serde_json::Value| ShapePathSegment {
        index: value["index"].as_u64().unwrap() as u32,
        match_name: value["match_name"].as_str().unwrap().to_owned(),
    };
    let match_name = case["match_name"].as_str().unwrap().to_owned();
    match path[0]["match_name"].as_str().unwrap() {
        "ADBE Root Vectors Group" => PropertyIdentity::Shape {
            path: path.iter().map(segment).collect(),
        },
        "ADBE Mask Parade" => PropertyIdentity::Mask {
            index: segment(&path[1]).index,
            match_name,
        },
        "ADBE Text Properties" => PropertyIdentity::TextAnimator {
            animator: segment(&path[2]).index,
            match_name,
        },
        other => panic!("unexpected readback root {other}"),
    }
}

/// Converter Shape indices are native storage ordinals (the Adobe-captured
/// Scale family excepted), not AE `propertyIndex`; Adobe paths are matched by
/// their unambiguous match-name chain. Mask/Text identities compare exactly.
fn same_property(converter: &PropertyIdentity, adobe: &PropertyIdentity) -> bool {
    match (converter, adobe) {
        (PropertyIdentity::Shape { path: a }, PropertyIdentity::Shape { path: b }) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.match_name == b.match_name)
        }
        (a, b) => a == b,
    }
}

fn components(value: &serde_json::Value) -> Vec<f64> {
    match value {
        serde_json::Value::Array(values) => values.iter().map(|v| v.as_f64().unwrap()).collect(),
        value => vec![value.as_f64().unwrap()],
    }
}

fn cases() -> Vec<serde_json::Value> {
    let readback: serde_json::Value = serde_json::from_str(READBACK).unwrap();
    readback["cases"].as_array().unwrap().clone()
}

#[test]
fn wiggle_targets_base_values_track_adobe_pre_expression_samples() {
    let project = project();
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    for case in cases() {
        let comp_id = case["composition_id"].as_u64().unwrap() as u32;
        let layer_id = case["layer_id"].as_u64().unwrap() as u32;
        let match_name = case["match_name"].as_str().unwrap();
        let crate::structure::ItemKind::Composition(comp) = &project.item(comp_id).unwrap().kind
        else {
            panic!("composition {comp_id} missing");
        };
        let mut model = Model::new(&items, comp_id, comp, true);
        let property = identity(&case);
        let slot = model
            .properties
            .iter()
            .position(|p| {
                p.comp_id == comp_id
                    && p.layer_id == layer_id
                    && same_property(&p.identity, &property)
            })
            .unwrap_or_else(|| panic!("{property:?} not modelled"));
        assert_eq!(model.properties[slot].error, None, "{match_name}");
        model.properties[slot].expression = Some(syntax::compile("value;").unwrap());
        let samples = case["samples"].as_array().unwrap();
        let times: Vec<f64> = samples
            .iter()
            .map(|s| s["time"].as_f64().unwrap())
            .collect();
        let actual = evaluate_grid(&mut prepare(&model).unwrap(), slot, &times).unwrap();
        let mut max_error = 0.0_f64;
        for (sample, actual) in samples.iter().zip(&actual) {
            let expected = components(&sample["pre"]);
            assert_eq!(actual.len(), expected.len(), "{match_name} dimensions");
            for (a, e) in actual.iter().zip(&expected) {
                max_error = max_error.max((a - e).abs());
            }
        }
        let tolerance = if match_name == "ADBE Vector Position" {
            0.25
        } else {
            1e-3
        };
        eprintln!(
            "{} {match_name}: max pre-expression error {max_error:e}",
            case["composition"]
        );
        assert!(
            max_error <= tolerance,
            "{} {match_name}: max error {max_error} exceeds {tolerance}",
            case["composition"]
        );
    }
}

/// Every converted keyframe track of the composition: property type, keys as
/// (seconds, components), plus all diagnostics.
/// One converted track: target name and keys as (seconds, components).
type Track = (String, Vec<(f64, Vec<f64>)>);

fn converted(comp_id: u32) -> (Vec<String>, Vec<Track>) {
    let project = project();
    let converted =
        crate::structure_document::to_structural_fx_document(&project, Some(comp_id)).unwrap();
    let json = String::from_utf8(converted.document.to_json_vec().unwrap()).unwrap();
    assert!(!json.to_ascii_lowercase().contains("jsscript"));
    let warnings = converted
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.clone())
        .collect();
    let tracks = converted
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .filter_map(|entry| {
            let track = entry.animator.keyframe_track()?;
            track.validate_for_target(&entry.target).unwrap();
            let keys = track
                .keyframes()
                .iter()
                .map(|key| {
                    let value = serde_json::to_value(key.value()).unwrap();
                    let numbers = match &value {
                        serde_json::Value::Number(n) => vec![n.as_f64().unwrap()],
                        serde_json::Value::Object(map) => map
                            .values()
                            .flat_map(|v| match v {
                                serde_json::Value::Number(n) => vec![n.as_f64().unwrap()],
                                serde_json::Value::Array(a) => {
                                    a.iter().filter_map(serde_json::Value::as_f64).collect()
                                }
                                _ => Vec::new(),
                            })
                            .collect(),
                        serde_json::Value::Array(a) => {
                            a.iter().filter_map(serde_json::Value::as_f64).collect()
                        }
                        _ => Vec::new(),
                    };
                    (key.layer_time().as_millis() as f64 / 1000.0, numbers)
                })
                .collect();
            let name = match entry.target.as_property() {
                Some(property) => format!("{:?}", property.property_type()),
                None => format!("{:?}", entry.target),
            };
            Some((name, keys))
        })
        .collect();
    (warnings, tracks)
}

/// Adobe's evaluated value at composition time `t`, linearly interpolated.
fn adobe_at(case: &serde_json::Value, t: f64) -> Vec<f64> {
    let samples = case["samples"].as_array().unwrap();
    let time = |s: &serde_json::Value| s["time"].as_f64().unwrap();
    let right = samples
        .iter()
        .position(|s| time(s) >= t - 1e-9)
        .unwrap_or(samples.len() - 1);
    let b = components(&samples[right]["evaluated"]);
    if right == 0 || (time(&samples[right]) - t).abs() < 1e-9 {
        return b;
    }
    let a = components(&samples[right - 1]["evaluated"]);
    let u = (t - time(&samples[right - 1])) / (time(&samples[right]) - time(&samples[right - 1]));
    a.iter().zip(&b).map(|(a, b)| a + (b - a) * u).collect()
}

#[test]
fn wiggle_targets_lower_to_editable_tracks_on_existing_targets() {
    let cases = cases();
    let case = |name: &str| {
        cases
            .iter()
            .find(|case| case["match_name"] == name)
            .unwrap_or_else(|| panic!("{name} readback missing"))
            .clone()
    };
    // (composition, FX track selector, native match name, deterministic?,
    //  native→FX factor, layer start for the local clock)
    for (comp_id, track, name, deterministic, factor, layer_start) in [
        (1, "EllipseSize", "ADBE Vector Ellipse Size", true, 1.0, 0.5),
        (1, "TrimEnd", "ADBE Vector Trim End", true, 1.0, 0.5),
        (
            1,
            "StrokeWidth",
            "ADBE Vector Stroke Width",
            false,
            1.0,
            0.5,
        ),
        (1, "PositionX", "ADBE Vector Position", false, 1.0, 0.5),
        (14, "feather", "ADBE Mask Feather", true, 1.0, 0.0),
        (14, "expansion", "ADBE Mask Offset", true, 1.0, 0.0),
        (14, "\"opacity\"", "ADBE Mask Opacity", false, 0.01, 0.0),
        (29, "\"opacity\"", "ADBE Text Opacity", true, 1.0, 0.0),
        (29, "\"position\"", "ADBE Text Position 3D", false, 1.0, 0.0),
    ] {
        let (warnings, tracks) = converted(comp_id);
        assert!(
            !warnings
                .iter()
                .any(|w| w.contains(name)
                    && w.contains("converter expression evaluation unsupported")),
            "{name}: evaluation denied: {warnings:?}"
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains(name) && w.contains("converter-evaluated expression lowered")),
            "{name}: no lowered expression diagnostic: {warnings:?}"
        );
        let matching: Vec<_> = tracks
            .iter()
            .filter(|(kind, _)| kind.contains(track))
            .collect();
        assert!(
            !matching.is_empty(),
            "{name}: no {track} track in {:?}",
            tracks.iter().map(|t| &t.0).collect::<Vec<_>>()
        );
        let (_, keys) = matching[0];
        assert!(
            keys.windows(2).any(|k| k[0].1 != k[1].1),
            "{name}: track does not vary"
        );
        if deterministic {
            let case = case(name);
            // Vector2 tracks share integer-millisecond keys: allow half a
            // millisecond of the fastest Adobe motion plus the fit tolerance.
            let samples = case["samples"].as_array().unwrap();
            let speed = samples
                .windows(2)
                .flat_map(|w| {
                    let dt = w[1]["time"].as_f64().unwrap() - w[0]["time"].as_f64().unwrap();
                    components(&w[1]["evaluated"])
                        .into_iter()
                        .zip(components(&w[0]["evaluated"]))
                        .map(move |(b, a)| ((b - a) / dt).abs())
                })
                .fold(0.0, f64::max);
            let tolerance = 0.0005 * speed * factor + 1e-3;
            let mut max_error = 0.0_f64;
            for (local, value) in keys {
                let expected = adobe_at(&case, local + layer_start);
                for (actual, expected) in value.iter().zip(&expected) {
                    max_error = max_error.max((actual - expected * factor).abs());
                }
            }
            eprintln!(
                "comp {comp_id} {name}: max key error vs Adobe evaluated {max_error:e} (bound {tolerance:e})"
            );
            assert!(
                max_error <= tolerance,
                "{name}: key error {max_error} vs Adobe evaluated values exceeds {tolerance}"
            );
        }
    }
}

#[test]
fn wiggle_targets_wiggle_diagnostics_are_contextual() {
    for (comp_id, name) in [
        (1, "ADBE Vector Fill Opacity"),
        (1, "ADBE Vector Stroke Width"),
        (1, "ADBE Vector Position"),
        (14, "ADBE Mask Opacity"),
        (29, "ADBE Text Position 3D"),
    ] {
        let (warnings, _) = converted(comp_id);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains(name) && w.contains("AE expression wiggle approximated")),
            "{name}: missing wiggle approximation diagnostic"
        );
    }
    let (warnings, _) = converted(29);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("evaluated once per layer, not per character"))
    );
}

/// Every native wiggle must actually wiggle: over two seconds at 1..3 Hz the
/// perturbation spans at least half its amplitude in every component, like
/// Adobe's own samples. A correlated lattice hash once made it a slow drift.
#[test]
fn wiggle_targets_perturbation_spans_its_amplitude_like_adobe() {
    let project = project();
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    for case in cases() {
        let expression = case["expression"].as_str().unwrap();
        let Some(arguments) = expression.strip_prefix("wiggle(") else {
            continue;
        };
        let amplitude: f64 = arguments
            .trim_end_matches(')')
            .split(',')
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        let comp_id = case["composition_id"].as_u64().unwrap() as u32;
        let layer_id = case["layer_id"].as_u64().unwrap() as u32;
        let crate::structure::ItemKind::Composition(comp) = &project.item(comp_id).unwrap().kind
        else {
            panic!("composition {comp_id} missing");
        };
        let model = Model::new(&items, comp_id, comp, true);
        let property = identity(&case);
        let slot = model
            .properties
            .iter()
            .position(|p| {
                p.comp_id == comp_id
                    && p.layer_id == layer_id
                    && same_property(&p.identity, &property)
            })
            .unwrap();
        let samples = case["samples"].as_array().unwrap();
        let times: Vec<f64> = samples
            .iter()
            .map(|s| s["time"].as_f64().unwrap())
            .collect();
        let ours = evaluate_grid(&mut prepare(&model).unwrap(), slot, &times).unwrap();
        let dimensions = components(&samples[0]["pre"]).len();
        // Planar text/shape vectors keep an unused Z; only moving axes count.
        let axes = if case["match_name"] == "ADBE Text Position 3D" {
            2
        } else {
            dimensions
        };
        for axis in 0..axes {
            let span = |values: &mut dyn Iterator<Item = f64>| {
                let values: Vec<f64> = values.collect();
                values.iter().copied().fold(f64::MIN, f64::max)
                    - values.iter().copied().fold(f64::MAX, f64::min)
            };
            let ours_span = span(
                &mut ours
                    .iter()
                    .zip(samples)
                    .map(|(value, sample)| value[axis] - components(&sample["pre"])[axis]),
            );
            let adobe_span = span(&mut samples.iter().map(|sample| {
                components(&sample["evaluated"])[axis] - components(&sample["pre"])[axis]
            }));
            eprintln!(
                "{} axis {axis}: perturbation span ours {ours_span:.3} adobe {adobe_span:.3} (amplitude {amplitude})",
                case["match_name"]
            );
            assert!(
                ours_span >= 0.5 * amplitude && adobe_span >= 0.5 * amplitude,
                "{} axis {axis}: perturbation span ours {ours_span} adobe {adobe_span} below half amplitude {amplitude}",
                case["match_name"]
            );
        }
    }
}
