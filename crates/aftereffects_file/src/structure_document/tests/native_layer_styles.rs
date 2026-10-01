//! Fresh-import contracts for the independently Adobe-authored Layer Styles panel.

use fx_schema::{
    BevelStyle, EditableFxCompositionDocument, EffectData, EffectPayload, GlowSource, LayerEffect,
    LayerStrokePosition, ShapeGradientType,
};
use serde_json::{Value, json};

use super::*;

const SOURCE: &[u8] =
    include_bytes!("../../../tests/fixtures/layer_styles/styles_static_adobe.aep");
const SOURCE_LEN: usize = 635_693;
const SOURCE_SHA256: &str = "0160adcfd09c70f4f13b7eedbb539b57e855312fbb0c7b2b44513aef8f418273";
const TEMPLATE: &str = include_str!("../../../tests/fixtures/effects_coverage/template.fx.json");

fn assert_close(actual: f64, expected: f64, context: &str) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "{context}: {actual} != {expected}"
    );
}

fn assert_values(actual: &[f64], expected: &[f64], context: &str) {
    assert_eq!(actual.len(), expected.len(), "{context} component count");
    for (index, (&actual, &expected)) in actual.iter().zip(expected).enumerate() {
        assert_close(actual, expected, &format!("{context}[{index}]"));
    }
}

fn effect_count(layers: &[fx_schema::Layer]) -> usize {
    layers
        .iter()
        .map(|layer| {
            layer.effects().len() + layer.child_layers().map(effect_count).unwrap_or_default()
        })
        .sum()
}

fn with_imported_effect(
    composition_id: u32,
    composition_name: &str,
    layer_id: u32,
    layer_name: &str,
    assert_effect: impl FnOnce(&LayerEffect),
) {
    assert_eq!(SOURCE.len(), SOURCE_LEN, "pinned Layer Styles source size");
    assert_eq!(
        format!("{:x}", Sha256::digest(SOURCE)),
        SOURCE_SHA256,
        "pinned Layer Styles source SHA-256"
    );
    let project = read_project(SOURCE).expect("independently Adobe-authored Layer Styles source");
    let source = composition(&project, composition_id);
    assert_eq!(
        project.item(composition_id).expect("composition").name,
        composition_name
    );
    assert_eq!((source.width, source.height), (320, 180));
    assert_eq!(source.duration_secs, 2.0);
    assert_eq!(source.frame_rate, 24.0);
    let source_owner = source
        .layers
        .iter()
        .find(|layer| layer.record.id() == layer_id)
        .expect("pinned native style owner");
    assert_eq!(source_owner.name.as_ref(), layer_name);

    let imported = to_structural_fx_document(&project, Some(composition_id))
        .expect("fresh Adobe-native Layer Style import");
    assert_imported_canvas_matches_source(source, &imported, composition_name);
    let imported_root = root(&imported);
    assert_eq!(imported_root.name, composition_name);
    assert_eq!(
        effect_count(&imported_root.layers),
        1,
        "one style effect in the imported composition"
    );
    let [owner] = imported_root.layers.as_slice() else {
        panic!("{composition_name}: native single-layer composition must retain one owner");
    };
    let owner = as_group(owner);
    assert_eq!(owner.name, layer_name, "native style owner name");
    assert!(
        owner
            .description
            .contains(&format!("comp={composition_id} layer={layer_id}")),
        "native owner identity retained: {}",
        owner.description
    );
    let [record] = owner.effects.as_slice() else {
        panic!("{composition_name}: exactly one style on its native owner");
    };
    let EffectData::Identified {
        enabled: true,
        effect: EffectPayload::Known(effect),
        ..
    } = record.data()
    else {
        panic!("{composition_name}: enabled typed style effect");
    };
    assert_effect(effect);

    let wire = imported
        .document
        .to_json_value()
        .expect("editable FX JSON")
        .to_string();
    assert!(!wire.contains("JsScript"), "no generated JsScript");
    assert!(!wire.contains("jsScript"), "no generated jsScript");
}

#[test]
fn adobe_drop_shadow_imports_nondefault_typed_controls() {
    with_imported_effect(1, "DropShadow", 15, "DropShadow Subject", |effect| {
        let LayerEffect::DropShadow(style) = effect else {
            panic!("typed Drop Shadow")
        };
        assert!(style.enabled);
        assert_values(
            &style.color,
            &[
                0.05000000074505806,
                0.11999999731779099,
                0.20000000298023224,
                0.7,
            ],
            "color",
        );
        let expected_offset = [
            -12.0 * 135_f64.to_radians().cos(),
            12.0 * 135_f64.to_radians().sin(),
        ];
        assert_values(&style.offset, &expected_offset, "angle/distance offset");
        let adjusted_spread = 0.20 * 0.8;
        assert_close(
            style.blur_radius.value(),
            9.0 * (1.0 - adjusted_spread) * 0.5,
            "radius formula",
        );
        assert_close(
            style.spread_radius.value(),
            9.0 * adjusted_spread,
            "spread formula",
        );
    });
}

