use super::*;
use fx_schema::{
    Justification, PropertyAnimator, PropertyTarget, TextLayer,
    animator::{KeyframeId, PropertyKeyframe, PropertyKeyframeTrack},
};

fn collect_texts(layers: &[fx_schema::Layer], output: &mut Vec<TextLayer>) {
    for layer in layers {
        match layer.data() {
            LayerData::Text(text) => output.push(text.clone()),
            LayerData::Group(group) => collect_texts(&group.layers, output),
            _ => {}
        }
    }
}

fn texts(project: &StructuralProject) -> Vec<TextLayer> {
    let imported = to_structural_fx_document(project, Some(1)).unwrap();
    let mut output = Vec::new();
    collect_texts(imported.document.composition().layers(), &mut output);
    output
}

fn native_texts() -> Vec<TextLayer> {
    texts(
        &read_project(include_bytes!(
            "../../../tests/fixtures/text/source_stroke_holds.aep"
        ))
        .unwrap(),
    )
}

fn entry(
    id: LayerId,
    property: PropType,
    values: Vec<(i64, PropertyValue)>,
) -> AnimationGraphEntry {
    AnimationGraphEntry {
        target: PropertyTarget::layer(id, property),
        animator: PropertyAnimator::keyframes(
            PropertyKeyframeTrack::new(
                values
                    .into_iter()
                    .enumerate()
                    .map(|(index, (time, value))| {
                        PropertyKeyframe::new(
                            KeyframeId::new(format!(
                                "source-stroke-{}-{property:?}-{index}",
                                id.value()
                            )),
                            fx_schema::TimeOffset::from_millis(time),
                            value,
                            PropertyKeyframeEasing::Hold,
                        )
                    })
                    .collect(),
            )
            .unwrap(),
        ),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

/// Explicit editable controls derived from the two independent native Hold states.
fn edited_document(second_width: f64) -> EditableFxCompositionDocument {
    let native = native_texts();
    let mut value = imported();
    let mut layers = Vec::new();
    let mut entries = Vec::new();
    for (index, box_text) in [false, true].into_iter().enumerate() {
        let first = native
            .iter()
            .find(|text| {
                text.source_text.box_text == box_text
                    && text.source_text.stroke_width.value() == 3.0
            })
            .unwrap();
        let second = native
            .iter()
            .find(|text| {
                text.source_text.box_text == box_text
                    && text.source_text.stroke_width.value() == 9.0
                    && text.source_text.apply_stroke
            })
            .unwrap();
        let id = LayerId::new(5701 + u64::try_from(index).unwrap());
        layers.push(json!({
            "type": "Text", "id": id, "name": if box_text {"Box source stroke"} else {"Point source stroke"},
            "parent": null, "activeRange": {"start": 0, "duration": 1000},
            "transform": {"anchorPoint": [0.0,0.0], "position": [160.0,90.0], "scale": [100.0,100.0], "rotation": 0.0, "opacity": 100.0},
            "sourceText": first.source_text
        }));
        entries.extend([
            entry(
                id,
                PropType::StrokeEnabled,
                vec![
                    (0, PropertyValue::Bool(true)),
                    (750, PropertyValue::Bool(false)),
                ],
            ),
            entry(
                id,
                PropType::StrokeColor,
                vec![
                    (
                        0,
                        PropertyValue::Color(first.source_text.stroke_color.unwrap()),
                    ),
                    (
                        500,
                        PropertyValue::Color(second.source_text.stroke_color.unwrap()),
                    ),
                ],
            ),
            entry(
                id,
                PropType::StrokeWidth,
                vec![
                    (0, PropertyValue::Float(3.0)),
                    (500, PropertyValue::Float(second_width)),
                ],
            ),
        ]);
    }
    value["composition"]["layers"] = json!(layers);
    value["composition"]["dynamics"] = json!({"entries": entries});
    EditableFxCompositionDocument::from_json_value(value).unwrap()
}

#[test]
fn source_stroke_native_import_preserves_enabled_controls_and_disabled_state() {
    let imported = native_texts();
    assert_eq!(imported.len(), 6);
    for box_text in [false, true] {
        let mut selected = imported
            .iter()
            .filter(|text| text.source_text.box_text == box_text)
            .collect::<Vec<_>>();
        selected.sort_by_key(|text| text.active_range.start.as_millis());
        assert_eq!(selected.len(), 3);
        for (text, (time, enabled, width, color)) in selected.into_iter().zip([
            (0, true, 3.0, [0.8, 0.3, 0.1, 1.0]),
            (500, true, 9.0, [0.1, 0.7, 0.9, 1.0]),
            (750, false, 9.0, [0.1, 0.7, 0.9, 1.0]),
        ]) {
            assert_eq!(text.active_range.start.as_millis(), time);
            assert_eq!(text.source_text.apply_stroke, enabled);
            assert_eq!(text.source_text.stroke_width.value(), width);
            assert_eq!(text.source_text.stroke_color, Some(color));
            assert!(text.source_text.stroke_over_fill);
            assert_eq!(text.source_text.text, "AB\nCD");
            assert_eq!(text.source_text.fill_color, [0.2, 0.4, 0.6, 1.0]);
            assert_eq!(text.source_text.baseline_shift, 7.0);
            assert_eq!(text.source_text.justification, Justification::Right);
        }
    }
}

#[test]
fn source_stroke_full_export_consumes_editable_hold_controls_and_input_edits() {
    for edited_width in [9.0, 11.0] {
        let output = to_aep(&edited_document(edited_width)).unwrap();
        let generated = read_project(&output.bytes).unwrap();
        assert_eq!(layers(&generated).len(), 2, "{:?}", output.diagnostics);
        let reimported = texts(&generated);
        assert_eq!(reimported.len(), 6);
        for box_text in [false, true] {
            let mut selected = reimported
                .iter()
                .filter(|text| text.source_text.box_text == box_text)
                .collect::<Vec<_>>();
            selected.sort_by_key(|text| text.active_range.start.as_millis());
            assert_eq!(selected.len(), 3);
            for (text, (time, enabled, width, color)) in selected.into_iter().zip([
                (0, true, 3.0, [0.8, 0.3, 0.1, 1.0]),
                (500, true, edited_width, [0.1, 0.7, 0.9, 1.0]),
                (750, false, edited_width, [0.1, 0.7, 0.9, 1.0]),
            ]) {
                assert_eq!(text.active_range.start.as_millis(), time);
                assert_eq!(text.source_text.apply_stroke, enabled);
                assert_eq!(text.source_text.stroke_width.value(), width);
                assert_eq!(text.source_text.stroke_color, Some(color));
                assert!(text.source_text.stroke_over_fill);
                assert_eq!(text.source_text.text, "AB\nCD");
                assert_eq!(text.source_text.fill_color, [0.2, 0.4, 0.6, 1.0]);
                assert_eq!(text.source_text.baseline_shift, 7.0);
                assert_eq!(text.source_text.justification, Justification::Right);
            }
        }
    }
}
