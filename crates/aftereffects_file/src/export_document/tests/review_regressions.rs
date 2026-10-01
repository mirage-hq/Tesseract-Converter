//! Explicit edits to the pinned native Solid import; supplementary CPU evidence,
//! not an independently authored Adobe export or rendering oracle.

use super::*;
use crate::properties::{root_runs, runs, unique_list};

fn find_rect(layers: &[Layer]) -> Option<&RectLayer> {
    layers.iter().find_map(|layer| match layer.data() {
        LayerData::Rect(rect) => Some(rect),
        LayerData::Group(group) => find_rect(&group.layers),
        _ => None,
    })
}

#[test]
fn review_solid_geometry_keys_export_as_editable_rectangle() {
    for inherited in [false, true] {
        for (property, values) in [
            (
                PropType::RectSize,
                [
                    PropertyValue::Vector2([160.0, 80.0]),
                    PropertyValue::Vector2([320.0, 120.0]),
                ],
            ),
            (
                PropType::RectRoundness,
                [PropertyValue::Float(0.0), PropertyValue::Float(12.0)],
            ),
        ] {
            let mut value = imported();
            let mut leaf = rect(&value, 100);
            leaf["rect"]["size"] = json!([160.0, 80.0]);
            if inherited {
                let mut group = value["composition"]["layers"][0].clone();
                group["transform"]["position"] = json!([20.0, 30.0]);
                leaf["parent"] = group["id"].clone();
                group["layers"] = json!([leaf]);
                value["composition"]["layers"] = json!([group]);
            } else {
                value["composition"]["layers"] = json!([leaf]);
            }
            value["composition"]["dynamics"] = json!({"entries": [keyed_entry(
                LayerId::new(100), property, [(0, values[0].clone()), (500, values[1].clone())],
            )]});
            let output = export(value);
            let native = read_project(&output.bytes).unwrap();
            assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
            let root = &layers(&native)[0];
            if inherited {
                assert_eq!(
                    root.record.layer_type(),
                    0,
                    "the nonidentity owner Group must remain a native precomposition occurrence: {:?}",
                    output.diagnostics
                );
                assert!(
                    matches!(
                        &native.item(root.record.source_id()).unwrap().kind,
                        ItemKind::Composition(_)
                    ),
                    "the inherited geometry route must reference an editable composition, not flatten to Solid footage"
                );
            } else {
                assert_eq!(
                    root.record.layer_type(),
                    4,
                    "an unwrapped geometry edit must emit a native Shape layer: {:?}",
                    output.diagnostics
                );
            }
            let reimport = to_structural_fx_document(&native, Some(1)).unwrap();
            let id = find_rect(reimport.document.composition().layers())
                .unwrap()
                .id;
            let entry = reimport
                .document
                .composition()
                .dynamics()
                .entries()
                .iter()
                .find(|entry| entry.target == fx_schema::PropertyTarget::layer(id, property))
                .expect("edited geometry keys survive export");
            let actual: Vec<_> = entry
                .animator
                .keyframe_track()
                .unwrap()
                .keyframes()
                .iter()
                .map(|key| key.value().clone())
                .collect();
            assert_eq!(actual, values);
        }
    }
}

