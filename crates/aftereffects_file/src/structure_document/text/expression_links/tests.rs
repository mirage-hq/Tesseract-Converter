use super::*;
use crate::properties::{NumericKeyframe, read_numeric};

fn data(id: &[u8; 4], value: impl Into<Vec<u8>>) -> Chunk {
    let mut value = value.into();
    if id == b"tdmn" {
        value.resize(40, 0);
    }
    Chunk::data(*id, value).unwrap()
}

fn name(value: &str) -> Chunk {
    let mut bytes = b"Utf8".to_vec();
    bytes.extend(u32::try_from(value.len()).unwrap().to_be_bytes());
    bytes.extend(value.as_bytes());
    data(b"tdsn", bytes)
}

fn meta(dimensions: usize) -> Vec<u8> {
    let mut meta = vec![0; 124];
    meta[..2].copy_from_slice(&[0xdb, 0x99]);
    meta[3] = u8::try_from(dimensions).unwrap();
    meta[12..16].copy_from_slice(&1_000_u32.to_be_bytes());
    meta
}

fn stored(values: &[f64], expression: Option<&str>) -> Chunk {
    let mut chunks = vec![
        data(b"tdb4", meta(values.len())),
        data(b"tdsb", vec![0, 0, 0, 1]),
        data(
            b"cdat",
            values
                .iter()
                .flat_map(|value| value.to_be_bytes())
                .collect::<Vec<_>>(),
        ),
    ];
    chunks.extend(expression.map(|text| data(b"Utf8", text)));
    Chunk::list(*b"tdbs", chunks)
}

/// `(milliseconds, value, interpolation, speed, influence %)` scalar keys.
fn keyed(keys: &[(i32, f64, u8, f64, f64)]) -> Chunk {
    let mut header = vec![0; 24];
    header[10..12].copy_from_slice(&u16::try_from(keys.len()).unwrap().to_be_bytes());
    header[18..20].copy_from_slice(&48_u16.to_be_bytes());
    header[23] = 4;
    let mut items = Vec::new();
    for &(time, value, interpolation, speed, influence) in keys {
        let mut item = vec![0; 48];
        item[..4].copy_from_slice(&time.to_be_bytes());
        item[4] = interpolation;
        item[5] = interpolation;
        for (offset, number) in [
            (8, value),
            (16, speed),
            (24, influence / 100.0),
            (32, speed),
            (40, influence / 100.0),
        ] {
            item[offset..offset + 8].copy_from_slice(&number.to_be_bytes());
        }
        items.extend(item);
    }
    Chunk::list(
        *b"tdbs",
        vec![
            data(b"tdb4", meta(1)),
            data(b"tdsb", vec![0, 0, 0, 1]),
            Chunk::list(*b"list", vec![data(b"lhd3", header), data(b"ldat", items)]),
        ],
    )
}

fn leaf(match_name: &str, storage: Chunk) -> Vec<Chunk> {
    vec![data(b"tdmn", match_name), storage]
}

fn group(match_name: &str, label: Option<&str>, children: Vec<Chunk>) -> Vec<Chunk> {
    let mut body: Vec<Chunk> = label.map(name).into_iter().collect();
    body.extend(children);
    vec![data(b"tdmn", match_name), Chunk::list(*b"tdgp", body)]
}

fn effect(kind: &str, label: &str, value: Chunk) -> Vec<Chunk> {
    vec![
        data(b"tdmn", kind),
        Chunk::list(
            *b"sspc",
            vec![Chunk::list(
                *b"tdgp",
                [
                    vec![name(label)],
                    // A different hidden value proves that index 1 skips it.
                    leaf(&format!("{kind}-0000"), stored(&[7.0], None)),
                    leaf(&format!("{kind}-0001"), value),
                    group(
                        "ADBE Effect Built In Params",
                        Some("Compositing Options"),
                        Vec::new(),
                    ),
                ]
                .concat(),
            )],
        ),
    ]
}

fn slider(label: &str, value: Chunk) -> Vec<Chunk> {
    effect("ADBE Slider Control", label, value)
}

