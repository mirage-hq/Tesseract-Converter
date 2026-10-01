//! CPU-only edge coverage for edited FX Effect Parade export.
//!
//! These checks assert fresh native structure and explicit omission diagnostics.
//! They are not Adobe acceptance or render-fidelity evidence.

use super::*;
use crate::{
    effects::native::{DecodedEffect, read_effects},
    writer::effects::{NativeEffect, NativeEffectProperty},
};
use fx_schema::{
    EffectId, EffectRecord, PropertyAnimator, PropertyTarget,
    animator::{AnimatorData, KeyframeId, PropertyKeyframe, PropertyKeyframeTrack},
};

const TEMPLATE: &str = include_str!("../../../tests/fixtures/effects_coverage/template.fx.json");

fn template() -> Value {
    serde_json::from_str(TEMPLATE).expect("valid effect coverage template")
}

fn record(id: u64, enabled: bool, effect: Value) -> EffectRecord {
    serde_json::from_value(json!({"id":id,"enabled":enabled,"effect":effect}))
        .expect("valid identified effect")
}

fn entry(
    effect_id: u64,
    param: &str,
    values: impl IntoIterator<Item = (i64, PropertyValue, PropertyKeyframeEasing)>,
) -> AnimationGraphEntry {
    let track = PropertyKeyframeTrack::new(
        values
            .into_iter()
            .enumerate()
            .map(|(index, (millis, value, easing))| {
                PropertyKeyframe::new(
                    KeyframeId::new(format!("effect-edge-{effect_id}-{param}-{index}")),
                    fx_schema::TimeOffset::from_millis(millis),
                    value,
                    easing,
                )
            })
            .collect(),
    )
    .expect("valid effect property track");
    AnimationGraphEntry {
        target: PropertyTarget::effect_param(EffectId::new(effect_id), param),
        animator: PropertyAnimator::keyframes(track),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

fn linear_entry(effect_id: u64, param: &str, start: f64, end: f64) -> AnimationGraphEntry {
    entry(
        effect_id,
        param,
        [
            (
                0,
                PropertyValue::Float(start),
                PropertyKeyframeEasing::Linear,
            ),
            (
                1_000,
                PropertyValue::Float(end),
                PropertyKeyframeEasing::Linear,
            ),
        ],
    )
}

fn cubic_entry(effect_id: u64, param: &str, start: f64, end: f64) -> AnimationGraphEntry {
    entry(
        effect_id,
        param,
        [
            (
                0,
                PropertyValue::Float(start),
                PropertyKeyframeEasing::Linear,
            ),
            (
                1_000,
                PropertyValue::Float(end),
                PropertyKeyframeEasing::CubicBezier {
                    x1: 0.2,
                    y1: 0.0,
                    x2: 0.8,
                    y2: 1.0,
                },
            ),
        ],
    )
}

fn hold_entry(effect_id: u64, param: &str, start: f64, end: f64) -> AnimationGraphEntry {
    entry(
        effect_id,
        param,
        [
            (
                0,
                PropertyValue::Float(start),
                PropertyKeyframeEasing::Linear,
            ),
            (
                1_000,
                PropertyValue::Float(end),
                PropertyKeyframeEasing::Hold,
            ),
        ],
    )
}

fn native_property<'a>(effect: &'a NativeEffect, name: &str) -> &'a NativeEffectProperty {
    effect
        .properties
        .iter()
        .find(|property| property.match_name == name)
        .unwrap_or_else(|| panic!("{name} missing from {}", effect.match_name))
}

fn decoded_value(effect: &DecodedEffect, name: &str) -> Vec<f64> {
    effect
        .parameters
        .iter()
        .find(|property| property.match_name == name)
        .unwrap_or_else(|| panic!("{name} missing from {}", effect.match_name))
        .numeric
        .as_ref()
        .unwrap_or_else(|error| panic!("{name}: {error}"))
        .values
        .clone()
}

fn diagnostic(output: &ExportedDocument, text: &str) -> bool {
    output
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains(text))
}

