//! Opt-in structural export coverage for the implemented E01-E16 branches.
//!
//! Every case starts from an explicitly edited FX document and writes a fresh
//! AEP. The pinned Adobe-authored source below is used only as a valid editable
//! document envelope; neither our own reader nor a writer round trip is an
//! independent Adobe export oracle. No generated AEP fixture is checked in.

use super::*;
use fx_schema::layer::{ShapeLineJoin, ShapePathCommand, ShapePolyStarType};
use sha2::{Digest, Sha256};

const SOURCE_SHA256: &str = "1004b3ee82efd5e24b90ff67d8dd8c89a92c9a5ae554d69d96dc8847cdc61537";
const SOURCE_COMPOSITION_ID: u32 = 1;

fn edited_input() -> Value {
    let bytes = include_bytes!("../../../tests/fixtures/properties/transform_unseparated.aep");
    assert_eq!(format!("{:x}", Sha256::digest(bytes)), SOURCE_SHA256);
    let source = read_project(bytes).expect("pinned Adobe-authored source");
    to_structural_fx_document(&source, Some(SOURCE_COMPOSITION_ID))
        .expect("pinned source composition")
        .document
        .to_json_value()
        .expect("editable source envelope")
}

fn fresh_export(value: Value) -> ExportedDocument {
    let edited =
        EditableFxCompositionDocument::from_json_value(value).expect("explicit edited FX input");
    let archive = edited.to_json_vec().expect("edited archive");
    let reopened =
        EditableFxCompositionDocument::from_json_slice(&archive).expect("fresh archive read");
    to_aep(&reopened).expect("fresh structural AEP export")
}

fn fresh_project(output: &ExportedDocument) -> StructuralProject {
    read_project(&output.bytes).expect("own-reader structural inspection")
}

fn oversized_key_entry(id: LayerId) -> fx_schema::animator::AnimationGraphEntry {
    use fx_schema::animator::{
        AnimationGraphEntry, KeyframeId, PropertyAnimator, PropertyKeyframe, PropertyKeyframeTrack,
    };
    let track = PropertyKeyframeTrack::new(
        (0..=10_000)
            .map(|index| {
                PropertyKeyframe::new(
                    KeyframeId::new(format!("oversized-{index}")),
                    fx_schema::TimeOffset::from_millis(index),
                    PropertyValue::Float(index as f64),
                    PropertyKeyframeEasing::Linear,
                )
            })
            .collect(),
    )
    .expect("valid FX track above the native writer limit");
    AnimationGraphEntry {
        target: fx_schema::PropertyTarget::layer(id, PropType::Rotation),
        animator: PropertyAnimator::keyframes(track),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

fn leaf(value: &Value, id: u64, name: &str) -> Value {
    let mut value = rect(value, id);
    value["name"] = json!(name);
    value
}

fn vector_shape(value: &Value, id: u64, name: &str, shape: Value) -> Value {
    let mut value = leaf(value, id, name);
    value["type"] = json!("Shape");
    value.as_object_mut().expect("layer object").remove("rect");
    value["description"] = json!("explicit edited FX vector input");
    value["shape"] = shape;
    value
}

fn solid_fill(color: [f64; 4]) -> Value {
    json!({"paint":{"type":"solid","color":color}})
}

fn path(left: f64) -> Value {
    json!({"commands":[
        {"type":"moveTo","x":left,"y":0.0},
        {"type":"cubicTo","c1x":left+8.0,"c1y":-5.0,"c2x":left+32.0,"c2y":25.0,"x":left+40.0,"y":20.0},
        {"type":"lineTo","x":left,"y":30.0},
        {"type":"close"}
    ]})
}

fn shape_fill(value: &Value, id: u64, name: &str, geometry: Value) -> Value {
    let mut shape = vector_shape(
        value,
        id,
        name,
        json!({"path":{"commands":[]},"fills":[solid_fill([0.2,0.4,0.8,1.0])],"strokes":[]}),
    );
    for (key, value) in geometry.as_object().expect("geometry object") {
        shape["shape"][key] = value.clone();
    }
    shape
}

fn boolean(value: &Value, id: u64, name: &str, op: &str, mut operands: Vec<Value>) -> Value {
    let span = value["composition"]["layers"][0]["playback"]["inputRange"].clone();
    let transform = value["composition"]["layers"][0]["transform"].clone();
    for operand in &mut operands {
        operand["parent"] = json!(id);
        operand["activeRange"] = span.clone();
        operand["transform"] = transform.clone();
        match operand["type"].as_str() {
            Some("Rect") => {
                operand["rect"]["fillEnabled"] = json!(false);
                operand["rect"]["strokeEnabled"] = json!(false);
            }
            Some("Shape") => {
                operand["shape"]["fills"] = json!([]);
                operand["shape"]["strokes"] = json!([]);
            }
            _ => {}
        }
    }
    json!({
        "type":"BooleanOperation", "id":id, "name":name,
        "parent":null, "activeRange":span, "transform":transform,
        "op":op, "layers":operands, "fills":[solid_fill([0.7,0.2,0.1,1.0])],
        "strokes":[]
    })
}

fn find_shape<'a>(layers: &'a [fx_schema::Layer], name: &str) -> Option<&'a fx_schema::ShapeLayer> {
    layers.iter().find_map(|layer| match layer.data() {
        LayerData::Shape(shape) if shape.name == name => Some(shape),
        LayerData::Group(group) => find_shape(&group.layers, name),
        _ => None,
    })
}

