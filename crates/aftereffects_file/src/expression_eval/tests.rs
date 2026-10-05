#[path = "api_tests.rs"]
mod api_tests;
#[path = "expression_apis2_tests.rs"]
mod expression_apis2_tests;
#[path = "expression_apis_tests.rs"]
mod expression_apis_tests;
#[path = "random_tests.rs"]
mod random_tests;
#[path = "wiggle_coverage_tests.rs"]
mod wiggle_coverage_tests;
#[path = "wiggle_targets_tests.rs"]
mod wiggle_targets_tests;

use super::*;
use crate::expression_samples::PropertyIdentity;
use model::{Comp, Effect, Key, LayerModel, Parameter, Property};
use std::collections::BTreeMap;

#[test]
fn expression_evaluator_matches_pinned_adobe_position_oracle() {
    const SOURCE: &[u8] =
        include_bytes!("../../tests/fixtures/expression_samples/sampled_position_expression.aep");
    const ORACLE: &[u8] = include_bytes!(
        "../../tests/fixtures/expression_samples/sampled_position_expression.v2.json"
    );
    let project = crate::structure::read_project(SOURCE).unwrap();
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    let crate::structure::ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("pinned composition missing");
    };
    let oracle = ExpressionSamples::from_json_for_source(ORACLE, SOURCE).unwrap();
    let observed = oracle.properties().first().unwrap();
    assert_eq!(oracle.properties().len(), 1);
    assert_eq!(observed.values().len(), 2001);
    let model = Model::new(&items, 1, comp, true);
    let slot = model
        .properties
        .iter()
        .position(|property| {
            property.comp_id == observed.composition_id()
                && property.layer_id == observed.layer_id()
                && &property.identity == observed.property()
        })
        .unwrap();
    let actual = evaluate_grid(
        &mut prepare(&model).unwrap(),
        slot,
        observed.sample_times_seconds(),
    )
    .unwrap();
    let mut maximum_error = 0.0_f64;
    for (index, (actual, native)) in actual.iter().zip(observed.values()).enumerate() {
        assert_eq!(actual.len(), native.len());
        for (actual, native) in actual.iter().zip(native) {
            let error = (actual - native).abs();
            maximum_error = maximum_error.max(error);
            assert!(error <= 1e-8, "sample {index}: {actual} != {native}");
        }
    }
    eprintln!("pinned Adobe Position oracle: 2001 vectors; maximum error {maximum_error}");
    let converted =
        crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
    // Source-frame lowering and the 2001-vector numerical oracle are distinct.
    // Check persisted editable keys against the actual 49 source-frame values.
    let frames = evaluate_occurrence(&items, 1, comp, &ExpressionSamples::default(), false);
    let frame_values = frames
        .lookup(1, observed.layer_id(), observed.property())
        .unwrap();
    assert_eq!(frame_values.values.len(), 49);
    let (_, error) = persisted_position_errors(
        &converted,
        &frame_values.sample_times_seconds,
        &frame_values.values,
    )
    .unwrap_or_else(|| panic!("Position animation omitted: {:?}", converted.diagnostics));
    assert!(error <= 1e-8, "persisted 49-frame Position error {error}");
    assert!(
        !String::from_utf8(converted.document.to_json_vec().unwrap())
            .unwrap()
            .to_ascii_lowercase()
            .contains("jsscript")
    );
}