#[test]
fn review_anchor_keys_reimport_and_preserve_solid_local_origin() {
    for vector in [false, true] {
        let mut value = imported();
        let mut leaf = rect(&value, 100);
        leaf["rect"]["position"] = json!([5.0, -7.0]);
        leaf["rect"]["size"] = json!([160.0, 80.0]);
        leaf["transform"]["anchorPoint"] = json!([30.0, 20.0]);
        if vector {
            leaf["description"] = json!("authored vector Rectangle");
        }
        value["composition"]["layers"] = json!([leaf]);
        value["composition"]["dynamics"] = json!({"entries": [
            keyed_entry(LayerId::new(100), PropType::AnchorPointX,
                [(0, PropertyValue::Float(30.0)), (500, PropertyValue::Float(40.0))]),
            keyed_entry(LayerId::new(100), PropType::AnchorPointY,
                [(0, PropertyValue::Float(20.0)), (500, PropertyValue::Float(30.0))]),
        ]});
        let output = export(value);
        let native = read_project(&output.bytes).unwrap();
        let reimport = to_structural_fx_document(&native, Some(1));
        let reimport = reimport.expect("Anchor keys produce a valid editable document");
        for property in [PropType::AnchorPointX, PropType::AnchorPointY] {
            let entry = reimport
                .document
                .composition()
                .dynamics()
                .entries()
                .iter()
                .find(|entry| {
                    entry
                        .target
                        .as_property()
                        .is_some_and(|p| p.property_type() == property)
                })
                .expect("anchor animation retained");
            assert_eq!(
                entry.animator.keyframe_track().unwrap().keyframes().len(),
                2
            );
            assert!(
                !entry
                    .animator
                    .keyframe_track()
                    .unwrap()
                    .has_spatial_tangents()
            );
        }
        let properties = crate::properties::read_transform(&layers(&native)[0].content).unwrap();
        let anchor = properties
            .iter()
            .find(|p| p.match_name == "ADBE Anchor Point")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        // Shape anchors use pixels; Solid anchors use source-relative cdat.
        let expected = if vector {
            [[30.0, 20.0, 0.0], [40.0, 30.0, 0.0]]
        } else {
            [
                [25.0 / 160.0, 27.0 / 80.0, 0.0],
                [35.0 / 160.0, 37.0 / 80.0, 0.0],
            ]
        };
        assert_eq!(anchor.keyframes[0].values, expected[0]);
        assert_eq!(anchor.keyframes[1].values, expected[1]);
    }
}

#[test]
fn review_unsupported_shape_encoding_preserves_supported_sibling() {
    for compound_path in [false, true] {
        let mut value = imported();
        let mut shape = rect(&value, 101);
        shape["type"] = json!("Shape");
        shape.as_object_mut().unwrap().remove("rect");
        shape["name"] = json!(if compound_path {
            "compound".to_owned()
        } else {
            "x".repeat(256)
        });
        shape["shape"] = if compound_path {
            json!({"path": {"commands": [
                {"type":"moveTo","x":0.0,"y":0.0},
                {"type":"lineTo","x":10.0,"y":10.0},
                {"type":"moveTo","x":20.0,"y":20.0},
                {"type":"lineTo","x":30.0,"y":30.0}
            ]}, "fills":[{"paint":{"type":"solid","color":[1.0,0.0,0.0,1.0]}}]})
        } else {
            json!({"path":{"commands":[]}, "ellipse":{"size":[100.0,50.0],"position":[0.0,0.0]},
                "fills":[{"paint":{"type":"solid","color":[1.0,0.0,0.0,1.0]}}]})
        };
        let sibling = rect(&value, 100);
        value["composition"]["layers"] = json!([shape, sibling]);
        let output = export(value);
        let native = read_project(&output.bytes).unwrap();
        // Compound static Paths export as native contours; the over-long name
        // remains an unsupported encoding that omits only its own layer.
        let expected: &[&str] = if compound_path {
            &["compound", "Current solid 100"]
        } else {
            &["Current solid 100"]
        };
        let names: Vec<&str> = layers(&native)
            .iter()
            .map(|layer| layer.name.as_ref())
            .collect();
        assert_eq!(names, expected, "{:?}", output.diagnostics);
        assert_eq!(
            output
                .diagnostics
                .iter()
                .any(|d| d.layer_id == Some(LayerId::new(101)) && d.message.contains("omitted")),
            !compound_path,
            "{:?}",
            output.diagnostics
        );
    }
}

