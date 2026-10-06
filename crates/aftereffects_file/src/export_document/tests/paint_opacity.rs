use super::*;
use crate::{properties, rifx::Chunk};

pub(super) fn numeric(chunks: &[Chunk], target: &str) -> Option<properties::NumericProperty> {
    if let Ok(runs) = properties::runs(chunks) {
        for (name, run) in runs {
            if name == target {
                let storage = properties::unique_list(run, *b"tdbs").unwrap();
                return Some(properties::read_numeric(storage).unwrap());
            }
        }
    }
    chunks
        .iter()
        .filter_map(Chunk::children)
        .find_map(|children| numeric(children, target))
}

fn document(boolean: bool, stroke: bool, opacity: f64, animated: bool) -> Value {
    // Explicit edited FX inputs: pinned Solid import is archive metadata only,
    // not an independent native fixture for paint/layer opacity combinations.
    let mut value = imported();
    let mut shape = rect(&value, 400);
    shape["type"] = json!("Shape");
    shape["name"] = json!("Independent opacity");
    shape["transform"] = value["composition"]["layers"][0]["transform"].clone();
    shape["transform"]["opacity"] = json!(80.0);
    shape.as_object_mut().unwrap().remove("rect");
    let mut paint = json!({
        "paint":{"type":"solid","color":[0.2,0.4,0.6,0.4]},
        "opacity":opacity
    });
    if stroke {
        paint["width"] = json!(6.0);
    }
    let fills = if stroke { json!([]) } else { json!([paint]) };
    let strokes = if stroke { json!([paint]) } else { json!([]) };
    shape["shape"] = json!({
        "path":{"commands":[
            {"type":"moveTo","x":0.0,"y":0.0},
            {"type":"lineTo","x":100.0,"y":0.0},
            {"type":"lineTo","x":100.0,"y":80.0},
            {"type":"close"}
        ]}, "fills":fills, "strokes":strokes
    });
    if boolean {
        let mut a = shape.clone();
        a["id"] = json!(401);
        a["parent"] = json!(400);
        a["transform"]["opacity"] = json!(100.0);
        a["shape"]["fills"] = json!([]);
        a["shape"]["strokes"] = json!([]);
        let mut b = a.clone();
        b["id"] = json!(402);
        shape = json!({
            "type":"BooleanOperation", "id":400, "name":"Independent opacity",
            "parent":null, "activeRange":shape["activeRange"],
            "transform":shape["transform"], "op":"union", "layers":[a,b],
            "fills":fills, "strokes":strokes
        });
    }
    value["composition"]["layers"] = json!([shape]);
    value["composition"]["dynamics"] = if animated {
        json!({"entries":[keyed_entry(
            LayerId::new(400), PropType::Opacity,
            [(0, PropertyValue::Float(20.0)), (500, PropertyValue::Float(70.0))],
        )]})
    } else {
        json!({"entries":[]})
    };
    value
}

#[test]
fn static_solid_alpha_keeps_fill_stroke_layer_keys_and_opaque_sibling() {
    let mut value = document(false, false, 1.0, true);
    let shape = &mut value["composition"]["layers"][0];
    shape["shape"]["fills"][0]["paint"]["color"][3] = json!(0.27);
    shape["shape"]["strokes"] = json!([{
        "paint":{"type":"solid","color":[1.0,0.91,0.77,0.82]},
        "opacity":1.0,"width":1.7
    }]);
    let mut opaque = shape.clone();
    opaque["id"] = json!(401);
    opaque["shape"]["fills"][0]["paint"]["color"][3] = json!(1.0);
    opaque["shape"]["strokes"][0]["paint"]["color"][3] = json!(1.0);
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(opaque);
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    let native_layers = layers(&native);
    assert_eq!(native_layers.len(), 2, "{:?}", output.diagnostics);
    let painted = &native_layers[0].content;
    assert_eq!(
        numeric(painted, "ADBE Vector Fill Opacity").unwrap().values,
        [27.0]
    );
    assert_eq!(
        numeric(painted, "ADBE Vector Stroke Opacity")
            .unwrap()
            .values,
        [82.0]
    );
    assert_eq!(
        numeric(painted, "ADBE Vector Fill Color").unwrap().values,
        [0.2, 0.4, 0.6, 1.0]
    );
    let layer_opacity = numeric(painted, "ADBE Opacity").unwrap();
    assert_eq!(layer_opacity.keyframes.len(), 2);
    for (key, (time, opacity)) in layer_opacity.keyframes.iter().zip([(0.0, 0.2), (0.5, 0.7)]) {
        assert_eq!(key.time_secs, time);
        assert!((key.values[0] - opacity).abs() < 1.0e-6);
    }
    let sibling = &native_layers[1].content;
    assert_eq!(
        numeric(sibling, "ADBE Vector Fill Opacity").unwrap().values,
        [100.0]
    );
    assert_eq!(
        numeric(sibling, "ADBE Vector Stroke Opacity")
            .unwrap()
            .values,
        [100.0]
    );
    assert_eq!(numeric(sibling, "ADBE Opacity").unwrap().values, [0.8]);
}