fn persisted_position_errors(
    converted: &crate::structure_document::StructuralConversion,
    times: &[f64],
    values: &[Vec<f64>],
) -> Option<(usize, f64)> {
    use fx_schema::animator::{PropertyKeyframeEasing, PropertyKeyframeTrack};
    use fx_schema::{PropType, PropertyValue};
    let entries = converted.document.composition().dynamics().entries();
    let mut maximum_error = 0.0_f64;
    let mut key_count = 0;
    for (component, property) in [PropType::PositionX, PropType::PositionY]
        .into_iter()
        .enumerate()
    {
        let matching: Vec<_> = entries
            .iter()
            .filter(|entry| {
                entry
                    .target
                    .as_property()
                    .is_some_and(|target| target.property_type() == property)
            })
            .collect();
        let [entry] = matching.as_slice() else {
            return None;
        };
        let track = entry.animator.keyframe_track()?;
        let persisted: PropertyKeyframeTrack =
            serde_json::from_slice(&serde_json::to_vec(track).unwrap()).unwrap();
        persisted.validate_for_target(&entry.target).unwrap();
        let keys = persisted.keyframes();
        key_count += keys.len();
        let value = |index: usize| match keys[index].value() {
            PropertyValue::Float(value) => *value,
            other => panic!("non-scalar Position key: {other:?}"),
        };
        for (time, native) in times.iter().zip(values) {
            let time_ms = time * 1000.0;
            let right = keys.partition_point(|key| (key.layer_time().as_millis() as f64) < time_ms);
            let actual = if right == 0 {
                value(0)
            } else if right == keys.len() {
                value(right - 1)
            } else if keys[right].layer_time().as_millis() as f64 == time_ms {
                value(right)
            } else {
                let start = keys[right - 1].layer_time().as_millis() as f64;
                let end = keys[right].layer_time().as_millis() as f64;
                let p = (time_ms - start) / (end - start);
                let progress = match keys[right].easing() {
                    PropertyKeyframeEasing::Hold => 0.0,
                    PropertyKeyframeEasing::Linear => p,
                    PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
                        assert_eq!((x1, x2), (1.0 / 3.0, 2.0 / 3.0));
                        3.0 * (1.0 - p).powi(2) * p * y1
                            + 3.0 * (1.0 - p) * p.powi(2) * y2
                            + p.powi(3)
                    }
                };
                value(right - 1) + (value(right) - value(right - 1)) * progress
            };
            assert!(actual.is_finite());
            maximum_error = maximum_error.max((actual - native[component]).abs());
            assert_eq!(
                native[2], 0.0,
                "the pinned native 2D Position has static zero Z"
            );
        }
    }
    Some((key_count, maximum_error))
}

#[test]
fn expression_actual_time_source_frames_and_native_vectors_lower_independently() {
    const SOURCE: &[u8] =
        include_bytes!("../../tests/fixtures/expression_samples/sampled_position_expression.aep");
    const ORACLE: &[u8] = include_bytes!(
        "../../tests/fixtures/expression_samples/sampled_position_expression.v2.json"
    );
    let project = crate::structure::read_project(SOURCE).unwrap();
    let items = project.items.iter().map(|item| (item.id, item)).collect();
    let crate::structure::ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("pinned comp missing")
    };
    let oracle = ExpressionSamples::from_json_for_source(ORACLE, SOURCE).unwrap();
    let observed = &oracle.properties()[0];
    let frames = evaluate_occurrence(&items, 1, comp, &ExpressionSamples::default(), false);
    let frame_values = frames
        .lookup(1, observed.layer_id(), observed.property())
        .unwrap();
    assert_eq!(frame_values.values.len(), 49);
    let converted =
        crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
    let (keys, error) = persisted_position_errors(
        &converted,
        &frame_values.sample_times_seconds,
        &frame_values.values,
    )
    .unwrap_or_else(|| {
        panic!(
            "49-frame Position lowering omitted: {:?}",
            converted.diagnostics
        )
    });
    assert!(error <= 1e-8, "49 source-frame lowering error {error}");
    eprintln!(
        "source-frame lowering: 49 vectors, {keys} editable Position keys, maximum error {error}"
    );
    let (_, unsampled_error) = persisted_position_errors(
        &converted,
        observed.sample_times_seconds(),
        observed.values(),
    )
    .unwrap();
    eprintln!(
        "same 49-frame track at 2001 native times (NOT its constraints): maximum error {unsampled_error}"
    );

    let model = Model::new(&items, 1, comp, true);
    let slot = model
        .properties
        .iter()
        .position(|property| {
            property.comp_id == 1
                && property.layer_id == observed.layer_id()
                && &property.identity == observed.property()
        })
        .unwrap();
    let values = evaluate_grid(
        &mut prepare(&model).unwrap(),
        slot,
        observed.sample_times_seconds(),
    )
    .unwrap();
    assert_eq!(values.len(), 2001);
    for (actual, native) in values.iter().zip(observed.values()) {
        for (actual, native) in actual.iter().zip(native) {
            assert!((actual - native).abs() <= 1e-8);
        }
    }
    // Retain native clock/identity, but lower fresh Boa values, not oracle values.
    let mut fresh = oracle.clone();
    fresh.properties[0].values = values;
    let native_converted =
        crate::structure_document::to_structural_fx_document_with_assets_and_expressions(
            &project,
            Some(1),
            &mut |_| false,
            &fresh,
        )
        .unwrap();
    let (keys, error) = persisted_position_errors(
        &native_converted,
        observed.sample_times_seconds(),
        observed.values(),
    )
    .unwrap_or_else(|| {
        panic!(
            "2001-vector Position lowering omitted: {:?}",
            native_converted.diagnostics
        )
    });
    assert!(error <= 1e-8, "2001-vector lowering error {error}");
    eprintln!(
        "native-vector lowering: 2001 vectors, {keys} editable Position keys, maximum error {error}"
    );
}

