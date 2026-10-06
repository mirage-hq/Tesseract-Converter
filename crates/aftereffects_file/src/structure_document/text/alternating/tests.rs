//! Synthetic decoded sources. They pin the admission rules and bounds; they
//! are not Adobe evidence.

use crate::properties::NumericKeyframe;

use super::super::{PercentHolds, cos};
use super::*;

const NATIVE: &str =
    "if(textIndex%2 == 0){\r\n\tselectorValue;\r\n}else{\r\n\t-selectorValue;\r\n}";

#[test]
fn only_the_complete_alternating_sign_statement_is_admitted() {
    for text in [
        NATIVE,
        "if ( textIndex % 2 == 0 ) { selectorValue } else { - selectorValue } ;",
        "if(textIndex%2==0){selectorValue;}else{-selectorValue;}",
    ] {
        assert!(alternating_sign_expression(text), "{text:?}");
    }
    for text in [
        "",
        "if(textIndex%2 == 1){selectorValue;}else{-selectorValue;}",
        "if(textIndex%2 == 0){-selectorValue;}else{selectorValue;}",
        "if(textIndex%2 === 0){selectorValue;}else{-selectorValue;}",
        "if(textIndex%20 == 0){selectorValue;}else{-selectorValue;}",
        "if(textIndex%2 == 0.5){selectorValue;}else{-selectorValue;}",
        "if(textIndexes%2 == 0){selectorValue;}else{-selectorValue;}",
        "if(textTotal%2 == 0){selectorValue;}else{-selectorValue;}",
        "if(textIndex%2 == 0){selectorValue*2;}else{-selectorValue;}",
        "if(textIndex%2 == 0){selectorValue;;}else{-selectorValue;}",
        "if(textIndex%2 == 0){selectorValue;}else{--selectorValue;}",
        "if(textIndex%2 == 0){selectorValue;}",
        "if(textIndex%2 == 0){selectorValue;}else{-selectorValue;}\r\n0;",
        "// odd\r\nif(textIndex%2 == 0){selectorValue;}else{-selectorValue;}",
        "textIndex%2 == 0 ? selectorValue : -selectorValue",
    ] {
        assert!(!alternating_sign_expression(text), "{text:?}");
    }
}

