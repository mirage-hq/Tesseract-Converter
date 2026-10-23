//! `tests/fixtures/color_ae_curves.aep`: Curves authored by hand in the AE
//! 26.5 UI (RGB channel, one point dragged from the diagonal to about input
//! 128 / output 160; Red/Green/Blue/Alpha untouched; 8 bpc, working space
//! None). Provenance and the native reference render are recorded in
//! `color_ae_fixtures.provenance.json`.
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::*;

const FIXTURE: &[u8] = include_bytes!("../../../tests/fixtures/color_ae_curves.aep");
const FIXTURE_SHA256: &str = "76610bf4bbfdaf545fbf6cb627ecc58106168d31fb0f7d13c70bba4fe143730b";
/// The authored RGB editor points as AE saved them.
const AUTHORED: [editor::Point; 3] = [[0, 0], [127, 162], [255, 255]];
/// The one-code piecewise-linear fit of AE's saved RGB table.
const FITTED_KNOTS: [(u8, u8); 8] = [
    (0, 0),
    (62, 86),
    (88, 119),
    (115, 150),
    (140, 175),
    (171, 201),
    (205, 225),
    (255, 255),
];

/// Every `aRbp` payload following a Curves control name, in file order.
fn payloads() -> Vec<&'static [u8]> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = FIXTURE[from..]
        .windows(CONTROL.len())
        .position(|window| window == CONTROL.as_bytes())
    {
        let name = from + at;
        let tag = name
            + FIXTURE[name..]
                .windows(4)
                .position(|window| window == b"aRbp")
                .unwrap();
        let len = u32::from_be_bytes(FIXTURE[tag + 4..tag + 8].try_into().unwrap()) as usize;
        found.push(&FIXTURE[tag + 8..tag + 8 + len]);
        from = name + CONTROL.len();
    }
    found
}

fn table(payload: &[u8], channel: usize) -> &[u8] {
    &payload[TABLES_AT + channel * SAMPLES..TABLES_AT + (channel + 1) * SAMPLES]
}

fn block(payload: &[u8], channel: usize) -> &[u8] {
    &payload[EDITOR_AT + channel * editor::BLOCK..EDITOR_AT + (channel + 1) * editor::BLOCK]
}

fn authored_payload() -> &'static [u8] {
    payloads()[1]
}

#[test]
fn ui_authored_curves_store_rgb_points_and_their_natural_spline_table() {
    assert_eq!(format!("{:x}", Sha256::digest(FIXTURE)), FIXTURE_SHA256);
    let found = payloads();
    // Definition default + instance, twice (as in the native colour catalog).
    assert_eq!(found.len(), 4);
    assert_eq!(found[0], default_payload().as_slice());
    assert_eq!(found[2], default_payload().as_slice());
    assert_eq!(found[1], found[3]);
    let authored = authored_payload();
    // Master (RGB) is its own table and block; the other channels are untouched.
    assert_eq!(editor::decode(block(authored, 0)).unwrap(), AUTHORED);
    assert_eq!(
        table(authored, 0),
        editor::spline_table(&AUTHORED).as_slice(),
        "AE's table is the half-up natural cubic spline through the points (0 codes)"
    );
    assert_eq!(table(authored, 0)[127], 162);
    let identity = identity_table();
    for channel in 1..CHANNELS {
        assert_eq!(table(authored, channel), identity.as_slice());
        assert_eq!(block(authored, channel), editor::encode(&editor::DIAGONAL));
    }
    // The UI leaves the dragged point selected (index 1); ours write -1.
    assert_eq!(&block(authored, 0)[68..72], &1_i32.to_be_bytes());
    let mut unselected = block(authored, 0).to_vec();
    unselected[68..72].copy_from_slice(&(-1_i32).to_be_bytes());
    assert_eq!(unselected, editor::encode(&AUTHORED));
    // A piecewise-linear reading of the same points would be 7 codes off.
    let linear = table(authored, 0)
        .iter()
        .enumerate()
        .map(|(x, y)| {
            let x = x as f64;
            let linear = if x <= 127.0 {
                x * 162.0 / 127.0
            } else {
                162.0 + (x - 127.0) * 93.0 / 128.0
            };
            (linear - f64::from(*y)).abs()
        })
        .fold(0.0, f64::max);
    assert!(linear > 6.0, "{linear}");
}

fn imported() -> (Value, Vec<String>) {
    assert_eq!(format!("{:x}", Sha256::digest(FIXTURE)), FIXTURE_SHA256);
    assert!(!FIXTURE.windows(7).any(|window| window == b"/Users/"));
    let project = crate::structure::read_project(FIXTURE).unwrap();
    let converted =
        crate::structure_document::to_structural_fx_document(&project, Some(2)).unwrap();
    let messages = converted
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.clone())
        .collect();
    (converted.document.to_json_value().unwrap(), messages)
}

