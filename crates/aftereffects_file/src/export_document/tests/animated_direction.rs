//! Independent Adobe direction-3 keys must use one vertex/tangent order
//! in both the static base and the editable animation.
use super::*;
use fx_schema::{PropertyValue, ShapePathCommand, animator::AnimatorData};
use sha2::{Digest, Sha256};

fn assert_keys(document: &EditableFxCompositionDocument, shift: f64) {
    let mut checked = 0;
    for entry in document.composition().dynamics().entries() {
        let AnimatorData::Keyframes { track, .. } = entry.animator.data() else {
            continue;
        };
        if !matches!(track.keyframes()[0].value(), PropertyValue::Path(_)) {
            continue;
        }
        assert_eq!(track.keyframes().len(), 2);
        for (index, key) in track.keyframes().iter().enumerate() {
            assert_eq!(key.layer_time().as_millis(), index as i64 * 1000);
            let PropertyValue::Path(path) = key.value() else {
                panic!("not editable Path")
            };
            let (x, y) = path.commands[0].endpoint().unwrap();
            assert!((x - (-80.0 + shift)).abs() < 0.001);
            assert!((y - (-40.0 + index as f64 * 20.0)).abs() < 0.001);
            assert!(
                matches!(path.commands[1], ShapePathCommand::CubicTo {c1x,c1y,c2x,c2y,x,y,..}
                if (c1x-(-65.0+shift)).abs()<0.001 && (c1y-(-60.0+index as f64*20.0)).abs()<0.001
                && (c2x-(-5.0+shift)).abs()<0.001 && (c2y-55.0).abs()<0.001
                && (x-(20.0+shift)).abs()<0.001 && (y-70.0).abs()<0.001),
                "{:?}",
                path.commands
            );
            assert_eq!(path.commands.last(), Some(&ShapePathCommand::Close));
            checked += 1;
        }
    }
    assert!(checked >= 2, "native keys missing");
}

#[test]
fn native_reversed_bezier_keys_keep_vertex_and_tangent_correspondence_through_export_and_edit() {
    let bytes = include_bytes!("../../../tests/fixtures/geometry/animated_direction/native.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "579b5b9aec9f8f35f8c0782170e4840440f0096771e90273234e274ab2da5d42"
    );
    let source = read_project(bytes).unwrap();
    let imported = to_structural_fx_document(&source, Some(1)).unwrap();
    assert_keys(&imported.document, 0.0);
    let exported = to_aep(&imported.document).unwrap();
    let reopened = read_project(&exported.bytes).unwrap();
    let reimported = to_structural_fx_document(&reopened, Some(1)).unwrap();
    assert_keys(&reimported.document, 0.0);
    let mut value: Value =
        serde_json::from_slice(&imported.document.to_json_vec().unwrap()).unwrap();
    fn shift_paths(value: &mut Value) {
        match value {
            Value::Object(map) => {
                if let Some(commands) = map.get_mut("commands").and_then(Value::as_array_mut) {
                    for command in commands {
                        for name in ["x", "c1x", "c2x"] {
                            if let Some(coordinate) = command.get_mut(name) {
                                *coordinate = json!(coordinate.as_f64().unwrap() + 30.0);
                            }
                        }
                    }
                } else {
                    for child in map.values_mut() {
                        shift_paths(child);
                    }
                }
            }
            Value::Array(values) => {
                for child in values {
                    shift_paths(child);
                }
            }
            _ => {}
        }
    }
    shift_paths(&mut value);
    let edited = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let exported = to_aep(&edited).unwrap();
    let reopened = read_project(&exported.bytes).unwrap();
    let reimported = to_structural_fx_document(&reopened, Some(1)).unwrap();
    assert_keys(&reimported.document, 30.0);
}