#[test]
fn single_paint_preserves_showreel_leaf_opacity_product() {
    // Reduced explicit FX inputs using the reported finale's paint/layer
    // factors. Geometry is synthetic, not independent Adobe-render evidence.
    for stroke in [false, true] {
        for gradient in [false, true] {
            let mut value = document(false, stroke, 100.0, false);
            let shape = &mut value["composition"]["layers"][0];
            shape["transform"]["opacity"] = json!(0.999);
            let paints = if stroke { "strokes" } else { "fills" };
            shape["shape"][paints][0]["paint"] = if gradient {
                json!({
                    "type":"gradient", "gradientType":"linear",
                    "start":[0.0,0.0], "end":[0.0,220.0],
                    "stops":[
                        {"offset":0.0,"color":[0.957,0.945,1.0,1.0]},
                        {"offset":0.42,"color":[0.098,0.902,1.0,1.0]},
                        {"offset":0.5,"color":[0.1,0.04,0.31,1.0]},
                        {"offset":0.54,"color":[1.0,0.18,0.533,1.0]},
                        {"offset":0.8,"color":[1.0,0.478,0.102,1.0]},
                        {"offset":1.0,"color":[1.0,0.824,0.247,1.0]}
                    ]
                })
            } else {
                json!({"type":"solid","color":[0.957,0.945,1.0,1.0]})
            };
            let output = export(value);
            let native = read_project(&output.bytes).unwrap();
            let native_layers = layers(&native);
            assert_eq!(native_layers.len(), 1, "{:?}", output.diagnostics);
            let opacity = numeric(&native_layers[0].content, "ADBE Opacity").unwrap();
            assert!(!opacity.animated);
            assert!(
                (opacity.values[0] - 0.999).abs() < 1.0e-6,
                "paint 100 × layer 0.999% must retain 99.9% coverage: {opacity:?}"
            );
            let paint_name = if stroke {
                "ADBE Vector Stroke Opacity"
            } else {
                "ADBE Vector Fill Opacity"
            };
            assert_eq!(
                numeric(&native_layers[0].content, paint_name)
                    .unwrap()
                    .values,
                [100.0]
            );
            assert!(output.diagnostics.iter().any(|diagnostic| {
                diagnostic.layer_id == Some(LayerId::new(400))
                    && diagnostic.message.contains("Single-paint Shape")
            }));
            assert!(
                !output
                    .diagnostics
                    .iter()
                    .any(|diagnostic| { diagnostic.message.contains("Overrange paint opacity") })
            );
        }
    }
}

#[test]
fn keyed_digit_with_static_base_100_exports_unclamped_keys() {
    for stroke in [false, true] {
        let mut value = document(false, stroke, 100.0, false);
        value["composition"]["layers"][0]["transform"]["opacity"] = json!(100.0);
        value["composition"]["dynamics"] = json!({"entries":[keyed_entry(
            LayerId::new(400), PropType::Opacity,
            [(0, PropertyValue::Float(1.0)), (9984, PropertyValue::Float(0.0))]
        )]});
        let output = export(value);
        let native = read_project(&output.bytes).unwrap();
        let native_layers = layers(&native);
        assert_eq!(native_layers.len(), 1, "{:?}", output.diagnostics);
        let opacity = numeric(&native_layers[0].content, "ADBE Opacity").unwrap();
        assert_eq!(opacity.keyframes.len(), 2);
        assert_eq!(opacity.keyframes[0].values, [1.0]);
        assert_eq!(opacity.keyframes[1].values, [0.0]);
        // Animated AE property stores no static payload; only its keys apply.
        assert!(opacity.values.is_empty());
        let paint_name = if stroke {
            "ADBE Vector Stroke Opacity"
        } else {
            "ADBE Vector Fill Opacity"
        };
        assert_eq!(
            numeric(&native_layers[0].content, paint_name)
                .unwrap()
                .values,
            [40.0]
        );
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("Single-paint Shape"))
        );
    }
}