/// One document showing `text`, with paragraph and character style runs of
/// the given UTF-16 lengths.
fn document(text: &str, paragraph_runs: &[usize], character_runs: &[usize]) -> Value {
    let hex: String = text
        .encode_utf16()
        .map(|unit| format!("{unit:04X}"))
        .collect();
    let runs = |lengths: &[usize]| {
        lengths
            .iter()
            .map(|length| format!("<< /1 {length} >>"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let cos = format!(
        "<< /0 << /0 <FEFF{hex}> /5 << /0 [ {} ] >> /6 << /0 [ {} ] >> >> >>",
        runs(paragraph_runs),
        runs(character_runs)
    );
    cos::parse(cos.as_bytes()).unwrap()
}

/// `text` in one paragraph and one character style run.
fn line(text: &str) -> Value {
    let units = text.encode_utf16().count();
    document(text, &[units], &[units])
}

fn source(document: Value, animators: Vec<AnimatorSource>) -> SourceText {
    SourceText {
        document_is_static: true,
        fonts: Vec::new(),
        documents: vec![document],
        document_starts: Vec::new(),
        percent: None,
        frame: None,
        animators,
        more_options: Vec::new(),
        path_options: Vec::new(),
        warnings: Vec::new(),
    }
}

/// Two linear keys of `values`.
fn keyed(values: &[f64]) -> NumericProperty {
    let key = |time_secs| NumericKeyframe {
        time_secs,
        values: values.to_vec(),
        in_interpolation: 1,
        out_interpolation: 1,
        in_speed: vec![0.0; values.len()],
        in_influence: vec![0.0; values.len()],
        out_speed: vec![0.0; values.len()],
        out_influence: vec![0.0; values.len()],
        spatial_in: Vec::new(),
        spatial_out: Vec::new(),
    };
    NumericProperty {
        animated: true,
        keyframes: vec![key(0.5), key(2.0)],
        ..native("", values).1.unwrap()
    }
}

fn animator(properties: NumericProperties, selectors: Vec<SelectorSource>) -> AnimatorSource {
    AnimatorSource {
        name: "Animator 1".into(),
        properties,
        selectors,
        evaluated: Vec::new(),
    }
}

fn range(properties: NumericProperties) -> SelectorSource {
    SelectorSource::Range { properties }
}

fn alternate() -> SelectorSource {
    SelectorSource::AlternatingSign {
        properties: Vec::new(),
    }
}

fn position() -> NumericProperties {
    vec![native(POSITION, &[0.0, 100.0])]
}

/// Position (0, 100) on a keyed Ramp Up range, then the Expression Selector.
fn admitted() -> AnimatorSource {
    animator(
        position(),
        vec![
            range(vec![
                native("ADBE Text Range Shape", &[2.0]),
                ("ADBE Text Percent Offset".into(), Ok(keyed(&[-100.0]))),
            ]),
            alternate(),
        ],
    )
}

fn with_range(extra: NumericProperties) -> AnimatorSource {
    let mut animator = admitted();
    let SelectorSource::Range { properties } = &mut animator.selectors[0] else {
        unreachable!()
    };
    properties.extend(extra);
    animator
}

fn rejection(source: &SourceText) -> String {
    candidate(source).err().expect("the expansion is rejected")
}

#[test]
fn text_must_be_one_static_printable_ascii_line() {
    for (text, gates) in [
        ("C\r", 1),
        ("Create\r", 3),
        ("Editable Selector\r", 9),
        ("No return", 5),
    ] {
        let expanded = candidate(&source(line(text), vec![admitted()]))
            .unwrap()
            .expect("admitted");
        let [base, correction] = expanded.source.animators.as_slice() else {
            panic!("{text:?}: one base and one correction animator");
        };
        assert_eq!(base.selectors.len(), 1, "{text:?}");
        assert_eq!(correction.selectors.len(), gates + 1, "{text:?}");
    }

    const TEXT: &str = "the text is not one line of 1 to 256 printable ASCII characters";
    let too_long = format!("{}\r", "a".repeat(257));
    for text in [
        "\r",
        too_long.as_str(),
        "Cre\rate\r",
        "Cre\tate\r",
        "Cre\u{3}ate\r",
        "Créate\r",
    ] {
        assert_eq!(
            rejection(&source(line(text), vec![admitted()])),
            TEXT,
            "{text:?}"
        );
    }
    for document in [
        document("Create\r", &[7], &[3, 4]),
        document("Create\r", &[3, 4], &[7]),
        document("Create\r", &[6], &[6]),
        document("Create\r", &[], &[7]),
    ] {
        assert_eq!(
            rejection(&source(document, vec![admitted()])),
            "the text has more than one paragraph or character style run"
        );
    }

    const KEYED: &str = "Source Text is keyed or expression-driven";
    let base = || source(line("Create\r"), vec![admitted()]);
    let mut animated = base();
    animated.document_is_static = false;
    let mut timed = base();
    timed.document_starts = vec![0.0];
    let mut expression = base();
    expression.percent = Some(PercentHolds {
        starts: Vec::new(),
        texts: vec!["5%".into()],
    });
    let mut documents = base();
    documents.documents.push(line("Create\r"));
    for rejected in [animated, timed, expression, documents] {
        assert_eq!(rejection(&rejected), KEYED);
    }
}

#[test]
fn animator_must_be_one_static_position_on_one_ordinary_range() {
    // Explicit native defaults, as Adobe's presets store them, are admitted.
    let explicit = with_range(vec![
        native(MODE, &[1.0]),
        native("ADBE Text Range Type2", &[1.0]),
        native("ADBE Text Randomize Order", &[0.0]),
        native("ADBE Text Selector Max Amount", &[50.0]),
    ]);
    assert!(
        candidate(&source(line("Create\r"), vec![explicit]))
            .unwrap()
            .is_some()
    );

    let ordinary = || vec![range(Vec::new()), alternate()];
    let mut scripted = native(POSITION, &[0.0, 100.0]);
    scripted.1.as_mut().unwrap().expression_enabled = true;
    let mut unlowered = native("ADBE Text Percent Start", &[0.0]);
    unlowered.1.as_mut().unwrap().expression_enabled = true;
    let cases = [
        (
            "does not have one Range Selector directly followed by the Expression Selector",
            vec![
                animator(position(), vec![alternate(), range(Vec::new())]),
                animator(
                    position(),
                    vec![range(Vec::new()), range(Vec::new()), alternate()],
                ),
                animator(
                    position(),
                    vec![range(Vec::new()), alternate(), range(Vec::new())],
                ),
                animator(position(), vec![alternate()]),
                animator(
                    position(),
                    vec![
                        SelectorSource::Wiggly {
                            properties: Vec::new(),
                        },
                        alternate(),
                    ],
                ),
            ],
        ),
        (
            "does not animate only a static, finite 2D Position",
            vec![
                animator(Vec::new(), ordinary()),
                animator(
                    [position(), vec![native("ADBE Text Opacity", &[0.0])]].concat(),
                    ordinary(),
                ),
                animator(vec![native(POSITION, &[0.0, 100.0, 5.0])], ordinary()),
                animator(vec![native(POSITION, &[1e308, 0.0])], ordinary()),
                animator(
                    vec![(POSITION.into(), Ok(keyed(&[0.0, 100.0])))],
                    ordinary(),
                ),
                animator(vec![scripted], ordinary()),
                animator(
                    vec![native("ADBE Text Anchor Point 3D", &[0.0, 100.0])],
                    ordinary(),
                ),
            ],
        ),
        (
            "Range Selector is not a nonrandom Characters Add selection",
            vec![
                with_range(vec![native(MODE, &[2.0])]),
                with_range(vec![native(MODE, &[3.0])]),
                with_range(vec![(MODE.into(), Ok(keyed(&[1.0])))]),
                with_range(vec![native("ADBE Text Range Type2", &[2.0])]),
                with_range(vec![native("ADBE Text Randomize Order", &[1.0])]),
            ],
        ),
        (
            "Range Selector Amount is not static from 0 to 100%",
            vec![
                with_range(vec![native("ADBE Text Selector Max Amount", &[120.0])]),
                with_range(vec![native("ADBE Text Selector Max Amount", &[-10.0])]),
                with_range(vec![(
                    "ADBE Text Selector Max Amount".into(),
                    Ok(keyed(&[50.0])),
                )]),
            ],
        ),
        (
            "Range Selector has a malformed field or an unlowered expression",
            vec![
                with_range(vec![(
                    "ADBE Text Percent End".into(),
                    Err(PropertyError::Layout("malformed")),
                )]),
                with_range(vec![unlowered]),
            ],
        ),
    ];
    for (reason, animators) in cases {
        for animator in animators {
            assert_eq!(
                rejection(&source(line("Create\r"), vec![animator])),
                format!("Text Animator 1 {reason}")
            );
        }
    }
}

#[test]
fn expansion_keeps_other_animators_in_place() {
    let plain = |name: &str| AnimatorSource {
        name: name.into(),
        properties: vec![native("ADBE Text Opacity", &[0.0])],
        selectors: vec![range(Vec::new())],
        evaluated: Vec::new(),
    };
    let animators = vec![
        plain("Animator 1"),
        AnimatorSource {
            name: "Animator 2".into(),
            ..admitted()
        },
        plain("Animator 3"),
    ];
    let expanded = candidate(&source(line("Create\r"), animators))
        .unwrap()
        .unwrap();
    let names: Vec<_> = expanded
        .source
        .animators
        .iter()
        .map(|animator| animator.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Animator 1",
            "Animator 2",
            "Animator 2 alternating position",
            "Animator 3"
        ]
    );
    assert!(
        candidate(&source(line("Create\r"), vec![plain("Animator 1")]))
            .unwrap()
            .is_none(),
        "no Expression Selector, no attempt"
    );
}

#[test]
fn expanded_selectors_are_bounded_per_text_before_allocation() {
    let text = |count: usize| line(&format!("{}\r", "a ".repeat(count / 2)));
    // At 256 characters one animator takes its Range Selector, 128 gates and the copy.
    let longest = candidate(&source(text(256), vec![admitted()]))
        .unwrap()
        .unwrap();
    let selectors: usize = longest
        .source
        .animators
        .iter()
        .map(|animator| animator.selectors.len())
        .sum();
    assert_eq!(selectors, MAX_SELECTORS);
    // Two animators share the bound: 2 x (32 + 2) fit, 2 x (64 + 2) do not.
    let second = || AnimatorSource {
        name: "Animator 2".into(),
        ..admitted()
    };
    assert!(
        candidate(&source(text(64), vec![admitted(), second()]))
            .unwrap()
            .is_some()
    );
    assert_eq!(
        rejection(&source(text(128), vec![admitted(), second()])),
        "its 132 expanded selectors exceed the bound of 130"
    );
}
