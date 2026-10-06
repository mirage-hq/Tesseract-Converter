//! Independent native one-vertex closed Bezier control; own-reader export checks
//! supplement, rather than replace, the separately recorded Adobe readback.
use super::*;
use fx_schema::{ShapeLayer, ShapePathCommand};
use sha2::{Digest, Sha256};

fn loop_shape(layers: &[Layer]) -> Option<&ShapeLayer> {
    layers.iter().find_map(|layer| match layer.data() {
        LayerData::Shape(shape) => Some(shape),
        LayerData::Group(group) => loop_shape(&group.layers),
        _ => None,
    })
}

#[test]
fn native_one_vertex_closed_cubic_survives_fresh_export_and_vertex_edit() {
    let bytes = include_bytes!("../../../tests/fixtures/geometry/one_vertex_loop/native.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "9908039bd36629cf8121841cbaea381558bc0c4b5d445395a394d53f1e89f753"
    );
    let native = read_project(bytes).unwrap();
    let imported = to_structural_fx_document(&native, Some(1)).unwrap();
    let shape = loop_shape(imported.document.composition().layers()).unwrap();
    assert_eq!(shape.shape.path.commands.len(), 3);
    assert_eq!(shape.shape.path.commands[0].endpoint(), Some((0.0, 0.0)));
    assert!(
        matches!(shape.shape.path.commands[1], ShapePathCommand::CubicTo { c1x, c1y, c2x, c2y, x, y, .. }
        if (c1x + 90.0).abs() < 0.001 && (c1y + 100.0).abs() < 0.001
        && (c2x - 90.0).abs() < 0.001 && (c2y + 100.0).abs() < 0.001
        && x == 0.0 && y == 0.0)
    );
    assert_eq!(shape.shape.path.commands[2], ShapePathCommand::Close);
    let output = to_aep(&imported.document).unwrap();
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|warning| warning.layer_id.is_some() && warning.message.contains("omitted")),
        "{:?}",
        output.diagnostics
    );
    let reopened = read_project(&output.bytes).unwrap();
    let reimported = to_structural_fx_document(&reopened, Some(1)).unwrap();
    let actual = loop_shape(reimported.document.composition().layers()).unwrap();
    assert_eq!(actual.shape.path.commands.len(), 3);
    assert_eq!(actual.shape.path.commands[0].endpoint(), Some((0.0, 0.0)));

    // Edit the actual imported Path, not its source AEP or a donor fallback.
    let mut value: Value =
        serde_json::from_slice(&imported.document.to_json_vec().unwrap()).unwrap();
    fn shift(value: &mut Value) {
        match value {
            Value::Object(map) => {
                if let Some(commands) = map.get_mut("commands").and_then(Value::as_array_mut) {
                    // Native static geometry also has authoritative constant Path
                    // entries: edit those values together with the typed base.
                    for command in commands {
                        for key in ["x", "c1x", "c2x"] {
                            if let Some(number) = command.get_mut(key) {
                                *number = json!(number.as_f64().unwrap() + 40.0);
                            }
                        }
                    }
                } else {
                    for child in map.values_mut() {
                        shift(child);
                    }
                }
            }
            Value::Array(values) => {
                for child in values {
                    shift(child);
                }
            }
            _ => {}
        }
    }
    shift(&mut value);
    let edited = EditableFxCompositionDocument::from_json_value(value).unwrap();
    let output = to_aep(&edited).unwrap();
    let native = read_project(&output.bytes).unwrap();
    let reimported = to_structural_fx_document(&native, Some(1)).unwrap();
    let actual = loop_shape(reimported.document.composition().layers()).unwrap();
    assert_eq!(actual.shape.path.commands[0].endpoint(), Some((40.0, 0.0)));
    assert_eq!(actual.shape.path.commands[2], ShapePathCommand::Close);
}