#[test]
fn expression_frame_grid_rejects_oversized_allocations() {
    assert!(frame_grid(1_000_000.0, 30.0).is_err());
    assert!(frame_grid(1.0, 30.0).is_ok());
}

fn property(comp_id: u32, layer_id: u32, code: &str, dimension: usize) -> Property {
    Property {
        comp_id,
        layer_id,
        identity: PropertyIdentity::Transform {
            match_name: "ADBE Position".into(),
        },
        initial: vec![0.0; dimension],
        keys: vec![
            Key {
                time: 0.0,
                value: vec![0.0; dimension],
                hold: false,
                ease: vec![None; dimension],
            },
            Key {
                time: 1.0,
                value: vec![100.0; dimension],
                hold: false,
                ease: vec![None; dimension],
            },
        ],
        expression: Some(syntax::compile(code).unwrap()),
        error: None,
        expression_enabled: true,
        notes: Vec::new(),
        range: None,
        text: None,
    }
}
fn model(code: &str, dimension: usize) -> Model {
    let subject = LayerModel {
        id: 1,
        name: "subject".into(),
        index: 1,
        three_d: false,
        in_point: Some(0.0),
        out_point: Some(1.0),
        transform: BTreeMap::from([("position".into(), 0)]),
        separated_position: None,
        width: 100.0,
        height: 100.0,
        default_anchor: [0.0, 0.0],
        enabled: true,
        parent: None,
        masks: Vec::new(),
        content: Vec::new(),
        source_text: None,
        effects: Vec::new(),
    };
    let controller = LayerModel {
        id: 2,
        name: "controller".into(),
        index: 2,
        three_d: false,
        in_point: Some(0.0),
        out_point: Some(1.0),
        transform: BTreeMap::new(),
        separated_position: None,
        width: 100.0,
        height: 100.0,
        default_anchor: [0.0, 0.0],
        enabled: true,
        parent: None,
        masks: Vec::new(),
        content: Vec::new(),
        source_text: None,
        effects: vec![Effect {
            name: "amount".into(),
            match_name: "ADBE Slider Control".into(),
            index: 1,
            parameters: vec![Parameter {
                name: "Slider".into(),
                match_name: "ADBE Slider Control-0001".into(),
                index: Some(1),
                slot: 1,
            }],
        }],
    };
    Model {
        allow_foreign: true,
        comps: vec![Comp {
            id: 1,
            name: "main".into(),
            width: 1920,
            height: 1080,
            duration: 1.0,
            fps: 30.0,
            display_start: 0.0,
            layers: vec![subject, controller],
            has_camera: false,
        }],
        properties: vec![
            property(1, 1, code, dimension),
            property(1, 2, "value*2", 1),
        ],
    }
}
fn evaluate(code: &str, time: f64, expected: &[f64]) {
    let mut prepared = prepare(&model(code, expected.len())).unwrap();
    let actual = evaluate_grid(&mut prepared, 0, &[time]).unwrap();
    assert_eq!(actual[0].len(), expected.len());
    for (actual, expected) in actual[0].iter().zip(expected) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "{code}: {actual} != {expected}"
        );
    }
}
#[test]
fn expression_named_helpers_use_native_key_records_and_recoverable_ranges() {
    evaluate(
        "function sample(t) { var n=nearestKey(t).index; if(key(n).time>t) { n--; } try { var a=key(n); var b=key(n+1); } catch(e) { return null; } return linear(t,a.time,b.time,a.value,b.value); } sample(time) || value",
        0.25,
        &[25.0],
    );
    evaluate(
        "function dimensions() { var dim=1; try { key(1)[1]; dim=2; key(1)[2]; dim=3; } catch(e) {} switch(dim) { case 1: return 10; case 2: return 20; default: return 30; } } [dimensions(),dimensions()]",
        0.25,
        &[20.0, 20.0],
    );
    evaluate("try { key(0); } catch(e) { 7; }", 0.25, &[7.0]);
    let model = model(
        "try { thisComp.layer('missing'); } catch(e) { var failure=null; 7; }",
        1,
    );
    let mut prepared = prepare(&model).unwrap();
    assert!(
        evaluate_grid(&mut prepared, 0, &[0.25]).is_err(),
        "catch must not clear a fatal dependency failure"
    );
}