#[test]
fn keyed_cubic_spark_preserves_native_speed_influence_and_clock() {
    let mut value = document(false, false, 100.0, false);
    value["composition"]["layers"][0]["transform"]["opacity"] = json!(100.0);
    value["composition"]["dynamics"] = json!({"entries":[{
        "target":{"kind":"layer","layerId":400,"propertyType":"opacity"},
        "animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"spark-0","layerTime":0,"value":{"type":"float","value":0.5147528349381107},"easing":{"type":"linear"}},
            {"id":"spark-1","layerTime":83,"value":{"type":"float","value":0.09536368537540305},
             "easing":{"type":"cubicBezier","x1":0.3333333333333333,"y1":0.56,"x2":0.6666666666666667,"y2":0.9}}
        ]}
    }]});
    let mut ordinary = value.clone();
    ordinary["composition"]["layers"][0]["shape"]["fills"][0]["opacity"] = json!(1.0);
    let baseline = read_project(&export(ordinary).bytes).unwrap();
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    let native_layers = layers(&native);
    assert_eq!(native_layers.len(), 1, "{:?}", output.diagnostics);
    let baseline_opacity = numeric(&layers(&baseline)[0].content, "ADBE Opacity").unwrap();
    let opacity = numeric(&native_layers[0].content, "ADBE Opacity").unwrap();
    assert_eq!(opacity.keyframes.len(), 2);
    for (actual, original) in opacity.keyframes.iter().zip(&baseline_opacity.keyframes) {
        assert_eq!(actual.time_secs, original.time_secs);
        assert!((actual.values[0] - original.values[0] * 100.0).abs() < 1.0e-9);
        assert!((actual.in_speed[0] - original.in_speed[0] * 100.0).abs() < 1.0e-8);
        assert!((actual.out_speed[0] - original.out_speed[0] * 100.0).abs() < 1.0e-8);
        assert_eq!(actual.in_influence, original.in_influence);
        assert_eq!(actual.out_influence, original.out_influence);
    }
    assert_eq!(
        numeric(&native_layers[0].content, "ADBE Vector Fill Opacity")
            .unwrap()
            .values,
        [40.0]
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("Single-paint Shape"))
    );
}

#[test]
fn separated_xy_shape_normalizes_sidecar_opacity_without_replacing_position() {
    let mut value = document(false, false, 100.0, false);
    let shape = &mut value["composition"]["layers"][0];
    shape["transform"]["opacity"] = json!(100.0);
    shape["transform"]["position"] = json!([10.0, 30.0]);
    shape["shape"]["fills"][0]["paint"]["color"][3] = json!(1.0);
    shape["activeRange"]["start"] = json!(390);
    value["composition"]["dynamics"] = json!({"entries":[
        keyed_entry(LayerId::new(400), PropType::PositionX,
            [(0, PropertyValue::Float(10.0)), (1000, PropertyValue::Float(20.0))]),
        keyed_entry(LayerId::new(400), PropType::PositionY,
            [(0, PropertyValue::Float(30.0)), (500, PropertyValue::Float(40.0))]),
        {"target":{"kind":"layer","layerId":400,"propertyType":"opacity"},
         "animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"spark-0","layerTime":0,"value":{"type":"float","value":0.5147528349381107},"easing":{"type":"linear"}},
            {"id":"spark-1","layerTime":83,"value":{"type":"float","value":0.09536368537540305},
             "easing":{"type":"cubicBezier","x1":0.3333333333333333,"y1":0.56,"x2":0.6666666666666667,"y2":0.9}}
         ]}}
    ]});
    let mut ordinary = value.clone();
    ordinary["composition"]["layers"][0]["shape"]["fills"][0]["opacity"] = json!(1.0);
    let baseline = read_project(&export(ordinary).bytes).unwrap();
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    let native_layers = layers(&native);
    assert_eq!(native_layers.len(), 1, "{:?}", output.diagnostics);
    let baseline_layer = &layers(&baseline)[0];
    let opacity = numeric(&native_layers[0].content, "ADBE Opacity").unwrap();
    let old_opacity = numeric(&baseline_layer.content, "ADBE Opacity").unwrap();
    assert_eq!(opacity.keyframes.len(), 2);
    for (new, old) in opacity.keyframes.iter().zip(&old_opacity.keyframes) {
        assert_eq!(new.time_secs, old.time_secs);
        assert!((new.values[0] - old.values[0] * 100.0).abs() < 1.0e-9);
        assert!((new.in_speed[0] - old.in_speed[0] * 100.0).abs() < 1.0e-8);
        assert!((new.out_speed[0] - old.out_speed[0] * 100.0).abs() < 1.0e-8);
        assert_eq!(new.in_influence, old.in_influence);
        assert_eq!(new.out_influence, old.out_influence);
    }
    for axis in ["ADBE Position_0", "ADBE Position_1"] {
        assert_eq!(
            numeric(&native_layers[0].content, axis),
            numeric(&baseline_layer.content, axis)
        );
    }
    assert_eq!(
        numeric(&native_layers[0].content, "ADBE Vector Fill Opacity")
            .unwrap()
            .values,
        [100.0]
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("Single-paint Shape"))
    );
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("Overrange paint opacity"))
    );
}

