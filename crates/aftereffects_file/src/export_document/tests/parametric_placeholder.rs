use super::*;

fn document_with_placeholder(path: Value, extra_geometry: Value) -> EditableFxCompositionDocument {
    let mut value = imported();
    let mut shape = rect(&value, 210);
    shape["type"] = json!("Shape");
    shape.as_object_mut().unwrap().remove("rect");
    shape["name"] = json!("Enter button");
    shape["shape"] = json!({
        "path": path,
        "ellipse": {"size": [82.0, 82.0], "position": [0.0, 0.0], "reversed": false},
        "polyStar": extra_geometry,
        "fills": [{"paint": {"type": "solid", "color": [0.0, 0.76, 0.94, 1.0]}}]
    });
    value["composition"]["layers"] = json!([shape]);
    value["composition"]["dynamics"] = json!({"entries": []});
    EditableFxCompositionDocument::from_json_value(value).unwrap()
}

#[test]
fn close_only_parametric_placeholder_keeps_editable_ellipse() {
    let document = document_with_placeholder(json!({"commands": [{"type": "close"}]}), Value::Null);
    let output = to_aep(&document).unwrap();
    let project = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&project).len(), 1, "{:?}", output.diagnostics);
    let imported = to_structural_fx_document(&project, Some(1)).unwrap();
    fn find_ellipse(layers: &[Layer]) -> Option<&fx_schema::layer::ShapeEllipse> {
        layers.iter().find_map(|layer| match layer.data() {
            LayerData::Shape(shape) if !shape.is_hidden => shape.shape.ellipse.as_ref(),
            LayerData::Group(group) => find_ellipse(&group.layers),
            _ => None,
        })
    }
    let ellipse = find_ellipse(imported.document.composition().layers()).unwrap();
    assert_eq!(ellipse.size, [82.0, 82.0]);
    assert_eq!(ellipse.position, [0.0, 0.0]);
}

#[test]
fn parametric_placeholder_does_not_admit_real_path_or_two_generators() {
    for document in [
        document_with_placeholder(
            json!({"commands": [{"type": "moveTo", "x": 0.0, "y": 0.0}, {"type": "close"}]}),
            Value::Null,
        ),
        document_with_placeholder(
            json!({"commands": [{"type": "close"}]}),
            json!({"points": 5.0, "outerRadius": 20.0}),
        ),
    ] {
        let output = to_aep(&document).unwrap();
        let project = read_project(&output.bytes).unwrap();
        assert!(layers(&project).is_empty(), "{:?}", output.diagnostics);
    }
}