#[test]
fn review_differently_eased_planar_axes_export_as_exact_separated_position() {
    let mut value = imported();
    let leaf = rect(&value, 100);
    let sibling = rect(&value, 101);
    value["composition"]["layers"] = json!([leaf, sibling]);
    let mut x = serde_json::to_value(keyed_entry(
        LayerId::new(100),
        PropType::PositionX,
        [
            (0, PropertyValue::Float(0.0)),
            (500, PropertyValue::Float(100.0)),
        ],
    ))
    .unwrap();
    x["animator"]["keyframes"][1]["easing"] = json!({"type":"hold"});
    let y = keyed_entry(
        LayerId::new(100),
        PropType::PositionY,
        [
            (0, PropertyValue::Float(0.0)),
            (750, PropertyValue::Float(150.0)),
        ],
    );
    value["composition"]["dynamics"] = json!({"entries":[x, y]});

    let output = export(value);
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("implicit default camera"))
    );
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(
        layers(&native)
            .iter()
            .filter(|layer| layer.record.layer_type() == 2)
            .count(),
        0,
        "planar separated Position must not add a camera"
    );
    let render_layers = layers(&native)
        .iter()
        .filter(|layer| layer.record.layer_type() != 2)
        .collect::<Vec<_>>();
    assert_eq!(render_layers.len(), 2, "{:?}", output.diagnostics);
    let edited = render_layers
        .iter()
        .find(|layer| layer.name.as_ref() == "Current solid 100")
        .expect("edited planar layer remains exported");
    assert!(!edited.record.flags().three_d_layer);
    let properties = crate::properties::read_transform(&edited.content).unwrap();
    let position = properties
        .iter()
        .find(|property| property.match_name == "ADBE Position")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    assert!(position.dimensions_separated);
    for (name, expected) in [
        ("ADBE Position_0", [(0.0, 0.0, 1, 3), (0.5, 100.0, 3, 1)]),
        ("ADBE Position_1", [(0.0, 0.0, 1, 1), (0.75, 150.0, 1, 1)]),
    ] {
        let keys = &properties
            .iter()
            .find(|property| property.match_name == name)
            .unwrap_or_else(|| panic!("missing native {name}"))
            .numeric
            .as_ref()
            .unwrap()
            .keyframes;
        assert_eq!(keys.len(), expected.len(), "{name}");
        for (key, (time, value, in_interpolation, out_interpolation)) in keys.iter().zip(expected) {
            assert_eq!(key.time_secs, time, "{name}");
            assert_eq!(key.values, [value], "{name}");
            assert_eq!(key.in_interpolation, in_interpolation, "{name}");
            assert_eq!(key.out_interpolation, out_interpolation, "{name}");
        }
    }
}

#[test]
fn review_single_axis_easing_exports_without_omitting_layer() {
    for property in [PropType::PositionX, PropType::PositionY] {
        for easing in [
            json!({"type":"hold"}),
            json!({"type":"cubicBezier","x1":0.3,"y1":0.0,"x2":0.7,"y2":1.0}),
        ] {
            let mut value = imported();
            let leaf = rect(&value, 100);
            value["composition"]["layers"] = json!([leaf]);
            let mut entry = serde_json::to_value(keyed_entry(
                LayerId::new(100),
                property,
                [
                    (0, PropertyValue::Float(0.0)),
                    (500, PropertyValue::Float(100.0)),
                ],
            ))
            .unwrap();
            entry["animator"]["keyframes"][1]["easing"] = easing.clone();
            value["composition"]["dynamics"] = json!({"entries":[entry]});
            let output = export(value);
            let native = read_project(&output.bytes).unwrap();
            assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
            let reimport = to_structural_fx_document(&native, Some(1)).unwrap();
            let entry = reimport
                .document
                .composition()
                .dynamics()
                .entries()
                .iter()
                .find(|entry| {
                    entry
                        .target
                        .as_property()
                        .is_some_and(|p| p.property_type() == property)
                })
                .unwrap();
            let track = entry.animator.keyframe_track().unwrap();
            assert_eq!(track.keyframes()[1].value(), &PropertyValue::Float(100.0));
            assert_eq!(
                serde_json::to_value(track.keyframes()[1].easing()).unwrap(),
                easing
            );
        }
    }
}

fn review_text_layer(
    value: &Value,
    id: u64,
    name: &str,
    hidden: bool,
    path_layer: Option<u64>,
) -> Value {
    let mut layer = rect(value, id);
    layer["type"] = json!("Text");
    layer["name"] = json!(name);
    layer["isHidden"] = json!(hidden);
    layer.as_object_mut().unwrap().remove("rect");
    layer["sourceText"] = json!({
        "text":"Editable path text", "fontFamily":"Inter-Regular",
        "fontStyle":"Regular", "fontSize":42.0, "applyFill":true,
        "fillColor":[0.1,0.2,0.3,1.0], "applyStroke":false,
        "strokeColor":[0.0,0.0,0.0,1.0], "strokeWidth":0.0,
        "strokeOverFill":false, "justification":"left", "tracking":0.0,
        "leading":50.0, "baselineShift":0.0, "boxText":false,
        "allCaps":false
    });
    if let Some(path_layer) = path_layer {
        layer["pathOptions"] = json!({
            "id": id + 10_000, "pathLayer": path_layer,
            "firstMargin":0.0, "lastMargin":0.0,
            "perpendicularToPath":false, "reversePath":false,
            "forceAlignment":false
        });
    }
    layer
}

