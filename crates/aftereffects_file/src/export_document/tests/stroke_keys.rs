use super::*;
use crate::{properties, rifx::Chunk};
use fx_schema::animator::{KeyframeId, PropertyAnimator, PropertyKeyframe, PropertyKeyframeTrack};

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

pub(super) fn numeric_layer<'a>(
    project: &'a StructuralProject,
    native_layers: &'a [crate::structure::Layer],
    target: &str,
) -> Option<(&'a crate::structure::Layer, properties::NumericProperty)> {
    for layer in native_layers {
        if let Some(property) = numeric(&layer.content, target) {
            return Some((layer, property));
        }
        let Some(item) = project.item(layer.record.source_id()) else {
            continue;
        };
        if let ItemKind::Composition(composition) = &item.kind
            && let Some(found) = numeric_layer(project, &composition.layers, target)
        {
            return Some(found);
        }
    }
    None
}

fn entry(property: PropType, easing: PropertyKeyframeEasing) -> AnimationGraphEntry {
    let (from, to) = if property == PropType::StrokeMiterLimit {
        (4.0, 12.0)
    } else {
        (2.0, 10.0)
    };
    let mut entry = keyed_entry(
        LayerId::new(400),
        property,
        [
            (0, PropertyValue::Float(from)),
            (500, PropertyValue::Float(to)),
        ],
    );
    entry.animator = PropertyAnimator::keyframes(
        PropertyKeyframeTrack::new(
            [(0, from), (500, to)]
                .into_iter()
                .enumerate()
                .map(|(index, (time, value))| {
                    PropertyKeyframe::new(
                        KeyframeId::new(format!("stroke-{property:?}-{index}")),
                        fx_schema::TimeOffset::from_millis(time),
                        PropertyValue::Float(value),
                        easing,
                    )
                })
                .collect(),
        )
        .unwrap(),
    );
    entry
}

pub(super) fn document(
    kind: &str,
    easing: PropertyKeyframeEasing,
    inherited: bool,
    delayed: bool,
) -> Value {
    // Explicit edited FX; the pinned Transform fixture supplies archive metadata,
    // not independent Adobe-native stroke-animation evidence.
    let mut value = imported();
    let mut leaf = rect(&value, 400);
    leaf["transform"] = value["composition"]["layers"][0]["transform"].clone();
    leaf["rect"]["fillEnabled"] = json!(false);
    leaf["rect"]["strokeEnabled"] = json!(true);
    leaf["rect"]["strokeColor"] = json!([0.2, 0.4, 0.6, 1.0]);
    leaf["rect"]["strokeWidth"] = json!(6.0);
    if delayed {
        leaf["activeRange"] = json!({"start":250,"duration":1500});
    }
    if kind != "Rect" {
        leaf["type"] = json!("Shape");
        leaf.as_object_mut().unwrap().remove("rect");
        leaf["shape"] = json!({
            "path":{"commands":[
                {"type":"moveTo","x":0.0,"y":0.0},
                {"type":"lineTo","x":100.0,"y":0.0},
                {"type":"lineTo","x":100.0,"y":80.0}, {"type":"close"}
            ]}, "fills":[], "strokes":[{
                "paint":{"type":"solid","color":[0.2,0.4,0.6,1.0]}, "width":6.0
            }]
        });
        if kind == "BooleanOperation" {
            let mut a = leaf.clone();
            a["id"] = json!(401);
            a["parent"] = json!(400);
            a["shape"]["strokes"] = json!([]);
            let mut b = a.clone();
            b["id"] = json!(402);
            leaf = json!({
                "type":"BooleanOperation", "id":400, "parent":null, "name":"Stroke keys",
                "activeRange":leaf["activeRange"], "transform":leaf["transform"],
                "op":"union", "layers":[a,b], "fills":[], "strokes":leaf["shape"]["strokes"]
            });
        }
    }
    if inherited {
        let mut group = value["composition"]["layers"][0].clone();
        leaf["parent"] = group["id"].clone();
        group["transform"]["rotation"] = json!(30.0);
        group["layers"] = json!([leaf]);
        leaf = group;
    }
    value["composition"]["layers"] = json!([leaf]);
    value["composition"]["dynamics"] = json!({"entries":[
        entry(PropType::StrokeWidth, easing), entry(PropType::StrokeMiterLimit, easing)
    ]});
    value
}

