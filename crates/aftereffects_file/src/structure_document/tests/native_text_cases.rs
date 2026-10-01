// Independent AE-authored text fixtures. Historical authoring-script and receipt
// identities are pinned in tests/fixtures/aep_authoring_provenance.json; the unsafe,
// machine-specific scripts are not shipped as runnable helpers. Importer output is
// not used as the expected oracle.
use super::*;

struct SourcePin {
    bytes: &'static [u8],
    path: &'static str,
    len: usize,
    sha256: &'static str,
}

const ANIMATOR_SOURCE: SourcePin = SourcePin {
    bytes: include_bytes!("../../../tests/fixtures/text/import_text_animator_channels.aep"),
    path: "text/import_text_animator_channels.aep",
    len: 2_780_109,
    sha256: "56a8b0b4665989d833734a87ad344deedb5d83e7f2b24c9c7e718fd4c69ea289",
};
const SELECTOR_SOURCE: SourcePin = SourcePin {
    bytes: include_bytes!("../../../tests/fixtures/text/import_text_selector_controls.aep"),
    path: "text/import_text_selector_controls.aep",
    len: 2_791_959,
    sha256: "7aedddf83a6a2f7afa849d3206d48dab35b6c43b5b66067bc268e1d39821f9b6",
};
const ADDITIONAL_SOURCE: SourcePin = SourcePin {
    bytes: include_bytes!("../../../tests/fixtures/text/import_text_additional_controls.aep"),
    path: "text/import_text_additional_controls.aep",
    len: 704_985,
    sha256: "65a5929c222b2d44727adc40f18438adaa5e339a52568dab308f4e58c06b44e1",
};
const ALL_CAPS_DOCUMENT_SOURCE: SourcePin = SourcePin {
    bytes: include_bytes!(
        "../../../tests/fixtures/pr4442_native/sources/text_document_allcaps.aep"
    ),
    path: "pr4442_native/sources/text_document_allcaps.aep",
    len: 92_777,
    sha256: "431090eb1dc597836645ee3a066d055c61ba09153b3cec3f99e82a5e7c22a33c",
};

#[derive(Clone, Copy)]
enum ExpectedValue {
    Scalar(f64),
    Vector2([f64; 2]),
    Color([f64; 4]),
}

impl ExpectedValue {
    fn json(self) -> Value {
        match self {
            Self::Scalar(value) => serde_json::json!(value),
            Self::Vector2(value) => serde_json::json!(value),
            Self::Color(value) => serde_json::json!(value),
        }
    }
}

struct ImportedTextCase {
    text: Value,
    editable: Value,
    diagnostics: Vec<String>,
}

fn pinned_project(pin: &SourcePin) -> StructuralProject {
    assert_eq!(pin.bytes.len(), pin.len, "{} byte count", pin.path);
    assert_eq!(
        format!("{:x}", Sha256::digest(pin.bytes)),
        pin.sha256,
        "{} SHA-256",
        pin.path
    );
    read_project(pin.bytes).unwrap_or_else(|error| panic!("{}: {error}", pin.path))
}

fn import_text_case(
    project: &StructuralProject,
    pin: &SourcePin,
    composition_id: u32,
    composition_name: &str,
) -> ImportedTextCase {
    let source = composition(project, composition_id);
    assert_eq!(
        source.frame_rate, 24.0,
        "{} comp {composition_id}",
        pin.path
    );
    assert_eq!(
        source.layers.len(),
        2,
        "{} comp {composition_id} must retain authored text and background layers",
        pin.path
    );

    let converted = to_structural_fx_document(project, Some(composition_id))
        .unwrap_or_else(|error| panic!("{} comp {composition_id}: {error}", pin.path));
    assert_imported_canvas_matches_source(
        source,
        &converted,
        &format!("{} comp {composition_id}", pin.path),
    );
    assert_eq!(root(&converted).name, composition_name);

    let mut text_layers = Vec::new();
    collect_text_layers(root(&converted), &mut text_layers);
    assert_eq!(
        text_layers.len(),
        1,
        "{} comp {composition_id} must import exactly one editable Text layer, not flatten it",
        pin.path
    );
    let text = serde_json::to_value(text_layers[0]).unwrap();
    let editable = serde_json::from_slice(&converted.document.to_json_vec().unwrap()).unwrap();
    let diagnostics = converted
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.clone())
        .collect();
    ImportedTextCase {
        text,
        editable,
        diagnostics,
    }
}