#[test]
fn true_3d_shape_never_normalizes_sidecar_opacity() {
    let mut value = document(false, false, 100.0, false);
    let shape = &mut value["composition"]["layers"][0];
    shape["transform"]["rotationX"] = json!(15.0);
    shape["transform"]["opacity"] = json!(1.0);
    shape["shape"]["fills"][0]["paint"]["color"][3] = json!(1.0);
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    let native_layers = layers(&native);
    let shape_layer = native_layers
        .iter()
        .find(|layer| numeric(&layer.content, "ADBE Vector Fill Opacity").is_some())
        .expect("generated Shape, not its camera");
    assert_eq!(
        numeric(&shape_layer.content, "ADBE Opacity")
            .unwrap()
            .values,
        [0.01]
    );
    assert_eq!(
        numeric(&shape_layer.content, "ADBE Vector Fill Opacity")
            .unwrap()
            .values,
        [100.0]
    );
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("Single-paint Shape"))
    );
}

#[test]
fn keyed_spark_negative_trough_keeps_native_handles_and_scaled_values() {
    // Reduced FX input from the native Spark 1 k31..32 values and handles;
    // this structure test is not an independent Adobe render comparison.
    let mut value = document(false, false, 100.0, false);
    value["composition"]["layers"][0]["transform"]["opacity"] = json!(100.0);
    value["composition"]["dynamics"] = json!({"entries":[{
        "target":{"kind":"layer","layerId":400,"propertyType":"opacity"},
        "animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"spark-31","layerTime":250,"value":{"type":"float","value":0.03977429969141072},"easing":{"type":"linear"}},
            {"id":"spark-32","layerTime":394,"value":{"type":"float","value":0.0},
             "easing":{"type":"cubicBezier","x1":0.3333333333333333,"y1":1.0789371568939217,"x2":0.6666666666666667,"y2":1.0512736311432933}}
        ]}
    }]});
    let mut ordinary = value.clone();
    ordinary["composition"]["layers"][0]["shape"]["fills"][0]["opacity"] = json!(1.0);
    let baseline = read_project(&export(ordinary).bytes).unwrap();
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    let native_layers = layers(&native);
    assert_eq!(native_layers.len(), 1, "{:?}", output.diagnostics);
    let original = numeric(&layers(&baseline)[0].content, "ADBE Opacity").unwrap();
    let opacity = numeric(&native_layers[0].content, "ADBE Opacity").unwrap();
    assert_eq!(opacity.keyframes.len(), 2);
    for (actual, before) in opacity.keyframes.iter().zip(&original.keyframes) {
        assert_eq!(actual.time_secs, before.time_secs);
        assert!((actual.values[0] - before.values[0] * 100.0).abs() < 1.0e-9);
        assert!((actual.in_speed[0] - before.in_speed[0] * 100.0).abs() < 1.0e-8);
        assert!((actual.out_speed[0] - before.out_speed[0] * 100.0).abs() < 1.0e-8);
        assert_eq!(actual.in_influence, before.in_influence);
        assert_eq!(actual.out_influence, before.out_influence);
    }
    assert_eq!(
        numeric(&native_layers[0].content, "ADBE Vector Fill Opacity")
            .unwrap()
            .values,
        [40.0]
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("negative troughs"))
    );
}

fn animated_pill(stroke: bool) -> Value {
    // Exact reported opacity/scale clocks, with reduced synthetic geometry.
    // The private original and Adobe render remain separate visual evidence.
    let mut value = document(false, stroke, 100.0, false);
    let id = if stroke { 70054 } else { 70053 };
    let shape = &mut value["composition"]["layers"][0];
    shape["id"] = json!(id);
    shape["transform"]["opacity"] = json!(0.999);
    shape["transform"]["scale"] = json!([0.0, 100.0]);
    let paints = if stroke { "strokes" } else { "fills" };
    shape["shape"][paints][0]["paint"]["color"] = if stroke {
        json!([1.0, 0.18, 0.533, 0.55])
    } else {
        json!([0.043, 0.024, 0.125, 1.0])
    };
    value["composition"]["dynamics"] = json!({"entries":[
        {"target":{"kind":"layer","layerId":id,"propertyType":"opacity"},
         "animator":{"type":"keyframes","enabled":true,"keyframes":[
             {"id":"opacity-0","layerTime":0,"value":{"type":"float","value":0.0},"easing":{"type":"linear"}},
             {"id":"opacity-1","layerTime":960,"value":{"type":"float","value":0.0},"easing":{"type":"hold"}},
             {"id":"opacity-2","layerTime":if stroke {1020} else {1010},"value":{"type":"float","value":0.999},"easing":{"type":"linear"}}
         ]}},
        {"target":{"kind":"layer","layerId":id,"propertyType":"scaleX"},
         "animator":{"type":"keyframes","enabled":true,"keyframes":[
             {"id":"scale-0","layerTime":960,"value":{"type":"float","value":0.0},"easing":{"type":"linear"}},
             {"id":"scale-1","layerTime":1130,"value":{"type":"float","value":100.0},"easing":{"type":"cubicBezier","x1":0.16,"y1":1.0,"x2":0.3,"y2":1.0}}
         ]}}
    ]});
    value
}