/// Layer content with an Effect Parade.
fn layer(effects: Vec<Chunk>) -> Vec<Chunk> {
    vec![Chunk::list(
        *b"tdgp",
        group("ADBE Effect Parade", None, effects),
    )]
}

fn range(label: &str, leaves: Vec<Chunk>) -> Vec<Chunk> {
    group("ADBE Text Selector", Some(label), leaves)
}

fn animator(label: &str, selectors: Vec<Chunk>) -> Vec<Chunk> {
    group(
        "ADBE Text Animator",
        Some(label),
        group("ADBE Text Selectors", None, selectors),
    )
}

fn text(animators: Vec<Chunk>) -> Vec<Chunk> {
    group("ADBE Text Animators", None, animators)
}

fn standard_effects() -> Vec<Chunk> {
    [
        slider("Value", stored(&[42.0], None)),
        slider("Width", stored(&[40.0], None)),
        slider(
            "Progress",
            keyed(&[(0, 0.0, 1, 0.0, 16.0), (800, 100.0, 1, 0.0, 16.0)]),
        ),
        slider(
            "Other",
            keyed(&[(100, 5.0, 1, 0.0, 16.0), (300, 6.0, 1, 0.0, 16.0)]),
        ),
        effect("Pseudo/choice", "Choice", stored(&[1.0], None)),
    ]
    .concat()
}

fn lower_in(
    content: &[Chunk],
    text_group: &[Chunk],
    expression: &str,
    own: &[f64],
) -> Result<NumericProperty, PropertyError> {
    let storage = [stored(own, Some(expression))];
    let storage = storage[0].children().unwrap();
    let own = read_numeric(storage).unwrap();
    assert!(own.expression_enabled);
    ExpressionLinks::new(content, text_group).lower(storage, &own)
}

fn lower(expression: &str) -> Result<NumericProperty, PropertyError> {
    lower_in(&layer(standard_effects()), &[], expression, &[-1.0])
}

fn constant(expression: &str) -> f64 {
    let lowered = lower(expression).unwrap_or_else(|error| panic!("{expression}: {error}"));
    assert!(!lowered.animated && !lowered.expression_enabled && !lowered.expression_present);
    let [value] = lowered.values[..] else {
        panic!("{expression}: {lowered:?}")
    };
    value
}

fn keys(property: &NumericProperty) -> Vec<(f64, f64)> {
    assert!(property.animated && property.values.is_empty());
    property
        .keyframes
        .iter()
        .map(|key| (key.time_secs, key.values[0]))
        .collect()
}

fn assert_keys(property: &NumericProperty, expected: &[(f64, f64)]) {
    let actual = keys(property);
    assert_eq!(actual.len(), expected.len(), "{actual:?}");
    for (&(time, value), &(expected_time, expected_value)) in actual.iter().zip(expected) {
        assert_eq!(time, expected_time, "{actual:?}");
        assert!((value - expected_value).abs() < 1e-12, "{actual:?}");
    }
}