fn review_text_path_guide(value: &Value, id: u64) -> Value {
    let mut guide = rect(value, id);
    guide["type"] = json!("Shape");
    guide["name"] = json!("Review text path guide");
    guide.as_object_mut().unwrap().remove("rect");
    guide["shape"] = json!({
        "path":{"commands":[
            {"type":"moveTo","x":-160.0,"y":0.0},
            {"type":"lineTo","x":160.0,"y":0.0}
        ]},
        "fills":[],
        "strokes":[{
            "paint":{"type":"solid","color":[1.0,0.2,0.1,1.0]},
            "width":4.0
        }]
    });
    guide
}

fn review_identity_group(value: &Value, id: u64, mut children: Vec<Value>) -> Value {
    let mut group = value["composition"]["layers"][0].clone();
    group["id"] = json!(id);
    group["name"] = json!("Review identity group");
    group["parent"] = Value::Null;
    group["layers"] = Value::Array(
        children
            .iter_mut()
            .map(|child| {
                child["parent"] = json!(id);
                child.clone()
            })
            .collect(),
    );
    group["isHidden"] = json!(false);
    group["blendMode"] = json!("normal");
    group["trackMatte"] = Value::Null;
    group["masks"] = json!([]);
    group["effects"] = json!([]);
    group["motionBlur"] = json!(false);
    group["paddingTop"] = json!(0.0);
    group["paddingRight"] = json!(0.0);
    group["paddingBottom"] = json!(0.0);
    group["paddingLeft"] = json!(0.0);
    group["fills"] = json!([]);
    group["cornerRadiusTopLeft"] = json!(0.0);
    group["cornerRadiusTopRight"] = json!(0.0);
    group["cornerRadiusBottomRight"] = json!(0.0);
    group["cornerRadiusBottomLeft"] = json!(0.0);
    group
}

fn review_mask_atom_count(layer: &crate::structure::Layer) -> usize {
    root_runs(&layer.content)
        .unwrap()
        .into_iter()
        .find(|(name, _)| *name == "ADBE Mask Parade")
        .map_or(0, |(_, parade)| {
            let parade = unique_list(parade, *b"tdgp").unwrap();
            runs(parade)
                .unwrap()
                .into_iter()
                .filter(|(name, _)| *name == "ADBE Mask Atom")
                .count()
        })
}

#[test]
fn review_export_top_level_sidecars_preserve_hidden_blend_motion_blur_and_matte() {
    let mut value = imported();
    let mut provider = rect(&value, 6_000);
    provider["name"] = json!("Review matte provider");
    let mut target = rect(&value, 6_001);
    target["name"] = json!("Review matted target");
    target["isHidden"] = json!(true);
    target["blendMode"] = json!("multiply");
    target["motionBlur"] = json!(true);
    target["trackMatte"] = json!({"mode":"alphaInverted","layer":6_000});
    value["composition"]["layers"] = json!([target, provider]);
    value["composition"]["dynamics"] = json!({"entries":[]});

    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    let target = layers(&native)
        .iter()
        .find(|layer| layer.name.as_ref() == "Review matted target")
        .expect("top-level target remains editable");
    let provider = layers(&native)
        .iter()
        .find(|layer| layer.name.as_ref() == "Review matte provider")
        .expect("matte provider remains editable");
    assert!(!target.record.flags().enabled);
    assert!(target.record.flags().motion_blur);
    assert_eq!(target.record.blend_mode(), 5);
    assert_eq!(target.record.track_matte_type(), 2);
    assert_eq!(target.record.matte_layer_id(), Some(provider.record.id()));
}