#[test]
fn single_paint_preserves_animated_showreel_pill_opacity() {
    for stroke in [false, true] {
        let value = animated_pill(stroke);
        let paints = if stroke { "strokes" } else { "fills" };
        let mut ordinary = value.clone();
        ordinary["composition"]["layers"][0]["shape"][paints][0]["opacity"] = json!(1.0);
        let baseline = read_project(&export(ordinary).bytes).unwrap();
        let output = export(value);
        let native = read_project(&output.bytes).unwrap();
        let native_layers = layers(&native);
        assert_eq!(native_layers.len(), 1, "{:?}", output.diagnostics);
        let content = &native_layers[0].content;
        let baseline_content = &layers(&baseline)[0].content;
        let opacity = numeric(content, "ADBE Opacity").unwrap();
        assert!(opacity.animated);
        assert_eq!(opacity.keyframes.len(), 3);
        let mut expected = numeric(baseline_content, "ADBE Opacity").unwrap();
        for key in &mut expected.keyframes {
            key.values[0] *= 100.0;
        }
        assert_eq!(
            opacity, expected,
            "preserve key clocks/easing, scale coverage"
        );
        for (key, (time, alpha)) in opacity.keyframes.iter().zip([
            (0.0, 0.0),
            (0.960, 0.0),
            (if stroke { 1.020 } else { 1.010 }, 0.999),
        ]) {
            assert!((key.time_secs - time).abs() < 1.0 / 24_576.0);
            assert!((key.values[0] - alpha).abs() < 1.0e-6);
        }
        let scale = numeric(content, "ADBE Scale").unwrap();
        assert!(scale.animated);
        assert_eq!(scale.keyframes.len(), 2);
        assert_eq!(scale, numeric(baseline_content, "ADBE Scale").unwrap());
        let paint_name = if stroke {
            "ADBE Vector Stroke Opacity"
        } else {
            "ADBE Vector Fill Opacity"
        };
        assert!(
            (numeric(content, paint_name).unwrap().values[0] - if stroke { 55.0 } else { 100.0 })
                .abs()
                < 1.0e-6
        );
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("Single-paint Shape"))
        );
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("Overrange paint opacity"))
        );
    }
}

#[test]
fn animated_single_paint_keeps_fallback_when_vector_keys_are_reconstructed() {
    for mode in ["skew", "skew_keys", "effectful_group"] {
        let mut value = animated_pill(false);
        match mode {
            "skew" => value["composition"]["layers"][0]["transform"]["skew"] = json!(15.0),
            "skew_keys" => value["composition"]["dynamics"]["entries"]
                .as_array_mut()
                .unwrap()
                .push(json!(keyed_entry(
                    LayerId::new(70053),
                    PropType::Skew,
                    [
                        (0, PropertyValue::Float(0.0)),
                        (500, PropertyValue::Float(15.0))
                    ],
                ))),
            _ => {
                let shape = value["composition"]["layers"][0].clone();
                value["composition"]["layers"] = json!([{
                    "type":"Group", "id":402, "name":"Effectful vector parent",
                    "playback":fixture_linear_playback(shape["activeRange"].clone(), shape["activeRange"].clone()),
                    "effects":[{"id":403,"enabled":true,"effect":{
                        "type":"glow","glowThreshold":20.0,"glowRadius":10.0,"glowIntensity":1.25
                    }}],
                    "transform":identity_fx_transform(), "layers":[shape]
                }]);
            }
        }
        let output = export(value);
        let native = read_project(&output.bytes).unwrap();
        assert_eq!(layers(&native).len(), 1, "{mode}: {:?}", output.diagnostics);
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.message.contains("Single-paint Shape")),
            "{mode}"
        );
        assert!(
            output
                .diagnostics
                .iter()
                .any(|d| d.message.contains("Overrange paint opacity")),
            "{mode}"
        );
        let mut keys = Vec::new();
        collect_group_opacity_keys(&layers(&native)[0].content, &mut keys);
        assert_eq!(keys.len(), 3, "{mode}: {keys:?}");
        for (actual, expected) in keys.iter().zip([0.0, 0.0, 0.999]) {
            assert!((actual - expected).abs() < 1.0e-6, "{mode}: {keys:?}");
        }
    }
}

