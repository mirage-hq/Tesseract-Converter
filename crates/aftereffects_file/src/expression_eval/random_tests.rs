use super::*;
fn random_sample(code: &str, dimension: usize, time: f64) -> (Vec<f64>, BTreeSet<String>) {
    let mut prepared = prepare(&model(code, dimension)).unwrap();
    let values = evaluate_grid(&mut prepared, 0, &[time]).unwrap();
    (values[0].clone(), prepared.approximated)
}

#[test]
fn random_shadowed_numeric_locals_keep_deterministic_admission() {
    for name in ["seedRandom", "random", "gaussRandom", "wiggle", "noise"] {
        let code = format!("var {name} = linear(time,0,1,0,100); {name};");
        let (values, approximated) = random_sample(&code, 1, 0.5);
        assert_eq!(values, vec![50.0], "local {name}");
        assert!(approximated.is_empty(), "local {name} is not a random API");
        assert!(syntax::compile(&format!("var {name}=1; {name}();")).is_err());
    }
}

#[test]
fn random_unshadowed_noise_call_keeps_approximation_diagnostic() {
    let (values, approximated) = random_sample("noise(time);", 1, 0.5);
    assert_eq!(values.len(), 1);
    assert!(values[0].is_finite());
    assert_eq!(approximated, BTreeSet::from(["noise".to_owned()]));
    assert!(syntax::compile("thisLayer.noise(time);").is_err());
}

#[test]
fn random_stream_is_repeatable_seeded_and_time_sensitive() {
    let code = "seedRandom(7); [random(),random(),random()];";
    let a = random_sample(code, 3, 0.3);
    assert_eq!(a, random_sample(code, 3, 0.3));
    assert_ne!(a.0, random_sample(code, 3, 0.4).0);
    assert!(a.0.iter().all(|v| (0.0..1.0).contains(v)));
    assert_ne!(a.0[0], a.0[1]);
    let frozen = "seedRandom(7,true); [random(),random(),random()];";
    assert_eq!(
        random_sample(frozen, 3, 0.3).0,
        random_sample(frozen, 3, 0.4).0
    );
    assert_ne!(
        random_sample(frozen, 3, 0.3).0,
        random_sample("seedRandom(8,true); [random(),random(),random()];", 3, 0.3).0
    );
    evaluate(
        "seedRandom(7,true); var a=random(); seedRandom(7,true); random()-a;",
        0.3,
        &[0.0],
    );
    assert_eq!(a.1, BTreeSet::from(["random".into(), "seedRandom".into()]));
}

#[test]
fn random_vector_bounds_and_gaussian_are_finite_and_repeatable() {
    let result = random_sample("seedRandom(9,true); random([2,4],[3,6]);", 2, 0.2).0;
    assert!((2.0..3.0).contains(&result[0]) && (4.0..6.0).contains(&result[1]));
    let one_bound = random_sample("random([2,4]);", 2, 0.2).0;
    assert!((0.0..2.0).contains(&one_bound[0]) && (0.0..4.0).contains(&one_bound[1]));
    let gaussian = random_sample("seedRandom(3,true); gaussRandom([2,4],[3,6]);", 2, 0.2);
    assert_eq!(
        gaussian,
        random_sample("seedRandom(3,true); gaussRandom([2,4],[3,6]);", 2, 0.2)
    );
    assert!(gaussian.0.iter().all(|v| v.is_finite()));
    assert!(gaussian.1.contains("gaussRandom"));
    evaluate("gaussRandom(2,2);", 0.2, &[2.0]);
}

#[test]
fn random_posterize_clock_and_property_streams_are_isolated() {
    assert_eq!(
        random_sample("posterizeTime(2); random();", 1, 0.1).0,
        random_sample("posterizeTime(2); random();", 1, 0.4).0
    );
    let mut m = model("random();", 1);
    m.properties[1].expression = Some(syntax::compile("random();").unwrap());
    let mut prepared = prepare(&m).unwrap();
    let a = evaluate_grid(&mut prepared, 0, &[0.3, 0.4, 0.3]).unwrap();
    assert_eq!(a[0], a[2]);
    let other = evaluate_grid(&mut prepared, 1, &[0.3]).unwrap();
    assert_ne!(a[0], other[0]);
    assert_eq!(a[0], evaluate_grid(&mut prepared, 0, &[0.3]).unwrap()[0]);
}

#[test]
fn random_wiggle_and_noise_respond_to_controls_and_custom_time() {
    let a = random_sample("wiggle(2,5);", 2, 0.3);
    assert_eq!(a, random_sample("wiggle(2,5);", 2, 0.3));
    assert!(a.0.iter().all(|v| (25.0..35.0).contains(v)));
    assert_ne!(a.0[0], a.0[1]);
    assert_ne!(a.0, random_sample("wiggle(3,5);", 2, 0.3).0);
    assert_ne!(a.0, random_sample("wiggle(2,5,2);", 2, 0.3).0);
    evaluate("wiggle(0,5);", 0.3, &[30.0]);
    evaluate("wiggle(2,0);", 0.3, &[30.0]);
    assert_eq!(
        random_sample("wiggle(2,5,1,0.5,0.7);", 1, 0.3).0,
        random_sample("wiggle(2,5);", 1, 0.7).0
    );
    let n = random_sample("noise([0.2,0.3,0.4]);", 1, 0.0);
    assert_eq!(n, random_sample("noise([0.2,0.3,0.4]);", 1, 1.0));
    assert!((-1.0..=1.0).contains(&n.0[0]));
    assert_ne!(n.0, random_sample("noise([0.4,0.3,0.4]);", 1, 0.0).0);
    assert!(n.1.contains("noise"));
    assert!(
        (random_sample("noise(0.999999);", 1, 0.0).0[0]
            - random_sample("noise(1.000001);", 1, 0.0).0[0])
            .abs()
            < 1e-5
    );
}

