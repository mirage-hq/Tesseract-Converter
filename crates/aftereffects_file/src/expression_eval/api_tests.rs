//! Extended AE expression API: interpolation, velocity, layer space, loops and
//! helpers, evaluated against analytically known values.

use super::*;

fn near(code: &str, time: f64, expected: &[f64], tolerance: f64) {
    let mut prepared = prepare(&model(code, expected.len())).unwrap();
    let actual = evaluate_grid(&mut prepared, 0, &[time]).unwrap_or_else(|e| panic!("{code}: {e}"));
    for (a, e) in actual[0].iter().zip(expected) {
        assert!((a - e).abs() <= tolerance, "{code}: {a} != {e}");
    }
}

#[test]
fn ease_family_uses_ae_influence_curves() {
    // ease is symmetric; easeIn/easeOut are complementary at the midpoint.
    near("ease(0.5,0,1,0,100)", 0.0, &[50.0], 1e-9);
    // Cubic Hermite values measured from AE: easeIn(0.5)=0.375, easeOut(0.5)=0.625.
    near("easeIn(0.5,0,1,0,1000)", 0.0, &[375.0], 1e-9);
    near("easeOut(0.5,0,1,0,1000)", 0.0, &[625.0], 1e-9);
    near(
        "easeIn(0.5,0,1,0,100) + easeOut(0.5,0,1,0,100)",
        0.0,
        &[100.0],
        1e-6,
    );
    near(
        "easeIn(0.25,0,1,0,100) < easeOut(0.25,0,1,0,100) ? 1 : 0",
        0.0,
        &[1.0],
        0.0,
    );
    near("ease(time,0,100)", 1.0, &[100.0], 1e-9);
    near("easeOut(2,0,1,[0,0],[10,20])", 0.0, &[10.0, 20.0], 1e-9);
}

#[test]
fn velocity_speed_and_smooth_follow_the_property_curve() {
    // Model keys move 0 -> 100 over one second on every component.
    near("velocity", 0.5, &[100.0, 100.0], 1e-6);
    near("[speed, 0]", 0.5, &[100.0 * 2f64.sqrt(), 0.0], 1e-6);
    near(
        "thisProperty.velocityAtTime(0.5)",
        0.0,
        &[100.0, 100.0],
        1e-6,
    );
    near(
        "[thisProperty.speedAtTime(0.5), 0]",
        0.0,
        &[100.0 * 2f64.sqrt(), 0.0],
        1e-6,
    );
    near("smooth(0.2, 5)", 0.5, &[50.0, 50.0], 1e-9);
}

/// Evaluate `code` on the controller's one-dimensional Slider, which may read
/// the subject layer's transform without a dependency cycle.
fn near_on_controller(code: &str, time: f64, expected: &[f64]) {
    let mut m = model("value", 2);
    m.properties[1].expression = Some(syntax::compile(code).unwrap());
    let actual = evaluate_grid(&mut prepare(&m).unwrap(), 1, &[time])
        .unwrap_or_else(|e| panic!("{code}: {e}"));
    for (a, e) in actual[0].iter().zip(expected) {
        assert!((a - e).abs() <= 1e-9, "{code}: {a} != {e}");
    }
}

#[test]
fn layer_space_transforms_round_trip_through_position() {
    // Identity anchor/scale/rotation: the subject's origin maps to Position.
    let subject = "thisComp.layer('subject')";
    near_on_controller(&format!("{subject}.toComp([0,0])[0]"), 0.5, &[50.0]);
    near_on_controller(&format!("{subject}.toWorld([10,0])[0]"), 0.5, &[60.0]);
    near_on_controller(
        &format!("{subject}.fromComp({subject}.toComp([3,4]))[1]"),
        0.5,
        &[4.0],
    );
    near_on_controller(&format!("{subject}.fromWorld([50,50])[0]"), 0.5, &[0.0]);
}

#[test]
fn loops_operators_and_helpers_evaluate_numerically() {
    near(
        "var s=0; for (var i=0; i<4; i++) { s += i; } s",
        0.0,
        &[6.0],
        0.0,
    );
    near("var n=0; while (n < 5) { n++; } n", 0.0, &[5.0], 0.0);
    near("var n=0; do { n += 2; } while (n < 5); n", 0.0, &[6.0], 0.0);
    near("7 % 3", 0.0, &[1.0], 0.0);
    near("var v=[1,2]; v *= 3; v", 0.0, &[3.0, 6.0], 0.0);
    near(
        "radiansToDegrees(degreesToRadians(45))",
        0.0,
        &[45.0],
        1e-12,
    );
    near("dot([1,2,3],[4,5,6])", 0.0, &[32.0], 0.0);
    near("cross([1,0,0],[0,1,0])", 0.0, &[0.0, 0.0, 1.0], 0.0);
    near(
        "hslToRgb(rgbToHsl([0.2,0.4,0.6,1]))",
        0.0,
        &[0.2, 0.4, 0.6, 1.0],
        1e-12,
    );
    near("lookAt([0,0,0],[0,0,10])", 0.0, &[0.0, 0.0, 0.0], 1e-12);
    near("var width = 3; width", 0.0, &[3.0], 0.0);
    near("thisLayer.width + height", 0.0, &[200.0], 0.0);
    near("active ? 1 : 0", 0.5, &[1.0], 0.0);
}