fn layer_effects(layer: &crate::structure::Layer, size: [f64; 2]) -> Vec<DecodedEffect> {
    let (effects, warnings) = read_effects(&layer.content, size);
    assert!(
        warnings.iter().all(|warning| !warning.contains("missing")),
        "{warnings:?}"
    );
    effects
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn static_only_affine_controls_keep_authored_bases_and_other_effects() {
    let mut noise = crate::effects::mapping::default_effect("turbulentNoise");
    noise["blend"] = json!(0.35);
    let lowered = super::super::effects::lower(
        &[
            record(
                1,
                true,
                json!({"type":"gaussianBlur","blurriness":9,"repeatEdgePixels":true}),
            ),
            record(2, true, json!({"type":"posterizeTime","frameRate":12})),
            record(3, true, noise),
        ],
        &[
            linear_entry(1, "repeatEdgePixels", 1.0, 0.0),
            linear_entry(2, "frameRate", 12.0, 24.0),
            linear_entry(3, "blend", 0.35, 0.8),
            linear_entry(1, "blurriness", 9.0, 18.0),
        ],
        [120.0, 80.0],
    );

    assert_eq!(lowered.effects.len(), 3, "supported occurrences survive");
    let repeat = native_property(&lowered.effects[0], "ADBE Gaussian Blur 2-0003");
    assert_eq!(repeat.values, [1.0]);
    assert!(
        repeat.animation.is_none(),
        "static Repeat Edge must not gain keys"
    );
    let frame_rate = native_property(&lowered.effects[1], "ADBE Posterize Time-0001");
    assert_eq!(frame_rate.values, [12.0]);
    assert!(frame_rate.animation.is_none());
    let blend = native_property(&lowered.effects[2], "ADBE AIF Perlin Noise 3D-0025");
    assert_eq!(blend.values, [35.0]);
    assert!(blend.animation.is_none());
    assert!(
        native_property(&lowered.effects[0], "ADBE Gaussian Blur 2-0001")
            .animation
            .is_some(),
        "an independent supported animator remains editable"
    );
    for param in ["repeatEdgePixels", "frameRate", "blend"] {
        assert!(
            lowered
                .warnings
                .iter()
                .any(|warning| warning.contains(param)
                    && warning.contains("static-only control animation omitted")),
            "missing precise static-only diagnostic for {param}: {:?}",
            lowered.warnings
        );
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn animated_vignette_export_diagnoses_static_fallback_without_losing_siblings() {
    let vignette = json!({"type":"vignette","amount":0.8,"radius":0.65,"feather":0.42});
    let static_result =
        super::super::effects::lower(&[record(7, true, vignette.clone())], &[], [320.0, 180.0]);
    let static_amount = native_property(&static_result.effects[0], "CS Vignette-0001");
    assert_eq!(static_amount.values, [80.0]);
    assert!(static_amount.animation.is_none());
    assert!(
        static_result
            .warnings
            .iter()
            .all(|warning| !warning.contains("animated CC Vignette export unsupported"))
    );

    let lowered = super::super::effects::lower(
        &[
            record(7, true, vignette),
            record(8, true, json!({"type":"gaussianBlur","blurriness":4})),
        ],
        &[
            linear_entry(7, "amount", 0.8, 1.2),
            linear_entry(7, "radius", 0.65, 0.9),
            linear_entry(8, "blurriness", 4.0, 12.0),
        ],
        [320.0, 180.0],
    );
    assert_eq!(lowered.effects.len(), 2, "Vignette and sibling survive");
    for (param, control, base) in [
        ("amount", "CS Vignette-0001", 80.0),
        ("radius", "CS Vignette-0002", 39.0),
    ] {
        let native = native_property(&lowered.effects[0], control);
        assert_eq!(native.values, [base], "authored {param} base retained");
        assert!(
            native.animation.is_none(),
            "{param} animation must not silently freeze"
        );
        assert!(
            lowered.warnings.iter().any(|warning| {
                warning.contains(&format!("Effect vignette / {param}"))
                    && warning.contains("animation omitted, authored base retained")
            }),
            "missing contextual unsupported diagnostic for {param}: {:?}",
            lowered.warnings
        );
    }
    assert!(
        native_property(&lowered.effects[1], "ADBE Gaussian Blur 2-0001")
            .animation
            .is_some(),
        "independent supported animation retained"
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn compatible_special_animation_maps_while_reciprocal_controls_retain_bases() {
    let lowered = super::super::effects::lower(
        &[
            record(
                11,
                true,
                json!({"type":"shiftChannels","takeRedFrom":"fullOff","takeGreenFrom":"green","takeBlueFrom":"blue"}),
            ),
            record(
                12,
                true,
                json!({"type":"dropShadow","enabled":true,"color":[0.2,0.3,0.4,0.5],"offset":[3,-4],"blurRadius":7,"spreadRadius":0,"blendMode":"normal"}),
            ),
            record(
                13,
                true,
                json!({"type":"ripple","amplitude":0.1,"frequency":25.132741228718345,"phase":0.5,"centerX":0.5,"centerY":0.5}),
            ),
            record(
                14,
                true,
                json!({"type":"waveWarp","waveHeight":0.1,"waveWidth":6,"direction":30,"phase":0.5}),
            ),
            record(
                15,
                true,
                json!({"type":"gaussianBlur","blurriness":4,"repeatEdgePixels":false}),
            ),
        ],
        &[
            entry(
                11,
                "takeRedFrom",
                [
                    (
                        0,
                        PropertyValue::String("fullOff".into()),
                        PropertyKeyframeEasing::Hold,
                    ),
                    (
                        1_000,
                        PropertyValue::String("fullOn".into()),
                        PropertyKeyframeEasing::Hold,
                    ),
                ],
            ),
            linear_entry(12, "blurRadius", 7.0, 12.0),
            linear_entry(13, "frequency", 25.132741228718345, 12.566370614359172),
            linear_entry(14, "waveWidth", 6.0, 12.0),
            linear_entry(15, "blurriness", 4.0, 10.0),
        ],
        [120.0, 80.0],
    );

    assert_eq!(
        lowered
            .effects
            .iter()
            .map(|effect| effect.match_name.as_str())
            .collect::<Vec<_>>(),
        [
            "ADBE Shift Channels",
            "ADBE Drop Shadow",
            "ADBE Ripple",
            "ADBE Wave Warp",
            "ADBE Gaussian Blur 2"
        ]
    );
    assert_eq!(
        native_property(&lowered.effects[0], "ADBE Shift Channels-0002").values,
        [10.0]
    );
    let softness = native_property(&lowered.effects[1], "ADBE Drop Shadow-0005");
    assert_eq!(softness.values, [7.0]);
    let softness_keys = &softness
        .animation
        .as_ref()
        .expect("Drop Shadow Softness has a compatible scalar native target")
        .keys;
    assert_eq!(softness_keys.len(), 2);
    assert_eq!(softness_keys[0].values, [7.0]);
    assert_eq!(softness_keys[1].values, [12.0]);
    assert!(
        (native_property(&lowered.effects[2], "ADBE Ripple-0005").values[0] - 30.0).abs() < 1e-10
    );
    assert_eq!(
        native_property(&lowered.effects[3], "ADBE Wave Warp-0003").values,
        [20.0]
    );
    assert!(
        native_property(&lowered.effects[4], "ADBE Gaussian Blur 2-0001")
            .animation
            .is_some()
    );
    for param in ["takeRedFrom", "frequency", "waveWidth"] {
        assert!(
            lowered.warnings.iter().any(|warning| {
                warning.contains(param) && warning.contains("no native animated target")
            }),
            "missing special/static diagnostic for {param}: {:?}",
            lowered.warnings
        );
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn cubic_point_and_color_tracks_keep_bases_while_scalar_sibling_animates() {
    let lowered = super::super::effects::lower(
        &[record(
            21,
            true,
            json!({
                "type":"gradientRamp",
                "startX":0.1,"startY":0.2,"startR":0.2,"startG":0.4,"startB":0.6,
                "endX":0.9,"endY":0.8,"endR":0.8,"endG":0.7,"endB":0.5,
                "shape":0,"blend":0.75
            }),
        )],
        &[
            cubic_entry(21, "startX", 0.1, 0.7),
            linear_entry(21, "startY", 0.2, 0.6),
            cubic_entry(21, "startR", 0.2, 0.9),
            linear_entry(21, "startG", 0.4, 0.1),
            linear_entry(21, "blend", 0.75, 0.5),
        ],
        [120.0, 80.0],
    );

    let ramp = &lowered.effects[0];
    let point = native_property(ramp, "ADBE Ramp-0001");
    assert_eq!(point.values, [12.0, 16.0]);
    assert!(
        point.animation.is_none(),
        "cubic Point must retain its base only"
    );
    let color = native_property(ramp, "ADBE Ramp-0002");
    assert_eq!(color.values, [0.2, 0.4, 0.6, 1.0]);
    assert!(
        color.animation.is_none(),
        "cubic Color must retain its base only"
    );
    assert!(
        native_property(ramp, "ADBE Ramp-0007").animation.is_some(),
        "supported scalar sibling remains animated"
    );
    assert!(
        lowered
            .warnings
            .iter()
            .any(|warning| warning.contains("cubic Point animation omitted"))
    );
    assert!(
        lowered
            .warnings
            .iter()
            .any(|warning| warning.contains("cubic color animation omitted"))
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn mixed_hold_and_continuous_point_components_retain_static_point() {
    let lowered = super::super::effects::lower(
        &[record(
            22,
            true,
            json!({
                "type":"gradientRamp",
                "startX":0,"startY":0,"startR":0,"startG":0,"startB":0,
                "endX":0.8,"endY":0.7,"endR":1,"endG":1,"endB":1,
                "shape":0,"blend":1
            }),
        )],
        &[
            hold_entry(22, "endX", 0.8, 0.4),
            linear_entry(22, "endY", 0.7, 0.2),
            hold_entry(22, "shape", 0.0, 1.0),
        ],
        [120.0, 80.0],
    );

    let point = native_property(&lowered.effects[0], "ADBE Ramp-0003");
    assert_eq!(point.values.len(), 2);
    for (actual, expected) in point.values.iter().zip([96.0, 56.0]) {
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }
    assert!(point.animation.is_none());
    assert!(
        native_property(&lowered.effects[0], "ADBE Ramp-0005")
            .animation
            .is_some(),
        "independent Hold popup survives the rejected coupled Point"
    );
    assert!(lowered.warnings.iter().any(|warning| warning.contains(
        "mixed simultaneous Hold/continuous components have no shared native key interpolation"
    )));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn dependent_script_and_duplicate_animators_are_not_executed_or_guessed() {
    let mut dependent = linear_entry(31, "glowThreshold", 20.0, 40.0);
    dependent
        .dependencies
        .push(PropertyTarget::layer(LayerId::new(900), PropType::Rotation));
    let script = AnimationGraphEntry {
        target: PropertyTarget::effect_param(EffectId::new(31), "glowRadius"),
        animator: PropertyAnimator::from_data(&AnimatorData::JsScript {
            code: Some("return input.time * 10;".into()),
            layer_time_js_code: None,
        })
        .expect("valid stored script animator"),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    };
    let duplicate_a = linear_entry(31, "glowIntensity", 0.5, 1.0);
    let duplicate_b = linear_entry(31, "glowIntensity", 0.5, 2.0);
    // AnimationGraph rejects duplicate targets at the document boundary. The
    // lowering slice still rejects them defensively rather than picking one.
    let lowered = super::super::effects::lower(
        &[record(
            31,
            true,
            json!({"type":"glow","glowThreshold":20,"glowRadius":8,"glowIntensity":0.5}),
        )],
        &[dependent, script, duplicate_a, duplicate_b],
        [120.0, 80.0],
    );

    let glow = &lowered.effects[0];
    for name in ["ADBE Glo2-0002", "ADBE Glo2-0003", "ADBE Glo2-0004"] {
        assert!(native_property(glow, name).animation.is_none(), "{name}");
    }
    for reason in [
        "dependent animator cannot be exported without executing a runtime",
        "script animators are not executed, replayed or baked",
        "duplicate animator target",
    ] {
        assert!(
            lowered
                .warnings
                .iter()
                .any(|warning| warning.contains(reason)),
            "missing {reason}: {:?}",
            lowered.warnings
        );
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn disabled_keyframes_export_the_runtime_visible_constant_not_stale_base() {
    let disabled_track = PropertyKeyframeTrack::new(vec![PropertyKeyframe::new(
        KeyframeId::new("disabled-threshold-source"),
        fx_schema::TimeOffset::from_millis(0),
        PropertyValue::Float(90.0),
        PropertyKeyframeEasing::Linear,
    )])
    .expect("valid disabled source track");
    let disabled = AnimationGraphEntry {
        target: PropertyTarget::effect_param(EffectId::new(41), "glowThreshold"),
        animator: PropertyAnimator::from_data(&AnimatorData::Keyframes {
            track: disabled_track,
            enabled: false,
            disabled_value: Some(PropertyValue::Float(40.0)),
        })
        .expect("valid disabled animator with runtime constant"),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    };
    let lowered = super::super::effects::lower(
        &[record(
            41,
            true,
            json!({"type":"glow","glowThreshold":20,"glowRadius":8,"glowIntensity":0.5}),
        )],
        &[disabled, linear_entry(41, "glowRadius", 8.0, 16.0)],
        [120.0, 80.0],
    );

    let threshold = native_property(&lowered.effects[0], "ADBE Glo2-0002");
    assert_eq!(
        threshold.values,
        [51.0],
        "authored base remains the static value"
    );
    let keys = &threshold
        .animation
        .as_ref()
        .expect("disabled runtime constant becomes one native key")
        .keys;
    assert_eq!(keys.len(), 1);
    assert!((keys[0].values[0] - 102.0).abs() < 1e-10);
    assert!(
        native_property(&lowered.effects[0], "ADBE Glo2-0003")
            .animation
            .is_some(),
        "supported sibling animator survives"
    );
    assert!(
        lowered
            .warnings
            .iter()
            .all(|warning| !warning.contains("disabled animator")),
        "{:?}",
        lowered.warnings
    );
}

fn budget_entry(effect_id: u64, param: &str, parity: i64, count: usize) -> AnimationGraphEntry {
    entry(
        effect_id,
        param,
        (0..count).map(|index| {
            (
                i64::try_from(index).expect("bounded index") * 2 + parity,
                PropertyValue::Float(index as f64 / count as f64),
                PropertyKeyframeEasing::Linear,
            )
        }),
    )
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn coupled_point_union_over_native_budget_retains_base_and_scalar_sibling() {
    let lowered = super::super::effects::lower(
        &[record(
            51,
            true,
            json!({"type":"radialBlur","amount":12,"centerX":0.25,"centerY":0.75}),
        )],
        &[
            budget_entry(51, "centerX", 0, 5_001),
            budget_entry(51, "centerY", 1, 5_000),
            linear_entry(51, "amount", 12.0, 24.0),
        ],
        [120.0, 80.0],
    );

    let center = native_property(&lowered.effects[0], "ADBE Radial Blur-0002");
    assert_eq!(center.values, [30.0, 60.0]);
    assert!(center.animation.is_none());
    assert!(
        native_property(&lowered.effects[0], "ADBE Radial Blur-0001")
            .animation
            .is_some()
    );
    assert!(
        lowered
            .warnings
            .iter()
            .any(|warning| warning.contains("coupled native property exceeds key budget")),
        "{:?}",
        lowered.warnings
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn owner_and_instance_switches_keep_order_in_one_effect_stack() {
    let mut input = template();
    let layer = &mut input["composition"]["layers"][0];
    layer["isHidden"] = json!(true);
    layer["effects"] = json!([
        {"id":61,"enabled":true,"effect":{"type":"gaussianBlur","blurriness":7,"repeatEdgePixels":false}},
        {"id":62,"enabled":false,"effect":{"type":"glow","glowThreshold":20,"glowRadius":8,"glowIntensity":0.5}},
        {"id":63,"enabled":true,"effect":{"type":"dropShadow","enabled":false,"color":[0,0,0,0.5],"offset":[2,3],"blurRadius":4,"spreadRadius":0,"blendMode":"normal"}},
        {"id":64,"enabled":true,"effect":{"type":"posterize","levels":9}}
    ]);

    let output = export(input);
    let native = read_project(&output.bytes).expect("fresh native project");
    let owner = layers(&native).first().expect("retained hidden owner");
    let flags = owner.record.flags();
    assert!(
        !flags.enabled,
        "hidden owner becomes a disabled native occurrence"
    );
    assert!(
        flags.effects_active,
        "the retained stack stays active on its owner"
    );
    let effects = layer_effects(owner, [120.0, 80.0]);
    assert_eq!(
        effects
            .iter()
            .map(|effect| (effect.match_name.as_str(), effect.enabled))
            .collect::<Vec<_>>(),
        [
            ("ADBE Gaussian Blur 2", true),
            ("ADBE Glo2", false),
            ("ADBE Drop Shadow", false),
            ("ADBE Posterize", true),
        ]
    );
    assert_eq!(
        decoded_value(&effects[0], "ADBE Gaussian Blur 2-0001"),
        [7.0]
    );
    assert_eq!(decoded_value(&effects[3], "ADBE Posterize-0001"), [9.0]);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn rect_solid_shape_and_text_owners_use_their_declared_effect_coordinate_planes() {
    let mut input = template();
    let mut solid = input["composition"]["layers"][0].take();
    solid["id"] = json!(71);
    solid["name"] = json!("Solid source owner");
    solid["description"] = json!("Editable AE solid; edge owner proof");
    solid["effects"] = json!([{"id":7101,"enabled":true,"effect":{
        "type":"radialBlur","amount":5,"centerX":0.25,"centerY":0.75
    }}]);

    let mut shape = solid.clone();
    shape["id"] = json!(72);
    shape["name"] = json!("Source-less Shape owner");
    shape["description"] = json!("explicit source-less Shape owner");
    shape["type"] = json!("Shape");
    shape.as_object_mut().expect("shape layer").remove("rect");
    shape["shape"] = json!({
        "path":{"commands":[
            {"type":"moveTo","x":-40,"y":-20},
            {"type":"lineTo","x":40,"y":-20},
            {"type":"lineTo","x":40,"y":20},
            {"type":"close"}
        ]},
        "fills":[{"paint":{"type":"solid","color":[0.2,0.4,0.6,1]}}],
        "strokes":[]
    });
    shape["effects"] = json!([{"id":7201,"enabled":true,"effect":{
        "type":"radialBlur","amount":5,"centerX":0.25,"centerY":0.75
    }}]);

    let text = json!({
        "type":"Text","id":73,"name":"Text owner","parent":null,
        "activeRange":{"start":0,"duration":2000},
        "transform":{
            "anchorPoint":[0,0],"position":[160,90],"scale":[100,100],
            "rotation":0,"opacity":100
        },
        "sourceText":{
            "text":"FX","fontFamily":"Inter-Regular","fontStyle":"Regular",
            "fontSize":42,"applyFill":true,"fillColor":[1,1,1,1],
            "applyStroke":false,"strokeColor":[0,0,0,1],"strokeWidth":0,
            "strokeOverFill":true,"justification":"center","tracking":0,
            "leading":50,"baselineShift":0,"boxText":false,"allCaps":false
        },
        "effects":[{"id":7301,"enabled":true,"effect":{
            "type":"radialBlur","amount":5,"centerX":0.25,"centerY":0.75
        }}]
    });
    input["composition"]["layers"] = json!([solid, shape, text]);

    let output = export(input);
    let native = read_project(&output.bytes).expect("fresh native owner project");
    let find = |name: &str| {
        layers(&native)
            .iter()
            .find(|layer| layer.name.as_ref() == name)
            .unwrap_or_else(|| panic!("missing {name}"))
    };
    let solid_layer = find("Solid source owner");
    let source = native
        .item(solid_layer.record.source_id())
        .expect("solid source item");
    assert_eq!(
        source.footage.expect("footage classification").main_source,
        crate::structure::FootageSourceKind::Solid
    );
    let solid_effect = layer_effects(solid_layer, [120.0, 80.0]);
    assert_eq!(
        decoded_value(&solid_effect[0], "ADBE Radial Blur-0002"),
        [30.0, 60.0]
    );
    for name in ["Source-less Shape owner", "Text owner"] {
        let effects = layer_effects(find(name), [320.0, 180.0]);
        assert_eq!(
            decoded_value(&effects[0], "ADBE Radial Blur-0002"),
            [80.0, 135.0],
            "{name} uses the composition-sized effect plane"
        );
    }
}

fn nested_effect_input() -> Value {
    let mut input = template();
    let mut child = input["composition"]["layers"][0].take();
    child["id"] = json!(82);
    child["name"] = json!("Inside source child");
    child["parent"] = json!(81);
    child["transform"]["anchorPoint"] = json!([0, 0]);
    child["transform"]["position"] = json!([0, 0]);
    child["effects"] = json!([{"id":8201,"enabled":true,"effect":{
        "type":"gaussianBlur","blurriness":19,"repeatEdgePixels":false
    }}]);
    let transform = child["transform"].clone();
    input["composition"]["layers"] = json!([{
        "type":"Group","id":81,"name":"Outer precomposition occurrence","parent":null,
        "activeRange":{"start":0,"duration":2000},"transform":transform,
        "layers":[child],
        "effects":[{"id":8101,"enabled":true,"effect":{
            "type":"radialBlur","amount":12,"centerX":0.25,"centerY":0.75
        }}]
    }]);

    input
}

// Unlike the diagnostic regression below, these require the editable child
// effect to survive. They are own-reader contracts, not Adobe acceptance proof.
fn coverage_contract_nested_effect(animated: bool) {
    let mut input = nested_effect_input();
    if animated {
        input["composition"]["dynamics"] = json!({"entries":[
            linear_entry(8201, "blurriness", 19.0, 37.0)
        ]});
    }
    let output = export(input);
    let native = read_project(&output.bytes).expect("fresh nested project");
    let occurrence = layers(&native).first().expect("outer occurrence retained");
    let ItemKind::Composition(source) = &native
        .item(occurrence.record.source_id())
        .expect("generated source")
        .kind
    else {
        panic!("expected an editable precomposition");
    };
    let outer = layer_effects(
        occurrence,
        [f64::from(source.width), f64::from(source.height)],
    );
    assert_eq!(outer.len(), 1, "child effect must not move to its parent");
    assert_eq!(outer[0].match_name, "ADBE Radial Blur");
    let inside = source
        .layers
        .iter()
        .find(|layer| layer.name.as_ref() == "Inside source child")
        .expect("child retained");
    let effects = layer_effects(inside, [120.0, 80.0]);
    assert_eq!(effects.len(), 1, "{:?}", output.diagnostics);
    assert_eq!(effects[0].match_name, "ADBE Gaussian Blur 2");
    let blur = effects[0]
        .parameters
        .iter()
        .find(|property| property.match_name == "ADBE Gaussian Blur 2-0001")
        .expect("editable child blurriness")
        .numeric
        .as_ref()
        .expect("numeric child blurriness");
    if animated {
        assert!(blur.animated);
        assert_eq!(blur.keyframes.len(), 2);
        assert_eq!(blur.keyframes[0].out_interpolation, 1);
        assert_eq!(blur.keyframes[1].in_interpolation, 1);
        for (key, (time, value)) in blur.keyframes.iter().zip([(0.0, 19.0), (1.0, 37.0)]) {
            assert_eq!(key.values, vec![value]);
            assert!((key.time_secs - time).abs() < 1e-6);
        }
    } else {
        assert!(!blur.animated);
        assert_eq!(blur.values, vec![19.0]);
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn coverage_contract_adjustment_exports_native_flag_and_effect() {
    let mut input = template();
    let background = input["composition"]["layers"][0].clone();
    let mut adjustment = background.clone();
    adjustment["id"] = json!(83);
    adjustment["type"] = json!("Adjustment");
    adjustment["name"] = json!("Edited adjustment");
    adjustment.as_object_mut().unwrap().remove("rect");
    adjustment["activeRange"] = json!({"start":250,"duration":1500});
    adjustment["effects"] = json!([{"id":8301,"enabled":true,"effect":{
        "type":"gaussianBlur","blurriness":23,"repeatEdgePixels":false
    }}]);
    input["composition"]["layers"] = json!([adjustment, background]);
    let output = export(input);
    let native = read_project(&output.bytes).expect("fresh adjustment export");
    assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
    let owner = &layers(&native)[0];
    assert_eq!(owner.name.as_ref(), "Edited adjustment");
    assert!(owner.record.flags().adjustment_layer);
    let start = owner.record.start_time().expect("finite native start");
    let stretch = owner.record.stretch().expect("finite native stretch");
    assert_eq!(start + owner.record.in_point().unwrap() * stretch, 0.25);
    assert_eq!(start + owner.record.out_point().unwrap() * stretch, 1.75);
    let effects = layer_effects(owner, [320.0, 180.0]);
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].match_name, "ADBE Gaussian Blur 2");
    assert_eq!(
        decoded_value(&effects[0], "ADBE Gaussian Blur 2-0001"),
        vec![23.0]
    );
    assert!(!layers(&native)[1].record.flags().adjustment_layer);
    assert!(layer_effects(&layers(&native)[1], [120.0, 80.0]).is_empty());
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn coverage_contract_nested_child_effect_base() {
    coverage_contract_nested_effect(false);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn coverage_contract_nested_child_effect_keys() {
    coverage_contract_nested_effect(true);
}

fn generated_source<'a>(
    native: &'a StructuralProject,
    owner: &crate::structure::Layer,
) -> &'a crate::structure::Composition {
    let ItemKind::Composition(source) = &native
        .item(owner.record.source_id())
        .expect("generated source")
        .kind
    else {
        panic!("effect owner must be a precomposition occurrence");
    };
    source
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn nested_precomposition_effects_keep_owner_order_switches_and_supported_siblings() {
    let mut input = nested_effect_input();
    let child = &mut input["composition"]["layers"][0]["layers"][0];
    child["effects"].as_array_mut().unwrap().extend([
        json!({"id":8202,"enabled":true,"effect":{"type":"vignette","amount":0.5}}),
        json!({"id":8203,"enabled":false,"effect":{"type":"posterize","levels":9}}),
    ]);
    let output = export(input);
    let native = read_project(&output.bytes).expect("fresh nested project");
    let occurrence = &layers(&native)[0];
    let source = generated_source(&native, occurrence);
    let size = [f64::from(source.width), f64::from(source.height)];
    let outer = layer_effects(occurrence, size);
    assert_eq!(outer.len(), 1);
    assert_eq!(outer[0].match_name, "ADBE Radial Blur");
    assert_eq!(
        decoded_value(&outer[0], "ADBE Radial Blur-0002"),
        [size[0] * 0.25, size[1] * 0.75]
    );
    assert_eq!(source.layers.len(), 1);
    let inside = &source.layers[0];
    assert_eq!(inside.name.as_ref(), "Inside source child");
    assert!(inside.record.flags().effects_active);
    let effects = layer_effects(inside, [120.0, 80.0]);
    assert_eq!(
        effects
            .iter()
            .map(|effect| (effect.match_name.as_str(), effect.enabled))
            .collect::<Vec<_>>(),
        [("ADBE Gaussian Blur 2", true), ("ADBE Posterize", false)]
    );
    assert_eq!(
        decoded_value(&effects[0], "ADBE Gaussian Blur 2-0001"),
        [19.0]
    );
    assert_eq!(decoded_value(&effects[1], "ADBE Posterize-0001"), [9.0]);
    assert!(diagnostic(&output, "Effect vignette"));
    assert!(!diagnostic(
        &output,
        "Effect stack omitted on a nested precomposition"
    ));
    assert!(
        output.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(82))
                && diagnostic.message.contains("may clip effect expansion")
        }),
        "{:?}",
        output.diagnostics
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn nested_precomposition_effect_animation_retains_authored_keys_on_child() {
    let mut input = nested_effect_input();
    input["composition"]["dynamics"]["entries"] = json!([
        linear_entry(8201, "blurriness", 19.0, 31.0),
        linear_entry(8101, "amount", 12.0, 24.0)
    ]);
    let output = export(input);
    let native = read_project(&output.bytes).expect("fresh animated nested project");
    let occurrence = &layers(&native)[0];
    let source = generated_source(&native, occurrence);
    for (effects, control, values) in [
        (
            layer_effects(&source.layers[0], [120.0, 80.0]),
            "ADBE Gaussian Blur 2-0001",
            [19.0, 31.0],
        ),
        (
            layer_effects(
                occurrence,
                [f64::from(source.width), f64::from(source.height)],
            ),
            "ADBE Radial Blur-0001",
            [12.0, 24.0],
        ),
    ] {
        assert_eq!(effects.len(), 1, "{control} stays on its owner");
        let numeric = effects[0]
            .parameters
            .iter()
            .find(|parameter| parameter.match_name == control)
            .expect("animated effect parameter")
            .numeric
            .as_ref()
            .expect("native numeric keys");
        assert_eq!(numeric.keyframes.len(), 2, "no frame baking");
        for (index, key) in numeric.keyframes.iter().enumerate() {
            assert_eq!(key.values, [values[index]]);
            assert!((key.time_secs - index as f64).abs() < 0.0001);
        }
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn nested_precomposition_effects_stay_on_each_level_and_do_not_leak_to_root_sibling() {
    let mut input = nested_effect_input();
    let mut inner = input["composition"]["layers"][0].take();
    inner["parent"] = json!(80);
    let mut sibling = inner["layers"][0].clone();
    sibling["id"] = json!(83);
    sibling["name"] = json!("Root sibling");
    sibling["parent"] = Value::Null;
    sibling["effects"] =
        json!([{"id":8301,"enabled":true,"effect":{"type":"posterize","levels":7}}]);
    input["composition"]["layers"] = json!([{
        "type":"Group","id":80,"name":"Top precomposition","parent":null,
        "activeRange":{"start":0,"duration":2000},"transform":inner["transform"],
        "layers":[inner],
        "effects":[{"id":8001,"enabled":true,"effect":{"type":"exposure","exposure":1,"offset":0,"gammaCorrection":1}}]
    }, sibling]);
    let output = export(input);
    let native = read_project(&output.bytes).expect("fresh twice-nested project");
    assert_eq!(layers(&native).len(), 2);
    let top = layers(&native)
        .iter()
        .find(|layer| layer.name.as_ref() == "Top precomposition")
        .unwrap();
    let middle = generated_source(&native, top);
    assert_eq!(middle.layers.len(), 1);
    let inside = generated_source(&native, &middle.layers[0]);
    assert_eq!(inside.layers.len(), 1);
    for (owner, size, expected) in [
        (
            top,
            [f64::from(middle.width), f64::from(middle.height)],
            "ADBE Exposure2",
        ),
        (
            &middle.layers[0],
            [f64::from(inside.width), f64::from(inside.height)],
            "ADBE Radial Blur",
        ),
        (&inside.layers[0], [120.0, 80.0], "ADBE Gaussian Blur 2"),
        (
            layers(&native)
                .iter()
                .find(|layer| layer.name.as_ref() == "Root sibling")
                .unwrap(),
            [120.0, 80.0],
            "ADBE Posterize",
        ),
    ] {
        let effects = layer_effects(owner, size);
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].match_name, expected);
    }
    assert!(!output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(83))
            && diagnostic.message.contains("may clip effect expansion")
    }));
}