#[test]
fn review_disabled_transform_uses_runtime_value_instead_of_typed_base() {
    let mut value = imported();
    let mut layer = rect(&value, 6_050);
    layer["description"] = json!("edited native vector Rectangle");
    layer["transform"]["position"] = json!([100.0, 200.0]);
    let mut position = keyed_entry(
        LayerId::new(6_050),
        PropType::PositionX,
        [
            (0, PropertyValue::Float(10.0)),
            (500, PropertyValue::Float(20.0)),
        ],
    );
    let mut data = position.animator.data().clone();
    let AnimatorData::Keyframes {
        enabled,
        disabled_value,
        ..
    } = &mut data
    else {
        panic!("keyed position")
    };
    *enabled = false;
    *disabled_value = Some(PropertyValue::Float(333.0));
    position.animator = fx_schema::animator::PropertyAnimator::from_data(&data).unwrap();
    value["composition"]["layers"] = json!([layer]);
    value["composition"]["dynamics"] = json!({"entries":[position]});

    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    let position = super::stroke_keys::numeric(&layers(&native)[0].content, "ADBE Position")
        .expect("native Position");
    assert!(
        !position.keyframes.is_empty(),
        "constant override is encoded as keys"
    );
    for key in &position.keyframes {
        assert_eq!(key.values[..2], [333.0, 200.0]);
    }
}

#[test]
fn review_disabled_animator_without_runtime_value_fails_closed() {
    let entry = keyed_entry(
        LayerId::new(6_051),
        PropType::PositionX,
        [
            (0, PropertyValue::Float(10.0)),
            (500, PropertyValue::Float(20.0)),
        ],
    );
    let mut data = entry.animator.data().clone();
    let AnimatorData::Keyframes {
        enabled,
        disabled_value,
        ..
    } = &mut data
    else {
        panic!("keyed position")
    };
    *enabled = false;
    *disabled_value = None;
    // The portable schema rejects this invalid state before export can see it.
    let error = fx_schema::animator::PropertyAnimator::from_data(&data).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("disabled keyframes require disabledValue")
    );
}

#[test]
fn review_failed_layer_transaction_discards_unpublished_normalization_diagnostics() {
    let mut value = imported();
    let child = rect(&value, 6_060);
    let mut group = review_identity_group(&value, 6_061, vec![child]);
    group["paddingLeft"] = json!(12.0);
    group["fills"] = json!([{"paint":{"type":"solid","color":[0.2,0.3,0.4,1.0]},"opacity":1.0}]);
    group["transform"]["skew"] = json!(15.0);
    let mut sibling = rect(&value, 6_062);
    sibling["name"] = json!("Review retained transaction sibling");
    value["composition"]["layers"] = json!([group, sibling]);
    value["composition"]["dynamics"] = json!({"entries":[]});

    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
    assert_eq!(
        layers(&native)[0].name.as_ref(),
        "Review retained transaction sibling"
    );
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(6_061))
            && diagnostic.message.contains("subtree omitted")
    }));
    assert!(!output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(6_061))
            && diagnostic
                .message
                .contains("normalized to an editable background child")
    }));
}