fn collect_group_opacity_keys(chunks: &[Chunk], values: &mut Vec<f64>) {
    if let Ok(runs) = properties::runs(chunks) {
        for (name, run) in runs {
            if name == "ADBE Vector Group Opacity" {
                let storage = properties::unique_list(run, *b"tdbs").unwrap();
                let property = properties::read_numeric(storage).unwrap();
                values.extend(property.keyframes.iter().map(|key| key.values[0]));
            }
        }
    }
    for children in chunks.iter().filter_map(Chunk::children) {
        collect_group_opacity_keys(children, values);
    }
}

#[test]
fn single_paint_normalization_keeps_matte_and_parent_opacity() {
    let mut value = document(false, false, 100.0, false);
    let mut owner = value["composition"]["layers"][0].clone();
    owner["name"] = json!("Chrome face");
    owner["transform"]["opacity"] = json!(0.999);
    owner["shape"]["fills"][0]["paint"]["color"][3] = json!(1.0);
    owner["trackMatte"] = json!({"layer":401,"mode":"alphaInverted"});
    let mut matte = owner.clone();
    matte.as_object_mut().unwrap().remove("trackMatte");
    matte["name"] = json!("Speed-line matte");
    matte["id"] = json!(401);
    let mut transform = json!(identity_fx_transform());
    transform["opacity"] = json!(40.0);
    value["composition"]["layers"] = json!([{
        "type":"Group", "id":402, "name":"Parent opacity boundary",
        "playback":fixture_linear_playback(owner["activeRange"].clone(), owner["activeRange"].clone()), "transform":transform,
        "layers":[matte,owner]
    }]);
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    let named = |name: &str| {
        native
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::Composition(comp) => Some(&comp.layers),
                _ => None,
            })
            .flatten()
            .find(|layer| layer.name.as_ref() == name)
            .unwrap()
    };
    let owner = named("Chrome face");
    let matte = named("Speed-line matte");
    assert_eq!(owner.record.matte_layer_id(), Some(matte.record.id()));
    assert_eq!(owner.record.track_matte_type(), 2);
    assert!(!matte.record.flags().enabled);
    for layer in [owner, matte] {
        assert!(
            (numeric(&layer.content, "ADBE Opacity").unwrap().values[0] - 0.999).abs() < 1.0e-6
        );
    }
    assert_eq!(
        numeric(&named("Parent opacity boundary").content, "ADBE Opacity")
            .unwrap()
            .values,
        [0.4]
    );
}

#[test]
fn single_paint_does_not_reinterpret_ordinary_low_layer_opacity() {
    let mut value = document(false, false, 1.0, false);
    value["composition"]["layers"][0]["transform"]["opacity"] = json!(0.999);
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(
        numeric(&layers(&native)[0].content, "ADBE Opacity")
            .unwrap()
            .values,
        [0.00999]
    );
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("Single-paint Shape"))
    );
}

#[test]
fn overrange_paint_preserves_layer_opacity_keys() {
    for stroke in [false, true] {
        let output = export(document(false, stroke, 100.0, true));
        let native = read_project(&output.bytes).unwrap();
        let layers = layers(&native);
        assert_eq!(layers.len(), 1, "{:?}", output.diagnostics);
        let name = if stroke {
            "ADBE Vector Stroke Opacity"
        } else {
            "ADBE Vector Fill Opacity"
        };
        let opacity = numeric(&layers[0].content, name).unwrap();
        assert!(!opacity.animated);
        assert_eq!(opacity.values, vec![100.0]);
        let layer_opacity = numeric(&layers[0].content, "ADBE Opacity").unwrap();
        assert!(layer_opacity.animated);
        assert_eq!(layer_opacity.keyframes.len(), 2);
        for (key, (time, expected)) in layer_opacity.keyframes.iter().zip([(0.0, 0.2), (0.5, 0.7)])
        {
            assert_eq!(key.time_secs, time);
            assert!((key.values[0] - expected).abs() < 1.0e-6);
        }
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(400))
                && diagnostic
                    .message
                    .contains("Layer opacity remains post-composite")
        }));
    }
}

fn collect_opacities(chunks: &[Chunk], name: &str, values: &mut Vec<f64>) {
    if let Ok(runs) = properties::runs(chunks) {
        for (match_name, run) in runs {
            if match_name == name {
                let storage = properties::unique_list(run, *b"tdbs").unwrap();
                let property = properties::read_numeric(storage).unwrap();
                assert!(!property.animated);
                values.push(property.values[0]);
            }
        }
    }
    for children in chunks.iter().filter_map(Chunk::children) {
        collect_opacities(children, name, values);
    }
}

