//! Converter-runtime compatibility regressions with analytical, not Adobe, oracles.
use super::*;

#[test]
fn legacy_numeric_if_else_without_semicolon_keeps_both_branches() {
    for (time, expected) in [(0.0, 0.0), (0.5, 100.0)] {
        evaluate(
            "if(thisComp.layer('controller').effect('amount')('Slider') == 100)100 else 0;",
            time,
            &[expected],
        );
    }
    evaluate("if (time > 0) -2.5 else +3", 1.0, &[-2.5]);
    evaluate("if (time > 0) -2.5 else +3", 0.0, &[3.0]);
    for source in [
        "if (time) random() else 0",
        "if (time) 100 else eval('0')",
        "if (Math.random()) 100 else 0",
    ] {
        assert!(syntax::compile(source).is_err(), "{source}");
    }
}

#[test]
fn implicit_single_assignment_api_aliases_keep_numeric_dependencies() {
    assert!(syntax::compile_text("s=effect('amount')('Slider'); s + '%' ").is_err());
    evaluate(
        "ctrl=thisComp.layer('controller').effect('amount')('Slider'); angle=degreesToRadians(ctrl); [100*Math.cos(angle),100*Math.sin(angle),ctrl.valueAtTime(0.75)]",
        0.45,
        &[0.0, 100.0, 150.0],
    );
    evaluate(
        "c=thisComp; l=c.layer('controller'); p=l.effect('amount')(1); p.valueAtTime(0.75)",
        0.5,
        &[150.0],
    );
    for code in [
        "p=thisProperty; p=1; p",
        "p=1; p=thisProperty; p",
        "p+=thisProperty; p",
        "p=thisProperty; p++; p",
        "p=thisProperty; var p=1; p",
        "var p=1; p=thisProperty; p",
        "function ref(){ p=thisProperty; return p; } ref().length || 1",
        "p=thisLayer; p.position=[1,2,3]; 1",
    ] {
        assert!(
            matches!(syntax::compile(code), Err(EvaluationError::Unsupported(_))),
            "implicit aliases must not bypass mutation/escape guards: {code}"
        );
    }
}

#[test]
fn safe_aliases_and_zero_rate_keep_ae_units_and_clock() {
    evaluate(
        "degrees_to_radians(180)/Math.PI + radians_to_degrees(Math.PI)/180",
        0.0,
        &[2.0],
    );
    evaluate("length(-3) + length(10,4)", 0.0, &[9.0]);
    evaluate("posterizeTime(0); time + value", 0.75, &[0.0]);
    evaluate("loop_out('CyClE',999)", 1.25, &[25.0]);
    evaluate("loop_in('OFFSET',999)", -0.25, &[-25.0]);
    evaluate("velocityAtTime(0.5)", 0.0, &[100.0]);
}

#[test]
fn callable_interfaces_and_metadata_resolve_stored_names() {
    evaluate(
        "thisComp('controller').effect('amount')('Slider')",
        0.5,
        &[100.0],
    );
    evaluate(
        "thisComp.numLayers + (thisComp._name === 'main' ? 1 : 0)",
        0.0,
        &[3.0],
    );
    let mut source = model(
        "thisComp('controller')('Transform')('ADBE Position').valueAtTime(0.5)",
        1,
    );
    source.comps[0].layers[1]
        .transform
        .insert("position".into(), 1);
    let mut prepared = prepare(&source).unwrap();
    assert_eq!(
        evaluate_grid(&mut prepared, 0, &[0.0]).unwrap(),
        vec![vec![100.0]]
    );
    evaluate("thisLayer._name === 'subject' ? 1 : 0", 0.0, &[1.0]);
}

#[test]
fn loops_use_selected_property_and_static_values() {
    let mut source = model(
        "thisComp.layer('controller').effect('amount')('Slider').loopOut('cycle')",
        1,
    );
    source.properties[1].expression = None;
    source.properties[1].initial = vec![7.0];
    source.properties[1].keys.clear();
    let mut prepared = prepare(&source).unwrap();
    assert_eq!(
        evaluate_grid(&mut prepared, 0, &[2.25]).unwrap(),
        vec![vec![7.0]]
    );
    source.properties[1].keys.push(Key {
        time: 1.0,
        value: vec![11.0],
        hold: false,
        ease: vec![None],
    });
    let mut prepared = prepare(&source).unwrap();
    assert_eq!(
        evaluate_grid(&mut prepared, 0, &[2.25]).unwrap(),
        vec![vec![11.0]]
    );
    source.properties[1].keys.push(Key {
        time: 2.0,
        value: vec![31.0],
        hold: false,
        ease: vec![None],
    });
    let mut prepared = prepare(&source).unwrap();
    assert_eq!(
        evaluate_grid(&mut prepared, 0, &[2.25]).unwrap(),
        vec![vec![16.0]]
    );
}

#[test]
fn world_points_and_directions_do_not_use_camera_projection() {
    let mut source = model("thisComp.layer('controller').toWorld([1,2,3])", 3);
    source.properties[1].expression = None;
    source.properties[1].keys.clear();
    source.properties[1].initial = vec![10.0, 20.0, 100.0];
    source.comps[0].layers[1].three_d = true;
    source.comps[0].layers[1]
        .transform
        .insert("position".into(), 1);
    let mut prepared = prepare(&source).unwrap();
    assert_eq!(
        evaluate_grid(&mut prepared, 0, &[0.0]).unwrap(),
        vec![vec![11.0, 22.0, 103.0]]
    );
    source.properties[0].expression =
        Some(syntax::compile("thisComp.layer('controller').toWorldVec([1,2,3])").unwrap());
    let mut prepared = prepare(&source).unwrap();
    assert_eq!(
        evaluate_grid(&mut prepared, 0, &[0.0]).unwrap(),
        vec![vec![1.0, 2.0, 3.0]]
    );
    source.properties[0].expression =
        Some(syntax::compile("thisComp.layer('controller').fromWorld([11,22,103])").unwrap());
    let mut prepared = prepare(&source).unwrap();
    assert_eq!(
        evaluate_grid(&mut prepared, 0, &[0.0]).unwrap(),
        vec![vec![1.0, 2.0, 3.0]]
    );
}

#[test]
fn unsupported_receivers_metadata_and_style_members_fail_closed() {
    for code in [
        "thisComp.pixelAspect || 1",
        "thisLayer.startTime || 0",
        "thisLayer.position.style.fontSize || 12",
        "thisLayer.toWorldVec.call(thisLayer,[1,2,3])",
        "var f=thisLayer.toWorldVec; f([1,2,3])",
        "thisComp.constructor('return 1')()",
    ] {
        assert!(syntax::compile(code).is_err(), "{code}");
    }
    for code in ["thisLayer('unknown')", "thisLayer.xRotation + 1"] {
        let mut prepared = prepare(&model(code, 1)).unwrap();
        assert!(evaluate_grid(&mut prepared, 0, &[0.0]).is_err(), "{code}");
    }
}