#[test]
fn review_export_animated_3d_rect_reaches_native_layer() {
    let mut value = imported();
    let mut layer = rect(&value, 6_100);
    layer["name"] = json!("Review animated 3D Rect");
    layer["description"] = json!("edited native vector Rectangle");
    layer["transform"]["position"] = json!([100.0, 200.0, 30.0]);
    layer["transform"]["orientation"] = json!([1.0, 2.0, 3.0]);
    let mut entries = vec![keyed_entry(
        LayerId::new(6_100),
        PropType::PositionZ,
        [
            (0, PropertyValue::Float(30.0)),
            (500, PropertyValue::Float(60.0)),
        ],
    )];
    for (property, start, end) in [
        (PropType::OrientationX, 1.0, 11.0),
        (PropType::OrientationY, 2.0, 22.0),
        (PropType::OrientationZ, 3.0, 33.0),
    ] {
        entries.push(keyed_entry(
            LayerId::new(6_100),
            property,
            [
                (0, PropertyValue::Float(start)),
                (500, PropertyValue::Float(end)),
            ],
        ));
    }
    let mut sibling = rect(&value, 6_101);
    sibling["name"] = json!("Review 3D sibling");
    value["composition"]["layers"] = json!([layer, sibling]);
    value["composition"]["dynamics"] = json!({"entries":entries});

    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(
        layers(&native)
            .iter()
            .filter(|layer| layer.record.layer_type() == 2)
            .count(),
        1,
        "3D roots require exactly one camera: {:?}",
        output.diagnostics
    );
    let render_layers = layers(&native)
        .iter()
        .filter(|layer| layer.record.layer_type() != 2)
        .collect::<Vec<_>>();
    assert_eq!(render_layers.len(), 2, "{:?}", output.diagnostics);
    assert!(
        render_layers
            .iter()
            .any(|layer| layer.name.as_ref() == "Review 3D sibling")
    );
    let layer = render_layers
        .iter()
        .find(|layer| layer.name.as_ref() == "Review animated 3D Rect")
        .expect("edited vector Rectangle remains exported");
    assert_eq!(layer.record.layer_type(), 4);
    assert!(layer.record.flags().three_d_layer);
    let properties = crate::properties::read_transform(&layer.content).unwrap();
    for name in ["ADBE Position", "ADBE Orientation"] {
        let property = properties
            .iter()
            .find(|property| property.match_name == name)
            .unwrap_or_else(|| panic!("missing native {name}"));
        let numeric = property.numeric.as_ref().unwrap();
        assert!(numeric.animated, "{name} must remain animated");
        assert_eq!(numeric.keyframes.len(), 2, "{name} key count");
    }
}

#[test]
fn review_export_animated_3d_boolean_reaches_native_layer_and_keeps_sibling() {
    let mut value = super::stroke_keys::document(
        "BooleanOperation",
        PropertyKeyframeEasing::Linear,
        false,
        false,
    );
    let boolean = &mut value["composition"]["layers"][0];
    boolean["name"] = json!("Review animated 3D Boolean");
    boolean["transform"]["position"] = json!([100.0, 200.0, 30.0]);
    boolean["transform"]["orientation"] = json!([1.0, 2.0, 3.0]);
    let entries = [
        (PropType::PositionZ, 30.0, 60.0),
        (PropType::RotationX, 4.0, 14.0),
        (PropType::RotationY, 5.0, 15.0),
        (PropType::OrientationX, 1.0, 11.0),
        (PropType::OrientationY, 2.0, 22.0),
        (PropType::OrientationZ, 3.0, 33.0),
    ]
    .into_iter()
    .map(|(property, start, end)| {
        keyed_entry(
            LayerId::new(400),
            property,
            [
                (0, PropertyValue::Float(start)),
                (500, PropertyValue::Float(end)),
            ],
        )
    })
    .collect::<Vec<_>>();
    let mut sibling = rect(&imported(), 6_201);
    sibling["name"] = json!("Review Boolean sibling");
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .push(sibling);
    value["composition"]["dynamics"] = json!({"entries":entries});

    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(
        layers(&native)
            .iter()
            .filter(|layer| layer.record.layer_type() == 2)
            .count(),
        1,
        "3D roots require exactly one camera: {:?}",
        output.diagnostics
    );
    let render_layers = layers(&native)
        .iter()
        .filter(|layer| layer.record.layer_type() != 2)
        .collect::<Vec<_>>();
    assert_eq!(render_layers.len(), 2, "{:?}", output.diagnostics);
    assert!(
        render_layers
            .iter()
            .any(|layer| layer.name.as_ref() == "Review Boolean sibling")
    );
    let layer = render_layers
        .iter()
        .find(|layer| layer.name.as_ref() == "Review animated 3D Boolean")
        .expect("edited Boolean remains exported");
    assert!(layer.record.flags().three_d_layer);
    let properties = crate::properties::read_transform(&layer.content).unwrap();
    for name in [
        "ADBE Position",
        "ADBE Orientation",
        "ADBE Rotate X",
        "ADBE Rotate Y",
    ] {
        let numeric = properties
            .iter()
            .find(|property| property.match_name == name)
            .unwrap_or_else(|| panic!("missing native {name}"))
            .numeric
            .as_ref()
            .unwrap();
        assert!(numeric.animated, "{name} must remain animated");
        assert_eq!(numeric.keyframes.len(), 2, "{name} key count");
    }
}

