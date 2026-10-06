use super::*;

fn options() -> AfterEffectsExportOptions {
    AfterEffectsExportOptions { fps: 30.0 }
}

fn path_document(code: &str) -> EditableFxCompositionDocument {
    modify(&document(code), |raw| {
        raw["composition"]["dynamics"]["entries"][0]["target"]["propertyType"] = json!("shapePath");
    })
}

#[test]
fn fast_scalar_and_path_evaluate_only_the_grid_and_fresh_validation() {
    for original in [
        document("return input.time.milliseconds;"),
        path_document("return {commands:[{type:'moveTo',x:input.time.milliseconds,y:0}]};"),
    ] {
        let entry = &original.composition().dynamics().entries()[0];
        let owner = Some(Owner {
            id: original.composition().layers()[0].id(),
            duration_ms: 100,
            start_ms: 0,
            clock_id: 0,
            unsupported_clock: false,
        });
        let mut fast_budget = Budget::default();
        let fast = bake_entry(
            entry,
            owner,
            original.composition().dynamics().entries(),
            &|_| owner,
            &mut BTreeSet::new(),
            &mut fast_budget,
            Sampling::new(&options()).unwrap(),
        )
        .unwrap();
        // Two 13-sample passes plus two out-of-order probes.
        assert_eq!(fast_budget.calls, 28);
        let keys = fast.keyframe_track().unwrap().keyframes();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].layer_time().as_millis(), 0);
        assert_eq!(keys[1].layer_time().as_millis(), 100);
    }
}

#[test]
fn fast_warning_makes_missed_subframe_pulses_explicit() {
    let original = document("return input.time.milliseconds === 4 ? 100 : 0;");
    let fast = prepare(&original).unwrap();
    assert_eq!(track(&fast, 0).keyframes().len(), 1);
    assert!(
        fast.diagnostics
            .iter()
            .any(|d| d.message.contains("subframe pulses"))
    );
    assert!(
        fast.diagnostics
            .iter()
            .any(|d| d.message.contains("sampled-grid fit validation"))
    );
}

#[test]
fn fast_scalar_rejects_a_call_counter_even_on_the_same_sample_grid() {
    let original = document("globalThis.n=(globalThis.n||0)+1; return globalThis.n;");
    let prepared = prepare_with_progress(&original, &options(), Progress::default()).unwrap();
    assert!(
        prepared.document.composition().dynamics().entries()[0]
            .animator
            .is_js_script()
    );
    assert!(
        prepared
            .diagnostics
            .iter()
            .any(|d| d.message.contains("was not baked"))
    );
}

#[test]
fn fast_path_still_rejects_history_dependent_geometry() {
    let original = path_document(
        "globalThis.n=(globalThis.n||0)+1; return {commands:[{type:'moveTo',x:globalThis.n,y:0}]};",
    );
    let prepared = prepare_with_progress(&original, &options(), Progress::default()).unwrap();
    assert!(
        prepared.document.composition().dynamics().entries()[0]
            .animator
            .is_js_script()
    );
    assert!(
        prepared
            .diagnostics
            .iter()
            .any(|d| d.message.contains("was not baked"))
    );
}

#[test]
fn fast_path_retains_sampled_topology_changes_and_owner_endpoint() {
    let original = path_document(
        "var t=input.time.milliseconds; return {commands:t<50?[{type:'moveTo',x:0,y:0}]:[{type:'moveTo',x:0,y:0},{type:'lineTo',x:10,y:0}]};",
    );
    let prepared = prepare_with_progress(&original, &options(), Progress::default()).unwrap();
    let keys = track(&prepared, 0).keyframes();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[1].layer_time().as_millis(), 50);
    assert_eq!(keys[1].easing(), PropertyKeyframeEasing::Hold);
}

#[test]
fn fast_selected_scope_preserves_other_scripts_and_source_archive() {
    let original = modify(&document("return input.time.milliseconds;"), |raw| {
        let mut other = raw["composition"]["layers"][0].clone();
        other["id"] = json!(8);
        raw["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .push(other);
        let mut entry = raw["composition"]["dynamics"]["entries"][0].clone();
        entry["target"]["layerId"] = json!(8);
        raw["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
            .push(entry);
    });
    let source = original.to_json_value().unwrap();
    let prepared = prepare_layers_with_progress(
        &original,
        &original.composition().layers()[..1],
        true,
        &options(),
        Progress::default(),
    )
    .unwrap();
    assert!(
        prepared.document.composition().dynamics().entries()[0]
            .animator
            .keyframe_track()
            .is_some()
    );
    assert!(
        prepared.document.composition().dynamics().entries()[1]
            .animator
            .is_js_script()
    );
    assert_eq!(source, original.to_json_value().unwrap());
}