#[test]
fn expression_native_key_dependency_errors_remain_sticky() {
    for call in ["key(1)", "nearestKey(time)"] {
        let code = format!(
            "try {{ thisComp.layer('controller').effect('amount')(1).{call}.value; }} catch(e) {{ value; }}"
        );
        let mut source = model(&code, 1);
        source.properties[1].error = Some("invalid native dependency clock".into());
        let mut prepared = prepare(&source).unwrap();
        assert!(
            evaluate_grid(&mut prepared, 0, &[0.25]).is_err(),
            "{call} must not bypass a fatal dependency rejection"
        );
    }
}

#[test]
fn expression_num_keys_dependency_errors_remain_sticky() {
    for reason in [
        "invalid native dependency clock",
        "unsupported native interpolation",
    ] {
        for code in [
            "thisComp.layer('controller').effect('amount')(1).numKeys",
            "try { thisComp.layer('controller').effect('amount')(1).numKeys; } catch(e) { 7; }",
        ] {
            let mut source = model(code, 1);
            source.properties[1].keys.clear();
            source.properties[1].error = Some(reason.into());
            let mut prepared = prepare(&source).unwrap();
            let error = evaluate_grid(&mut prepared, 0, &[0.0, 0.25]).unwrap_err();
            assert!(error.to_string().contains(reason), "{code}: {error}");
        }
    }
    evaluate(
        "thisComp.layer('controller').effect('amount')(1).numKeys",
        0.25,
        &[2.0],
    );
}

#[test]
fn expression_helpers_reject_api_reference_escapes() {
    for code in [
        "function layerRef(){ return thisLayer; } layerRef().length || 1",
        "function propertyRef(){ return thisProperty; } propertyRef() ? 100 : 0",
        "function ref(){ return thisComp; } ref().length || 1",
        "function ref(){ return transform; } ref().length || 1",
        "function ref(){ return effect('amount'); } ref().length || 1",
        "var alias=thisLayer; function ref(){ return alias; } ref().length || 1",
        "function ref(){ return alias; } var alias=thisLayer; ref().length || 1",
        "function ref(){ return time ? thisLayer : thisComp; } ref().length || 1",
        "function lengthOf(x){ return x.length || 1; } lengthOf(thisLayer)",
        "function truth(x){ return x ? 100 : 0; } truth(thisProperty)",
        "function forward(x){ return x; } function ref(){ return forward(thisLayer); } ref().length || 1",
        "function ref(){ return [thisLayer]; } ref()[0].length || 1",
        "var refs=[[thisProperty]]; function ref(){ return refs; } ref()[0][0] ? 100 : 0",
    ] {
        assert!(
            matches!(syntax::compile(code), Err(EvaluationError::Unsupported(_))),
            "API reference escaped helper admission: {code}"
        );
    }
    evaluate("function twice(x){ return x*2; } twice(time)", 0.25, &[0.5]);
    evaluate(
        "function dimensions(){ return [thisComp.width,thisComp.height]; } dimensions()",
        0.25,
        &[1920.0, 1080.0],
    );
    evaluate(
        "function first(){ return key(1); } first()[0]",
        0.25,
        &[0.0],
    );
    evaluate(
        "function component(k){ return k[0]; } component(key(2))",
        0.25,
        &[100.0],
    );
    evaluate(
        "function sample(){ return thisProperty.key(2).value; } sample()",
        0.25,
        &[100.0],
    );
}

