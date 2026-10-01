//! Public-derived same-effect alias regressions for the pinned native Mosaic.
//!
//! Sources: `effects_coverage/native_animated_controls.aep` composition 326 and
//! `native_static_controls.aep` composition 339 (Adobe-authored). The expressions
//! and renames are converter-test derivations (see `mosaic_derived`), modeled on
//! the observed `effect("Mosaic")("Horizontal Blocks")` Vertical Blocks link
//! whose instance keeps AE's placeholder display name and plugin name "Mosaic".

use serde_json::Value;

use super::mosaic_derived::{
    ANIMATED_CONTROLS, ANIMATED_MOSAIC, HORIZONTAL, STATIC_CONTROLS, STATIC_MOSAIC, VERTICAL,
    VERTICAL_ALIAS, rename_mosaic, set_expression,
};
use super::*;

struct Imported {
    json: Value,
    diagnostics: Vec<String>,
}

fn import(source: &[u8], composition: u32, edit: impl FnOnce(&mut Layer)) -> Imported {
    let mut project = read_project(source).expect("pinned native Mosaic source");
    edit(&mut composition_mut(&mut project, composition).layers[0]);
    let converted =
        to_structural_fx_document(&project, Some(composition)).expect("derived Mosaic import");
    Imported {
        json: converted
            .document
            .to_json_value()
            .expect("editable FX JSON"),
        diagnostics: converted
            .diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.message)
            .collect(),
    }
}

fn mosaic(imported: &Imported) -> Value {
    fn find(layers: &[Value]) -> Option<Value> {
        layers.iter().find_map(|layer| {
            layer["effects"]
                .as_array()
                .and_then(|effects| {
                    effects
                        .iter()
                        .find(|effect| effect["effect"]["type"] == "mosaic")
                        .cloned()
                })
                .or_else(|| find(layer["layers"].as_array().map_or(&[], Vec::as_slice)))
        })
    }
    find(imported.json["composition"]["layers"].as_array().unwrap())
        .expect("editable Mosaic effect")
}

/// One editable key: `(layerTime, value, easing)`.
type Key = (i64, f64, String);

/// The editable keys of one Mosaic parameter track, with their key ids.
struct Track {
    keys: Vec<Key>,
    ids: Vec<String>,
}

