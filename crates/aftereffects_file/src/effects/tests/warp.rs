//! Independent static source plus explicitly edited native exports; not pixel proof.
use super::*;
use serde_json::json;
use sha2::{Digest, Sha256};

const SOURCE: &[u8] = include_bytes!("../../../tests/fixtures/effects/warp_static.aep");

fn source() -> StructuralProject {
    assert_eq!(
        format!("{:x}", Sha256::digest(SOURCE)),
        "533103d97b25dfec382cfc2e30bebb2a5d0a1785449f15d3012e668cefa7d25d"
    );
    read_project(SOURCE).unwrap()
}

fn native_target(project: &StructuralProject, id: u32) -> native::DecodedEffect {
    let ItemKind::Composition(composition) = &project.item(id).unwrap().kind else {
        panic!("native composition")
    };
    composition
        .layers
        .iter()
        .flat_map(|layer| native::read_effects(&layer.content, [320.0, 180.0]).0)
        .next()
        .unwrap()
}

#[test]
fn warp_static_spherize_native_import_is_editable_bulge() {
    let project = source();
    let native = native_target(&project, 14);
    assert_eq!(native.match_name, "ADBE Spherize");
    assert_eq!(
        native
            .parameters
            .iter()
            .find(|p| p.match_name.ends_with("-0001"))
            .unwrap()
            .numeric
            .as_ref()
            .unwrap()
            .values,
        [70.0]
    );
    let (document, warnings) = imported_case(&project, 14);
    let effect = &effect_payload(&document, "bulge").expect("Spherize retained")["effect"];
    for (field, expected) in [
        ("centerX", 144.0 / 320.0),
        ("centerY", 84.0 / 180.0),
        ("horizontalRadius", 70.0 / 320.0),
        ("verticalRadius", 70.0 / 180.0),
        ("bulgeHeight", std::f64::consts::PI - 2.0),
    ] {
        close(&[effect[field].as_f64().unwrap()], &[expected], field);
    }
    assert!(
        warnings.iter().any(|w| w.contains("Spherize approximated")),
        "{warnings:?}"
    );
    assert!(!document.to_string().contains("JsScript"));
}

#[test]
fn warp_static_spherize_rejects_unsafe_geometry_and_handles_zero_radius() {
    let original = native_target(&source(), 14);
    for radius in [-1.0, 180.0, f64::INFINITY, f64::NAN] {
        let mut native = original.clone();
        native
            .parameters
            .iter_mut()
            .find(|p| p.match_name.ends_with("-0001"))
            .unwrap()
            .numeric
            .as_mut()
            .unwrap()
            .values = vec![radius];
        assert!(crate::effects::special::validate_spherize(&native, [320, 180]).is_err());
    }
    let mut native = original;
    native
        .parameters
        .iter_mut()
        .find(|p| p.match_name.ends_with("-0002"))
        .unwrap()
        .numeric
        .as_mut()
        .unwrap()
        .expression_present = true;
    assert!(
        crate::effects::special::validate_spherize(&native, [320, 180]).is_ok(),
        "disabled stored expression retains static controls"
    );

    let radius = native
        .parameters
        .iter_mut()
        .find(|p| p.match_name.ends_with("-0001"))
        .unwrap()
        .numeric
        .as_mut()
        .unwrap();
    radius.values = vec![0.0];
    assert!(crate::effects::special::validate_spherize(&native, [320, 180]).is_ok());
    let mut payload = crate::effects::mapping::default_effect("bulge");
    crate::effects::special::import(&native, [320.0, 180.0], &mut payload);
    assert_eq!(payload["bulgeHeight"], 0.0);
    assert_eq!(payload["horizontalRadius"], 0.5);
    native
        .parameters
        .iter_mut()
        .find(|p| p.match_name.ends_with("-0002"))
        .unwrap()
        .numeric
        .as_mut()
        .unwrap()
        .expression_enabled = true;
    assert!(crate::effects::special::validate_spherize(&native, [320, 180]).is_err());
}