#[test]
fn review_omitted_matte_provider_prunes_dependents_transitively_but_not_siblings() {
    let mut value = imported();
    let mut provider = rect(&value, 7_000);
    provider["type"] = json!("Shape");
    provider.as_object_mut().unwrap().remove("rect");
    provider["name"] = json!("x".repeat(256));
    provider["shape"] = json!({
        "path":{"commands":[]},
        "ellipse":{"size":[100.0,50.0],"position":[0.0,0.0]},
        "fills":[{"paint":{"type":"solid","color":[1.0,0.0,0.0,1.0]}}]
    });
    let mut first = rect(&value, 7_001);
    first["name"] = json!("First omitted dependent");
    first["trackMatte"] = json!({"mode":"alpha","layer":7_000});
    let mut second = rect(&value, 7_002);
    second["name"] = json!("Second omitted dependent");
    second["trackMatte"] = json!({"mode":"alpha","layer":7_001});
    let mut sibling = rect(&value, 7_003);
    sibling["name"] = json!("Unrelated retained sibling");
    value["composition"]["layers"] = json!([second, first, provider, sibling]);
    value["composition"]["dynamics"] = json!({"entries":[]});

    let output = export(value);
    assert_eq!(
        output.omitted_layer_ids,
        [7_000, 7_001, 7_002]
            .into_iter()
            .map(LayerId::new)
            .collect(),
        "source failure and transitive owners must be typed omissions"
    );
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
    assert_eq!(
        layers(&native)[0].name.as_ref(),
        "Unrelated retained sibling"
    );
    for (owner, target) in [(7_001, 7_000), (7_002, 7_001)] {
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(owner))
                && diagnostic.message.contains(&format!("FX layer {target}"))
                && diagnostic.message.contains("omitted transitively")
        }));
    }
}

#[test]
fn review_nonexistent_matte_provider_remains_a_strict_writer_error() {
    let mut value = imported();
    let mut target = rect(&value, 7_100);
    target["trackMatte"] = json!({"mode":"alpha","layer":9_999});
    value["composition"]["layers"] = json!([target]);
    value["composition"]["dynamics"] = json!({"entries":[]});
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();

    let error = to_aep(&document)
        .err()
        .expect("strict validation must fail");
    assert_eq!(
        error,
        AepWriteError::Invalid("native layer reference crosses or escapes its composition")
    );
}

#[test]
fn review_export_hidden_owner_leaves_visible_text_path_guide() {
    let mut value = imported();
    let guide = review_text_path_guide(&value, 6_200);
    let owner = review_text_layer(
        &value,
        6_201,
        "Review hidden text path owner",
        true,
        Some(6_200),
    );
    value["composition"]["layers"] = json!([guide, owner]);
    value["composition"]["dynamics"] = json!({"entries":[]});

    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(
        layers(&native)
            .iter()
            .filter(|layer| layer.name.as_ref() == "Review text path guide")
            .count(),
        1,
        "a disabled owner must not consume its visible guide: {:?}",
        output.diagnostics
    );
}

#[test]
fn review_export_visible_owner_consumes_text_path_guide_once() {
    let mut value = imported();
    let guide = review_text_path_guide(&value, 6_300);
    let owner = review_text_layer(
        &value,
        6_301,
        "Review visible text path owner",
        false,
        Some(6_300),
    );
    value["composition"]["layers"] = json!([guide, owner]);
    value["composition"]["dynamics"] = json!({"entries":[]});

    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert!(
        layers(&native)
            .iter()
            .all(|layer| layer.name.as_ref() != "Review text path guide")
    );
    let owner = layers(&native)
        .iter()
        .find(|layer| layer.name.as_ref() == "Review visible text path owner")
        .expect("visible Text owner remains editable");
    assert_eq!(review_mask_atom_count(owner), 1);
    assert!(
        output.omitted_layer_ids.is_empty(),
        "consumed guides are not failed layers"
    );
}