#[test]
fn edited_stroke_keys_keep_values_easing_and_leaf_clock_for_each_native_kind() {
    for kind in ["Rect", "Shape", "BooleanOperation"] {
        for (easing, interpolation) in [
            (PropertyKeyframeEasing::Linear, 1),
            (PropertyKeyframeEasing::Hold, 3),
            (
                PropertyKeyframeEasing::CubicBezier {
                    x1: 0.25,
                    y1: 0.2,
                    x2: 0.75,
                    y2: 0.8,
                },
                2,
            ),
        ] {
            for inherited in [false, true] {
                for delayed in [false, true] {
                    let output = export(document(kind, easing, inherited, delayed));
                    let native = read_project(&output.bytes).unwrap();
                    assert_eq!(layers(&native).len(), 1, "{kind}: {:?}", output.diagnostics);
                    let (layer, width) =
                        numeric_layer(&native, layers(&native), "ADBE Vector Stroke Width")
                            .expect("stroke owner survives in its generated composition");
                    if inherited {
                        assert_eq!(
                            numeric_layer(&native, layers(&native), "ADBE Rotate Z")
                                .expect("inherited Group rotation survives")
                                .1
                                .values,
                            vec![30.0]
                        );
                    }
                    if delayed {
                        assert_eq!(layer.record.start_time(), Some(0.25));
                    }
                    for (name, property) in [
                        ("ADBE Vector Stroke Width", width),
                        (
                            "ADBE Vector Stroke Miter Limit",
                            numeric(&layer.content, "ADBE Vector Stroke Miter Limit").unwrap(),
                        ),
                    ] {
                        assert!(property.animated);
                        assert_eq!(property.keyframes.len(), 2);
                        let (from, to) = if name == "ADBE Vector Stroke Width" {
                            (2.0, 10.0)
                        } else {
                            (4.0, 12.0)
                        };
                        assert_eq!(property.keyframes[0].values, vec![from]);
                        assert_eq!(property.keyframes[1].values, vec![to]);
                        assert_eq!(property.keyframes[0].time_secs, 0.0);
                        assert_eq!(property.keyframes[1].time_secs, 0.5);
                        assert_eq!(property.keyframes[0].out_interpolation, interpolation);
                        assert_eq!(property.keyframes[1].in_interpolation, interpolation);
                        if interpolation == 2 {
                            assert!((property.keyframes[0].out_speed[0] - 12.8).abs() < 1e-6);
                            assert!((property.keyframes[1].in_speed[0] - 12.8).abs() < 1e-6);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn unsupported_stroke_motion_is_omitted_without_losing_a_solid_sibling() {
    for case in [
        "out_of_bounds",
        "overshoot",
        "absent_stroke",
        "constant",
        "disabled",
    ] {
        let easing = if case == "overshoot" {
            PropertyKeyframeEasing::CubicBezier {
                x1: 0.25,
                y1: -0.5,
                x2: 0.75,
                y2: 1.0,
            }
        } else {
            PropertyKeyframeEasing::Linear
        };
        let mut value = document("Rect", easing, false, false);
        let mut width = entry(PropType::StrokeWidth, easing);
        if case == "out_of_bounds" {
            width = keyed_entry(
                LayerId::new(400),
                PropType::StrokeWidth,
                [
                    (0, PropertyValue::Float(1.0)),
                    (500, PropertyValue::Float(100_001.0)),
                ],
            );
        }
        if case == "constant" {
            width.animator = PropertyAnimator::constant(PropertyValue::Float(6.0)).unwrap();
        }
        if case == "disabled" {
            let mut data = width.animator.data().clone();
            let AnimatorData::Keyframes {
                enabled,
                disabled_value,
                ..
            } = &mut data
            else {
                panic!("keyed test entry");
            };
            *enabled = false;
            *disabled_value = Some(PropertyValue::Float(8.0));
            width.animator = PropertyAnimator::from_data(&data).unwrap();
        }
        value["composition"]["dynamics"] = json!({"entries":[width]});
        if case == "absent_stroke" {
            value["composition"]["layers"][0]["rect"]["strokeEnabled"] = json!(false);
            value["composition"]["layers"][0]["rect"]["fillEnabled"] = json!(true);
        }
        value["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .push(rect(&imported(), 900));
        let output = export(value);
        let native = read_project(&output.bytes).unwrap();
        if matches!(case, "constant" | "disabled") {
            assert_eq!(layers(&native).len(), 2, "{case}: {:?}", output.diagnostics);
            let stroke_layer = layers(&native)
                .iter()
                .find(|layer| layer.name.as_ref() != "Current solid 900")
                .expect("supported constant/disabled Stroke remains editable");
            let (_, width) = numeric_layer(
                &native,
                std::slice::from_ref(stroke_layer),
                "ADBE Vector Stroke Width",
            )
            .expect("supported constant/disabled Stroke has a native property");
            assert!(width.animated);
            assert_eq!(width.keyframes.len(), 1);
            assert_eq!(width.keyframes[0].time_secs, 0.0);
            assert_eq!(
                width.keyframes[0].values,
                [if case == "disabled" { 8.0 } else { 6.0 }]
            );
        } else {
            assert_eq!(layers(&native).len(), 1, "{case}: {:?}", output.diagnostics);
            assert_eq!(layers(&native)[0].name.as_ref(), "Current solid 900");
            assert!(output.diagnostics.iter().any(|diagnostic| {
                diagnostic.layer_id == Some(LayerId::new(400))
                    && diagnostic.message.contains("omitted")
            }));
        }
    }
}
