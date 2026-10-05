use super::*;

fn mixed_document() -> EditableFxCompositionDocument {
    let scalar = document("return input.time.milliseconds;")
        .to_json_value()
        .unwrap();
    modify(
        &text_document("return input.time.milliseconds < 500 ? 'before' : 'after';"),
        |raw| {
            let text_entry = raw["composition"]["dynamics"]["entries"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["target"]["propertyType"] == "textContent")
                .unwrap()
                .clone();
            let mut path_layer = scalar["composition"]["layers"][0].clone();
            path_layer["id"] = json!(8);
            raw["composition"]["layers"]
                .as_array_mut()
                .unwrap()
                .extend([scalar["composition"]["layers"][0].clone(), path_layer]);
            let scalar_entry = scalar["composition"]["dynamics"]["entries"][0].clone();
            let mut path_entry = scalar_entry.clone();
            path_entry["target"]["layerId"] = json!(8);
            path_entry["target"]["propertyType"] = json!("shapePath");
            path_entry["animator"]["layerTimeJsCode"] =
                json!("return {commands:[{type:'moveTo',x:input.time.milliseconds,y:0}]};");
            let mut failed = scalar_entry.clone();
            failed["target"]["propertyType"] = json!("positionY");
            failed["animator"]["layerTimeJsCode"] =
                json!("globalThis.calls = (globalThis.calls || 0) + 1; return globalThis.calls;");
            raw["composition"]["dynamics"]["entries"] =
                json!([scalar_entry, text_entry, path_entry, failed]);
        },
    )
}

#[test]
fn parallel_mixed_tracks_preserve_serial_keys_and_ordered_diagnostics() {
    let original = mixed_document();
    let options = AfterEffectsExportOptions::default();
    let roots = original.composition().layers();
    let serial =
        prepare_scripts_with_workers(&original, roots, false, &options, Progress::default(), 1)
            .unwrap();
    let parallel =
        prepare_scripts_with_workers(&original, roots, false, &options, Progress::default(), 2)
            .unwrap();
    assert_eq!(
        serial.document.to_json_value().unwrap(),
        parallel.document.to_json_value().unwrap()
    );
    assert_eq!(serial.diagnostics, parallel.diagnostics);
    for index in 0..3 {
        assert!(!track(&parallel, index).keyframes().is_empty());
    }
    assert!(
        parallel.document.composition().dynamics().entries()[3]
            .animator
            .is_js_script()
    );
}

#[test]
fn parallel_selected_scope_keeps_unselected_scripts_unchanged() {
    let original = mixed_document();
    let options = AfterEffectsExportOptions::default();
    let roots = &original.composition().layers()[..1];
    let serial =
        prepare_scripts_with_workers(&original, roots, true, &options, Progress::default(), 1)
            .unwrap();
    let parallel =
        prepare_scripts_with_workers(&original, roots, true, &options, Progress::default(), 2)
            .unwrap();
    assert_eq!(
        serial.document.to_json_value().unwrap(),
        parallel.document.to_json_value().unwrap()
    );
    assert_eq!(serial.diagnostics, parallel.diagnostics);
    assert!(
        parallel.document.composition().dynamics().entries()[0]
            .animator
            .is_js_script()
    );
    assert!(
        !parallel.document.composition().dynamics().entries()[1]
            .animator
            .is_js_script()
    );
}

#[test]
fn failed_text_fit_reserves_the_same_ids_as_original_sampling() {
    let original = text_document(
        "globalThis.calls = (globalThis.calls || 0) + 1; return String(globalThis.calls);",
    );
    let entry = original
        .composition()
        .dynamics()
        .entries()
        .iter()
        .find(|entry| entry.animator.is_js_script())
        .unwrap();
    let owner = Owner {
        id: original.composition().layers()[0].id(),
        duration_ms: 100,
        start_ms: 0,
        unsupported_clock: false,
        clock_id: 0,
    };
    let mut builder = keys::Builder::default();
    let mut budget = Budget::default();
    let sampling = Sampling::new(&AfterEffectsExportOptions::default()).unwrap();
    let result = fit_entry(
        entry,
        Some(owner),
        original.composition().dynamics().entries(),
        &|_| Some(owner),
        &mut builder,
        &mut budget,
        sampling,
    );
    assert!(matches!(result, Err(BakeError::Validation(100))));
    let mut expected = BTreeSet::new();
    let code = match entry.animator.data() {
        AnimatorData::JsScript {
            layer_time_js_code: Some(code),
            ..
        } => code,
        _ => unreachable!(),
    };
    let identity =
        conversion_identity_seed(&serde_json::to_vec(&entry.target).unwrap(), code.as_bytes());
    for time in owner.times(sampling) {
        converted_keyframe_id(identity, time as i64, &mut expected);
    }
    let mut actual = BTreeSet::new();
    assert!(
        builder
            .finish(result, &entry.target, &mut actual, &mut budget)
            .is_err()
    );
    assert_eq!(actual, expected);
    assert_eq!(budget.keys, 0);
}