fn rejected(expression: &str) -> String {
    match lower(expression) {
        Ok(lowered) => panic!("{expression} must be rejected: {lowered:?}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn numeric_index_one_reads_only_the_slider_value() {
    assert_eq!(constant("effect(\"Value\")(1)"), 42.0);
    assert_eq!(constant("effect('Value')( 1 );"), 42.0);
    assert_eq!(
        constant("effect(\"Value\")(\"ADBE Slider Control-0001\")"),
        42.0
    );
    for expression in [
        "effect(\"Value\")(0)",
        "effect(\"Value\")(2)",
        "effect(\"Value\")(10)",
        "effect(\"Value\")(1.0)",
        "effect(\"Value\")(\"ADBE Slider Control-0000\")",
        "effect(\"Value\")(\"Compositing Options\")",
    ] {
        rejected(expression);
    }
    assert!(rejected("effect(\"Choice\")(1)").contains("non-Slider effect"));
    assert!(rejected("effect(\"Missing\")(1)").contains("Slider not found"));
    let duplicate = layer([slider("Value", stored(&[1.0], None)), standard_effects()].concat());
    assert!(
        lower_in(&duplicate, &[], "effect(\"Value\")(1)", &[0.0])
            .unwrap_err()
            .to_string()
            .contains("ambiguous")
    );
}

#[test]
fn direct_static_angle_control_replaces_stale_text_rotation() {
    let content = layer(effect(
        "ADBE Angle Control",
        "Blur & Fade In - Rotation",
        stored(&[53.0], None),
    ));
    for expression in [
        "effect(\"Blur & Fade In - Rotation\")(1);",
        "effect(\"Blur & Fade In - Rotation\")(\"ADBE Angle Control-0001\")",
    ] {
        let lowered = lower_in(&content, &[], expression, &[-67.0]).unwrap();
        assert_eq!(lowered.values, [53.0]);
        assert!(!lowered.expression_enabled && !lowered.expression_present);
        assert!(!lowered.animated && lowered.keyframes.is_empty());
    }
    for expression in [
        "effect(\"Blur & Fade In - Rotation\")(0)",
        "effect(\"Blur & Fade In - Rotation\")(2)",
        "effect(\"Blur & Fade In - Rotation\")(1.0)",
        "effect(\"Blur & Fade In - Rotation\")(\"ADBE Angle Control-0000\")",
        "effect(\"Blur & Fade In - Rotation\")(1);evil()",
        "effect(\"Blur & Fade In - Rotation\")(1)+time",
        "effect(\"Blur & Fade In - Rotation\")(1)+1",
    ] {
        assert!(
            lower_in(&content, &[], expression, &[-67.0]).is_err(),
            "{expression}"
        );
    }
    for value in [
        stored(&[53.0], Some("time")),
        stored(&[f64::INFINITY], None),
        keyed(&[(0, 0.0, 1, 0.0, 16.0), (800, 53.0, 1, 0.0, 16.0)]),
    ] {
        let content = layer(effect("ADBE Angle Control", "Angle", value));
        assert!(lower_in(&content, &[], "effect(\"Angle\")(1)", &[-67.0]).is_err());
    }
    let ambiguous = layer(
        [
            effect("ADBE Angle Control", "Angle", stored(&[53.0], None)),
            slider("Angle", stored(&[0.0], None)),
        ]
        .concat(),
    );
    assert!(lower_in(&ambiguous, &[], "effect(\"Angle\")(1)", &[-67.0]).is_err());
    assert!(
        lower_in(
            &content,
            &[],
            "effect(\"Blur & Fade In - Rotation\")(1)",
            &[0.0, 0.0, 0.0]
        )
        .is_err()
    );
}

#[test]
fn affine_sums_map_values_and_speeds_without_moving_keys() {
    assert_eq!(constant("100-effect(\"Width\")(1);"), 60.0);
    assert_eq!(constant("  -effect('Width')(1) + 5 ; "), -35.0);
    assert_eq!(
        constant("var a = effect(\"Width\")(1);\r\nb = a - 1.5e1;\nvar c = .5 + b;\r\nc"),
        25.5
    );

    let bezier = [stored(&[0.0], Some("100 - effect(\"Curve\")(1)"))];
    let content = layer(slider(
        "Curve",
        keyed(&[(250, 10.0, 2, 30.0, 40.0), (1_250, 70.0, 2, -5.0, 60.0)]),
    ));
    let storage = bezier[0].children().unwrap();
    let own = read_numeric(storage).unwrap();
    let lowered = ExpressionLinks::new(&content, &[])
        .lower(storage, &own)
        .unwrap();
    assert_eq!(keys(&lowered), [(0.25, 90.0), (1.25, 30.0)]);
    let [first, second] = &lowered.keyframes[..] else {
        panic!("{lowered:?}")
    };
    let NumericKeyframe {
        in_speed,
        out_speed,
        in_influence,
        out_influence,
        in_interpolation,
        ..
    } = first;
    // Negation flips speeds; influences and Bezier interpolation are kept.
    assert_eq!((in_speed[0], out_speed[0]), (-30.0, -30.0));
    assert!((in_influence[0] - 40.0).abs() < 1e-9 && (out_influence[0] - 40.0).abs() < 1e-9);
    assert_eq!(*in_interpolation, 2);
    assert_eq!(second.out_speed[0], 5.0);
    assert_eq!(lowered.value_kind, NumericValueKind::Continuous);
}

#[test]
fn grammar_rejects_tails_extra_statements_and_unsafe_tokens() {
    for (expression, reason) in [
        ("effect(\"Value\")(1).value", "suffix"),
        ("effect(\"Value\")(1) * 2", "suffix"),
        ("effect(\"Value\")(1); x()", "suffix"),
        ("effect(\"Value\")(1);;", "suffix"),
        ("effect(\"Value\")(1) // note", "suffix"),
        ("var a = effect(\"Value\")(1); a; 5", "suffix"),
        ("effect(\"Value\")(1).valueAtTime(0)", "suffix"),
        ("100--effect(\"Value\")(1)", "adjacent sign"),
        ("100 - -effect(\"Value\")(1)", "adjacent sign"),
        ("1 + +2", "adjacent sign"),
        ("--effect(\"Value\")(1)", "operand"),
        ("010 - effect(\"Value\")(1)", "numeric literal"),
        ("1e309 - effect(\"Value\")(1)", "numeric literal"),
        ("100px - effect(\"Value\")(1)", "numeric literal"),
        ("1.5.2", "numeric literal"),
        ("var a = 1; var a = 2; a", "duplicate expression binding"),
        (
            "var a = effect(\"Value\")(1); 5",
            "unused expression binding",
        ),
        ("b + 1", "unknown expression identifier"),
        ("var a = a + 1; a", "unknown expression identifier"),
        ("var if = 1; if", "reserved"),
        ("var value = 1; value", "reserved"),
        ("var linear = 1; linear", "reserved"),
        ("var a = 1 var b = 2; a + b", "semicolon"),
        ("var a = 1\nvar b = a; b", "semicolon"),
        ("a == effect(\"Value\")(1)", "unknown expression identifier"),
        (
            "thisComp.layer(\"x\").effect(\"Value\")(1)",
            "unknown expression identifier",
        ),
        ("time + 1", "unknown expression identifier"),
        (
            "effect(\"Progress\")(1) + effect(\"Other\")(1)",
            "more than one animated control",
        ),
        ("effect(\"Value\")(1) ? 1 : 2", "suffix"),
    ] {
        let error = rejected(expression);
        assert!(error.contains(reason), "{expression}: {error}");
    }
    // One animated operand plus static operands stays an exact affine map.
    let lowered = lower("effect(\"Progress\")(1) + effect(\"Value\")(1) - 2").unwrap();
    assert_eq!(keys(&lowered), [(0.0, 40.0), (0.8, 140.0)]);
}

#[test]
fn linear_needs_static_increasing_bounds_and_an_unclamped_input() {
    // Arbitrary binding and control names; slope 2, intercept -100.
    let offset = "var progress = effect(\"Progress\")(1);\r\nvar span = 100-effect(\"Value\")(1);\r\n\r\nlinear(progress,0,100,-100,100-span);";
    assert_keys(&lower(offset).unwrap(), &[(0.0, -100.0), (0.8, 42.0)]);
    // A static input clamps exactly.
    assert_eq!(constant("linear(150, 0, 100, -100, 100)"), 100.0);
    assert_eq!(constant("linear(-5, 0, 100, 3, 9)"), 3.0);
    assert_eq!(constant("linear(25, 0, 100, 0, 1)"), 0.25);
    // Hold segments stay between their key values.
    let hold = layer(slider(
        "Steps",
        keyed(&[(0, 20.0, 3, 0.0, 16.0), (500, 80.0, 3, 0.0, 16.0)]),
    ));
    let lowered = lower_in(
        &hold,
        &[],
        "linear(effect(\"Steps\")(1), 0, 100, 0, 1)",
        &[0.0],
    );
    assert_keys(&lowered.unwrap(), &[(0.0, 0.2), (0.5, 0.8)]);

    for (expression, reason) in [
        // The 0→100 Progress leaves [10, 90]: linear() would clamp.
        (
            "linear(effect(\"Progress\")(1), 10, 90, 0, 1)",
            "leave its range",
        ),
        (
            "linear(effect(\"Progress\")(1), 100, 0, 0, 1)",
            "increasing input range",
        ),
        (
            "linear(effect(\"Progress\")(1), 5, 5, 0, 1)",
            "increasing input range",
        ),
        (
            "linear(effect(\"Value\")(1), 0, 100, 0, effect(\"Progress\")(1))",
            "bounds must be static",
        ),
        (
            "linear(effect(\"Progress\")(1), 0, 100, 1)",
            "five arguments",
        ),
        (
            "linear(effect(\"Progress\")(1), 0, 100, 1, 2, 3)",
            "five arguments",
        ),
        ("linear(1e300, 0, 1e-300, 0, 1e300)", "non-finite"),
        ("linear(effect(\"Progress\")(1))", "five arguments"),
    ] {
        let error = rejected(expression);
        assert!(error.contains(reason), "{expression}: {error}");
    }
    let bezier = layer(slider(
        "Eased",
        keyed(&[(0, 0.0, 2, 0.0, 33.0), (500, 100.0, 2, 0.0, 33.0)]),
    ));
    let error = lower_in(
        &bezier,
        &[],
        "linear(effect(\"Eased\")(1), 0, 100, 0, 1)",
        &[0.0],
    )
    .unwrap_err();
    assert!(error.to_string().contains("overshoot"), "{error}");
}

#[test]
fn vectors_need_static_components_that_fit_the_owner() {
    let position = "var x = value[0];\r\nvar y = effect(\"Value\")(1);\r\n\r\n[x,y];";
    let content = layer(standard_effects());
    let lowered = lower_in(&content, &[], position, &[3.0, 73.0, 0.0]).unwrap();
    assert_eq!(lowered.values, [3.0, 42.0]);
    assert!(!lowered.animated && !lowered.expression_enabled);
    let lowered = lower_in(&content, &[], "[value[2], 1, value[0]]", &[3.0, 73.0, 9.0]).unwrap();
    assert_eq!(lowered.values, [9.0, 1.0, 3.0]);
    for (expression, own, reason) in [
        (
            "[value[0], effect(\"Progress\")(1)]",
            &[0.0, 0.0][..],
            "must be static",
        ),
        ("[1]", &[0.0, 0.0], "unsupported vector"),
        ("[1, 2, 3, 4, 5]", &[0.0, 0.0], "unsupported vector"),
        ("[1, 2, 3]", &[0.0, 0.0], "dimensions"),
        ("effect(\"Value\")(1)", &[0.0, 0.0], "dimensions"),
        ("[1, 2]", &[0.0], "dimensions"),
        ("value[0]", &[0.0], "static vector property"),
        ("[value[3], 1]", &[0.0, 0.0], "value component"),
        ("[value[10], 1]", &[0.0, 0.0], "value component"),
        ("[value, 1]", &[0.0, 0.0], "value component"),
    ] {
        let error = lower_in(&content, &[], expression, own)
            .unwrap_err()
            .to_string();
        assert!(error.contains(reason), "{expression}: {error}");
    }
}

#[test]
fn selector_aliases_resolve_unique_named_percentage_fields() {
    let offset = "linear(effect(\"Progress\")(1),0,100,-100,100)";
    let source = range(
        "Lead",
        [
            leaf("ADBE Text Percent Start", stored(&[12.0], None)),
            leaf("ADBE Text Percent Offset", stored(&[9.0], Some(offset))),
            group(
                "ADBE Text Range Advanced",
                None,
                [
                    leaf("ADBE Text Range Units", stored(&[1.0], None)),
                    leaf("ADBE Text Levels Max Ease", stored(&[-33.0], None)),
                ]
                .concat(),
            ),
        ]
        .concat(),
    );
    // A chained alias resolves through its own expression, never stale storage.
    let relay = range(
        "Relay",
        leaf(
            "ADBE Text Percent Offset",
            stored(
                &[45.0],
                Some("text.animator(\"First\").selector(\"Lead\").offset"),
            ),
        ),
    );
    let index_units = range(
        "Counted",
        group(
            "ADBE Text Range Advanced",
            None,
            leaf("ADBE Text Range Units", stored(&[2.0], None)),
        ),
    );
    let wiggly = group("ADBE Text Wiggly Selector", Some("Shake"), Vec::new());
    let unnamed = range(DEFAULT_NAME, Vec::new());
    let text_group = text(
        [
            animator("First", source),
            animator("Second", [relay, index_units, wiggly, unnamed].concat()),
            animator("Twin", Vec::new()),
            animator("Twin", Vec::new()),
            animator(
                "Pair",
                [range("Same", Vec::new()), range("Same", Vec::new())].concat(),
            ),
        ]
        .concat(),
    );
    let content = layer(standard_effects());
    let resolve = |expression: &str| lower_in(&content, &text_group, expression, &[0.0]);
    let alias = |animator: &str, selector: &str, field: &str| {
        resolve(&format!(
            "text.animator(\"{animator}\").selector(\"{selector}\").{field};"
        ))
    };
    assert_eq!(alias("First", "Lead", "start").unwrap().values, [12.0]);
    // Sparse fields keep AE's native defaults.
    assert_eq!(alias("First", "Lead", "end").unwrap().values, [100.0]);
    assert_eq!(
        alias("First", "Lead", "advanced.easeLow").unwrap().values,
        [0.0]
    );
    assert_eq!(
        alias("First", "Lead", "advanced.easeHigh").unwrap().values,
        [-33.0]
    );
    let expected = [(0.0, -100.0), (0.8, 100.0)];
    assert_eq!(keys(&alias("First", "Lead", "offset").unwrap()), expected);
    assert_eq!(keys(&alias("Second", "Relay", "offset").unwrap()), expected);
    // Aliases compose with the same affine grammar.
    let sum = resolve("100 - text.animator('First').selector('Lead').start").unwrap();
    assert_eq!(sum.values, [88.0]);

    for (animator, selector, field, reason) in [
        ("Missing", "Lead", "start", "text animator is absent"),
        ("First", "Missing", "start", "Range Selector is absent"),
        ("Twin", "Any", "start", "ambiguous text animator name"),
        ("Pair", "Same", "start", "ambiguous Range Selector name"),
        ("Second", "Counted", "start", "unmaterialized Index field"),
        ("Second", "Shake", "start", "not a Range Selector"),
        ("Second", DEFAULT_NAME, "start", "Range Selector is absent"),
        ("First", "Lead", "advanced.amount", "field alias"),
        ("First", "Lead", "value", "field alias"),
        ("First", "Lead", "offset.value", "suffix"),
    ] {
        let error = alias(animator, selector, field).unwrap_err().to_string();
        assert!(
            error.contains(reason),
            "{animator}/{selector}/{field}: {error}"
        );
    }
}

#[test]
fn index_selector_aliases_read_active_index_values_without_percent_scaling() {
    let selector = range(
        "Lead",
        [
            leaf("ADBE Text Percent Start", stored(&[75.0], None)),
            leaf("ADBE Text Percent End", stored(&[90.0], None)),
            leaf("ADBE Text Percent Offset", stored(&[30.0], None)),
            leaf("ADBE Text Index Start", stored(&[1.25], None)),
            leaf("ADBE Text Index End", stored(&[3.5], None)),
            leaf(
                "ADBE Text Index Offset",
                keyed(&[(0, -0.5, 1, 0.0, 0.0), (800, 1.5, 1, 0.0, 0.0)]),
            ),
            group(
                "ADBE Text Range Advanced",
                None,
                leaf("ADBE Text Range Units", stored(&[2.0], None)),
            ),
        ]
        .concat(),
    );
    let text_group = text(animator("Rig", selector));
    let content = layer(Vec::new());
    let alias = |field: &str| {
        lower_in(
            &content,
            &text_group,
            &format!("text.animator('Rig').selector('Lead').{field}"),
            &[0.0],
        )
        .unwrap()
    };
    assert_eq!(alias("start").values, [1.25]);
    assert_eq!(alias("end").values, [3.5]);
    assert_eq!(keys(&alias("offset")), [(0.0, -0.5), (0.8, 1.5)]);
}

#[test]
fn index_selector_aliases_do_not_fall_back_to_dormant_percentage_fields() {
    let selector = range(
        "Lead",
        [
            leaf("ADBE Text Percent Start", stored(&[75.0], None)),
            leaf("ADBE Text Percent End", stored(&[90.0], None)),
            leaf("ADBE Text Percent Offset", stored(&[30.0], None)),
            group(
                "ADBE Text Range Advanced",
                None,
                leaf("ADBE Text Range Units", stored(&[2.0], None)),
            ),
        ]
        .concat(),
    );
    let text_group = text(animator("Rig", selector));
    for field in ["start", "end", "offset"] {
        let error = lower_in(
            &layer(Vec::new()),
            &text_group,
            &format!("text.animator('Rig').selector('Lead').{field}"),
            &[0.0],
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("unmaterialized Index field"), "{error}");
    }
}

#[test]
fn index_selector_aliases_resolve_mixed_unit_chains_and_active_field_cycles() {
    let index = range(
        "Index",
        [
            leaf("ADBE Text Index Start", stored(&[1.25], None)),
            group(
                "ADBE Text Range Advanced",
                None,
                leaf("ADBE Text Range Units", stored(&[2.0], None)),
            ),
        ]
        .concat(),
    );
    let percentage = range(
        "Percentage",
        leaf(
            "ADBE Text Percent Start",
            stored(
                &[75.0],
                Some("text.animator('Rig').selector('Index').start"),
            ),
        ),
    );
    let text_group = text(animator("Rig", [index, percentage].concat()));
    let content = layer(Vec::new());
    let resolved = lower_in(
        &content,
        &text_group,
        "text.animator('Rig').selector('Percentage').start",
        &[0.0],
    )
    .unwrap();
    // Every alias hop is in native units, regardless of the owner's units.
    assert_eq!(resolved.values, [1.25]);

    let cyclic = range(
        "Index",
        [
            leaf("ADBE Text Percent Start", stored(&[75.0], None)),
            leaf(
                "ADBE Text Index Start",
                stored(
                    &[1.25],
                    Some("text.animator('Rig').selector('Index').start"),
                ),
            ),
            group(
                "ADBE Text Range Advanced",
                None,
                leaf("ADBE Text Range Units", stored(&[2.0], None)),
            ),
        ]
        .concat(),
    );
    let text_group = text(animator("Rig", cyclic));
    let error = lower_in(
        &content,
        &text_group,
        "text.animator('Rig').selector('Index').start",
        &[0.0],
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("cyclic Range Selector alias"), "{error}");
}

#[test]
fn index_selector_aliases_reject_duplicate_active_fields() {
    let selector = range(
        "Lead",
        [
            leaf("ADBE Text Index Start", stored(&[1.25], None)),
            leaf("ADBE Text Index Start", stored(&[2.25], None)),
            group(
                "ADBE Text Range Advanced",
                None,
                leaf("ADBE Text Range Units", stored(&[2.0], None)),
            ),
        ]
        .concat(),
    );
    let text_group = text(animator("Rig", selector));
    let error = lower_in(
        &layer(Vec::new()),
        &text_group,
        "text.animator('Rig').selector('Lead').start",
        &[0.0],
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("duplicate Range Selector field"), "{error}");
}

#[test]
fn index_selector_aliases_keep_dynamic_and_unknown_unit_guards() {
    for units in [
        stored(&[3.0], None),
        stored(&[1.5], None),
        stored(&[2.0], Some("value")),
        keyed(&[(0, 2.0, 2, 0.0, 0.0), (800, 1.0, 2, 0.0, 0.0)]),
    ] {
        let selector = range(
            "Lead",
            [
                leaf("ADBE Text Index Start", stored(&[1.25], None)),
                group(
                    "ADBE Text Range Advanced",
                    None,
                    leaf("ADBE Text Range Units", units),
                ),
            ]
            .concat(),
        );
        let text_group = text(animator("Rig", selector));
        let error = lower_in(
            &layer(Vec::new()),
            &text_group,
            "text.animator('Rig').selector('Lead').start",
            &[0.0],
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("static Percentage or Index units"),
            "{error}"
        );
    }
}

#[test]
fn alias_cycles_and_unbounded_chains_are_rejected() {
    let field = |label: &str, next: &str| {
        range(
            label,
            leaf(
                "ADBE Text Percent Start",
                stored(
                    &[0.0],
                    Some(format!("text.animator(\"Rig\").selector(\"{next}\").start").as_str()),
                ),
            ),
        )
    };
    let cycle = text(animator("Rig", [field("A", "B"), field("B", "A")].concat()));
    let error = lower_in(
        &layer(standard_effects()),
        &cycle,
        "text.animator(\"Rig\").selector(\"A\").start",
        &[0.0],
    )
    .unwrap_err();
    assert!(error.to_string().contains("cyclic"), "{error}");

    let names: Vec<String> = (0..=MAX_ALIASES + 1)
        .map(|index| format!("S{index}"))
        .collect();
    let mut selectors: Vec<Chunk> = names
        .windows(2)
        .flat_map(|pair| field(&pair[0], &pair[1]))
        .collect();
    selectors.extend(range(
        names.last().unwrap(),
        leaf("ADBE Text Percent Start", stored(&[5.0], None)),
    ));
    let chain = text(animator("Rig", selectors));
    let error = lower_in(
        &layer(standard_effects()),
        &chain,
        "text.animator(\"Rig\").selector(\"S0\").start",
        &[0.0],
    )
    .unwrap_err();
    assert!(error.to_string().contains("bounded work"), "{error}");
}

#[test]
fn deeply_nested_linear_falls_back_without_exhausting_the_stack() {
    // Each level used to recurse through sum -> operand -> linear.
    let depth = 20_000;
    let deep = format!("{}0{}", "linear(".repeat(depth), ",0,1,0,1)".repeat(depth));
    let error = rejected(&deep);
    assert!(error.contains("bounded size"), "{error}");
}

#[test]
fn nested_linear_and_oversized_expressions_are_rejected() {
    let error = rejected("linear(linear(0, 0, 1, 0, 1), 0, 1, 0, 1)");
    assert!(error.contains("nested linear()"), "{error}");
    let error = rejected("linear(0, 0, linear(1, 0, 1, 0, 1), 0, 1)");
    assert!(error.contains("nested linear()"), "{error}");
    // A flat call is still admitted up to the size limit.
    let flat = "linear(effect(\"Value\")(1), 0, 100, 0, 1)";
    let padded = format!("{flat}{}", " ".repeat(MAX_EXPRESSION_BYTES - flat.len()));
    assert_eq!(padded.len(), MAX_EXPRESSION_BYTES);
    assert!((constant(&padded) - 0.42).abs() < 1e-12);
    let error = rejected(&format!("{padded} "));
    assert!(error.contains("bounded size"), "{error}");
}

#[test]
fn bindings_beyond_the_budget_are_rejected() {
    let chain = |count: usize| {
        let mut text = String::from("var b0 = effect(\"Value\")(1);");
        for index in 1..count {
            text += &format!(" var b{index} = b{} + 1;", index - 1);
        }
        text + &format!(" b{}", count - 1)
    };
    let admitted = constant(&chain(MAX_BINDINGS));
    assert_eq!(admitted, 42.0 + (MAX_BINDINGS - 1) as f64);
    let error = rejected(&chain(MAX_BINDINGS + 1));
    assert!(error.contains("bounded binding count"), "{error}");
}

#[test]
fn bounds_apply_to_aliased_expressions() {
    let target = |expression: &str| {
        text(animator(
            "Rig",
            range(
                "Lead",
                leaf("ADBE Text Percent Start", stored(&[0.0], Some(expression))),
            ),
        ))
    };
    let alias = "text.animator(\"Rig\").selector(\"Lead\").start";
    let content = layer(standard_effects());
    let oversized = format!("0{}", " ".repeat(MAX_EXPRESSION_BYTES));
    for (expression, reason) in [
        (
            "linear(linear(0, 0, 1, 0, 1), 0, 1, 0, 1)",
            "nested linear()",
        ),
        (oversized.as_str(), "bounded size"),
    ] {
        let error = lower_in(&content, &target(expression), alias, &[0.0])
            .unwrap_err()
            .to_string();
        assert!(error.contains(reason), "{error}");
    }
}