fn color_curves(value: &Value) -> Vec<Value> {
    fn walk(value: &Value, found: &mut Vec<Value>) {
        for layer in value["layers"].as_array().into_iter().flatten() {
            for record in layer["effects"].as_array().into_iter().flatten() {
                if record["effect"]["type"] == "colorCurves" {
                    found.push(record["effect"].clone());
                }
            }
            walk(layer, found);
        }
    }
    let mut found = Vec::new();
    walk(&value["composition"], &mut found);
    found
}

#[test]
fn native_ui_curves_fixture_imports_the_rgb_curve_as_the_fx_master_curve() {
    let (value, messages) = imported();
    let found = color_curves(&value);
    assert_eq!(found.len(), 1, "{value}");
    let curves = &found[0]["curves"];
    let identity = serde_json::json!([{"x": 0.0, "y": 0.0}, {"x": 1.0, "y": 1.0}]);
    for channel in ["red", "green", "blue"] {
        assert_eq!(
            curves[channel], identity,
            "{channel} stays identity (not folded)"
        );
    }
    let master = curves["master"].as_array().unwrap();
    assert_eq!(master.len(), FITTED_KNOTS.len(), "{master:?}");
    for (point, (x, y)) in master.iter().zip(FITTED_KNOTS) {
        assert!((point["x"].as_f64().unwrap() - f64::from(x) / 255.0).abs() < 1e-12);
        assert!((point["y"].as_f64().unwrap() - f64::from(y) / 255.0).abs() < 1e-12);
    }
    // Within one code of AE's saved table at every sample.
    let (effect, _) = from_bytes(authored_payload()).unwrap();
    let LayerEffect::ColorCurves { curves: typed, .. } = effect else {
        panic!("ColorCurves")
    };
    for (x, native) in table(authored_payload(), 0).iter().enumerate() {
        let fx = evaluate(&typed.master, x as f64 / 255.0) * 255.0;
        assert!((fx - f64::from(*native)).abs() <= 1.0 + 1e-9, "{x}: {fx}");
    }
    let curves_messages: Vec<_> = messages
        .iter()
        .filter(|message| message.contains("CurvesCustom"))
        .collect();
    assert!(
        curves_messages.iter().any(|message| message.contains(
            "RGB editor points (0,0) (127,162) (255,255) imported as 8 piecewise-linear FX knots"
        ) && message
            .contains("equals AE's natural cubic spline")),
        "{curves_messages:?}"
    );
    assert!(
        curves_messages
            .iter()
            .any(|message| message.contains("8-bit") && message.contains("approx")),
        "{curves_messages:?}"
    );
}

#[test]
fn imported_ui_curve_exports_its_knots_as_editor_points_and_reimports() {
    let (effect, _) = from_bytes(authored_payload()).unwrap();
    let LayerEffect::ColorCurves { curves, .. } = effect else {
        panic!("ColorCurves")
    };
    let (bytes, warnings) = encode(&curves);
    let knots: Vec<editor::Point> = FITTED_KNOTS
        .iter()
        .map(|(x, y)| [u16::from(*x), u16::from(*y)])
        .collect();
    assert_eq!(editor::decode(block(&bytes, 0)).unwrap(), knots);
    for channel in 1..CHANNELS {
        assert_eq!(block(&bytes, channel), editor::encode(&editor::DIAGONAL));
    }
    // The written table is the FX piecewise-linear curve: within one code of AE's.
    assert!(editor::max_deviation(table(&bytes, 0), table(authored_payload(), 0)) <= 1);
    // AE's spline through the written knots stays close to the authored curve.
    let spline = editor::spline_table(&knots);
    let deviation = editor::max_deviation(&spline, table(authored_payload(), 0));
    assert!(deviation <= 2, "{deviation}");
    assert!(
        warnings.iter().any(|warning| warning
            .contains("Curves RGB: the 8 FX knots are written as AE editor points")),
        "{warnings:?}"
    );
    let (effect, _) = from_bytes(&bytes).unwrap();
    let LayerEffect::ColorCurves { curves: back, .. } = effect else {
        panic!("ColorCurves")
    };
    assert!(back.red.is_identity() && back.green.is_identity() && back.blue.is_identity());
    for x in 0..SAMPLES {
        let input = x as f64 / 255.0;
        assert!(
            (evaluate(&back.master, input) - evaluate(&curves.master, input)).abs() * 255.0 <= 1.5
        );
    }
}
