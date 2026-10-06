use super::stroke_keys::{document, numeric_layer};
use super::*;

fn input(inherited: bool) -> Value {
    let mut value = document("Shape", PropertyKeyframeEasing::Linear, inherited, false);
    value["composition"]["dynamics"] = json!({"entries": [
        keyed_entry(400.into(), PropType::TrimStart, [(0, PropertyValue::Float(0.0)), (500, PropertyValue::Float(50.0))]),
        keyed_entry(400.into(), PropType::TrimEnd, [(0, PropertyValue::Float(0.0)), (500, PropertyValue::Float(100.0))]),
        keyed_entry(400.into(), PropType::TrimOffset, [(0, PropertyValue::Float(0.0)), (500, PropertyValue::Float(90.0))])
    ]});
    value
}

#[test]
fn inert_shape_trim_keys_retain_untrimmed_editable_owner() {
    for inherited in [false, true] {
        let output = export(input(inherited));
        let mut control = input(inherited);
        control["composition"]["dynamics"] = json!({"entries":[]});
        assert_eq!(
            output.bytes,
            export(control).bytes,
            "inert Trim must not affect native geometry or paint"
        );
        let native = read_project(&output.bytes).unwrap();
        assert!(
            numeric_layer(&native, layers(&native), "ADBE Vector Stroke Width").is_some(),
            "{:?}",
            output.diagnostics
        );
        assert!(numeric_layer(&native, layers(&native), "ADBE Vector Trim End").is_none());
        assert!(
            output
                .diagnostics
                .iter()
                .any(|warning| warning.layer_id == Some(400.into())
                    && warning
                        .message
                        .contains("Trim controls target a Shape without the modifier")),
            "{:?}",
            output.diagnostics
        );
    }
}

#[test]
fn installed_shape_trim_still_exports_editable_keys() {
    let mut value = input(false);
    value["composition"]["layers"][0]["shape"]["trim"] =
        json!({"start":0.0,"end":100.0,"offset":0.0});
    let output = export(value);
    let native = read_project(&output.bytes).unwrap();
    let (_, end) = numeric_layer(&native, layers(&native), "ADBE Vector Trim End")
        .expect("installed Trim retained");
    assert_eq!(end.keyframes.len(), 2);
    assert_eq!(end.keyframes[0].values, vec![0.0]);
    assert_eq!(end.keyframes[1].values, vec![100.0]);
    assert!(!output.diagnostics.iter().any(|warning| {
        warning
            .message
            .contains("Trim controls target a Shape without the modifier")
    }));
}