#[test]
fn value_time_and_linear_hold_value_at_time_use_native_clock() {
    evaluate("value", 0.25, &[25.0]);
    evaluate("time*10", 0.25, &[2.5]);
    evaluate("valueAtTime(0.75)", 0.25, &[75.0]);
    evaluate("thisProperty.valueAtTime(0.75)", 0.25, &[75.0]);
    let mut model = model("valueAtTime(0.75)", 1);
    model.properties[0].keys[0].hold = true;
    let mut prepared = prepare(&model).unwrap();
    assert_eq!(
        evaluate_grid(&mut prepared, 0, &[0.5]).unwrap(),
        vec![vec![0.0]]
    );
    model.properties[0].expression = Some(syntax::compile("valueAtTime(1)").unwrap());
    assert_eq!(
        evaluate_grid(&mut prepare(&model).unwrap(), 0, &[0.5]).unwrap(),
        vec![vec![100.0]]
    );
}
#[test]
fn vector_ast_preserves_precedence_completion_and_comments() {
    evaluate("var x=[1,2]; x=x*2+[3,4]; x;", 0.0, &[5.0, 8.0]);
    evaluate("-[1,2]+[6,8]/2", 0.0, &[2.0, 2.0]);
    evaluate("var x=[1,2]; if(time>0) x=x+[3,4]; x", 0.5, &[4.0, 6.0]);
    evaluate("length([3,4])", 0.0, &[5.0]);
    evaluate("length([3,4],[0,0])", 0.0, &[5.0]);
    evaluate("normalize([3,4])", 0.0, &[0.6, 0.8]);
    evaluate("add([1,2],sub([8,10],[3,4]))", 0.0, &[6.0, 8.0]);
    evaluate("mul([3,4],2)", 0.0, &[6.0, 8.0]);
    evaluate("div([6,8],2)", 0.0, &[3.0, 4.0]);
}
#[test]
fn linear_clamp_and_math_are_deterministic() {
    evaluate("linear(time,0,1,10,20)", 0.25, &[12.5]);
    evaluate("linear(time,[0,10],[10,30])", 0.25, &[2.5, 15.0]);
    evaluate("linear(time,10,20)", -0.25, &[10.0]);
    evaluate("clamp(time,0,1)", 2.0, &[1.0]);
    evaluate(
        "Math.floor(time*10)+Math.abs(-3)+Math.cos(0)+Math.sqrt(4)",
        0.25,
        &[8.0],
    );
    evaluate("Math.PI", 0.0, &[std::f64::consts::PI]);
}
#[test]
fn frames_and_posterize_update_expression_clock_and_value_reads() {
    evaluate("framesToTime(15)", 0.0, &[0.5]);
    evaluate("timeToFrames(0.51)", 0.0, &[15.0]);
    evaluate("timeToFrames(-0.51,30,true)", 0.0, &[-16.0]);
    evaluate("posterizeTime(2);time", 0.75, &[0.5]);
    evaluate("posterizeTime(2);value", 0.75, &[50.0]);
    evaluate(
        "posterizeTime(2);thisComp.layer('controller').effect('amount')('Slider')",
        0.75,
        &[100.0],
    );
}
#[test]
fn layer_effect_transform_and_foreign_aliases_resolve_lazy_dependencies() {
    for code in [
        "thisComp.layer('controller').effect('amount')('Slider')",
        "thisComp.layer(2).effect(1)(1)",
        "comp('main').layer(2).effect('amount')('Slider')",
    ] {
        evaluate(code, 0.25, &[50.0]);
    }
    evaluate(
        "var c=thisComp; var l=c.layer('controller'); var p=l.effect('amount')(1); p.valueAtTime(0.75)",
        0.5,
        &[150.0],
    );
    evaluate("thisProperty.value", 0.25, &[25.0]);
    evaluate(
        "[thisProperty.value[0],thisProperty.value[1]]",
        0.25,
        &[25.0, 25.0],
    );
}
#[test]
fn cycles_missing_references_dimensions_and_nonfinite_fail_atomically() {
    for code in [
        "1/0",
        "thisComp.layer('missing').effect('amount')(1)",
        "normalize([0,0])",
        "[1,2]+[1,2,3]",
        "[1,2]+3",
        "thisLayer.transform.position.value",
    ] {
        let mut prepared = prepare(&model(code, 1)).unwrap();
        assert!(
            evaluate_grid(&mut prepared, 0, &[0.0, 0.5]).is_err(),
            "{code}"
        );
    }
    let mut model = model("thisComp.layer(2).effect(1)(1)", 1);
    model.properties[1].expression =
        Some(syntax::compile("thisComp.layer(1).transform.position.valueAtTime(time-1)").unwrap());
    let mut prepared = prepare(&model).unwrap();
    assert!(
        evaluate_grid(&mut prepared, 0, &[0.5])
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );
    assert!(evaluate_grid(&mut prepared, 0, &[0.25]).is_err());
}
#[test]
fn loop_modes_keep_boundaries_and_negative_cycles() {
    evaluate("loopOut('cycle')", 1.25, &[25.0]);
    evaluate("loopOut('pingpong')", 1.25, &[75.0]);
    evaluate("loopOut('offset')", 1.25, &[125.0]);
    evaluate("loopOut('continue')", 1.25, &[125.0]);
    evaluate("loopOut('cycle')", 1.0, &[100.0]);
    evaluate("loopIn('cycle')", -0.25, &[75.0]);
    evaluate("loopIn('offset')", -0.25, &[-25.0]);
}
#[test]
fn per_frame_invocations_are_fresh_and_frame_grid_is_inclusive() {
    let mut prepared = prepare(&model("var x=value; x=x+time; x", 1)).unwrap();
    assert_eq!(
        evaluate_grid(&mut prepared, 0, &[0.0, 0.5, 0.0]).unwrap(),
        vec![vec![0.0], vec![50.5], vec![0.0]]
    );
    let grid = frame_grid(1.0, 30.0).unwrap();
    assert_eq!(grid.len(), 31);
    assert_eq!(grid[0], 0.0);
    assert_eq!(grid[30], 1.0);
    assert!(frame_grid(1.0, 0.0).is_err());
}