#[test]
fn review_audit_overflowing_text_is_omitted_without_losing_siblings() {
    for auto_leading in [false, true] {
        let mut value = imported();
        let mut text = review_text_layer(&value, 7_100, "Overflowing text", false, None);
        if auto_leading {
            text["sourceText"]["fontSize"] = json!(f64::MAX);
            text["sourceText"]["leading"] = Value::Null;
        } else {
            text["sourceText"]["boxText"] = json!(true);
            text["sourceText"]["boxSize"] = json!([f64::MAX, 1.0]);
            text["sourceText"]["boxPosition"] = json!([f64::MAX, 0.0]);
        }
        value["composition"]["layers"] = json!([text, rect(&value, 7_101)]);
        value["composition"]["dynamics"] = json!({"entries":[]});
        let output = export(value);
        let native = read_project(&output.bytes).unwrap();
        assert_eq!(layers(&native).len(), 1, "{:?}", output.diagnostics);
        assert_eq!(layers(&native)[0].name.as_ref(), "Current solid 7101");
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.layer_id == Some(LayerId::new(7_100)))
        );
    }
}

#[test]
fn review_audit_generated_background_does_not_capture_dangling_matte() {
    let mut value = imported();
    let mut group = review_identity_group(&value, 7_000, vec![rect(&value, 7_010)]);
    group["paddingLeft"] = json!(12.0);
    group["fills"] = json!([{"paint":{"type":"solid","color":[0.2,0.3,0.4,1.0]},"opacity":1.0}]);
    let mut owner = rect(&value, 7_020);
    owner["trackMatte"] = json!({"mode":"alpha","layer":7_001});
    value["composition"]["layers"] = json!([group, owner]);
    value["composition"]["dynamics"] = json!({"entries":[]});
    let document = EditableFxCompositionDocument::from_json_value(value).unwrap();
    assert_eq!(
        to_aep(&document).err(),
        Some(AepWriteError::Invalid(
            "native layer reference crosses or escapes its composition"
        )),
        "generated content must not repair a dangling matte"
    );
}

#[test]
fn review_audit_generated_background_does_not_capture_dangling_animation() {
    let mut value = imported();
    let mut group = review_identity_group(&value, 7_000, vec![rect(&value, 7_010)]);
    group["paddingLeft"] = json!(12.0);
    group["fills"] = json!([{"paint":{"type":"solid","color":[0.2,0.3,0.4,1.0]},"opacity":1.0}]);
    value["composition"]["layers"] = json!([group]);
    value["composition"]["dynamics"] = json!({"entries": [keyed_entry(
        LayerId::new(7_001), PropType::Opacity,
        [(0, PropertyValue::Float(0.0)), (500, PropertyValue::Float(100.0))],
    )]});
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    let mut backgrounds = 0;
    for item in &native.items {
        if let ItemKind::Composition(comp) = &item.kind {
            for layer in &comp.layers {
                if layer.name.as_ref() == "Review identity group Background" {
                    backgrounds += 1;
                }
                for property in crate::properties::read_transform(&layer.content).unwrap() {
                    if property.match_name == "ADBE Opacity" {
                        assert!(
                            !property.numeric.unwrap().animated,
                            "dangling animator captured {}",
                            layer.name
                        );
                    }
                }
            }
        }
    }
    assert_eq!(backgrounds, 1, "generated background must remain editable");
}

#[test]
fn review_export_group_without_background_does_not_reserve_generated_id() {
    let mut value = imported();
    let group_id = u64::MAX;
    let child_a = rect(&value, group_id - 1);
    let child_b = review_text_layer(&value, group_id - 2, "Review non-vector child", false, None);
    let group = review_identity_group(&value, group_id, vec![child_a, child_b]);
    value["composition"]["layers"] = json!([group]);
    value["composition"]["dynamics"] = json!({"entries":[]});

    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    assert!(
        output
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.message.contains("identity space is exhausted")),
        "a layout-free Group must not reserve a background identity: {:?}",
        output.diagnostics
    );
    let solid_name = format!("Current solid {}", group_id - 1);
    for name in [
        "Review identity group",
        "Review non-vector child",
        solid_name.as_str(),
    ] {
        assert!(
            layers(&native)
                .iter()
                .any(|layer| layer.name.as_ref() == name),
            "missing {name}: {:?}",
            output.diagnostics
        );
    }
}