#[test]
fn adobe_inner_shadow_imports_nondefault_typed_controls() {
    with_imported_effect(16, "InnerShadow", 29, "InnerShadow Subject", |effect| {
        let LayerEffect::InnerShadow(style) = effect else {
            panic!("typed Inner Shadow")
        };
        assert!(style.enabled());
        assert_values(
            &style.color(),
            &[
                0.10000000149011612,
                0.05000000074505806,
                0.20000000298023224,
                0.65,
            ],
            "color",
        );
        assert_values(
            &style.offset(),
            &[
                -8.0 * 145_f64.to_radians().cos(),
                8.0 * 145_f64.to_radians().sin(),
            ],
            "angle/distance offset",
        );
        assert_close(style.size().value(), 12.0, "size");
        assert_close(style.choke(), 0.15, "choke");
    });
}

#[test]
fn adobe_outer_glow_imports_nondefault_typed_controls() {
    with_imported_effect(30, "OuterGlow", 43, "OuterGlow Subject", |effect| {
        let LayerEffect::OuterGlow(style) = effect else {
            panic!("typed Outer Glow")
        };
        assert!(style.enabled);
        assert_values(
            &style.color,
            &[0.10000000149011612, 0.800000011920929, 1.0, 0.85],
            "color",
        );
        assert_close(style.size.value(), 14.0, "size");
        assert_close(style.spread, 0.25, "spread");
        assert_close(style.range, 0.75, "range");
    });
}

#[test]
fn adobe_inner_glow_imports_nondefault_typed_controls() {
    with_imported_effect(44, "InnerGlow", 57, "InnerGlow Subject", |effect| {
        let LayerEffect::InnerGlow(style) = effect else {
            panic!("typed Inner Glow")
        };
        assert!(style.enabled());
        assert_values(
            &style.color(),
            &[1.0, 0.20000000298023224, 0.6000000238418579, 0.7],
            "color",
        );
        assert_close(style.size().value(), 12.0, "size");
        assert_close(style.choke(), 0.1, "choke");
        assert_close(style.range(), 0.7, "range");
        assert_eq!(style.source(), GlowSource::Center);
    });
}

#[test]
fn adobe_bevel_emboss_imports_nondefault_typed_controls() {
    with_imported_effect(58, "BevelEmboss", 71, "BevelEmboss Subject", |effect| {
        let LayerEffect::BevelEmboss(style) = effect else {
            panic!("typed Bevel and Emboss")
        };
        assert!(style.enabled());
        assert_eq!(style.style(), BevelStyle::InnerBevel);
        assert_close(style.depth(), 1.6, "depth");
        assert_close(style.size().value(), 10.0, "size");
        assert_close(style.soften().value(), 2.0, "soften");
        assert_close(style.angle(), 120.0, "angle");
        assert_close(style.altitude(), 35.0, "altitude");
        assert_values(&style.highlight_color(), &[1.0, 1.0, 1.0, 0.8], "highlight");
        assert_values(&style.shadow_color(), &[0.0, 0.0, 0.0, 0.6], "shadow");
    });
}

#[test]
fn adobe_satin_imports_nondefault_typed_controls() {
    with_imported_effect(72, "Satin", 85, "Satin Subject", |effect| {
        let LayerEffect::Satin(style) = effect else {
            panic!("typed Satin")
        };
        assert!(style.enabled());
        assert_values(
            &style.color(),
            &[0.5, 0.10000000149011612, 0.25, 0.55],
            "color",
        );
        assert_values(
            &style.offset(),
            &[
                -13.0 * 25_f64.to_radians().cos(),
                13.0 * 25_f64.to_radians().sin(),
            ],
            "angle/distance offset",
        );
        assert_close(style.size().value(), 8.0, "size");
        assert!(!style.invert());
    });
}

#[test]
fn adobe_color_overlay_imports_as_constant_gradient_overlay() {
    with_imported_effect(86, "ColorOverlay", 99, "ColorOverlay Subject", |effect| {
        let LayerEffect::GradientOverlay(style) = effect else {
            panic!("typed constant Gradient Overlay")
        };
        assert!(style.enabled());
        assert_close(
            style.opacity(),
            1.0,
            "opacity is carried by constant stop alpha",
        );
        assert_eq!(style.gradient_type(), ShapeGradientType::Linear);
        assert_values(&style.start(), &[0.0, 0.0], "start");
        assert_values(&style.end(), &[1.0, 0.0], "end");
        assert_eq!(style.stops().len(), 2);
        for (index, stop) in style.stops().iter().enumerate() {
            assert_close(stop.offset, index as f64, "constant stop offset");
            assert_values(
                &stop.color,
                &[
                    0.8999999761581421,
                    0.20000000298023224,
                    0.10000000149011612,
                    0.65,
                ],
                "constant stop color",
            );
        }
    });
}