#[test]
fn warp_static_wave_native_phase_and_current_edited_export_have_no_implicit_clock() {
    let project = source();
    let native = native_target(&project, 27);
    assert_eq!(native.match_name, "ADBE Wave Warp");
    assert_eq!(
        native
            .parameters
            .iter()
            .find(|p| p.match_name.ends_with("-0005"))
            .unwrap()
            .numeric
            .as_ref()
            .unwrap()
            .values,
        [0.0]
    );
    let (mut document, warnings) = imported_case(&project, 27);
    let effect = &effect_payload(&document, "waveWarp").unwrap()["effect"];
    for (field, expected) in [
        ("waveHeight", 8.0 / 180.0),
        ("waveWidth", 6.4),
        ("direction", 90.0),
        ("phase", std::f64::consts::PI / 6.0),
    ] {
        close(&[effect[field].as_f64().unwrap()], &[expected], field);
    }
    assert!(
        !warnings.iter().any(|w| w.contains("ADBE Wave Warp-0005")),
        "{warnings:?}"
    );
    fn edit(node: &mut Value) {
        if let Some(effects) = node.get_mut("effects").and_then(Value::as_array_mut) {
            for record in effects {
                if record["effect"]["type"] == "waveWarp" {
                    record["effect"]["phase"] = json!(std::f64::consts::PI / 2.0);
                    record["effect"]["waveHeight"] = json!(0.1);
                    record["effect"]["waveWidth"] = json!(8.0);
                }
            }
        }
        if let Some(composition) = node.get_mut("composition") {
            edit(composition);
        }
        if let Some(layers) = node.get_mut("layers").and_then(Value::as_array_mut) {
            for layer in layers {
                edit(layer);
            }
        }
    }
    edit(&mut document);
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(document).unwrap();
    let output = crate::export_document::to_aep(&document).unwrap();
    let exported = read_project(&output.bytes).unwrap();
    let effects: Vec<_> = exported
        .items
        .iter()
        .flat_map(|item| match &item.kind {
            ItemKind::Composition(comp) => comp
                .layers
                .iter()
                .flat_map(|layer| native::read_effects(&layer.content, [320.0, 180.0]).0)
                .collect(),
            _ => Vec::new(),
        })
        .filter(|effect| effect.match_name == "ADBE Wave Warp")
        .collect();
    assert_eq!(effects.len(), 1);
    for (suffix, expected) in [
        ("-0002", 18.0),
        ("-0003", 40.0),
        ("-0005", 0.0),
        ("-0007", 90.0),
    ] {
        let numeric = effects[0]
            .parameters
            .iter()
            .find(|p| p.match_name.ends_with(suffix))
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        close(&numeric.values, &[expected], suffix);
    }
}

fn visit(node: &mut Value, action: &mut impl FnMut(&mut Value)) {
    action(node);
    if let Some(comp) = node.get_mut("composition") {
        visit(comp, action);
    }
    if let Some(layers) = node.get_mut("layers").and_then(Value::as_array_mut) {
        for layer in layers {
            visit(layer, action);
        }
    }
}

#[test]
fn warp_static_spherize_current_signed_bulge_export() {
    let (mut document, _) = imported_case(&source(), 14);
    visit(&mut document, &mut |node| {
        if let Some(effects) = node.get_mut("effects").and_then(Value::as_array_mut) {
            for effect in effects {
                if effect["effect"]["type"] == "bulge" {
                    effect["enabled"] = json!(false);
                    for (field, value) in [
                        ("bulgeHeight", -0.75),
                        ("horizontalRadius", 0.2),
                        ("verticalRadius", 0.25),
                        ("centerX", 0.3),
                        ("centerY", 0.6),
                    ] {
                        effect["effect"][field] = json!(value);
                    }
                }
            }
        }
    });
    let output = crate::export_document::to_aep(
        &fx_schema::EditableFxCompositionDocument::from_json_value(document).unwrap(),
    )
    .unwrap();
    let project = read_project(&output.bytes).unwrap();
    let effects: Vec<_> = project
        .items
        .iter()
        .flat_map(|item| match &item.kind {
            ItemKind::Composition(comp) => comp
                .layers
                .iter()
                .flat_map(|layer| native::read_effects(&layer.content, [320., 180.]).0)
                .collect(),
            _ => Vec::new(),
        })
        .filter(|effect| effect.match_name == "ADBE Bulge")
        .collect();
    assert_eq!(effects.len(), 1);
    assert!(!effects[0].enabled);
    for (suffix, expected) in [
        ("-0001", vec![64.]),
        ("-0002", vec![45.]),
        ("-0003", vec![96., 108.]),
        ("-0004", vec![-0.75]),
    ] {
        let p = effects[0]
            .parameters
            .iter()
            .find(|p| p.match_name.ends_with(suffix))
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        close(&p.values, &expected, suffix);
    }
}