fn collect_text_layers<'a>(group: &'a GroupLayer, text_layers: &mut Vec<&'a fx_schema::TextLayer>) {
    for layer in &group.layers {
        match layer.data() {
            FxLayer::Group(child) => collect_text_layers(child, text_layers),
            FxLayer::Text(text) => text_layers.push(text),
            _ => {}
        }
    }
}

fn assert_animator_source_text(text: &Value) {
    let source = &text["sourceText"];
    assert_eq!(source["text"], "Editable motion\nSecond line");
    assert_eq!(source["fontFamily"], "ArialMT");
    assert_eq!(source["fontStyle"], "");
    assert_eq!(source["fontSize"], 72.0);
    assert_eq!(source["fillColor"], serde_json::json!([0.9, 0.7, 0.2, 1.0]));
    assert_eq!(
        source["strokeColor"],
        serde_json::json!([0.2, 0.6, 1.0, 1.0])
    );
    assert_eq!(source["strokeWidth"], 2.0);
    assert_eq!(text["animators"].as_array().unwrap().len(), 1);
}

fn assert_additional_source_text(text: &Value) {
    let source = &text["sourceText"];
    assert_eq!(source["text"], "Mixed Case Text\nSecond Line");
    assert_eq!(source["fontFamily"], "ArialMT");
    assert_eq!(source["fontSize"], 72.0);
}

fn animation_entry<'a>(editable: &'a Value, item_id: &Value, property: &str) -> &'a Value {
    let matching = editable["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| {
            entry["target"]["kind"] == "fxItemProperty"
                && entry["target"]["itemId"] == *item_id
                && entry["target"]["propertyName"] == property
        })
        .collect::<Vec<_>>();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one editable track for item {item_id} property {property}"
    );
    matching[0]
}

fn assert_two_keys(entry: &Value, first: ExpectedValue, second: ExpectedValue) {
    let keys = entry["animator"]["keyframes"].as_array().unwrap();
    assert_eq!(keys.len(), 2);
    // Native color keys retain f32 channel precision; keep exact comparisons
    // against those stored values rather than idealized f64 decimal literals.
    let stored_value = |value: ExpectedValue| match value {
        ExpectedValue::Color(channels) => {
            serde_json::json!(channels.map(|channel| f64::from(channel as f32)))
        }
        other => other.json(),
    };
    assert_eq!(keys[0]["layerTime"], 500);
    assert_eq!(keys[0]["value"]["value"], stored_value(first));
    assert_eq!(keys[1]["layerTime"], 2000);
    assert_eq!(keys[1]["value"]["value"], stored_value(second));
}

struct AnimatorCase {
    name: &'static str,
    static_id: u32,
    keyed_id: u32,
    property: &'static str,
    initial: ExpectedValue,
    authored: ExpectedValue,
}