#[test]
fn single_paint_normalization_survives_vector_hierarchy_and_skew() {
    for skew in [false, true] {
        let mut value = document(false, false, 100.0, false);
        let shape = &mut value["composition"]["layers"][0];
        shape["transform"]["opacity"] = json!(0.999);
        shape["shape"]["fills"][0]["paint"]["color"][3] = json!(1.0);
        if skew {
            shape["transform"]["skew"] = json!(15.0);
            value["composition"]["dynamics"]["entries"] = json!([keyed_entry(
                LayerId::new(400),
                PropType::ScaleX,
                [
                    (0, PropertyValue::Float(80.0)),
                    (500, PropertyValue::Float(100.0))
                ],
            )]);
        } else {
            value["composition"]["layers"] = json!([{
                "type":"Group", "id":402, "name":"Inline vector parent",
                "playback":fixture_linear_playback(shape["activeRange"].clone(), shape["activeRange"].clone()),
                "transform":identity_fx_transform(), "layers":[shape]
            }]);
        }
        let output = export(value);
        let native = read_project(&output.bytes).unwrap();
        let native_layers = layers(&native);
        assert_eq!(native_layers.len(), 1, "{:?}", output.diagnostics);
        let content = &native_layers[0].content;
        let mut opacities = Vec::new();
        collect_opacities(content, "ADBE Vector Group Opacity", &mut opacities);
        assert_eq!(
            opacities
                .iter()
                .filter(|value| (**value - 99.9).abs() < 1.0e-6)
                .count(),
            1,
            "{opacities:?}"
        );
        assert_eq!(numeric(content, "ADBE Opacity").unwrap().values, [1.0]);
        assert_eq!(
            numeric(content, "ADBE Vector Fill Opacity").unwrap().values,
            [100.0]
        );
    }
}

#[test]
fn overlapping_overrange_paints_keep_post_composite_layer_opacity() {
    for stroke in [false, true] {
        for three_d in [false, true] {
            for animated in [false, true] {
                let mut value = document(false, stroke, 100.0, animated);
                let shape = &mut value["composition"]["layers"][0];
                shape["transform"]["opacity"] = json!(50.0);
                if three_d {
                    shape["transform"]["rotationX"] = json!(5.0);
                }
                let paints = if stroke { "strokes" } else { "fills" };
                let mut paint = shape["shape"][paints][0].clone();
                paint["paint"]["color"][3] = json!(1.0);
                // Identical paints on the same path overlap everywhere.
                shape["shape"][paints] = json!([paint.clone(), paint]);
                let output = export(value);
                let native = read_project(&output.bytes).unwrap();
                let native_layers = layers(&native);
                assert_eq!(native_layers.len(), if three_d { 2 } else { 1 });
                let layer = native_layers
                    .iter()
                    .find(|layer| layer.record.layer_type() == 4)
                    .expect("Shape retained alongside its optional camera");
                assert_eq!(layer.record.flags().three_d_layer, three_d);
                let name = if stroke {
                    "ADBE Vector Stroke Opacity"
                } else {
                    "ADBE Vector Fill Opacity"
                };
                let mut opacities = Vec::new();
                collect_opacities(&layer.content, name, &mut opacities);
                assert_eq!(opacities, vec![100.0, 100.0]);
                let opacity = numeric(&layer.content, "ADBE Opacity").unwrap();
                assert_eq!(opacity.animated, animated);
                if animated {
                    assert_eq!(opacity.keyframes.len(), 2);
                    for (key, (time, expected)) in
                        opacity.keyframes.iter().zip([(0.0, 0.2), (0.5, 0.7)])
                    {
                        assert_eq!(key.time_secs, time);
                        assert!((key.values[0] - expected).abs() < 1.0e-6);
                    }
                } else {
                    assert_eq!(opacity.values, vec![0.5]);
                    // Native source-over combines paints before the layer gate:
                    // overlap is 0.5, not 0.75 from two half-transparent paints.
                    let paint_alpha =
                        1.0 - (1.0 - opacities[0] / 100.0) * (1.0 - opacities[1] / 100.0);
                    assert_eq!(paint_alpha * opacity.values[0], 0.5);
                }
            }
        }
    }
}