#[test]
fn warp_static_mirror_native_half_plane_and_finite_support() {
    let (mut document, warnings) = imported_case(&source(), 1);
    assert!(
        warnings.iter().any(|w| w.contains("Mirror approximated")),
        "{warnings:?}"
    );
    let mut reflected = 0;
    let mut retained = 0;
    let mut guides = 0;
    let mut ids = std::collections::HashSet::new();
    visit(&mut document, &mut |node| {
        if let Some(id) = node.get("id").and_then(Value::as_u64) {
            assert!(ids.insert(id), "unique copied layer identities");
        }
        match node["name"].as_str() {
            Some("Mirror reflected half") => {
                reflected += 1;
                assert_eq!(node["transform"]["anchorPoint"], json!([144., 84.]));
                assert_eq!(node["transform"]["position"], json!([144., 84.]));
                assert_eq!(node["transform"]["scale"], json!([-100., 100.]));
                assert_eq!(node["transform"]["rotation"], 60.);
                assert_eq!(node["masks"].as_array().unwrap().len(), 1);
            }
            Some("Mirror retained source half") => {
                retained += 1;
                assert_eq!(node["masks"].as_array().unwrap().len(), 1);
            }
            Some("Mirror finite canvas guide") => {
                guides += 1;
            }
            _ => {}
        }
    });
    assert_eq!((reflected, retained, guides), (1, 1, 1));
    // Native2334 independently observes red at (65,105) and its reflection
    // near (165.3,162.9), while green on the discarded half disappears.
    let n = [30_f64.to_radians().cos(), 30_f64.to_radians().sin()];
    let dot = n[0] * (65. - 144.) + n[1] * (105. - 84.);
    assert!(dot < 0.);
    close(
        &[65. - 2. * n[0] * dot, 105. - 2. * n[1] * dot],
        &[165.3134665, 162.9160069],
        "native reflected marker",
    );
    assert!(!document.to_string().contains("JsScript"));
}

#[test]
fn warp_native_invalid_wavelength_is_rejected_not_replaced_by_default() {
    let original = native_target(&source(), 27);
    for invalid in [0., -1., f64::INFINITY] {
        let mut effect = original.clone();
        effect
            .parameters
            .iter_mut()
            .find(|p| p.match_name.ends_with("-0003"))
            .unwrap()
            .numeric
            .as_mut()
            .unwrap()
            .values = vec![invalid];
        assert!(crate::effects::special::validate_import(&effect).is_err());
    }
}