const ANIMATOR_CASES: &[AnimatorCase] = &[
    AnimatorCase {
        name: "ANCHOR",
        static_id: 1,
        keyed_id: 17,
        property: "anchorPoint",
        initial: ExpectedValue::Vector2([0.0, 0.0]),
        authored: ExpectedValue::Vector2([25.0, 15.0]),
    },
    AnimatorCase {
        name: "POSITION",
        static_id: 32,
        keyed_id: 47,
        property: "position",
        initial: ExpectedValue::Vector2([0.0, 0.0]),
        authored: ExpectedValue::Vector2([0.0, -60.0]),
    },
    AnimatorCase {
        name: "SCALE",
        static_id: 62,
        keyed_id: 77,
        property: "scale",
        initial: ExpectedValue::Vector2([100.0, 100.0]),
        authored: ExpectedValue::Vector2([135.0, 70.0]),
    },
    AnimatorCase {
        name: "ROTATION",
        static_id: 92,
        keyed_id: 107,
        property: "rotation",
        initial: ExpectedValue::Scalar(0.0),
        authored: ExpectedValue::Scalar(25.0),
    },
    AnimatorCase {
        name: "SKEW",
        static_id: 122,
        keyed_id: 137,
        property: "skew",
        initial: ExpectedValue::Scalar(0.0),
        authored: ExpectedValue::Scalar(20.0),
    },
    AnimatorCase {
        name: "SKEW_AXIS",
        static_id: 152,
        keyed_id: 167,
        property: "skewAxis",
        initial: ExpectedValue::Scalar(0.0),
        authored: ExpectedValue::Scalar(45.0),
    },
    AnimatorCase {
        name: "TRACKING",
        static_id: 182,
        keyed_id: 197,
        property: "tracking",
        initial: ExpectedValue::Scalar(0.0),
        authored: ExpectedValue::Scalar(30.0),
    },
    AnimatorCase {
        name: "STROKE_WIDTH",
        static_id: 212,
        keyed_id: 227,
        property: "strokeWidth",
        initial: ExpectedValue::Scalar(0.0),
        authored: ExpectedValue::Scalar(8.0),
    },
    AnimatorCase {
        name: "BLUR",
        static_id: 242,
        keyed_id: 257,
        property: "blur",
        initial: ExpectedValue::Vector2([0.0, 0.0]),
        authored: ExpectedValue::Vector2([8.0, 12.0]),
    },
    AnimatorCase {
        name: "OPACITY",
        static_id: 272,
        keyed_id: 287,
        property: "opacity",
        initial: ExpectedValue::Scalar(100.0),
        authored: ExpectedValue::Scalar(30.0),
    },
    AnimatorCase {
        name: "FILL_COLOR",
        static_id: 302,
        keyed_id: 317,
        property: "fillColor",
        initial: ExpectedValue::Color([0.9, 0.7, 0.2, 1.0]),
        authored: ExpectedValue::Color([0.2, 0.9, 0.4, 1.0]),
    },
    AnimatorCase {
        name: "STROKE_COLOR",
        static_id: 332,
        keyed_id: 347,
        property: "strokeColor",
        initial: ExpectedValue::Color([0.2, 0.6, 1.0, 1.0]),
        authored: ExpectedValue::Color([1.0, 0.2, 0.5, 1.0]),
    },
    AnimatorCase {
        name: "LINE_SPACING",
        static_id: 362,
        keyed_id: 377,
        property: "lineSpacing",
        initial: ExpectedValue::Scalar(0.0),
        authored: ExpectedValue::Scalar(30.0),
    },
    AnimatorCase {
        name: "LINE_ANCHOR",
        static_id: 392,
        keyed_id: 407,
        property: "lineAnchor",
        initial: ExpectedValue::Scalar(0.0),
        authored: ExpectedValue::Scalar(75.0),
    },
    AnimatorCase {
        name: "CHAR_OFFSET",
        static_id: 422,
        keyed_id: 437,
        property: "characterOffset",
        initial: ExpectedValue::Scalar(0.0),
        authored: ExpectedValue::Scalar(3.0),
    },
    AnimatorCase {
        name: "CHAR_REPLACE",
        static_id: 452,
        keyed_id: 467,
        property: "characterValue",
        initial: ExpectedValue::Scalar(65.0),
        authored: ExpectedValue::Scalar(90.0),
    },
];

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_text_animator_static_controls_import_editable_values() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for case in ANIMATOR_CASES {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/text/import_text_animator_channels.aep",
            case.static_id,
            || {
                let project = pinned_project(&ANIMATOR_SOURCE);
                let imported = import_text_case(
                    &project,
                    &ANIMATOR_SOURCE,
                    case.static_id,
                    &format!("ANIMATOR_{}_STATIC", case.name),
                );
                assert_animator_source_text(&imported.text);
                assert_eq!(
                    imported.text["animators"][0]["selectors"]
                        .as_array()
                        .unwrap()
                        .len(),
                    1
                );
                if matches!(case.name, "ANCHOR" | "ROTATION") {
                    super::adobe_feature_additions::assert_text_animator_owner(
                        &imported.text,
                        &imported.editable,
                        case.property,
                        false,
                    );
                }
                assert_eq!(
                    imported.text["animators"][0][case.property],
                    case.authored.json(),
                    "ANIMATOR_{}_STATIC",
                    case.name
                );
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_text_animator_keyed_controls_import_editable_tracks() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for case in ANIMATOR_CASES {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/text/import_text_animator_channels.aep",
            case.keyed_id,
            || {
                let project = pinned_project(&ANIMATOR_SOURCE);
                let imported = import_text_case(
                    &project,
                    &ANIMATOR_SOURCE,
                    case.keyed_id,
                    &format!("ANIMATOR_{}_KEYED", case.name),
                );
                assert_animator_source_text(&imported.text);
                assert_eq!(
                    imported.text["animators"][0]["selectors"]
                        .as_array()
                        .unwrap()
                        .len(),
                    1
                );
                if matches!(case.name, "ANCHOR" | "ROTATION") {
                    super::adobe_feature_additions::assert_text_animator_owner(
                        &imported.text,
                        &imported.editable,
                        case.property,
                        true,
                    );
                }
                let animator_id = &imported.text["animators"][0]["id"];
                let entry = animation_entry(&imported.editable, animator_id, case.property);
                assert_two_keys(entry, case.initial, case.authored);
            },
        );
    }
    cases.finish();
}