#[test]
fn random_dependency_approximations_propagate_without_tainting_deterministic_samples() {
    let mut m = model("thisComp.layer('controller').effect('amount')(1)+1;", 1);
    m.properties[1].expression =
        Some(syntax::compile("seedRandom(2,true); gaussRandom();").unwrap());
    let mut prepared = prepare(&m).unwrap();
    evaluate_grid(&mut prepared, 0, &[0.2]).unwrap();
    assert_eq!(
        prepared.approximated,
        BTreeSet::from(["seedRandom".into(), "gaussRandom".into()])
    );
    assert!(random_sample("value+1;", 1, 0.2).1.is_empty());
}

#[test]
fn random_noise_rejects_empty_coordinates_without_partial_samples() {
    for code in [
        "noise([]);",
        "noise([1,2,3,4]);",
        "time < 0.5 ? noise([1]) : noise([]);",
    ] {
        let mut prepared = prepare(&model(code, 1)).unwrap();
        let error = evaluate_grid(&mut prepared, 0, &[0.2, 0.8]).expect_err(code);
        let expected = if code.contains("noise([])") {
            // The production finite() guard rejects empty arrays before the lattice.
            "nonfinite or nonnumeric expression value"
        } else {
            "noise supports one, two or three coordinates"
        };
        assert!(error.to_string().contains(expected), "{code}: {error}");
    }
    for code in [
        "noise(1);",
        "noise([1]);",
        "noise([1,2]);",
        "noise([1,2,3]);",
    ] {
        let (values, approximated) = random_sample(code, 1, 0.2);
        assert!(values[0].is_finite(), "{code}");
        assert!(approximated.contains("noise"), "{code}");
    }
}

#[test]
fn random_invalid_arguments_fail_without_returning_partial_samples() {
    for code in [
        "random([1,2],[3,4,5]);",
        "seedRandom(2,1); random();",
        "wiggle(-1,2);",
        "wiggle(1,2,17);",
        "noise([1,2,3,4]);",
        "wiggle(1,2,1,0.5,1e20);",
    ] {
        let mut prepared = prepare(&model(code, 1)).unwrap();
        assert!(evaluate_grid(&mut prepared, 0, &[0.2]).is_err(), "{code}");
    }
    assert!(syntax::compile("Math.random();").is_err());
    assert!(syntax::compile("textIndex+random();").is_err());
}

#[test]
fn random_planar_wiggle_keeps_native_position_z_zero() {
    let mut m = model("wiggle(2,5);", 3);
    m.properties[0].keys[1].value[2] = 0.0;
    let values = evaluate_grid(&mut prepare(&m).unwrap(), 0, &[0.3]).unwrap();
    assert_eq!(values[0][2], 0.0);
    assert_ne!(values[0][0], 30.0);
}

#[test]
fn random_every_api_emits_contextual_converter_approximation_diagnostic() {
    use crate::rifx::Chunk;
    fn replace_expression(chunks: &mut [Chunk], code: &str) -> usize {
        let mut replaced = 0;
        for chunk in chunks {
            let property = chunk.list_kind() == Some(*b"tdbs");
            if let Some(children) = chunk.children_mut() {
                if property
                    && children.iter().any(|child| {
                        child.id() == *b"tdb4"
                            && child
                                .data_payload()
                                .is_some_and(|bytes| bytes.len() == 124 && bytes[119] & 1 == 0)
                    })
                {
                    for child in children.iter_mut().filter(|child| child.id() == *b"Utf8") {
                        *child = Chunk::data(*b"Utf8", code.as_bytes()).unwrap();
                        replaced += 1;
                    }
                }
                replaced += replace_expression(children, code);
            }
        }
        replaced
    }
    // Mutated public native source is supplementary converter-path evidence,
    // not an independently Adobe-authored random feature oracle.
    let source =
        include_bytes!("../../tests/fixtures/expression_samples/sampled_position_expression.aep");
    for (api, code) in [
        ("seedRandom", "seedRandom(3,true); [random(),random(),0];"),
        ("random", "[random(),random(),0];"),
        ("gaussRandom", "[gaussRandom(),gaussRandom(),0];"),
        ("wiggle", "wiggle(2,5);"),
        ("noise", "[noise(time),noise(time+1),0];"),
    ] {
        let mut project = crate::structure::read_project(source).unwrap();
        let item = project.items.iter_mut().find(|item| item.id == 1).unwrap();
        let crate::structure::ItemKind::Composition(comp) = &mut item.kind else {
            panic!("public composition missing");
        };
        let changed: usize = comp
            .layers
            .iter_mut()
            .map(|layer| replace_expression(&mut layer.content, code))
            .sum();
        assert_eq!(changed, 1);
        let native_id = comp.layers[0].record.id();
        let converted =
            crate::structure_document::to_structural_fx_document(&project, Some(1)).unwrap();
        assert!(
            converted.diagnostics.iter().any(|diagnostic| {
                diagnostic.composition_id == Some(1)
                    && diagnostic.layer_id == Some(native_id)
                    && diagnostic
                        .message
                        .contains(&format!("AE expression {api} approximated"))
            }),
            "missing {api} diagnostic: {:?}",
            converted.diagnostics
        );
    }
}