#[test]
fn warp_mirror_order_bypass_and_unsafe_control_preserve_owner_and_blurs() {
    fn occurrence(parade: &[crate::rifx::Chunk], name: &str) -> std::ops::Range<usize> {
        let start = parade
            .iter()
            .position(|c| c.id() == *b"tdmn" && chunk_match_name(c) == Some(name))
            .unwrap();
        let end = parade[start + 1..]
            .iter()
            .position(|c| c.id() == *b"tdmn")
            .map_or(parade.len(), |offset| start + 1 + offset);
        start..end
    }
    let catalog = read_project(CATALOG).unwrap();
    let ItemKind::Composition(comp) = &catalog.item(1).unwrap().kind else {
        panic!("catalog comp")
    };
    let mut donor = comp.layers[0].clone();
    let parade = named_group_mut(&mut donor.content, "ADBE Effect Parade").unwrap();
    let gaussian = parade[occurrence(parade, "ADBE Gaussian Blur 2")].to_vec();
    for mode in [
        "enabled",
        "default-options",
        "zero-opacity",
        "partial-opacity",
        "effect-mask",
        "bypass",
        "unsafe",
    ] {
        let mut project = source();
        let ItemKind::Composition(comp) =
            &mut project.items.iter_mut().find(|i| i.id == 1).unwrap().kind
        else {
            panic!("Mirror comp")
        };
        let owner = comp
            .layers
            .iter_mut()
            .find(|l| l.record.id() == 13)
            .unwrap();
        let parade = named_group_mut(&mut owner.content, "ADBE Effect Parade").unwrap();
        let span = occurrence(parade, "ADBE Mirror");
        let descriptor = parade[span.clone()]
            .iter_mut()
            .find(|c| c.list_kind() == Some(*b"sspc"))
            .unwrap()
            .children_mut()
            .unwrap();
        let body = descriptor
            .iter_mut()
            .find(|c| c.list_kind() == Some(*b"tdgp"))
            .unwrap()
            .children_mut()
            .unwrap();
        if matches!(
            mode,
            "default-options" | "zero-opacity" | "partial-opacity" | "effect-mask"
        ) {
            use crate::rifx::Chunk;
            let name = |name: &str| {
                // Native tdmn records are exactly 40 bytes, including padding.
                let mut bytes = vec![0; 40];
                bytes[..name.len()].copy_from_slice(name.as_bytes());
                Chunk::data(*b"tdmn", bytes).unwrap()
            };
            let angle = occurrence(body, "ADBE Mirror-0002");
            let mut scalar = body[angle]
                .iter()
                .find(|c| c.list_kind() == Some(*b"tdbs"))
                .unwrap()
                .clone();
            let options = named_group_mut(body, "ADBE Effect Built In Params").unwrap();
            // Synthetic variants of independently read control identities; the
            // source file is never changed. Default has an empty mask parade.
            options.clear();
            if matches!(mode, "zero-opacity" | "partial-opacity") {
                let value = if mode == "zero-opacity" { 0_f64 } else { 50. };
                let leaf = scalar.children_mut().unwrap();
                let cdat = leaf.iter_mut().find(|c| c.id() == *b"cdat").unwrap();
                *cdat = Chunk::data(*b"cdat", value.to_be_bytes().to_vec()).unwrap();
                options.extend([name("ADBE Effect Mask Opacity"), scalar]);
            } else {
                let masks = if mode == "effect-mask" {
                    vec![
                        name("Unrepresented mask reference"),
                        Chunk::list(*b"tdgp", vec![]),
                    ]
                } else {
                    vec![]
                };
                options.extend([
                    name("ADBE Effect Mask Parade"),
                    Chunk::list(*b"tdgp", masks),
                ]);
            }
        }
        if mode == "bypass" {
            let flag = body.iter_mut().find(|c| c.id() == *b"tdsb").unwrap();
            let mut bytes = flag.data_payload().unwrap().to_vec();
            bytes[3] &= !1;
            *flag = crate::rifx::Chunk::data(*b"tdsb", bytes).unwrap();
        } else if mode == "unsafe" {
            let angle = occurrence(body, "ADBE Mirror-0002");
            let leaf = body[angle]
                .iter_mut()
                .find(|c| c.list_kind() == Some(*b"tdbs"))
                .unwrap()
                .children_mut()
                .unwrap();
            let value = leaf.iter_mut().find(|c| c.id() == *b"cdat").unwrap();
            let mut bytes = value.data_payload().unwrap().to_vec();
            bytes[..8].copy_from_slice(&f64::NAN.to_be_bytes());
            *value = crate::rifx::Chunk::data(*b"cdat", bytes).unwrap();
        }
        parade.splice(span.end..span.end, gaussian.clone());
        parade.splice(span.start..span.start, gaussian.clone());
        let (mut document, warnings) = imported_case(&project, 1);
        assert!(
            !warnings.iter().any(|w| w.contains("match-name length")),
            "{mode} must exercise the intended compositing control: {warnings:?}"
        );
        if mode == "zero-opacity" {
            assert!(
                !warnings.iter().any(|w| w.contains("Mirror not lowered")),
                "zero opacity must reach supported identity, not malformed decline: {warnings:?}"
            );
        }
        let mut blurs = 0;
        let mut input_blurs = 0;
        let mut reflected = 0;
        visit(&mut document, &mut |node| {
            let count = node
                .get("effects")
                .and_then(Value::as_array)
                .map_or(0, |effects| {
                    effects
                        .iter()
                        .filter(|e| e["effect"]["type"] == "gaussianBlur")
                        .count()
                });
            blurs += count;
            if node["name"] == "Source before Mirror" {
                input_blurs += count;
            }
            if node["name"] == "Mirror reflected half" {
                reflected += 1;
            }
        });
        if matches!(mode, "enabled" | "default-options") {
            assert_eq!(
                (blurs, input_blurs, reflected),
                (3, 2, 1),
                "prefix twice, suffix once: {warnings:?}"
            );
        } else {
            assert_eq!(
                (blurs, reflected),
                (2, 0),
                "owner/siblings retained: {warnings:?}"
            );
            if matches!(mode, "partial-opacity" | "effect-mask") {
                let control = if mode == "partial-opacity" {
                    "Effect Opacity"
                } else {
                    "Effect Masks"
                };
                assert!(
                    warnings
                        .iter()
                        .any(|w| w.contains("Mirror not lowered") && w.contains(control)),
                    "{warnings:?}"
                );
            }
            if mode == "unsafe" {
                assert!(
                    warnings.iter().any(|w| w.contains("Mirror not lowered")),
                    "{warnings:?}"
                );
            }
        }
    }
}