#[test]
fn cubic_paint_color_retains_keys_with_linear_approximation_and_sibling() {
    use fx_schema::animator::{
        KeyframeId, PropertyAnimator, PropertyKeyframe, PropertyKeyframeTrack,
    };
    for boolean in [false, true] {
        for stroke in [false, true] {
            let property = if stroke {
                PropType::StrokeColor
            } else {
                PropType::FillColor
            };
            let mut value = document(boolean, stroke, 0.5, true);
            value["composition"]["layers"]
                .as_array_mut()
                .unwrap()
                .push(rect(&imported(), 901));
            let values = [
                (0, PropertyValue::Color([1.0, 0.0, 0.0, 1.0])),
                (250, PropertyValue::Color([0.0, 1.0, 0.0, 1.0])),
                (500, PropertyValue::Color([0.0, 0.0, 1.0, 1.0])),
                (750, PropertyValue::Color([1.0, 1.0, 1.0, 1.0])),
            ];
            let easings = [
                PropertyKeyframeEasing::Linear,
                PropertyKeyframeEasing::CubicBezier {
                    x1: 0.25,
                    y1: 0.0,
                    x2: 0.75,
                    y2: 1.0,
                },
                PropertyKeyframeEasing::Hold,
                PropertyKeyframeEasing::Linear,
            ];
            let mut entry = keyed_entry(LayerId::new(400), property, values.clone());
            entry.animator = PropertyAnimator::keyframes(
                PropertyKeyframeTrack::new(
                    values
                        .into_iter()
                        .zip(easings)
                        .enumerate()
                        .map(|(index, ((time, value), easing))| {
                            PropertyKeyframe::new(
                                KeyframeId::new(format!("paint-cubic-{index}")),
                                fx_schema::TimeOffset::from_millis(time),
                                value,
                                easing,
                            )
                        })
                        .collect(),
                )
                .unwrap(),
            );
            value["composition"]["dynamics"]["entries"]
                .as_array_mut()
                .unwrap()
                .push(json!(entry));
            let output = export(value);
            let native = read_project(&output.bytes).unwrap();
            assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
            let owner = layers(&native)
                .iter()
                .find(|layer| layer.name.as_ref() == "Independent opacity")
                .unwrap();
            let color = numeric(
                &owner.content,
                if stroke {
                    "ADBE Vector Stroke Color"
                } else {
                    "ADBE Vector Fill Color"
                },
            )
            .unwrap();
            assert!(color.animated);
            let expected = [
                (0.0, vec![1.0, 0.0, 0.0, 1.0], 1, 1),
                (0.25, vec![0.0, 1.0, 0.0, 1.0], 1, 3),
                (0.5, vec![0.0, 0.0, 1.0, 1.0], 3, 1),
                (0.75, vec![1.0, 1.0, 1.0, 1.0], 1, 1),
            ];
            assert_eq!(color.keyframes.len(), expected.len());
            for (key, (time, values, incoming, outgoing)) in color.keyframes.iter().zip(expected) {
                assert_eq!(key.time_secs, time);
                assert_eq!(key.values, values);
                assert_eq!(key.in_interpolation, incoming);
                assert_eq!(key.out_interpolation, outgoing);
            }
            assert_eq!(
                numeric(
                    &owner.content,
                    if stroke {
                        "ADBE Vector Stroke Opacity"
                    } else {
                        "ADBE Vector Fill Opacity"
                    }
                )
                .unwrap()
                .values,
                [50.0],
                "color keys leave paint opacity independent of the unused static color alpha"
            );
            assert_eq!(
                numeric(&owner.content, "ADBE Opacity")
                    .unwrap()
                    .keyframes
                    .len(),
                2
            );
            assert!(
                layers(&native)
                    .iter()
                    .any(|layer| layer.name.as_ref() == "Current solid 901")
            );
            assert!(output.diagnostics.iter().any(|d| {
                d.layer_id == Some(LayerId::new(400))
                    && d.message
                        .contains("Cubic paint color easing approximated as Linear")
            }));
        }
    }
}

#[test]
fn paint_alpha_and_opacity_are_not_folded_into_layer_opacity_or_keys() {
    for boolean in [false, true] {
        for stroke in [false, true] {
            for opacity in [0.0, 0.25, 1.0] {
                for animated in [false, true] {
                    let output = export(document(boolean, stroke, opacity, animated));
                    let native = read_project(&output.bytes).unwrap();
                    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
                    let content = &layers(&native)[0].content;
                    let paint_name = if stroke {
                        "ADBE Vector Stroke Opacity"
                    } else {
                        "ADBE Vector Fill Opacity"
                    };
                    let paint = numeric(content, paint_name).unwrap();
                    assert!(!paint.animated);
                    assert_eq!(paint.values, vec![opacity * 100.0 * 0.4]);
                    let color_name = if stroke {
                        "ADBE Vector Stroke Color"
                    } else {
                        "ADBE Vector Fill Color"
                    };
                    assert_eq!(
                        numeric(content, color_name).unwrap().values,
                        vec![0.2, 0.4, 0.6, 1.0]
                    );
                    let layer = numeric(content, "ADBE Opacity").unwrap();
                    assert_eq!(layer.animated, animated);
                    if animated {
                        assert_eq!(layer.keyframes.len(), 2);
                        for (key, (time, value)) in
                            layer.keyframes.iter().zip([(0.0, 0.2), (0.5, 0.7)])
                        {
                            assert_eq!(key.time_secs, time);
                            assert_eq!(key.values, vec![value]);
                            assert_eq!((key.in_interpolation, key.out_interpolation), (1, 1));
                        }
                    } else {
                        assert_eq!(layer.values, vec![0.8]);
                    }
                }
            }
        }
    }
}
