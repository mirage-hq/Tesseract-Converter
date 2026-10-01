//! Explicit edited-FX Adjustment inputs and fresh native-output assertions.
//! Own-reader checks are structural evidence, not independent Adobe acceptance.
use super::*;
use crate::{
    effects::native::{DecodedEffect, read_effects},
    properties::{read_transform, root_runs, runs, unique_list},
};

fn document(source: &str) -> Value {
    serde_json::from_str(source).expect("committed explicit Adjustment FX input")
}

fn fresh_native(source: &str) -> (ExportedDocument, StructuralProject) {
    let value = document(source);
    let output = export(value.clone());
    if let Some(directory) = std::env::var_os("AEP_EFFECTS_COVERAGE_DIR") {
        let name = value["composition"]["name"].as_str().unwrap();
        let path = std::path::Path::new(&directory).join(name);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(path.with_extension("fx.json"), source).unwrap();
        std::fs::write(path.with_extension("aep"), &output.bytes).unwrap();
        let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
        tesseract_file::TesseractFileBuilder::try_new(document)
            .unwrap()
            .write(path.with_extension("tsrct"))
            .unwrap();
    }
    let native = read_project(&output.bytes).expect("fresh native AEP");
    (output, native)
}

fn layer<'a>(native: &'a StructuralProject, name: &str) -> &'a crate::structure::Layer {
    native
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(composition) => Some(composition.layers.as_slice()),
            _ => None,
        })
        .flatten()
        .find(|layer| layer.name.as_ref() == name)
        .unwrap_or_else(|| panic!("missing native layer {name}"))
}

fn names(native: &StructuralProject) -> Vec<&str> {
    layers(native)
        .iter()
        .map(|layer| layer.name.as_ref())
        .collect()
}

fn effects(layer: &crate::structure::Layer) -> Vec<DecodedEffect> {
    let (effects, warnings) = read_effects(&layer.content, [320.0, 180.0]);
    // Native Pro Levels also contains UI-only/custom histogram controls, which
    // the numeric effect reader deliberately reports rather than interpreting.
    const UI_CONTROLS: [&str; 11] = [
        "0002", "0003", "0009", "0010", "0016", "0017", "0023", "0024", "0030", "0031", "0037",
    ];
    assert!(warnings.iter().all(|warning| UI_CONTROLS.iter().any(|suffix| {
        warning == &format!("ADBE Pro Levels2/ADBE Pro Levels2-{suffix}: unsupported or malformed property: unsupported effect default kind")
    })), "unexpected effect warnings: {warnings:?}");
    effects
}

fn effect_value(effect: &DecodedEffect, match_name: &str) -> Vec<f64> {
    effect
        .parameters
        .iter()
        .find(|parameter| parameter.match_name == match_name)
        .unwrap_or_else(|| panic!("missing {match_name} on {}", effect.match_name))
        .numeric
        .as_ref()
        .unwrap_or_else(|error| panic!("non-numeric {match_name}: {error}"))
        .values
        .clone()
}