#[test]
fn warp_spherize_disabled_expression_import_keeps_static_owner() {
    fn expression(chunks: &mut [crate::rifx::Chunk], enabled: bool) -> bool {
        if let Some(index) = chunks
            .iter()
            .position(|c| c.id() == *b"tdmn" && chunk_match_name(c) == Some("ADBE Spherize-0001"))
        {
            let end = chunks[index + 1..]
                .iter()
                .position(|c| c.id() == *b"tdmn")
                .map_or(chunks.len(), |offset| index + 1 + offset);
            // parT declarations repeat tdmn but own pard, not numeric metadata.
            let Some(leaf) = chunks[index + 1..end]
                .iter_mut()
                .find(|c| c.list_kind() == Some(*b"tdbs"))
                .and_then(crate::rifx::Chunk::children_mut)
            else {
                return false;
            };
            let meta = leaf.iter_mut().find(|c| c.id() == *b"tdb4").unwrap();
            let mut bytes = meta.data_payload().unwrap().to_vec();
            bytes[120] |= 1;
            if enabled {
                bytes[119] &= !1;
            } else {
                bytes[119] |= 1;
            }
            *meta = crate::rifx::Chunk::data(*b"tdb4", bytes).unwrap();
            return true;
        }
        chunks.iter_mut().any(|c| {
            c.children_mut()
                .is_some_and(|children| expression(children, enabled))
        })
    }
    for enabled in [false, true] {
        let mut project = source();
        let ItemKind::Composition(comp) =
            &mut project.items.iter_mut().find(|i| i.id == 14).unwrap().kind
        else {
            panic!("Spherize comp")
        };
        let owner = comp
            .layers
            .iter_mut()
            .find(|l| l.record.id() == 26)
            .unwrap();
        assert!(expression(&mut owner.content, enabled));
        let (mut document, warnings) = imported_case(&project, 14);
        assert_eq!(
            effect_payload(&document, "bulge").is_some(),
            !enabled,
            "{warnings:?}"
        );
        let mut vectors = 0;
        visit(&mut document, &mut |node| {
            if node["type"] == "Rect" || node["type"] == "Shape" {
                vectors += 1;
            }
        });
        assert!(
            vectors > 4,
            "asymmetric owner content retained, not an empty placeholder"
        );
        if enabled {
            assert!(
                warnings
                    .iter()
                    .any(|w| w.contains("Spherize") && w.contains("expressions")),
                "{warnings:?}"
            );
        }
    }
}