#[test]
fn expression_captured_shape_is_diagnosed_instead_of_silently_dropped() {
    // Synthetic identity tests fail-closed capture handling, not Shape fidelity.
    let source =
        include_bytes!("../../tests/fixtures/expression_samples/sampled_position_expression.aep");
    let mut raw: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../tests/fixtures/expression_samples/sampled_position_expression.v2.json"
    ))
    .unwrap();
    raw["version"] = serde_json::json!(3);
    raw["properties"][0]["property"] = serde_json::json!({"kind":"shape", "path":[
        {"index":2,"match_name":"ADBE Root Vectors Group"},
        {"index":1,"match_name":"ADBE Vector Scale"}
    ]});
    let samples =
        ExpressionSamples::from_json_for_source(&serde_json::to_vec(&raw).unwrap(), source)
            .unwrap();
    let project = crate::structure::read_project(source).unwrap();
    let converted =
        crate::structure_document::to_structural_fx_document_with_assets_and_expressions(
            &project,
            Some(1),
            &mut |_| true,
            &samples,
        )
        .unwrap();
    assert!(converted.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("captured native Shape expression target")
            && diagnostic
                .message
                .contains("no editable expression mapping")
    }));
}

/// Independent 60-step reference for the shim's converter-identical easing.
fn reference_ease(progress: f64, [x1, y1, x2, y2]: [f64; 4]) -> f64 {
    let cubic = |t: f64, a: f64, b: f64| {
        let s = 1.0 - t;
        3.0 * s * s * t * a + 3.0 * s * t * t * b + t * t * t
    };
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..60 {
        let middle = (low + high) * 0.5;
        if cubic(middle, x1, x2) < progress {
            low = middle;
        } else {
            high = middle;
        }
    }
    cubic((low + high) * 0.5, y1, y2)
}