fn mask_atom_count(layer: &crate::structure::Layer) -> usize {
    root_runs(&layer.content)
        .expect("native layer properties")
        .into_iter()
        .find(|(name, _)| *name == "ADBE Mask Parade")
        .map_or(0, |(_, parade)| {
            let parade = unique_list(parade, *b"tdgp").expect("native mask parade");
            runs(parade)
                .expect("native mask records")
                .into_iter()
                .filter(|(name, _)| *name == "ADBE Mask Atom")
                .count()
        })
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_shared_writer_two_solid_control() {
    let (_, native) = fresh_native(include_str!(
        "../../../tests/fixtures/adjustment/fx_export/writer-two-solid.fx.json"
    ));
    assert_eq!(names(&native), ["Foreground", "Background"]);
    let ItemKind::Composition(root) = &native.item(1).unwrap().kind else {
        panic!("root composition");
    };
    assert_eq!(root.layers.len(), 2);
    for (index, expected) in [[0.5, 0.125, 0.125], [0.25, 0.25, 0.25]]
        .into_iter()
        .enumerate()
    {
        let source = native.item(root.layers[index].record.source_id()).unwrap();
        assert_eq!(
            source.solid.as_ref().unwrap().as_ref().unwrap().color,
            expected
        );
        assert!(!root.layers[index].record.flags().adjustment_layer);
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_export_scope() {
    crate::adobe_test_support::export_case("adjustment-scope", || {
        let (_output, native) = fresh_native(include_str!(
            "../../../tests/fixtures/adjustment/fx_export/adjustment-scope.fx.json"
        ));
        assert_eq!(
            names(&native),
            [
                "upper-sentinel-37x23",
                "scope-adjustment",
                "lower-amber-83x117",
                "lower-cobalt-127x71"
            ]
        );
        let adjustment = layer(&native, "scope-adjustment");
        assert!(adjustment.record.flags().adjustment_layer);
        let effects = effects(adjustment);
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].match_name, "ADBE Gaussian Blur 2");
        assert_eq!(
            effect_value(&effects[0], "ADBE Gaussian Blur 2-0001"),
            [19.0]
        );
    });
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_export_stack_order() {
    crate::adobe_test_support::export_case("adjustment-stack-order", || {
        let (_output, native) = fresh_native(include_str!(
            "../../../tests/fixtures/adjustment/fx_export/adjustment-stack-order.fx.json"
        ));
        assert_eq!(
            names(&native),
            [
                "upper-sentinel-37x23",
                "stack-blur-upper",
                "stack-levels-lower",
                "lower-amber-83x117",
                "lower-cobalt-127x71"
            ]
        );
        let blur = layer(&native, "stack-blur-upper");
        let levels = layer(&native, "stack-levels-lower");
        assert!(blur.record.flags().adjustment_layer);
        assert!(levels.record.flags().adjustment_layer);
        assert_eq!(effects(blur)[0].match_name, "ADBE Gaussian Blur 2");
        assert_eq!(effects(levels)[0].match_name, "ADBE Pro Levels2");
    });
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_export_effect_order() {
    crate::adobe_test_support::export_case("adjustment-effect-order", || {
        let (_output, native) = fresh_native(include_str!(
            "../../../tests/fixtures/adjustment/fx_export/adjustment-effect-order.fx.json"
        ));
        let effects = effects(layer(&native, "effect-order-adjustment"));
        assert_eq!(
            effects
                .iter()
                .map(|effect| effect.match_name.as_str())
                .collect::<Vec<_>>(),
            ["ADBE Gaussian Blur 2", "ADBE Pro Levels2"]
        );
        assert_eq!(
            effect_value(&effects[0], "ADBE Gaussian Blur 2-0001"),
            [21.0]
        );
        assert_eq!(effect_value(&effects[1], "ADBE Pro Levels2-0006"), [2.35]);
    });
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_export_keys() {
    crate::adobe_test_support::export_case("adjustment-keys", || {
        let (_output, native) = fresh_native(include_str!(
            "../../../tests/fixtures/adjustment/fx_export/adjustment-keys.fx.json"
        ));
        let adjustment = layer(&native, "keyed-adjustment-owner-offset");
        assert!(adjustment.record.flags().adjustment_layer);
        assert_eq!(adjustment.record.start_time(), Some(0.25));
        assert_eq!(adjustment.record.in_point(), Some(0.0));
        assert_eq!(adjustment.record.out_point(), Some(1.5));
        let effects = effects(adjustment);
        let blur = effects[0]
            .parameters
            .iter()
            .find(|parameter| parameter.match_name == "ADBE Gaussian Blur 2-0001")
            .expect("native blur control")
            .numeric
            .as_ref()
            .expect("native keyed blur");
        assert_eq!(
            blur.keyframes
                .iter()
                .map(|key| key.values[0])
                .collect::<Vec<_>>(),
            [4.0, 31.0]
        );
        assert_eq!(
            blur.keyframes
                .iter()
                .map(|key| key.time_secs + 0.25)
                .collect::<Vec<_>>(),
            [0.5, 1.25]
        );
        let opacity = read_transform(&adjustment.content)
            .expect("native adjustment transform")
            .into_iter()
            .find(|property| property.match_name == "ADBE Opacity")
            .expect("native opacity")
            .numeric
            .expect("native keyed opacity");
        assert_eq!(
            opacity
                .keyframes
                .iter()
                .map(|key| key.values[0])
                .collect::<Vec<_>>(),
            [0.28, 0.86]
        );
        assert_eq!(
            opacity
                .keyframes
                .iter()
                .map(|key| key.time_secs + 0.25)
                .collect::<Vec<_>>(),
            [0.75, 1.5]
        );
    });
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_export_disabled() {
    crate::adobe_test_support::export_case("adjustment-disabled", || {
        let (_output, native) = fresh_native(include_str!(
            "../../../tests/fixtures/adjustment/fx_export/adjustment-disabled.fx.json"
        ));
        assert_eq!(names(&native).len(), 5);
        let empty = layer(&native, "enabled-empty-adjustment");
        let disabled = layer(&native, "disabled-adjustment-with-effect");
        assert!(empty.record.flags().adjustment_layer && empty.record.flags().enabled);
        assert!(effects(empty).is_empty());
        assert!(disabled.record.flags().adjustment_layer);
        assert!(!disabled.record.flags().enabled);
        let effects = effects(disabled);
        assert_eq!(effects[0].match_name, "ADBE Gaussian Blur 2");
        assert_eq!(
            effect_value(&effects[0], "ADBE Gaussian Blur 2-0001"),
            [27.0]
        );
        assert!(layer(&native, "lower-amber-83x117").record.flags().enabled);
        assert!(layer(&native, "lower-cobalt-127x71").record.flags().enabled);
    });
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_export_span() {
    crate::adobe_test_support::export_case("adjustment-span", || {
        let (_output, native) = fresh_native(include_str!(
            "../../../tests/fixtures/adjustment/fx_export/adjustment-span.fx.json"
        ));
        let adjustment = layer(&native, "finite-span-adjustment");
        assert!(adjustment.record.flags().adjustment_layer);
        assert_eq!(adjustment.record.start_time(), Some(0.5));
        assert_eq!(adjustment.record.in_point(), Some(0.0));
        assert_eq!(adjustment.record.out_point(), Some(1.0));
        assert_eq!(
            names(&native)[2..],
            ["lower-amber-83x117", "lower-cobalt-127x71"]
        );
    });
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_export_nested() {
    crate::adobe_test_support::export_case("adjustment-nested", || {
        let (_output, native) = fresh_native(include_str!(
            "../../../tests/fixtures/adjustment/fx_export/adjustment-nested.fx.json"
        ));
        assert_eq!(
            names(&native),
            [
                "upper-sentinel-37x23",
                "linked-nested-support",
                "outer-lower-sibling-91x43"
            ]
        );
        // Independent Adobe source stores precomposition anchors in source-relative
        // units. The previous writer encoded pixels, which Adobe multiplied again.
        use sha2::{Digest, Sha256};
        let source = include_bytes!("../../../tests/fixtures/adjustment/native_controls.aep");
        assert_eq!(
            format!("{:x}", Sha256::digest(source)),
            "ed45319ed014ba979a9e0b4868aa635775f288fe851c39d9242aab097020f26f"
        );
        let oracle = read_project(source).unwrap();
        let ItemKind::Composition(oracle_comp) = &oracle.item(146).unwrap().kind else {
            panic!("pinned nested composition");
        };
        let oracle_layer = oracle_comp
            .layers
            .iter()
            .find(|layer| layer.record.source_id() == 126)
            .unwrap();
        let anchor = |layer: &crate::structure::Layer| {
            crate::properties::read_static_source_relative_anchor(&layer.content)
                .unwrap()
                .unwrap()
        };
        // Adobe omits its untouched center-anchor record; its independent UI
        // readback pins that implicit default, rather than using our reader as oracle.
        assert!(
            crate::properties::read_static_source_relative_anchor(&oracle_layer.content)
                .unwrap()
                .is_none()
        );
        let controls: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/adjustment/native-readback.json"
        ))
        .unwrap();
        let comp = controls["compositions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|comp| comp["id"] == 146)
            .unwrap();
        let occurrence = comp["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["source"]["id"] == 126)
            .unwrap();
        let native_anchor = occurrence["transform"]
            .as_array()
            .unwrap()
            .iter()
            .find(|property| property["matchName"] == "ADBE Anchor Point")
            .unwrap();
        assert_eq!(native_anchor["value"], json!([160, 90, 0]));
        assert_eq!(anchor(layer(&native, "linked-nested-support")), [0.5, 0.5]);
        let adjustment = layer(&native, "nested-local-adjustment");
        assert!(adjustment.record.flags().adjustment_layer);
        assert_eq!(effects(adjustment)[0].match_name, "ADBE Gaussian Blur 2");
        let nested = native
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(composition)
                    if composition
                        .layers
                        .iter()
                        .any(|layer| layer.name.as_ref() == "nested-local-adjustment") =>
                {
                    Some(composition)
                }
                _ => None,
            })
            .expect("editable nested support composition");
        assert_eq!(
            nested
                .layers
                .iter()
                .map(|layer| layer.name.as_ref())
                .collect::<Vec<_>>(),
            [
                "upper-sentinel-37x23",
                "nested-local-adjustment",
                "lower-amber-83x117",
                "lower-cobalt-127x71"
            ]
        );
    });
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_export_masks() {
    crate::adobe_test_support::export_case("adjustment-masks", || {
        let (_output, native) = fresh_native(include_str!(
            "../../../tests/fixtures/adjustment/fx_export/adjustment-masks.fx.json"
        ));
        let adjustment = layer(&native, "masked-adjustment");
        assert!(adjustment.record.flags().adjustment_layer);
        assert_eq!(mask_atom_count(adjustment), 1);
        assert_eq!(effects(adjustment)[0].match_name, "ADBE Pro Levels2");
        assert_eq!(
            names(&native),
            [
                "upper-sentinel-37x23",
                "masked-adjustment",
                "lower-amber-83x117",
                "lower-cobalt-127x71"
            ]
        );
    });
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_export_matte() {
    crate::adobe_test_support::export_case("adjustment-matte", || {
        let (_output, native) = fresh_native(include_str!(
            "../../../tests/fixtures/adjustment/fx_export/adjustment-matte.fx.json"
        ));
        let provider = layer(&native, "independent-alpha-matte-provider-119x67");
        let adjustment = layer(&native, "matted-adjustment");
        assert!(adjustment.record.flags().adjustment_layer);
        assert_eq!(adjustment.record.track_matte_type(), 2);
        assert_eq!(
            adjustment.record.matte_layer_id(),
            Some(provider.record.id())
        );
        assert!(!provider.record.flags().enabled);
        assert_eq!(
            names(&native)[3..],
            ["lower-amber-83x117", "lower-cobalt-127x71"]
        );
    });
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn adjustment_export_parent_normalized_gate() {
    crate::adobe_test_support::export_case("adjustment-parent", || {
        let (_output, native) = fresh_native(include_str!(
            "../../../tests/fixtures/adjustment/fx_export/adjustment-parent.fx.json"
        ));
        let adjustment = layer(&native, "parented-masked-adjustment");
        assert!(adjustment.record.flags().adjustment_layer);
        assert_eq!(
            adjustment.record.parent_id(),
            0,
            "normalized FX gate must not recreate stale source parenting"
        );
        assert_eq!(mask_atom_count(adjustment), 1);
        assert_eq!(effects(adjustment)[0].match_name, "ADBE Pro Levels2");
        assert_eq!(
            names(&native),
            [
                "upper-sentinel-37x23",
                "parented-masked-adjustment",
                "lower-amber-83x117",
                "lower-cobalt-127x71"
            ]
        );
    });
}