#[test]
fn adobe_gradient_overlay_imports_native_stops_size_angle_and_offset() {
    with_imported_effect(
        100,
        "GradientOverlay",
        113,
        "GradientOverlay Subject",
        |effect| {
            let LayerEffect::GradientOverlay(style) = effect else {
                panic!("typed Gradient Overlay")
            };
            assert!(style.enabled());
            assert_close(style.opacity(), 0.85, "opacity");
            assert_eq!(style.gradient_type(), ShapeGradientType::Linear);
            let axis = [
                35_f64.to_radians().cos() * 80.0 * 0.5,
                35_f64.to_radians().sin() * 80.0 * 0.5,
            ];
            assert_values(
                &style.start(),
                &[10.0 - axis[0], -5.0 - axis[1]],
                "80%/35-degree start around native offset",
            );
            assert_values(
                &style.end(),
                &[10.0 + axis[0], -5.0 + axis[1]],
                "80%/35-degree end around native offset",
            );
            assert_eq!(
                style.stops().len(),
                2,
                "actual decoded native gradient stops"
            );
            assert_close(style.stops()[0].offset, 0.0, "white stop offset");
            assert_values(&style.stops()[0].color, &[1.0, 1.0, 1.0, 1.0], "white stop");
            assert_close(style.stops()[1].offset, 1.0, "black stop offset");
            assert_values(&style.stops()[1].color, &[0.0, 0.0, 0.0, 1.0], "black stop");
        },
    );
}

#[test]
fn adobe_stroke_imports_nondefault_typed_position() {
    with_imported_effect(114, "Stroke", 127, "Stroke Subject", |effect| {
        let LayerEffect::Stroke(style) = effect else {
            panic!("typed Stroke")
        };
        assert!(style.enabled());
        assert_values(
            &style.color(),
            &[0.10000000149011612, 0.5, 1.0, 0.9],
            "color",
        );
        assert_close(style.width().value(), 8.0, "width");
        assert_eq!(style.position(), LayerStrokePosition::Inside);
    });
}

fn style_group_flags(layer: &crate::structure::Layer, style_name: &str) -> (u32, u32, u32) {
    fn flags(run: &[crate::rifx::Chunk]) -> u32 {
        let group = crate::properties::unique_list(run, *b"tdgp").expect("native group");
        let bytes = crate::properties::data(group, *b"tdsb").expect("native tdsb flags");
        u32::from_be_bytes(bytes.try_into().expect("four-byte tdsb flags"))
    }
    let roots = crate::properties::root_runs(&layer.content).expect("property roots");
    let style_root = roots
        .iter()
        .find(|(name, _)| *name == "ADBE Layer Styles")
        .expect("Layer Styles root")
        .1;
    let children = crate::properties::runs(
        crate::properties::unique_list(style_root, *b"tdgp").expect("style children"),
    )
    .expect("style runs");
    let blend = children
        .iter()
        .find(|(name, _)| *name == "ADBE Blend Options Group")
        .expect("Blend Options")
        .1;
    let style = children
        .iter()
        .find(|(name, _)| *name == style_name)
        .expect("named style")
        .1;
    (flags(style_root), flags(blend), flags(style))
}

fn generated_outer_glow(enabled: bool) -> crate::export_document::ExportedDocument {
    let mut input: Value = serde_json::from_str(TEMPLATE).expect("FX template");
    input["composition"]["layers"][0]["effects"] = json!([{
        "id": 9901, "enabled": true, "effect": {
            "type": "outerGlow", "enabled": enabled, "color": [0.1, 0.8, 1.0, 0.85],
            "size": 14.0, "spread": 0.25, "range": 0.75, "blendMode": "screen"
        }
    }]);
    crate::export_document::to_aep(
        &EditableFxCompositionDocument::from_json_value(input).expect("editable FX input"),
    )
    .expect("fresh native Outer Glow export")
}

#[test]
fn generated_layer_style_tdsb_flags_match_the_native_adobe_fixture() {
    let native = read_project(SOURCE).expect("pinned native Layer Styles source");
    let native_owner = composition(&native, 30)
        .layers
        .iter()
        .find(|layer| layer.record.id() == 43)
        .expect("native Outer Glow owner");
    let expected = style_group_flags(native_owner, "outerGlow/enabled");
    assert_eq!(
        expected,
        (1, 1, 1),
        "Adobe-native master, Blend Options, and enabled style flags"
    );

    let generated = generated_outer_glow(true);
    let project = read_project(&generated.bytes).expect("fresh generated Outer Glow AEP");
    let owner = composition(&project, 1)
        .layers
        .iter()
        .find(|layer| {
            !crate::layer_styles::read(&layer.content, [320.0, 180.0])
                .styles
                .is_empty()
        })
        .expect("generated Outer Glow owner");
    assert_eq!(style_group_flags(owner, "outerGlow/enabled"), expected);

    let disabled = generated_outer_glow(false);
    let project = read_project(&disabled.bytes).expect("fresh disabled Outer Glow AEP");
    let owner = composition(&project, 1)
        .layers
        .iter()
        .find(|layer| {
            crate::properties::root_runs(&layer.content)
                .is_ok_and(|roots| roots.iter().any(|(name, _)| *name == "ADBE Layer Styles"))
        })
        .expect("generated disabled Outer Glow owner");
    assert_eq!(style_group_flags(owner, "outerGlow/enabled"), (1, 1, 2));
}