#[test]
fn loop_durations_repeat_the_trailing_window() {
    // Keys 0 -> 100 over [0,1]; the last 0.5s window repeats after 1s.
    near("loopOutDuration('cycle', 0.5)", 1.25, &[75.0, 75.0], 1e-9);
    near("loopInDuration('cycle', 0.5)", -0.25, &[25.0, 25.0], 1e-9);
}

#[test]
fn runaway_loops_and_unknown_members_fail_instead_of_guessing() {
    let mut prepared = prepare(&model("while (true) {} value", 2)).unwrap();
    assert!(evaluate_grid(&mut prepared, 0, &[0.0]).is_err());
    for code in [
        "content('Missing').size",
        "mask(1).maskOpacity",
        "thisLayer.parent.position",
    ] {
        let mut prepared = prepare(&model(code, 2)).unwrap();
        assert!(evaluate_grid(&mut prepared, 0, &[0.0]).is_err(), "{code}");
    }
}

#[test]
fn legal_ranges_clamp_expression_results_like_ae() {
    let mut m = model("150", 1);
    m.properties[0].range = Some([Some(0.0), Some(100.0)]);
    let values = evaluate_grid(&mut prepare(&m).unwrap(), 0, &[0.0]).unwrap();
    assert_eq!(values[0], vec![100.0]);
    let mut m = model("[-5, 3]", 2);
    m.properties[0].range = Some([Some(0.0), None]);
    let values = evaluate_grid(&mut prepare(&m).unwrap(), 0, &[0.0]).unwrap();
    assert_eq!(values[0], vec![0.0, 3.0]);
}

#[test]
fn source_text_expressions_evaluate_to_strings() {
    let mut m = model(
        "value + ' ' + (time * 10).toFixed(1) + `!` + String(7).padStart(3, '0')",
        1,
    );
    m.properties[0].text = Some("Score".into());
    m.properties[0].initial = Vec::new();
    let texts = evaluate_text_grid(&mut prepare(&m).unwrap(), 0, &[0.25, 1.0]).unwrap();
    assert_eq!(
        texts,
        vec!["Score 2.5!007".to_owned(), "Score 10.0!007".to_owned()]
    );
}

#[test]
fn three_d_layer_space_round_trips_through_the_default_camera() {
    let mut m = model("value", 3);
    m.comps[0].layers[0].three_d = true;
    m.properties[0].keys.clear();
    m.properties[0].initial = vec![100.0, 50.0, 0.0];
    let mut rotation_y = property(1, 1, "value", 1);
    rotation_y.identity = PropertyIdentity::Transform {
        match_name: "ADBE Rotate Y".into(),
    };
    rotation_y.keys.clear();
    rotation_y.initial = vec![90.0];
    rotation_y.expression = None;
    rotation_y.expression_enabled = false;
    m.properties.push(rotation_y);
    m.comps[0].layers[0].transform.insert("yRotation".into(), 2);
    m.properties[0].expression = None;
    m.properties[0].expression_enabled = false;
    m.properties[1].expression = Some(
        syntax::compile(
            "var l=thisComp.layer('subject'); var q=l.fromComp(l.toComp([10,-4,7])); q[0]*100+q[1]*10+q[2]",
        )
        .unwrap(),
    );
    let values = evaluate_grid(&mut prepare(&m).unwrap(), 1, &[0.0]).unwrap();
    assert!(
        (values[0][0] - (1000.0 - 40.0 + 7.0)).abs() < 1e-6,
        "{values:?}"
    );
}

#[test]
fn source_text_random_apis_report_their_approximation() {
    let mut m = model("Math.round(random(100)) + '%'", 1);
    m.properties[0].text = Some("x".into());
    m.properties[0].initial = Vec::new();
    let mut prepared = prepare(&m).unwrap();
    let texts = evaluate_text_grid(&mut prepared, 0, &[0.0, 0.5]).unwrap();
    assert!(texts.iter().all(|t| t.ends_with('%')));
    assert!(prepared.approximated.contains("random"));
    // A deterministic Source Text owner does not inherit the previous report.
    m.properties[0].expression = Some(syntax::compile("value + '!'").unwrap());
    let mut prepared = prepare(&m).unwrap();
    evaluate_text_grid(&mut prepared, 0, &[0.0]).unwrap();
    assert!(prepared.approximated.is_empty());
}