fn find_rect<'a>(layers: &'a [fx_schema::Layer], name: &str) -> Option<&'a RectLayer> {
    layers.iter().find_map(|layer| match layer.data() {
        LayerData::Rect(rect) if rect.name == name => Some(rect),
        LayerData::Group(group) => find_rect(&group.layers, name),
        _ => None,
    })
}

fn find_boolean<'a>(
    layers: &'a [fx_schema::Layer],
    name: &str,
) -> Option<&'a fx_schema::BooleanOperationLayer> {
    layers.iter().find_map(|layer| match layer.data() {
        LayerData::BooleanOperation(boolean) if boolean.name == name => Some(boolean),
        LayerData::Group(group) => find_boolean(&group.layers, name),
        _ => None,
    })
}

fn reimport(output: &ExportedDocument) -> fx_schema::EditableFxCompositionDocument {
    to_structural_fx_document(&fresh_project(output), Some(1))
        .expect("own-reader editable structural reimport")
        .document
}

fn property_values(
    document: &fx_schema::EditableFxCompositionDocument,
    id: LayerId,
    property: PropType,
) -> Vec<PropertyValue> {
    document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .find(|entry| entry.target == fx_schema::PropertyTarget::layer(id, property))
        .unwrap_or_else(|| panic!("missing {property:?} native record"))
        .animator
        .keyframe_track()
        .expect("native numeric key track")
        .keyframes()
        .iter()
        .map(|key| key.value().clone())
        .collect()
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn edited_archive_composition_and_solid_are_fresh_native_records() {
    let mut value = edited_input();
    value["dimensions"] = json!({"width":777,"height":431});
    value["duration"] = json!(1.001);
    value["backgroundColor"] = json!([0.1, 0.2, 0.3, 1.0]);
    let mut solid = leaf(&value, 101, "Edited native solid");
    solid["rect"]["size"] = json!([123.0, 77.0]);
    solid["rect"]["position"] = json!([9.0, -4.0]);
    solid["rect"]["fillColor"] = json!([0.125, 0.5, 0.875, 1.0]);
    solid["transform"]["anchorPoint"] = json!([40.0, 30.0]);
    solid["transform"]["position"] = json!([211.0, 133.0]);
    value["composition"]["layers"] = json!([solid]);
    value["composition"]["dynamics"] = json!({"entries":[]});

    let output = fresh_export(value);
    let project = fresh_project(&output);
    let ItemKind::Composition(comp) = &project.item(1).expect("fresh composition").kind else {
        panic!("fresh composition item")
    };
    assert_eq!((comp.width, comp.height), (777, 431));
    assert_eq!(comp.frame_rate, 24.0);
    assert_eq!(comp.pixel_aspect, (1, 1));
    assert_eq!(comp.layers.len(), 1, "{:?}", output.diagnostics);
    let layer = &comp.layers[0];
    assert_ne!(layer.record.source_id(), 0);
    let source = project
        .item(layer.record.source_id())
        .expect("fresh Solid source item")
        .solid
        .as_ref()
        .expect("fresh Solid source record")
        .as_ref()
        .expect("valid fresh Solid source record");
    assert_eq!((source.width, source.height), (123, 77));
    assert_eq!(source.color, [0.125_f32, 0.5, 0.875]);
    assert_eq!(source.pixel_aspect, (1, 1));
    assert_ne!(
        output.bytes.as_slice(),
        include_bytes!("../../../tests/fixtures/properties/transform_unseparated.aep")
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("25 frames"))
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("background omitted"))
    );
    let transform =
        crate::properties::read_transform(&layer.content).expect("native solid Transform");
    assert_eq!(
        transform
            .iter()
            .find(|property| property.match_name == "ADBE Anchor Point")
            .expect("native Solid anchor")
            .numeric
            .as_ref()
            .expect("numeric native Solid anchor")
            .values,
        [31.0, 34.0, 0.0]
    );
    assert_eq!(
        transform
            .iter()
            .find(|property| property.match_name == "ADBE Position")
            .expect("native Solid position")
            .numeric
            .as_ref()
            .expect("numeric native Solid position")
            .values,
        [211.0, 133.0, 0.0]
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn rectangle_fill_and_each_supported_stroke_join_keep_native_semantics() {
    for (index, join) in ["miter", "round", "bevel"].into_iter().enumerate() {
        let mut value = edited_input();
        let mut fill = leaf(&value, 200, "Fill rectangle");
        fill["description"] = json!("edited vector Rectangle");
        fill["rect"]["size"] = json!([140.0, 60.0]);
        fill["rect"]["position"] = json!([7.0, -9.0]);
        fill["rect"]["roundness"] = json!(11.0);
        fill["rect"]["fillColor"] = json!([0.2, 0.3, 0.4, 1.0]);
        let mut stroke = leaf(&value, 201, &format!("Stroke {join}"));
        stroke["description"] = json!("edited vector Rectangle");
        stroke["rect"]["fillEnabled"] = json!(false);
        stroke["rect"]["strokeEnabled"] = json!(true);
        stroke["rect"]["strokeColor"] = json!([0.8, 0.1, 0.2, 1.0]);
        stroke["rect"]["strokeWidth"] = json!(3.0 + index as f64);
        stroke["rect"]["strokeJoin"] = json!(join);
        stroke["rect"]["strokeMiterLimit"] = json!(9.0);
        value["composition"]["layers"] = json!([fill, stroke]);
        value["composition"]["dynamics"] = json!({"entries":[]});

        let output = fresh_export(value);
        let imported = reimport(&output);
        let fill = find_rect(imported.composition().layers(), "Fill rectangle")
            .expect("native fill Rectangle");
        assert_eq!(fill.rect.size, [140.0, 60.0]);
        assert_eq!(fill.rect.position, [0.0, 0.0]);
        assert_eq!(fill.transform.position, Position::TwoD([77.0, 21.0]));
        assert_eq!(fill.rect.roundness, 11.0);
        assert!(fill.rect.fill_enabled);
        assert!(!fill.rect.stroke_enabled);
        let stroke = find_rect(imported.composition().layers(), &format!("Stroke {join}"))
            .expect("native stroke Rectangle");
        assert!(!stroke.rect.fill_enabled);
        assert!(stroke.rect.stroke_enabled);
        assert_eq!(stroke.rect.stroke_width.value(), 3.0 + index as f64);
        assert_eq!(
            stroke.rect.stroke_join,
            [
                ShapeLineJoin::Miter,
                ShapeLineJoin::Round,
                ShapeLineJoin::Bevel
            ][index]
        );
        assert_eq!(stroke.rect.stroke_miter_limit, 9.0);
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn path_ellipse_star_and_polygon_export_as_editable_native_geometry() {
    let mut value = edited_input();
    let path_layer = shape_fill(&value, 300, "Cubic path", json!({"path":path(4.0)}));
    let ellipse = shape_fill(
        &value,
        301,
        "Ellipse",
        json!({
            "ellipse":{"size":[120.0,66.0],"position":[9.0,-6.0],"reversed":true}
        }),
    );
    let star = shape_fill(
        &value,
        302,
        "Star",
        json!({
            "polyStar":{"starType":"star","points":7.0,"position":[3.0,4.0],"rotation":15.0,
                "outerRadius":60.0,"innerRadius":24.0,"outerRoundness":5.0,"innerRoundness":2.0,"reversed":false}
        }),
    );
    let polygon = shape_fill(
        &value,
        303,
        "Polygon",
        json!({
            "polyStar":{"starType":"polygon","points":6.0,"position":[-2.0,8.0],"rotation":22.0,
                "outerRadius":48.0,"innerRadius":20.0,"outerRoundness":3.0,"innerRoundness":1.0,"reversed":true}
        }),
    );
    value["composition"]["layers"] = json!([path_layer, ellipse, star, polygon]);
    value["composition"]["dynamics"] = json!({"entries":[]});

    let output = fresh_export(value);
    let imported = reimport(&output);
    let path =
        find_shape(imported.composition().layers(), "Cubic path").expect("native cubic Path");
    assert!(
        path.shape
            .path
            .commands
            .iter()
            .any(|command| matches!(command, ShapePathCommand::CubicTo { .. }))
    );
    assert_eq!(
        path.shape.path.commands.last(),
        Some(&ShapePathCommand::Close)
    );
    let ellipse = find_shape(imported.composition().layers(), "Ellipse").expect("native Ellipse");
    assert_eq!(ellipse.shape.ellipse.as_ref().unwrap().size, [120.0, 66.0]);
    assert!(ellipse.shape.ellipse.as_ref().unwrap().reversed);
    let star = find_shape(imported.composition().layers(), "Star").expect("native Star");
    assert_eq!(
        star.shape.poly_star.as_ref().unwrap().star_type,
        ShapePolyStarType::Star
    );
    assert_eq!(star.shape.poly_star.as_ref().unwrap().points, 7.0);
    let polygon = find_shape(imported.composition().layers(), "Polygon").expect("native Polygon");
    assert_eq!(
        polygon.shape.poly_star.as_ref().unwrap().star_type,
        ShapePolyStarType::Polygon
    );
    assert!(polygon.shape.poly_star.as_ref().unwrap().reversed);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn single_shape_paint_opacity_remains_separate_from_native_layer_opacity() {
    let mut value = edited_input();
    let mut shape = shape_fill(
        &value,
        350,
        "Paint opacity",
        json!({"ellipse":{"size":[80.0,50.0],"position":[0.0,0.0]}}),
    );
    shape["shape"]["fills"][0]["opacity"] = json!(0.5);
    shape["transform"]["opacity"] = json!(80.0);
    value["composition"]["layers"] = json!([shape]);
    value["composition"]["dynamics"] = json!({"entries":[]});
    let output = fresh_export(value);
    let project = fresh_project(&output);
    let transform = crate::properties::read_transform(&layers(&project)[0].content).unwrap();
    assert_eq!(
        transform
            .iter()
            .find(|property| property.match_name == "ADBE Opacity")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap()
            .values,
        [0.8]
    );
    assert_eq!(
        super::stroke_keys::numeric(&layers(&project)[0].content, "ADBE Vector Fill Opacity")
            .unwrap()
            .values,
        [50.0]
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn static_transform_matrix_covers_rect_shape_and_boolean_leaves() {
    let mut value = edited_input();
    let mut rect = leaf(&value, 400, "Transformed rect");
    rect["description"] = json!("edited vector Rectangle");
    let shape = shape_fill(&value, 401, "Transformed shape", json!({"path":path(0.0)}));
    let a = shape_fill(
        &value,
        403,
        "A",
        json!({"ellipse":{"size":[50.0,40.0],"position":[0.0,0.0]}}),
    );
    let b = leaf(&value, 404, "B");
    let boolean = boolean(&value, 402, "Transformed boolean", "union", vec![a, b]);
    let mut layers = vec![rect, shape, boolean];
    for (index, layer) in layers.iter_mut().enumerate() {
        layer["transform"]["anchorPoint"] = json!([11.0, 13.0]);
        layer["transform"]["position"] = json!([100.0 + index as f64, 200.0 - index as f64]);
        layer["transform"]["scale"] = json!([-80.0, 125.0]);
        layer["transform"]["rotation"] = json!(17.0);
        layer["transform"]["opacity"] = json!(73.0);
    }
    value["composition"]["layers"] = json!(layers);
    value["composition"]["dynamics"] = json!({"entries":[]});

    let output = fresh_export(value);
    let project = fresh_project(&output);
    assert_eq!(super::layers(&project).len(), 3, "{:?}", output.diagnostics);
    for (index, layer) in super::layers(&project).iter().enumerate() {
        let properties = crate::properties::read_transform(&layer.content).unwrap();
        let numeric = |name: &str| {
            properties
                .iter()
                .find(|property| property.match_name == name)
                .unwrap()
                .numeric
                .as_ref()
                .unwrap()
                .values
                .clone()
        };
        assert_eq!(numeric("ADBE Anchor Point"), [11.0, 13.0, 0.0]);
        assert_eq!(
            numeric("ADBE Position"),
            [100.0 + index as f64, 200.0 - index as f64, 0.0]
        );
        assert_eq!(numeric("ADBE Scale"), [-0.8, 1.25, 1.0]);
        assert_eq!(numeric("ADBE Rotate Z"), [17.0]);
        assert_eq!(numeric("ADBE Opacity"), [0.73]);
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn transform_channels_hold_linear_bezier_and_spatial_tangents_are_native() {
    let mut value = edited_input();
    let leaf = leaf(&value, 500, "Animated transform");
    value["composition"]["layers"] = json!([leaf]);
    let mut entries = vec![
        serde_json::to_value(keyed_entry(
            LayerId::new(500),
            PropType::AnchorPointX,
            [
                (0, PropertyValue::Float(10.0)),
                (600, PropertyValue::Float(30.0)),
            ],
        ))
        .unwrap(),
        serde_json::to_value(keyed_entry(
            LayerId::new(500),
            PropType::PositionX,
            [
                (0, PropertyValue::Float(100.0)),
                (600, PropertyValue::Float(180.0)),
            ],
        ))
        .unwrap(),
        serde_json::to_value(keyed_entry(
            LayerId::new(500),
            PropType::PositionY,
            [
                (0, PropertyValue::Float(200.0)),
                (600, PropertyValue::Float(240.0)),
            ],
        ))
        .unwrap(),
        serde_json::to_value(keyed_entry(
            LayerId::new(500),
            PropType::ScaleY,
            [
                (0, PropertyValue::Float(100.0)),
                (600, PropertyValue::Float(140.0)),
            ],
        ))
        .unwrap(),
        serde_json::to_value(keyed_entry(
            LayerId::new(500),
            PropType::Rotation,
            [
                (0, PropertyValue::Float(0.0)),
                (600, PropertyValue::Float(45.0)),
            ],
        ))
        .unwrap(),
        serde_json::to_value(keyed_entry(
            LayerId::new(500),
            PropType::Opacity,
            [
                (0, PropertyValue::Float(100.0)),
                (600, PropertyValue::Float(30.0)),
            ],
        ))
        .unwrap(),
    ];
    entries[0]["animator"]["keyframes"][1]["easing"] = json!({"type":"hold"});
    entries[3]["animator"]["keyframes"][1]["easing"] =
        json!({"type":"cubicBezier","x1":0.25,"y1":0.1,"x2":0.75,"y2":0.9});
    for entry in &mut entries[1..=2] {
        entry["animator"]["keyframes"][0]["spatialOutTangent"] = json!(12.0);
        entry["animator"]["keyframes"][1]["spatialInTangent"] = json!(-8.0);
    }
    value["composition"]["dynamics"] = json!({"entries":entries});

    let output = fresh_export(value);
    let project = fresh_project(&output);
    assert_eq!(layers(&project).len(), 1, "{:?}", output.diagnostics);
    let native = crate::properties::read_transform(&layers(&project)[0].content).unwrap();
    for name in [
        "ADBE Anchor Point",
        "ADBE Position",
        "ADBE Scale",
        "ADBE Rotate Z",
        "ADBE Opacity",
    ] {
        assert_eq!(
            native
                .iter()
                .find(|property| property.match_name == name)
                .unwrap()
                .numeric
                .as_ref()
                .unwrap()
                .keyframes
                .len(),
            2,
            "{name}"
        );
    }
    let imported = reimport(&output);
    let id = find_rect(imported.composition().layers(), "Animated transform")
        .unwrap()
        .id;
    assert_eq!(
        property_values(&imported, id, PropType::AnchorPointX),
        vec![PropertyValue::Float(10.0), PropertyValue::Float(30.0)]
    );
    assert_eq!(
        property_values(&imported, id, PropType::ScaleY),
        vec![PropertyValue::Float(100.0), PropertyValue::Float(140.0)]
    );
    let position = imported
        .composition()
        .dynamics()
        .entries()
        .iter()
        .find(|entry| entry.target == fx_schema::PropertyTarget::layer(id, PropType::PositionX))
        .unwrap()
        .animator
        .keyframe_track()
        .unwrap();
    assert!(position.has_spatial_tangents());
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn rectangle_size_position_and_roundness_tracks_keep_asymmetric_values() {
    let mut value = edited_input();
    let mut rect = leaf(&value, 600, "Animated rectangle");
    rect["description"] = json!("edited vector Rectangle");
    rect["rect"]["position"] = json!([7.0, -3.0]);
    rect["rect"]["size"] = json!([80.0, 40.0]);
    value["composition"]["layers"] = json!([rect]);
    value["composition"]["dynamics"] = json!({"entries":[
        keyed_entry(LayerId::new(600), PropType::RectSize, [(0,PropertyValue::Vector2([80.0,40.0])),(500,PropertyValue::Vector2([150.0,70.0]))]),
        keyed_entry(LayerId::new(600), PropType::RectRoundness, [(0,PropertyValue::Float(2.0)),(500,PropertyValue::Float(19.0))])
    ]});

    let output = fresh_export(value);
    let imported = reimport(&output);
    let rect = find_rect(imported.composition().layers(), "Animated rectangle").unwrap();
    assert_eq!(
        property_values(&imported, rect.id, PropType::RectSize),
        vec![
            PropertyValue::Vector2([80.0, 40.0]),
            PropertyValue::Vector2([150.0, 70.0])
        ]
    );
    assert_eq!(
        property_values(&imported, rect.id, PropType::RectRoundness),
        vec![PropertyValue::Float(2.0), PropertyValue::Float(19.0)]
    );
    assert_eq!(
        property_values(&imported, rect.id, PropType::PositionX),
        vec![PropertyValue::Float(47.0), PropertyValue::Float(82.0)]
    );
    assert_eq!(
        property_values(&imported, rect.id, PropType::PositionY),
        vec![PropertyValue::Float(17.0), PropertyValue::Float(32.0)]
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn ellipse_star_and_polygon_positive_geometry_tracks_roundtrip() {
    let cases = [
        (
            700,
            "Animated ellipse",
            shape_fill(
                &edited_input(),
                700,
                "Animated ellipse",
                json!({"ellipse":{"size":[90.0,50.0],"position":[2.0,3.0]}}),
            ),
            vec![
                (
                    PropType::EllipseSize,
                    PropertyValue::Vector2([90.0, 50.0]),
                    PropertyValue::Vector2([130.0, 70.0]),
                ),
                (
                    PropType::EllipsePosition,
                    PropertyValue::Vector2([2.0, 3.0]),
                    PropertyValue::Vector2([12.0, 8.0]),
                ),
            ],
        ),
        (
            701,
            "Animated star",
            shape_fill(
                &edited_input(),
                701,
                "Animated star",
                json!({"polyStar":{"starType":"star","points":5.0,"position":[0.0,0.0],"rotation":0.0,"outerRadius":50.0,"innerRadius":20.0,"outerRoundness":0.0,"innerRoundness":0.0}}),
            ),
            vec![
                (
                    PropType::PolyStarPosition,
                    PropertyValue::Vector2([0.0, 0.0]),
                    PropertyValue::Vector2([9.0, 7.0]),
                ),
                (
                    PropType::PolyStarRotation,
                    PropertyValue::Float(0.0),
                    PropertyValue::Float(30.0),
                ),
                (
                    PropType::PolyStarOuterRadius,
                    PropertyValue::Float(50.0),
                    PropertyValue::Float(65.0),
                ),
                (
                    PropType::PolyStarInnerRadius,
                    PropertyValue::Float(20.0),
                    PropertyValue::Float(28.0),
                ),
                (
                    PropType::PolyStarOuterRoundness,
                    PropertyValue::Float(0.0),
                    PropertyValue::Float(6.0),
                ),
                (
                    PropType::PolyStarInnerRoundness,
                    PropertyValue::Float(0.0),
                    PropertyValue::Float(4.0),
                ),
            ],
        ),
        (
            702,
            "Animated polygon",
            shape_fill(
                &edited_input(),
                702,
                "Animated polygon",
                json!({"polyStar":{"starType":"polygon","points":5.0,"position":[0.0,0.0],"rotation":0.0,"outerRadius":50.0,"innerRadius":20.0,"outerRoundness":0.0,"innerRoundness":0.0}}),
            ),
            vec![(
                PropType::PolyStarPoints,
                PropertyValue::Float(5.0),
                PropertyValue::Float(8.0),
            )],
        ),
    ];
    for (id, name, layer, tracks) in cases {
        let mut value = edited_input();
        value["composition"]["layers"] = json!([layer]);
        let mut entries = Vec::new();
        for (property, first, second) in &tracks {
            let mut entry = serde_json::to_value(keyed_entry(
                LayerId::new(id),
                *property,
                [(0, first.clone()), (500, second.clone())],
            ))
            .unwrap();
            if *property == PropType::PolyStarPoints {
                entry["animator"]["keyframes"][1]["easing"] = json!({"type":"hold"});
            }
            entries.push(entry);
        }
        value["composition"]["dynamics"] = json!({"entries":entries});
        let output = fresh_export(value);
        let imported = reimport(&output);
        let shape = find_shape(imported.composition().layers(), name)
            .unwrap_or_else(|| panic!("{name}: {:?}", output.diagnostics));
        for (property, first, second) in tracks {
            assert_eq!(
                property_values(&imported, shape.id, property),
                vec![first, second],
                "{name} {property:?}"
            );
        }
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn boolean_operation_and_operand_kind_matrix_is_editable_and_ordered() {
    for (index, op) in ["union", "subtract", "intersect", "exclude"]
        .into_iter()
        .enumerate()
    {
        let mut value = edited_input();
        let path_operand = shape_fill(&value, 810, "Path operand", json!({"path":path(0.0)}));
        let rect_operand = leaf(&value, 811, "Rect operand");
        let nested_a = shape_fill(
            &value,
            813,
            "Nested ellipse",
            json!({"ellipse":{"size":[40.0,30.0],"position":[0.0,0.0]}}),
        );
        let nested_b = shape_fill(
            &value,
            814,
            "Nested star",
            json!({"polyStar":{"points":5.0,"outerRadius":20.0}}),
        );
        let nested = boolean(&value, 812, "Nested", "exclude", vec![nested_a, nested_b]);
        let layer = boolean(
            &value,
            800 + index as u64,
            &format!("Boolean {op}"),
            op,
            vec![path_operand, rect_operand, nested],
        );
        value["composition"]["layers"] = json!([layer]);
        value["composition"]["dynamics"] = json!({"entries":[]});
        let output = fresh_export(value);
        let imported = reimport(&output);
        let boolean = find_boolean(imported.composition().layers(), &format!("Boolean {op}"))
            .unwrap_or_else(|| panic!("{op}: {:?}", output.diagnostics));
        assert_eq!(
            boolean.op,
            [
                fx_schema::BooleanOp::Union,
                fx_schema::BooleanOp::Subtract,
                fx_schema::BooleanOp::Intersect,
                fx_schema::BooleanOp::Exclude
            ][index]
        );
        assert_eq!(boolean.layers.len(), 3);
        assert!(matches!(boolean.layers[0].data(), LayerData::Shape(_)));
        assert!(matches!(boolean.layers[1].data(), LayerData::Shape(_)));
        assert!(matches!(
            boolean.layers[2].data(),
            LayerData::BooleanOperation(_)
        ));
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn mixed_leaf_order_and_precise_unsupported_omission_preserve_siblings() {
    let mut value = edited_input();
    let solid = leaf(&value, 900, "01 solid");
    let mut rect = leaf(&value, 901, "02 rect");
    rect["description"] = json!("edited vector Rectangle");
    let shape = shape_fill(
        &value,
        902,
        "03 shape",
        json!({"ellipse":{"size":[50.0,30.0],"position":[0.0,0.0]}}),
    );
    let a = shape_fill(
        &value,
        904,
        "A",
        json!({"ellipse":{"size":[30.0,30.0],"position":[0.0,0.0]}}),
    );
    let b = leaf(&value, 905, "B");
    let boolean = boolean(&value, 903, "04 boolean", "intersect", vec![a, b]);
    let mut rejected = shape_fill(&value, 906, "05 dashed", json!({"path":path(0.0)}));
    rejected["shape"]["fills"] = json!([]);
    rejected["shape"]["strokes"] = json!([{"paint":{"type":"solid","color":[1.0,0.0,0.0,1.0]},"width":4.0,"dashes":[8.0,4.0]}]);
    let tail = leaf(&value, 907, "06 tail");
    value["composition"]["layers"] = json!([solid, rect, shape, boolean, rejected, tail]);
    value["composition"]["dynamics"] = json!({"entries":[]});

    let output = fresh_export(value);
    assert_eq!(
        layers(&fresh_project(&output))
            .iter()
            .map(|layer| layer.name.as_ref())
            .collect::<Vec<_>>(),
        ["01 solid", "02 rect", "03 shape", "04 boolean", "06 tail"]
    );
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(906))
            && diagnostic
                .message
                .contains("one owned solid Fill or undashed Stroke")
            && diagnostic.message.contains("convertible siblings retained")
    }));
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn transparent_wrapper_normalization_distinguishes_identity_static_keyed_and_multichild() {
    let mut value = edited_input();
    let mut identity_group = value["composition"]["layers"][0].clone();
    identity_group["name"] = json!("Identity wrapper");
    let mut identity_leaf = leaf(&value, 1001, "Identity child");
    identity_leaf["parent"] = identity_group["id"].clone();
    identity_group["layers"] = json!([identity_leaf]);

    let mut static_group = value["composition"]["layers"][0].clone();
    static_group["id"] = json!(1002);
    static_group["name"] = json!("Static wrapper");
    static_group["transform"]["position"] = json!([321.0, 123.0]);
    let mut static_leaf = leaf(&value, 1003, "Static child");
    static_leaf["parent"] = json!(1002);
    static_group["layers"] = json!([static_leaf]);

    let mut keyed_group = value["composition"]["layers"][0].clone();
    keyed_group["id"] = json!(1004);
    keyed_group["name"] = json!("Keyed wrapper");
    let mut keyed_leaf = leaf(&value, 1005, "Keyed child");
    keyed_leaf["parent"] = json!(1004);
    keyed_group["layers"] = json!([keyed_leaf]);

    let mut rejected_group = value["composition"]["layers"][0].clone();
    rejected_group["id"] = json!(1006);
    rejected_group["name"] = json!("Rejected wrapper");
    rejected_group["transform"]["opacity"] = json!(50.0);
    let mut rejected_a = leaf(&value, 1007, "Rejected A");
    let mut rejected_b = leaf(&value, 1008, "Rejected B");
    rejected_a["parent"] = json!(1006);
    rejected_b["parent"] = json!(1006);
    rejected_group["layers"] = json!([rejected_a, rejected_b]);

    value["composition"]["layers"] =
        json!([identity_group, static_group, keyed_group, rejected_group]);
    value["composition"]["dynamics"] = json!({"entries":[keyed_entry(LayerId::new(1004), PropType::Opacity, [(0,PropertyValue::Float(100.0)),(500,PropertyValue::Float(40.0))])]});
    let output = fresh_export(value);
    let project = fresh_project(&output);
    assert_eq!(
        layers(&project)
            .iter()
            .map(|layer| layer.name.as_ref())
            .collect::<Vec<_>>(),
        ["Identity child", "Static child", "Keyed child"]
    );
    let static_transform = crate::properties::read_transform(&layers(&project)[1].content).unwrap();
    assert_eq!(
        static_transform
            .iter()
            .find(|property| property.match_name == "ADBE Position")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap()
            .values,
        [321.0, 123.0, 0.0]
    );
    let keyed_transform = crate::properties::read_transform(&layers(&project)[2].content).unwrap();
    assert_eq!(
        keyed_transform
            .iter()
            .find(|property| property.match_name == "ADBE Opacity")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap()
            .keyframes
            .len(),
        2
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.layer_id == Some(LayerId::new(1006))
                && diagnostic.message.contains("multi-child Group"))
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn active_range_boundaries_use_native_ticks_and_reject_empty_span_only() {
    let cases = [
        (1100, "full", json!({"start":0,"duration":2000}), true),
        (1101, "fractional", json!({"start":1,"duration":999}), true),
        (1102, "late", json!({"start":1500,"duration":500}), true),
        (1103, "clipped", json!({"start":1750,"duration":1000}), true),
        (1104, "empty", json!({"start":2000,"duration":1}), false),
    ];
    let mut value = edited_input();
    value["duration"] = json!(2.0);
    value["composition"]["layers"] = Value::Array(
        cases
            .iter()
            .map(|(id, name, range, _)| {
                let mut layer = leaf(&value, *id, name);
                layer["activeRange"] = range.clone();
                layer
            })
            .collect(),
    );
    value["composition"]["dynamics"] = json!({"entries":[]});
    let output = fresh_export(value);
    let project = fresh_project(&output);
    assert_eq!(
        layers(&project)
            .iter()
            .map(|layer| layer.name.as_ref())
            .collect::<Vec<_>>(),
        ["full", "fractional", "late", "clipped"]
    );
    let fractional = &layers(&project)[1].record;
    assert!((fractional.start_time().unwrap() - 1.0 / 1000.0).abs() < 0.0001);
    assert!((fractional.out_point().unwrap() - 999.0 / 1000.0).abs() < 0.0001);
    let clipped = &layers(&project)[3].record;
    assert!((clipped.start_time().unwrap() - 1.75).abs() < 0.0001);
    assert!((clipped.out_point().unwrap() - 0.25).abs() < 0.0001);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.layer_id == Some(LayerId::new(1104))
                && diagnostic.message.contains("no positive active span"))
    );
}

#[test]
fn boolean_export_keeps_more_than_512_editable_operands() {
    let mut value = edited_input();
    let operands = (0..513)
        .map(|index| leaf(&value, 20_000 + index, &format!("operand-{index}")))
        .collect();
    let layer = boolean(&value, 10_000, "large union", "union", operands);
    value["composition"]["layers"] = json!([layer]);
    value["composition"]["dynamics"] = json!({"entries":[]});
    let output = fresh_export(value);
    let imported = reimport(&output);
    let union = find_boolean(imported.composition().layers(), "large union")
        .unwrap_or_else(|| panic!("{:?}", output.diagnostics));
    assert_eq!(union.layers.len(), 513);
    assert_eq!(union.op, fx_schema::layer::BooleanOp::Union);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn composition_preserves_all_513_leaves_without_a_layer_count_cutoff() {
    let mut value = edited_input();
    value["composition"]["layers"] = Value::Array(
        (0..513)
            .map(|index| leaf(&value, 20_000 + index, &format!("leaf {index:03}")))
            .collect(),
    );
    value["composition"]["dynamics"] = json!({"entries":[]});
    let output = fresh_export(value);
    let project = fresh_project(&output);
    assert_eq!(layers(&project).len(), 513);
    assert_eq!(layers(&project).first().unwrap().name.as_ref(), "leaf 000");
    assert_eq!(layers(&project).last().unwrap().name.as_ref(), "leaf 512");
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.message.contains("solid count (512) limit") })
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn preflight_name_path_key_range_and_depth_failures_are_leaf_scoped() {
    let mut value = edited_input();
    let good = leaf(&value, 1200, "supported sibling");
    let mut bad_name = leaf(&value, 1201, &"n".repeat(256));
    bad_name["description"] = json!("edited vector Rectangle");
    let mut bad_path = shape_fill(
        &value,
        1202,
        "compound path",
        json!({"path":{"commands":[
            {"type":"moveTo","x":0.0,"y":0.0},{"type":"lineTo","x":10.0,"y":0.0},
            {"type":"moveTo","x":20.0,"y":0.0},{"type":"lineTo","x":30.0,"y":0.0}
        ]}}),
    );
    bad_path["description"] = json!("explicit unsupported compound path");
    let mut bad_range = leaf(&value, 1203, "bad range");
    bad_range["activeRange"] = json!({"start":999999999999999999_u64,"duration":1000});
    let bad_keys = leaf(&value, 1204, "too many keys");
    value["composition"]["layers"] = json!([bad_name, bad_path, bad_range, bad_keys, good]);
    value["composition"]["dynamics"] = json!({"entries":[oversized_key_entry(LayerId::new(1204))]});
    let output = fresh_export(value);
    assert_eq!(
        layers(&fresh_project(&output))
            .iter()
            .map(|layer| layer.name.as_ref())
            .collect::<Vec<_>>(),
        ["supported sibling"]
    );
    for (id, message) in [
        (1201, "name must be 1..=255"),
        (1202, "path"),
        (1203, "native clock range"),
        (1204, "keyframe"),
    ] {
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.layer_id == Some(LayerId::new(id))
                    && diagnostic.message.to_lowercase().contains(message)),
            "{id} {message}: {:?}",
            output.diagnostics
        );
    }

    let mut value = edited_input();
    let sibling = leaf(&value, 1300, "depth sibling");
    let mut nested = leaf(&value, 1350, "depth leaf");
    for depth in (1301_u64..=1349).rev() {
        let mut group = value["composition"]["layers"][0].clone();
        group["id"] = json!(depth);
        group["name"] = json!(format!("depth {depth}"));
        nested["parent"] = json!(depth);
        group["layers"] = json!([nested]);
        nested = group;
    }
    value["composition"]["layers"] = json!([nested, sibling]);
    value["composition"]["dynamics"] = json!({"entries":[]});
    let output = fresh_export(value);
    assert_eq!(
        layers(&fresh_project(&output))
            .iter()
            .map(|layer| layer.name.as_ref())
            .collect::<Vec<_>>(),
        ["depth sibling"]
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("depth (48)"))
    );
}