#[test]
fn wiggle_base_value_follows_converter_cubic_easing_between_keys() {
    let ease = [0.42, 0.0, 0.58, 1.0];
    let mut m = model("value;", 2);
    m.properties[0].keys[1].ease = vec![Some(ease); 2];
    let mut prepared = prepare(&m).unwrap();
    let times = [0.1, 0.25, 0.5, 0.8];
    let values = evaluate_grid(&mut prepared, 0, &times).unwrap();
    for (time, value) in times.iter().zip(values) {
        let expected = 100.0 * reference_ease(*time, ease);
        for component in value {
            assert!(
                (component - expected).abs() < 1e-5,
                "t={time}: {component} != {expected}"
            );
        }
    }
    // The eased base is what wiggle perturbs: equal controls, equal stream.
    let mut wiggle = model("wiggle(2,5) - value;", 2);
    wiggle.properties[0].keys[1].ease = vec![Some(ease); 2];
    let eased = evaluate_grid(&mut prepare(&wiggle).unwrap(), 0, &[0.3]).unwrap();
    let linear = evaluate_grid(
        &mut prepare(&model("wiggle(2,5) - value;", 2)).unwrap(),
        0,
        &[0.3],
    )
    .unwrap();
    for (a, b) in eased[0].iter().zip(&linear[0]) {
        assert!(
            (a - b).abs() < 1e-9,
            "wiggle offset must not depend on easing"
        );
    }
}

#[test]
fn separated_position_composes_dimension_properties_for_transform_position() {
    let mut m = model("transform.position[0] + 2 * transform.position[1];", 1);
    // Slot 0 is the combined Position leaf; separated storage is read through
    // the dimension slots, exactly as AE exposes `transform.position`.
    m.properties[0].error =
        Some("separated Position is read through its dimension properties".into());
    m.properties[0].expression = None;
    m.properties[0].expression_enabled = false;
    let mut x = property(1, 1, "value;", 1);
    x.identity = PropertyIdentity::Transform {
        match_name: "ADBE Position_0".into(),
    };
    x.expression = None;
    x.expression_enabled = false;
    let mut y = property(1, 1, "value;", 1);
    y.identity = PropertyIdentity::Transform {
        match_name: "ADBE Position_1".into(),
    };
    y.keys[1].value = vec![10.0];
    y.expression = None;
    y.expression_enabled = false;
    let mut consumer = property(
        1,
        1,
        "transform.position[0] + 2 * transform.position[1];",
        1,
    );
    consumer.identity = PropertyIdentity::Transform {
        match_name: "ADBE Rotate Z".into(),
    };
    m.properties.extend([x, y, consumer]);
    let subject = &mut m.comps[0].layers[0];
    subject.transform = BTreeMap::from([
        ("position".into(), 0),
        ("xPosition".into(), 2),
        ("yPosition".into(), 3),
        ("rotation".into(), 4),
    ]);
    subject.separated_position = Some(vec![2, 3]);
    let values = evaluate_grid(&mut prepare(&m).unwrap(), 4, &[0.5]).unwrap();
    assert!(
        (values[0][0] - (50.0 + 2.0 * 5.0)).abs() < 1e-9,
        "{values:?}"
    );
}