struct RangeCase {
    id: u32,
    name: &'static str,
    property: &'static str,
    expected: ExpectedValue,
    expected_text: Option<&'static str>,
}

const RANGE_CASES: &[RangeCase] = &[
    RangeCase {
        id: 1,
        name: "PERCENT_START",
        property: "start",
        expected: ExpectedValue::Scalar(0.25),
        expected_text: None,
    },
    RangeCase {
        id: 17,
        name: "PERCENT_END",
        property: "end",
        expected: ExpectedValue::Scalar(0.6),
        expected_text: None,
    },
    RangeCase {
        id: 32,
        name: "PERCENT_OFFSET",
        property: "offset",
        expected: ExpectedValue::Scalar(0.3),
        expected_text: None,
    },
    RangeCase {
        id: 47,
        name: "INDEX_START",
        property: "start",
        expected: ExpectedValue::Scalar(3.0),
        expected_text: Some("index"),
    },
    RangeCase {
        id: 62,
        name: "INDEX_END",
        property: "end",
        expected: ExpectedValue::Scalar(8.0),
        expected_text: Some("index"),
    },
    RangeCase {
        id: 77,
        name: "INDEX_OFFSET",
        property: "offset",
        expected: ExpectedValue::Scalar(2.0),
        expected_text: Some("index"),
    },
    RangeCase {
        id: 92,
        name: "AMOUNT",
        property: "amount",
        expected: ExpectedValue::Scalar(0.45),
        expected_text: None,
    },
    RangeCase {
        id: 107,
        name: "EASE_HIGH",
        property: "easeHigh",
        expected: ExpectedValue::Scalar(0.6),
        expected_text: None,
    },
    RangeCase {
        id: 122,
        name: "EASE_LOW",
        property: "easeLow",
        expected: ExpectedValue::Scalar(0.6),
        expected_text: None,
    },
    RangeCase {
        id: 137,
        name: "RANDOM_ORDER",
        property: "randomizeOrder",
        expected: ExpectedValue::Scalar(1.0),
        expected_text: Some("bool"),
    },
    RangeCase {
        id: 152,
        name: "RANDOM_SEED",
        property: "randomSeed",
        expected: ExpectedValue::Scalar(23.0),
        expected_text: None,
    },
    RangeCase {
        id: 167,
        name: "SHAPE_1",
        property: "shape",
        expected: ExpectedValue::Scalar(0.0),
        expected_text: Some("square"),
    },
    RangeCase {
        id: 182,
        name: "SHAPE_2",
        property: "shape",
        expected: ExpectedValue::Scalar(0.0),
        expected_text: Some("rampUp"),
    },
    RangeCase {
        id: 197,
        name: "SHAPE_3",
        property: "shape",
        expected: ExpectedValue::Scalar(0.0),
        expected_text: Some("rampDown"),
    },
    RangeCase {
        id: 212,
        name: "SHAPE_4",
        property: "shape",
        expected: ExpectedValue::Scalar(0.0),
        expected_text: Some("triangle"),
    },
    RangeCase {
        id: 227,
        name: "SHAPE_5",
        property: "shape",
        expected: ExpectedValue::Scalar(0.0),
        expected_text: Some("round"),
    },
    RangeCase {
        id: 242,
        name: "SHAPE_6",
        property: "shape",
        expected: ExpectedValue::Scalar(0.0),
        expected_text: Some("smooth"),
    },
    RangeCase {
        id: 257,
        name: "MODE_1",
        property: "mode",
        expected: ExpectedValue::Scalar(0.0),
        expected_text: Some("add"),
    },
    RangeCase {
        id: 272,
        name: "MODE_2",
        property: "mode",
        expected: ExpectedValue::Scalar(0.0),
        expected_text: Some("subtract"),
    },
    RangeCase {
        id: 287,
        name: "MODE_3",
        property: "mode",
        expected: ExpectedValue::Scalar(0.0),
        expected_text: Some("intersect"),
    },
    RangeCase {
        id: 302,
        name: "MODE_4",
        property: "mode",
        expected: ExpectedValue::Scalar(0.0),
        expected_text: Some("min"),
    },
    RangeCase {
        id: 317,
        name: "MODE_5",
        property: "mode",
        expected: ExpectedValue::Scalar(0.0),
        expected_text: Some("max"),
    },
    RangeCase {
        id: 332,
        name: "MODE_6",
        property: "mode",
        expected: ExpectedValue::Scalar(0.0),
        expected_text: Some("difference"),
    },
    RangeCase {
        id: 347,
        name: "BASED_ON_1",
        property: "basedOn",
        expected: ExpectedValue::Scalar(0.0),
        expected_text: Some("characters"),
    },
    RangeCase {
        id: 362,
        name: "BASED_ON_2",
        property: "basedOn",
        expected: ExpectedValue::Scalar(0.0),
        expected_text: Some("charactersExcludingSpaces"),
    },
    RangeCase {
        id: 377,
        name: "BASED_ON_3",
        property: "basedOn",
        expected: ExpectedValue::Scalar(0.0),
        expected_text: Some("words"),
    },
    RangeCase {
        id: 392,
        name: "BASED_ON_4",
        property: "basedOn",
        expected: ExpectedValue::Scalar(0.0),
        expected_text: Some("lines"),
    },
];

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_text_range_selector_controls_import_authored_values() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for case in RANGE_CASES {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/text/import_text_selector_controls.aep",
            case.id,
            || {
                let project = pinned_project(&SELECTOR_SOURCE);
                let imported = import_text_case(
                    &project,
                    &SELECTOR_SOURCE,
                    case.id,
                    &format!("RANGE_{}", case.name),
                );
                assert_animator_source_text(&imported.text);
                let animator = &imported.text["animators"][0];
                assert_eq!(animator["position"], serde_json::json!([0.0, -70.0]));
                let selector = &animator["selectors"][0];
                match case.expected_text {
                    Some("bool") => assert_eq!(selector[case.property], true),
                    Some(expected) => assert_eq!(selector[case.property], expected),
                    None => assert_eq!(selector[case.property], case.expected.json()),
                }
                if case.name.starts_with("INDEX_") {
                    assert_eq!(selector["units"], "index");
                } else {
                    assert_eq!(selector["units"], "percentage");
                }
            },
        );
    }
    cases.finish();
}