fn track(imported: &Imported, effect: &Value, param: &str) -> Option<Track> {
    let entries = imported.json["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    let matches: Vec<_> = entries
        .iter()
        .filter(|entry| {
            entry["target"]["kind"] == "effectProperty"
                && entry["target"]["effectId"] == effect["id"]
                && entry["target"]["paramName"] == param
        })
        .collect();
    assert!(matches.len() <= 1, "{param}: one editable track at most");
    let keys = matches.first()?["animator"]["keyframes"]
        .as_array()
        .unwrap();
    Some(Track {
        keys: keys
            .iter()
            .map(|key| {
                (
                    key["layerTime"].as_i64().unwrap(),
                    key["value"]["value"].as_f64().unwrap(),
                    key["easing"]["type"].as_str().unwrap().to_owned(),
                )
            })
            .collect(),
        ids: keys
            .iter()
            .map(|key| key["id"].as_str().unwrap().to_owned())
            .collect(),
    })
}

fn native_horizontal_keys() -> Vec<Key> {
    vec![(0, 12.0, "linear".into()), (1000, 24.0, "linear".into())]
}

fn lowered(imported: &Imported) -> bool {
    imported.diagnostics.iter().any(|message| {
        message.contains(VERTICAL)
            && message.contains(&format!("same-effect alias of {HORIZONTAL} lowered"))
    })
}

#[test]
fn derived_vertical_alias_follows_the_native_horizontal_curve_not_its_own_keys() {
    let imported = import(ANIMATED_CONTROLS, ANIMATED_MOSAIC, |layer| {
        set_expression(layer, VERTICAL, VERTICAL_ALIAS, true);
    });
    let effect = mosaic(&imported);
    // The native Vertical keys are 8→16; the enabled expression replaces them.
    assert_eq!(effect["effect"]["verticalBlocks"], 12.0, "{effect}");
    assert_eq!(effect["effect"]["horizontalBlocks"], 12.0, "{effect}");
    let horizontal =
        track(&imported, &effect, "horizontalBlocks").expect("native Horizontal track");
    let vertical =
        track(&imported, &effect, "verticalBlocks").expect("lowered Vertical alias track");
    assert_eq!(horizontal.keys, native_horizontal_keys());
    assert_eq!(
        vertical.keys,
        native_horizontal_keys(),
        "same normalized keys and easing"
    );
    assert!(
        vertical.ids.iter().all(|id| !horizontal.ids.contains(id)),
        "independent editable keys, not a shared/live link: {:?}",
        vertical.ids
    );
    assert!(lowered(&imported), "{:#?}", imported.diagnostics);
    assert!(
        !imported
            .diagnostics
            .iter()
            .any(|message| message.contains(&format!("{VERTICAL}: AE expression not executed"))),
        "stale cached value must not remain the reported result: {:#?}",
        imported.diagnostics
    );
    assert!(!imported.json.to_string().contains("JsScript"));
}

#[test]
fn derived_static_vertical_alias_uses_the_referenced_static_value() {
    let imported = import(STATIC_CONTROLS, STATIC_MOSAIC, |layer| {
        set_expression(layer, VERTICAL, VERTICAL_ALIAS, true);
    });
    let effect = mosaic(&imported);
    assert_eq!(effect["effect"]["verticalBlocks"], 12.0, "{effect}");
    assert_eq!(effect["effect"]["horizontalBlocks"], 12.0, "{effect}");
    assert!(track(&imported, &effect, "verticalBlocks").is_none());
    assert!(lowered(&imported), "{:#?}", imported.diagnostics);
}

#[test]
fn derived_parameter_match_name_reference_is_the_same_identity() {
    let imported = import(ANIMATED_CONTROLS, ANIMATED_MOSAIC, |layer| {
        set_expression(
            layer,
            VERTICAL,
            r#" effect( 'Mosaic' )( 'ADBE Mosaic-0001' ) ; "#,
            true,
        );
    });
    let effect = mosaic(&imported);
    let vertical = track(&imported, &effect, "verticalBlocks").expect("lowered alias");
    assert_eq!(vertical.keys, native_horizontal_keys());
}

#[test]
fn derived_disabled_alias_keeps_the_native_vertical_keys() {
    let imported = import(ANIMATED_CONTROLS, ANIMATED_MOSAIC, |layer| {
        set_expression(layer, VERTICAL, VERTICAL_ALIAS, false);
    });
    let effect = mosaic(&imported);
    assert_eq!(effect["effect"]["verticalBlocks"], 8.0, "{effect}");
    let vertical = track(&imported, &effect, "verticalBlocks").expect("own Vertical keys");
    assert_eq!(
        vertical.keys,
        vec![(0, 8.0, "linear".into()), (1000, 16.0, "linear".into())]
    );
    assert!(!lowered(&imported));
}

#[test]
fn derived_explicit_rename_wins_over_the_plugin_name() {
    // AE resolves effect("…") by the instance's current name. A renamed Mosaic
    // is not the target of effect("Mosaic"), even though its plugin name is.
    let stale = import(ANIMATED_CONTROLS, ANIMATED_MOSAIC, |layer| {
        rename_mosaic(layer, "Pixelate");
        set_expression(layer, VERTICAL, VERTICAL_ALIAS, true);
    });
    let effect = mosaic(&stale);
    assert_eq!(
        effect["effect"]["verticalBlocks"], 8.0,
        "fallback keeps authored initial value"
    );
    assert!(track(&stale, &effect, "verticalBlocks").is_none());
    assert!(!lowered(&stale));
    assert!(
        stale
            .diagnostics
            .iter()
            .any(|message| message.contains(VERTICAL)
                && message.contains("same-effect alias not lowered")),
        "{:#?}",
        stale.diagnostics
    );

    let renamed = import(ANIMATED_CONTROLS, ANIMATED_MOSAIC, |layer| {
        rename_mosaic(layer, "Pixelate");
        set_expression(
            layer,
            VERTICAL,
            r#"effect("Pixelate")("Horizontal Blocks")"#,
            true,
        );
    });
    let effect = mosaic(&renamed);
    let vertical = track(&renamed, &effect, "verticalBlocks").expect("renamed alias");
    assert_eq!(vertical.keys, native_horizontal_keys());
}

#[test]
fn derived_cycles_and_incomplete_expressions_keep_the_existing_fallback() {
    for (vertical, horizontal, reason) in [
        (
            r#"effect("Mosaic")("Vertical Blocks")"#,
            None,
            Some("cyclic"),
        ),
        (
            VERTICAL_ALIAS,
            Some(r#"effect("Mosaic")("Vertical Blocks")"#),
            Some("cyclic"),
        ),
        (r#"effect("Mosaic")("Horizontal Blocks") * 2"#, None, None),
        (
            r#"effect("Mosaic")("Horizontal Blocks").valueAtTime(0)"#,
            None,
            None,
        ),
        (
            r#"thisLayer.effect("Mosaic")("Horizontal Blocks")"#,
            None,
            None,
        ),
        (
            r#"effect("Mosaic")("Missing Control")"#,
            None,
            Some("not found"),
        ),
    ] {
        let imported = import(ANIMATED_CONTROLS, ANIMATED_MOSAIC, |layer| {
            set_expression(layer, VERTICAL, vertical, true);
            if let Some(horizontal) = horizontal {
                set_expression(layer, HORIZONTAL, horizontal, true);
            }
        });
        let effect = mosaic(&imported);
        assert_eq!(
            effect["effect"]["verticalBlocks"], 8.0,
            "{vertical}: {effect}"
        );
        assert!(
            track(&imported, &effect, "verticalBlocks").is_none(),
            "{vertical}"
        );
        assert!(
            !lowered(&imported),
            "{vertical}: {:#?}",
            imported.diagnostics
        );
        assert!(
            imported
                .diagnostics
                .iter()
                .any(|message| message.contains(&format!("{VERTICAL}: AE expression not executed"))),
            "{vertical}: existing contextual fallback: {:#?}",
            imported.diagnostics
        );
        if let Some(reason) = reason {
            assert!(
                imported
                    .diagnostics
                    .iter()
                    .any(|message| message.contains(VERTICAL)
                        && message.contains("same-effect alias not lowered")
                        && message.contains(reason)),
                "{vertical}: {reason}: {:#?}",
                imported.diagnostics
            );
        }
    }
}
