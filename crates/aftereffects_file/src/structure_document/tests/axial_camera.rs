use super::*;
use crate::{
    rifx::Chunk,
    writer::{
        CompositionSpec, LayerSpec, NativeCameraSpec, SolidLayerSpec, SolidTransform,
        write_composition,
    },
};
use fx_schema::{Position, PropType, PropertyTarget};

fn project() -> StructuralProject {
    let spec = CompositionSpec {
        name: "Axial camera".into(),
        width: 640,
        height: 360,
        duration_frames: 48,
    };
    let bytes = write_composition(
        &spec,
        &[
            LayerSpec::Camera(NativeCameraSpec {
                center: [320.0, 180.0],
                distance: 800.0,
            }),
            LayerSpec::Solid(SolidLayerSpec {
                name: "Paint".into(),
                width: 80,
                height: 40,
                color: [1.0, 0.0, 0.0],
                transform: SolidTransform {
                    anchor: [40.0, 20.0],
                    position: [320.0, 180.0],
                    scale: [100.0, 100.0],
                    rotation: 0.0,
                    opacity: 100.0,
                },
            }),
        ],
    )
    .unwrap();
    let mut project = read_project(&bytes).unwrap();
    let comp = composition_mut(&mut project, 1);
    comp.layers[0].name = "Authored camera".into();
    set_position(&mut comp.layers[0].content, &[320.0, 180.0, -400.0], false);
    comp.layers[1].record = comp.layers[1]
        .record
        .clone()
        .with_three_d_layer(true)
        .unwrap();
    project
}

fn set_position(chunks: &mut [Chunk], values: &[f64], separated: bool) {
    let mut position = false;
    for chunk in chunks {
        if chunk.id() == *b"tdmn" {
            position = chunk
                .data_payload()
                .unwrap()
                .split(|byte| *byte == 0)
                .next()
                .unwrap()
                == b"ADBE Position";
        } else if position && chunk.list_kind() == Some(*b"tdbs") {
            for child in chunk.children_mut().unwrap() {
                if child.id() == *b"cdat" {
                    let mut bytes = child.data_payload().unwrap().to_vec();
                    for (index, value) in values.iter().enumerate() {
                        bytes[index * 8..index * 8 + 8].copy_from_slice(&value.to_be_bytes());
                    }
                    *child = Chunk::data(*b"cdat", bytes).unwrap();
                } else if child.id() == *b"tdsb" && separated {
                    let mut bytes = child.data_payload().unwrap().to_vec();
                    bytes[2] |= 8;
                    *child = Chunk::data(*b"tdsb", bytes).unwrap();
                }
            }
        } else if let Some(children) = chunk.children_mut() {
            set_position(children, values, separated);
        }
    }
}

fn position_storage(chunks: &mut [Chunk]) -> Option<&mut Vec<Chunk>> {
    let mut position = false;
    for chunk in chunks {
        if chunk.id() == *b"tdmn" {
            position = chunk.data_payload()?.split(|byte| *byte == 0).next()? == b"ADBE Position";
        } else if position && chunk.list_kind() == Some(*b"tdbs") {
            return chunk.children_mut();
        } else if let Some(children) = chunk.children_mut()
            && let Some(found) = position_storage(children)
        {
            return Some(found);
        }
    }
    None
}