struct WigglyCase {
    id: u32,
    name: &'static str,
    property: &'static str,
    expected: ValueKind,
    diagnostic: Option<&'static str>,
}

#[derive(Clone, Copy)]
enum ValueKind {
    Number(f64),
    Text(&'static str),
}

const WIGGLY_CASES: &[WigglyCase] = &[
    WigglyCase {
        id: 407,
        name: "SPEED",
        property: "speed",
        expected: ValueKind::Number(4.0),
        diagnostic: None,
    },
    WigglyCase {
        id: 422,
        name: "MAX_AMOUNT",
        property: "amount",
        expected: ValueKind::Number(60.0),
        diagnostic: Some("asymmetric min/max [-100, 60]"),
    },
    WigglyCase {
        id: 437,
        name: "MIN_AMOUNT",
        property: "amount",
        expected: ValueKind::Number(100.0),
        diagnostic: Some("asymmetric min/max [-40, 100]"),
    },
    WigglyCase {
        id: 452,
        name: "SEED",
        property: "seed",
        expected: ValueKind::Number(23.0),
        diagnostic: None,
    },
    WigglyCase {
        id: 467,
        name: "MODE",
        property: "mode",
        expected: ValueKind::Text("subtract"),
        diagnostic: None,
    },
];

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_text_wiggly_selector_controls_import_or_diagnose_authored_values() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for case in WIGGLY_CASES {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/text/import_text_selector_controls.aep",
            case.id,
            || {
                let project = pinned_project(&SELECTOR_SOURCE);
                let imported = import_text_case(
                    &project,
                    &SELECTOR_SOURCE,
                    case.id,
                    &format!("WIGGLY_{}", case.name),
                );
                assert_animator_source_text(&imported.text);
                let animator = &imported.text["animators"][0];
                assert!(animator["selectors"].as_array().unwrap().is_empty());
                let selector = &animator["wigglySelectors"][0];
                match case.expected {
                    ValueKind::Number(expected) => assert_eq!(selector[case.property], expected),
                    ValueKind::Text(expected) => assert_eq!(selector[case.property], expected),
                }
                if let Some(expected) = case.diagnostic {
                    assert!(
                        imported
                            .diagnostics
                            .iter()
                            .any(|message| message.contains(expected)),
                        "WIGGLY_{} must diagnose the authored lossy min/max mapping: {:?}",
                        case.name,
                        imported.diagnostics
                    );
                }
            },
        );
    }
    cases.finish();
}

/// Both independently AE-authored All Caps sources store caps code 2 in field
/// 12 of the character style; the AUTO_LEADING composition of the same source
/// stores 0 for the same mixed-case string. Import sets `allCaps` and keeps the
/// stored string instead of rewriting it in uppercase.
#[test]
fn native_all_caps_source_text_imports_all_caps_and_keeps_its_string() {
    let no_caps_diagnostic = |imported: &ImportedTextCase| {
        assert!(
            imported
                .diagnostics
                .iter()
                .all(|message| !message.contains("field 12")),
            "{:?}",
            imported.diagnostics
        );
    };
    let project = pinned_project(&ADDITIONAL_SOURCE);
    for (composition_id, name, all_caps) in [(1, "ALL_CAPS", true), (77, "AUTO_LEADING", false)] {
        let imported = import_text_case(&project, &ADDITIONAL_SOURCE, composition_id, name);
        assert_additional_source_text(&imported.text);
        assert_eq!(imported.text["sourceText"]["allCaps"], all_caps, "{name}");
        no_caps_diagnostic(&imported);
    }

    let project = pinned_project(&ALL_CAPS_DOCUMENT_SOURCE);
    let imported = import_text_case(
        &project,
        &ALL_CAPS_DOCUMENT_SOURCE,
        1,
        "PR4442_TEXT_DOCUMENT_ALLCAPS",
    );
    assert_eq!(imported.text["sourceText"]["text"], "Mixed Case Native");
    assert_eq!(imported.text["sourceText"]["allCaps"], true);
    no_caps_diagnostic(&imported);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn native_text_additional_controls_import_editable_values_and_tracks() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();

    cases.run(
        "crates/aftereffects_file/tests/fixtures/text/import_text_additional_controls.aep",
        1,
        || {
            let project = pinned_project(&ADDITIONAL_SOURCE);
            let all_caps = import_text_case(&project, &ADDITIONAL_SOURCE, 1, "ALL_CAPS");
            assert_additional_source_text(&all_caps.text);
            assert_eq!(all_caps.text["sourceText"]["allCaps"], true);
        },
    );

    cases.run(
        "crates/aftereffects_file/tests/fixtures/text/import_text_additional_controls.aep",
        17,
        || {
            let project = pinned_project(&ADDITIONAL_SOURCE);
            let alignment = import_text_case(&project, &ADDITIONAL_SOURCE, 17, "ANCHOR_ALIGNMENT");
            assert_additional_source_text(&alignment.text);
            assert_eq!(
                alignment.text["anchorOptions"]["groupingAlignment"],
                serde_json::json!([40.0, -30.0])
            );
            assert_eq!(alignment.text["animators"][0]["rotation"], 35.0);
        },
    );

    cases.run(
        "crates/aftereffects_file/tests/fixtures/text/import_text_additional_controls.aep",
        32,
        || {
            let project = pinned_project(&ADDITIONAL_SOURCE);
            let alignment_keyed =
                import_text_case(&project, &ADDITIONAL_SOURCE, 32, "ANCHOR_ALIGNMENT_KEYED");
            assert_additional_source_text(&alignment_keyed.text);
            let anchor_id = &alignment_keyed.text["anchorOptions"]["id"];
            assert_two_keys(
                animation_entry(&alignment_keyed.editable, anchor_id, "groupingAlignment"),
                ExpectedValue::Vector2([0.0, 0.0]),
                ExpectedValue::Vector2([40.0, -30.0]),
            );
        },
    );

    cases.run(
        "crates/aftereffects_file/tests/fixtures/text/import_text_additional_controls.aep",
        47,
        || {
            let project = pinned_project(&ADDITIONAL_SOURCE);
            let first_margin =
                import_text_case(&project, &ADDITIONAL_SOURCE, 47, "FIRST_MARGIN_KEYED");
            assert_additional_source_text(&first_margin.text);
            let path_id = &first_margin.text["pathOptions"]["id"];
            assert!(first_margin.text["pathOptions"]["pathLayer"].is_number());
            assert_two_keys(
                animation_entry(&first_margin.editable, path_id, "firstMargin"),
                ExpectedValue::Scalar(0.0),
                ExpectedValue::Scalar(150.0),
            );
        },
    );

    cases.run(
        "crates/aftereffects_file/tests/fixtures/text/import_text_additional_controls.aep",
        62,
        || {
            let project = pinned_project(&ADDITIONAL_SOURCE);
            let last_margin =
                import_text_case(&project, &ADDITIONAL_SOURCE, 62, "LAST_MARGIN_KEYED");
            assert_additional_source_text(&last_margin.text);
            assert_eq!(last_margin.text["pathOptions"]["forceAlignment"], true);
            let path_id = &last_margin.text["pathOptions"]["id"];
            assert_two_keys(
                animation_entry(&last_margin.editable, path_id, "lastMargin"),
                ExpectedValue::Scalar(0.0),
                ExpectedValue::Scalar(150.0),
            );
        },
    );

    cases.run(
        "crates/aftereffects_file/tests/fixtures/text/import_text_additional_controls.aep",
        77,
        || {
            let project = pinned_project(&ADDITIONAL_SOURCE);
            let auto_leading = import_text_case(&project, &ADDITIONAL_SOURCE, 77, "AUTO_LEADING");
            assert_additional_source_text(&auto_leading.text);
            assert!(auto_leading.text["sourceText"]["leading"].is_null());
        },
    );

    cases.run(
        "crates/aftereffects_file/tests/fixtures/text/import_text_additional_controls.aep",
        92,
        || {
            let project = pinned_project(&ADDITIONAL_SOURCE);
            let box_text = import_text_case(&project, &ADDITIONAL_SOURCE, 92, "BOX_SIZE");
            assert_eq!(
                box_text.text["sourceText"]["text"],
                "Box text wrapping across multiple lines to show paragraph justification."
            );
            assert_eq!(box_text.text["sourceText"]["boxText"], true);
            assert_eq!(
                box_text.text["sourceText"]["boxSize"],
                serde_json::json!([420.0, 280.0])
            );
        },
    );

    cases.run(
        "crates/aftereffects_file/tests/fixtures/text/import_text_additional_controls.aep",
        108,
        || {
            let project = pinned_project(&ADDITIONAL_SOURCE);
            let justified =
                import_text_case(&project, &ADDITIONAL_SOURCE, 108, "PARAGRAPH_FULL_JUSTIFY");
            assert_eq!(justified.text["sourceText"]["boxText"], true);
            assert_eq!(justified.text["sourceText"]["justification"], "justify");
        },
    );

    cases.finish();
}