fn depth_project() -> StructuralProject {
    let mut project = project();
    let comp = composition_mut(&mut project, 1);
    let camera = position_storage(&mut comp.layers[0].content).unwrap();
    let meta = camera
        .iter_mut()
        .find(|chunk| chunk.id() == *b"tdb4")
        .unwrap();
    let mut bytes = meta.data_payload().unwrap().to_vec();
    bytes[5] |= 8;
    bytes[12..16].copy_from_slice(&24_u32.to_be_bytes());
    bytes[68] = 1;
    *meta = Chunk::data(*b"tdb4", bytes).unwrap();
    let mut header = vec![0; 24];
    header[10..12].copy_from_slice(&2_u16.to_be_bytes());
    header[18..20].copy_from_slice(&128_u16.to_be_bytes());
    header[23] = 4;
    let mut keys = vec![0; 256];
    for (index, (time, z)) in [(0_i32, -400_f64), (24, -600.0)].into_iter().enumerate() {
        let key = &mut keys[index * 128..(index + 1) * 128];
        key[..4].copy_from_slice(&time.to_be_bytes());
        key[4..6].copy_from_slice(&[1, 1]);
        for (offset, value) in [
            (32, 1.0 / 3.0),
            (48, 1.0 / 3.0),
            (56, 320.0),
            (64, 180.0),
            (72, z),
        ] {
            key[offset..offset + 8].copy_from_slice(&value.to_be_bytes());
        }
    }
    camera.push(Chunk::list(
        *b"list",
        vec![
            Chunk::data(*b"lhd3", header).unwrap(),
            Chunk::data(*b"ldat", keys).unwrap(),
        ],
    ));
    set_static_transform(
        &mut comp.layers[1],
        &[("ADBE Position", &[320.0, 180.0, -100.0])],
    );
    position_storage(&mut comp.layers[1].content)
        .unwrap()
        .push(Chunk::data(*b"Utf8", b"[320 + 40*time, 180, -100 - 50*time]".to_vec()).unwrap());
    project
}

fn scalar_at(track: &fx_schema::animator::PropertyKeyframeTrack, millis: i64) -> f64 {
    use fx_schema::{PropertyValue, animator::PropertyKeyframeEasing};
    let keys = track.keyframes();
    let value = |index: usize| match keys[index].value() {
        PropertyValue::Float(value) => *value,
        _ => panic!("expected scalar"),
    };
    let next = keys.partition_point(|key| key.layer_time().as_millis() <= millis);
    if next == 0 {
        return value(0);
    }
    if next == keys.len() {
        return value(next - 1);
    }
    let from = next - 1;
    let progress = (millis - keys[from].layer_time().as_millis()) as f64
        / (keys[next].layer_time().as_millis() - keys[from].layer_time().as_millis()) as f64;
    let eased = match keys[next].easing() {
        PropertyKeyframeEasing::Linear => progress,
        PropertyKeyframeEasing::Hold => 0.0,
        PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
            use crate::export_document::effects::{bezier, invert_bezier};
            bezier(invert_bezier(progress, x1, x2), y1, y2)
        }
    };
    value(from) + (value(next) - value(from)) * eased
}

#[test]
fn axial_camera_depth_samples_compensate_implicit_projection_without_replacing_paint() {
    let converted = to_structural_fx_document(&depth_project(), Some(1)).unwrap();
    let stage = root(&converted)
        .layers
        .iter()
        .map(as_group)
        .find(|group| group.name == "Axial camera projection")
        .expect("depth-animated paint must retain the explicit axial camera");
    let owner = as_group(&stage.layers[0]);
    assert_eq!(owner.name, "Paint");
    assert!(matches!(owner.transform.position, Position::ThreeD(_)));
    assert!(
        !owner.layers.is_empty(),
        "editable source paint must survive"
    );
    let entries = converted.document.composition().dynamics().entries();
    let track = |id, property| {
        entries
            .iter()
            .find(|entry| entry.target == PropertyTarget::layer(id, property))
            .unwrap()
            .animator
            .keyframe_track()
            .unwrap()
    };
    let d = 640.0 * fx_schema::DEFAULT_CAMERA_DISTANCE_PER_WIDTH;
    for millis in [0, 500, 1000, 1500] {
        let seconds = millis as f64 / 1000.0;
        let z = scalar_at(track(owner.id, PropType::PositionZ), millis);
        let camera_z = -400.0 - 200.0 * seconds.min(1.0);
        let expected = 800.0 / (z - camera_z);
        let implicit = d / (d + z);
        for property in [PropType::ScaleX, PropType::ScaleY] {
            let ratio = scalar_at(track(stage.id, property), millis) / 100.0;
            assert!(
                (implicit * ratio - expected).abs() < 1e-6,
                "{millis}ms: {} != {expected}",
                implicit * ratio
            );
        }
        assert!((z - (-100.0 - 50.0 * seconds)).abs() < 1e-6);
    }
    assert!(
        !converted
            .document
            .to_json_vec()
            .unwrap()
            .windows(8)
            .any(|bytes| bytes == b"jsScript")
    );
}

#[test]
fn axial_camera_sampled_depth_declines_crossings_and_keeps_source_paint() {
    let mut project = depth_project();
    let comp = composition_mut(&mut project, 1);
    let expression = position_storage(&mut comp.layers[1].content)
        .unwrap()
        .iter_mut()
        .find(|chunk| chunk.id() == *b"Utf8")
        .unwrap();
    *expression = Chunk::data(*b"Utf8", b"[320,180,-500]".to_vec()).unwrap();
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    assert!(converted.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("sampled paint reaches/crosses a camera plane")
    }));
    assert!(
        root(&converted)
            .layers
            .iter()
            .map(as_group)
            .any(|group| group.name == "Paint" && !group.layers.is_empty())
    );
    assert!(
        !root(&converted)
            .layers
            .iter()
            .map(as_group)
            .any(|group| group.name == "Axial camera projection")
    );
}

#[test]
fn axial_camera_declines_auto_oriented_camera_parent() {
    let mut project = project();
    let comp = composition_mut(&mut project, 1);
    let mut parent = comp.layers[1].clone();
    patch(&mut parent, 0, &99_u32.to_be_bytes());
    set_static_transform(
        &mut parent,
        &[
            ("ADBE Anchor Point", &[0.0; 3]),
            ("ADBE Position", &[0.0; 3]),
            ("ADBE Scale", &[1.0; 3]),
        ],
    );
    comp.layers.push(parent);
    patch(&mut comp.layers[0], 132, &99_u32.to_be_bytes());
    assert!(
        super::super::axial_camera::prepare(comp, &comp.layers[1], &[])
            .unwrap()
            .is_some()
    );
    let flags = comp.layers[2].record.raw_bytes()[38] | 1;
    patch(&mut comp.layers[2], 38, &[flags]);
    let error = super::super::axial_camera::prepare(comp, &comp.layers[1], &[])
        .err()
        .expect("auto-oriented camera parents must not project as untilted cameras");
    assert!(error.contains("auto-oriented camera parent"), "{error}");
}

#[test]
fn axial_camera_keeps_editable_paint_and_projects_only_three_d_owners() {
    let mut project = project();
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    let value = serde_json::to_value(converted.document.composition()).unwrap();
    fn projection(value: &Value) -> Option<&Value> {
        if value.get("name").and_then(Value::as_str) == Some("Axial camera projection") {
            return Some(value);
        }
        value.get("layers")?.as_array()?.iter().find_map(projection)
    }
    let stage = projection(&value)
        .expect("explicit camera must project editable 3D paint, not disappear as a placeholder");
    assert_eq!(
        stage["transform"]["scale"],
        serde_json::json!([200.0, 200.0])
    );
    assert_eq!(
        stage["transform"]["anchorPoint"],
        serde_json::json!([320.0, 180.0])
    );
    assert!(
        stage["layers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|layer| layer["name"] == "Paint")
    );
    assert!(!value.to_string().contains("jsScript"));
    set_position(
        &mut composition_mut(&mut project, 1).layers[1].content,
        &[320.0, 180.0],
        true,
    );
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    let value = serde_json::to_value(converted.document.composition()).unwrap();
    assert_eq!(
        projection(&value).unwrap()["transform"]["scale"],
        serde_json::json!([200.0, 200.0])
    );
    composition_mut(&mut project, 1).layers[1].record = composition(&project, 1).layers[1]
        .record
        .clone()
        .with_three_d_layer(false)
        .unwrap();
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    assert!(projection(&serde_json::to_value(converted.document.composition()).unwrap()).is_none());
}
