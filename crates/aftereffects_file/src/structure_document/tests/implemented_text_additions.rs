// Test-only additions for SHA-pinned, independently AE-authored text fixtures.
//
// Expected controls come from the authoring JSX identities and execution-receipt
// hashes recorded in `tests/fixtures/aep_authoring_provenance.json`, not from the
// importer. These structural tests do not establish AEP/FX render fidelity.
use super::*;

use fx_schema::text_animator::{SelectorUnits, WigglySelector};
use fx_schema::{AnchorPointGrouping, Justification, TextLayer};

struct SourcePin {
    bytes: &'static [u8],
    path: &'static str,
    sha256: &'static str,
}

const DOCUMENT_SOURCE: SourcePin = SourcePin {
    bytes: include_bytes!("../../../tests/fixtures/text/import_text_document_controls.aep"),
    path: "text/import_text_document_controls.aep",
    sha256: "348aa59a40d708941e9aa0df9a73e9c916dc3379eb75868c0481109fce09eb63",
};
const FULL_JUSTIFY_SOURCE: SourcePin = SourcePin {
    bytes: include_bytes!("../../../tests/fixtures/text/import_full_justification_variants.aep"),
    path: "text/import_full_justification_variants.aep",
    sha256: "c182f48ab8577ccbdb1e851674bc43b56f1b73eeb4082b6b989b21b8733ca7fc",
};
const ENABLE_SOURCE: SourcePin = SourcePin {
    bytes: include_bytes!("../../../tests/fixtures/text/import_text_enable_cases.aep"),
    path: "text/import_text_enable_cases.aep",
    sha256: "e4d770901943ef5bfc54c7726dcf1ac3ec3888343e0b6254c6930e92903e83a0",
};
const PATH_SOURCE: SourcePin = SourcePin {
    bytes: include_bytes!("../../../tests/fixtures/text/import_text_path_options.aep"),
    path: "text/import_text_path_options.aep",
    sha256: "019db8c748e3b306591bdbade1cbb83ed466481abf860c3488db10a0a6d485ec",
};
const SELECTOR_ANIMATION_SOURCE: SourcePin = SourcePin {
    bytes: include_bytes!("../../../tests/fixtures/text/import_selector_animation.aep"),
    path: "text/import_selector_animation.aep",
    sha256: "a5765c7fdbc4d6d323d40323ede6af41ee8d6a3d7dd9e1262d06d07ef242ae66",
};
const REMAINING_SELECTOR_SOURCE: SourcePin = SourcePin {
    bytes: include_bytes!("../../../tests/fixtures/text/import_remaining_selector_animation.aep"),
    path: "text/import_remaining_selector_animation.aep",
    sha256: "ac04500b466a07be529cbfdc3c5fd95c412db68c53f8adc3bb74fc56c886d515",
};
const SELECTOR_ORDER_SOURCE: SourcePin = SourcePin {
    bytes: include_bytes!("../../../tests/fixtures/text/import_selector_order_cases.aep"),
    path: "text/import_selector_order_cases.aep",
    sha256: "d8c9c8674a4d26ae8903f14648029ae60165f93b41bea7a97cc5a5039c3f07b2",
};

fn pinned_project(pin: &SourcePin) -> StructuralProject {
    assert_eq!(
        format!("{:x}", Sha256::digest(pin.bytes)),
        pin.sha256,
        "{} SHA-256",
        pin.path
    );
    read_project(pin.bytes).unwrap_or_else(|error| panic!("{}: {error}", pin.path))
}

fn import_case(
    project: &StructuralProject,
    pin: &SourcePin,
    composition_id: u32,
    composition_name: &str,
) -> StructuralConversion {
    let source = composition(project, composition_id);
    assert_eq!(
        source.frame_rate, 24.0,
        "{} comp {composition_id}",
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
    converted
}

fn collect_text_layers<'a>(group: &'a GroupLayer, output: &mut Vec<&'a TextLayer>) {
    for layer in &group.layers {
        match layer.data() {
            FxLayer::Group(child) => collect_text_layers(child, output),
            FxLayer::Text(text) => output.push(text),
            _ => {}
        }
    }
}

fn text_layers(converted: &StructuralConversion) -> Vec<&TextLayer> {
    let mut layers = Vec::new();
    collect_text_layers(root(converted), &mut layers);
    layers
}

fn only_text(converted: &StructuralConversion) -> &TextLayer {
    let layers = text_layers(converted);
    assert_eq!(
        layers.len(),
        1,
        "the selected native composition must import one editable Text layer, not flattened media"
    );
    layers[0]
}

fn diagnostic_messages(converted: &StructuralConversion) -> Vec<&str> {
    converted
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect()
}

fn assert_f32_color(actual: [f64; 4], expected: [f32; 4]) {
    assert_eq!(actual, expected.map(f64::from));
}

fn assert_document_baseline(text: &TextLayer) {
    assert_eq!(text.source_text.text, "Editable\nText");
    assert_eq!(&*text.source_text.font_family, "ArialMT");
    // A dash-less PostScript name is kept whole, with no guessed style.
    assert_eq!(&*text.source_text.font_style, "");
}

enum DocumentExpectation {
    Point,
    Box,
    FontSize(f64),
    FillColor([f32; 4]),
    FillOff,
    StrokeColor([f32; 4]),
    StrokeWidth(f64),
    StrokeOverFill,
    Justification(Justification),
    Tracking(f64),
    Leading(f64),
    BaselineShift(f64),
}

struct DocumentCase {
    composition_id: u32,
    name: &'static str,
    expected: DocumentExpectation,
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn intrinsic_text_document_controls_import_typed_editable_values() {
    let mut batch = crate::adobe_test_support::CaseBatch::new();
    let cases = [
        DocumentCase {
            composition_id: 1,
            name: "TEXT_POINT",
            expected: DocumentExpectation::Point,
        },
        DocumentCase {
            composition_id: 17,
            name: "TEXT_BOX",
            expected: DocumentExpectation::Box,
        },
        DocumentCase {
            composition_id: 32,
            name: "TEXT_FONT_SIZE",
            expected: DocumentExpectation::FontSize(110.0),
        },
        DocumentCase {
            composition_id: 47,
            name: "TEXT_FILL_COLOR",
            expected: DocumentExpectation::FillColor([0.2, 0.8, 0.4, 1.0]),
        },
        DocumentCase {
            composition_id: 62,
            name: "TEXT_FILL_OFF",
            expected: DocumentExpectation::FillOff,
        },
        DocumentCase {
            composition_id: 77,
            name: "TEXT_STROKE_COLOR",
            expected: DocumentExpectation::StrokeColor([0.2, 0.7, 1.0, 1.0]),
        },
        DocumentCase {
            composition_id: 92,
            name: "TEXT_STROKE_WIDTH",
            expected: DocumentExpectation::StrokeWidth(10.0),
        },
        DocumentCase {
            composition_id: 107,
            name: "TEXT_STROKE_OVER_FILL",
            expected: DocumentExpectation::StrokeOverFill,
        },
        DocumentCase {
            composition_id: 122,
            name: "TEXT_JUSTIFY_LEFT",
            expected: DocumentExpectation::Justification(Justification::Left),
        },
        DocumentCase {
            composition_id: 137,
            name: "TEXT_JUSTIFY_CENTER",
            expected: DocumentExpectation::Justification(Justification::Center),
        },
        DocumentCase {
            composition_id: 152,
            name: "TEXT_JUSTIFY_RIGHT",
            expected: DocumentExpectation::Justification(Justification::Right),
        },
        DocumentCase {
            composition_id: 167,
            name: "TEXT_TRACKING",
            expected: DocumentExpectation::Tracking(120.0),
        },
        DocumentCase {
            composition_id: 182,
            name: "TEXT_LEADING",
            expected: DocumentExpectation::Leading(130.0),
        },
        DocumentCase {
            composition_id: 197,
            name: "TEXT_BASELINE_SHIFT",
            expected: DocumentExpectation::BaselineShift(30.0),
        },
    ];

    for case in cases {
        batch.run(
            "crates/aftereffects_file/tests/fixtures/text/import_text_document_controls.aep",
            case.composition_id,
            || {
                let project = pinned_project(&DOCUMENT_SOURCE);
                let converted =
                    import_case(&project, &DOCUMENT_SOURCE, case.composition_id, case.name);
                let text = only_text(&converted);
                assert_document_baseline(text);
                match case.expected {
                    DocumentExpectation::Point => {
                        assert!(!text.source_text.box_text);
                        assert!(text.source_text.box_size.is_none());
                    }
                    DocumentExpectation::Box => {
                        assert!(text.source_text.box_text);
                        assert_eq!(text.source_text.box_size, Some([600.0, 300.0]));
                    }
                    DocumentExpectation::FontSize(expected) => {
                        assert_eq!(text.source_text.font_size.value(), expected);
                    }
                    DocumentExpectation::FillColor(expected) => {
                        assert!(text.source_text.apply_fill);
                        assert_f32_color(text.source_text.fill_color, expected);
                    }
                    DocumentExpectation::FillOff => {
                        assert!(!text.source_text.apply_fill);
                        assert!(text.source_text.apply_stroke);
                        assert_f32_color(
                            text.source_text
                                .stroke_color
                                .expect("authored stroke color"),
                            [1.0, 0.3, 0.2, 1.0],
                        );
                        assert_eq!(text.source_text.stroke_width.value(), 3.0);
                    }
                    DocumentExpectation::StrokeColor(expected) => {
                        assert!(text.source_text.apply_stroke);
                        assert_f32_color(
                            text.source_text
                                .stroke_color
                                .expect("authored stroke color"),
                            expected,
                        );
                        assert_eq!(text.source_text.stroke_width.value(), 4.0);
                    }
                    DocumentExpectation::StrokeWidth(expected) => {
                        assert!(text.source_text.apply_stroke);
                        assert_eq!(text.source_text.stroke_width.value(), expected);
                    }
                    DocumentExpectation::StrokeOverFill => {
                        assert!(text.source_text.apply_fill);
                        assert!(text.source_text.apply_stroke);
                        assert!(text.source_text.stroke_over_fill);
                    }
                    DocumentExpectation::Justification(expected) => {
                        assert_eq!(text.source_text.justification, expected);
                    }
                    DocumentExpectation::Tracking(expected) => {
                        assert_eq!(text.source_text.tracking, expected);
                    }
                    DocumentExpectation::Leading(expected) => {
                        assert_eq!(
                            text.source_text.leading.map(|value| value.value()),
                            Some(expected)
                        );
                    }
                    DocumentExpectation::BaselineShift(expected) => {
                        assert_eq!(text.source_text.baseline_shift, expected);
                    }
                }
            },
        );
    }
    batch.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn full_justification_variants_map_to_supported_typed_justification() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (composition_id, name, source_layer_id, native_justification_code, final_line_behavior) in [
        (1, "FULL_JUSTIFY_LAST_CENTER", 16, 5, "centers"),
        (17, "FULL_JUSTIFY_LAST_RIGHT", 31, 4, "right-aligns"),
        (32, "FULL_JUSTIFY_LAST_FULL", 46, 6, "fully justifies"),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/text/import_full_justification_variants.aep",
            composition_id,
            || {
                let project = pinned_project(&FULL_JUSTIFY_SOURCE);
                let converted =
                    import_case(&project, &FULL_JUSTIFY_SOURCE, composition_id, name);
                let text = only_text(&converted);
                assert_eq!(
                    text.source_text.text,
                    "Editable paragraph with sufficient words to wrap across several lines and a short final line."
                );
                assert_eq!(&*text.source_text.font_family, "ArialMT");
                assert_eq!(text.source_text.font_size.value(), 64.0);
                assert!(text.source_text.box_text);
                assert_eq!(text.source_text.box_size, Some([700.0, 400.0]));
                assert_eq!(text.source_text.justification, Justification::Justify);
                let first_baseline = text
                    .source_text
                    .box_first_baseline
                    .expect("AE cached box-text first baseline");
                assert!((first_baseline - (-154.17676)).abs() < 1.0e-6);

                let expected_justification_diagnostic = format!(
                    "Source Text first paragraph-style run field 0 justification code {native_justification_code} {final_line_behavior} the final line, but the destination Justify mode leaves the final line left-aligned; Justify used"
                );
                assert!(
                    converted.diagnostics.iter().any(|diagnostic| {
                        diagnostic.limitation == Limitation::Properties
                            && diagnostic.composition_id == Some(composition_id)
                            && diagnostic.layer_id == Some(source_layer_id)
                            && diagnostic.message == expected_justification_diagnostic
                    }),
                    "{} comp {composition_id} layer {source_layer_id}: native first paragraph-style run field 0 is code {native_justification_code}; expected {expected_justification_diagnostic:?}, actual diagnostics: {:#?}",
                    FULL_JUSTIFY_SOURCE.path,
                    converted.diagnostics
                );
                let expected_baseline_diagnostic = "Source Text box first-line baseline is imported from AE's cached source layout as a fixed value; destination text or font-size edits/animation and line-layout changes do not recompute it automatically";
                assert!(
                    converted.diagnostics.iter().any(|diagnostic| {
                        diagnostic.limitation == Limitation::Properties
                            && diagnostic.composition_id == Some(composition_id)
                            && diagnostic.layer_id == Some(source_layer_id)
                            && diagnostic.message == expected_baseline_diagnostic
                    }),
                    "{} comp {composition_id} layer {source_layer_id}: cached baseline {first_baseline} requires a fixed-baseline approximation diagnostic; actual diagnostics: {:#?}",
                    FULL_JUSTIFY_SOURCE.path,
                    converted.diagnostics
                );
            },
        );
    }
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn timed_source_text_imports_authored_hold_segments() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    cases.run(
        "crates/aftereffects_file/tests/fixtures/text/import_text_document_controls.aep",
        212,
        || {
            let project = pinned_project(&DOCUMENT_SOURCE);
            let converted = import_case(&project, &DOCUMENT_SOURCE, 212, "TEXT_SOURCE_TEXT_HOLD");
            let layers = text_layers(&converted);
            assert_eq!(
                layers.len(),
                2,
                "both authored Source Text values stay editable"
            );
            assert_eq!(layers[0].source_text.text, "Editable\nText");
            assert_eq!(layers[0].active_range.start.as_secs(), 0.0);
            assert_eq!(layers[0].active_range.end().as_secs(), 2.0);
            assert_eq!(layers[1].source_text.text, "Second\nText");
            assert_eq!(layers[1].active_range.start.as_secs(), 2.0);
            assert!(layers[1].active_range.end().as_secs() > 2.0);
            assert!(diagnostic_messages(&converted).iter().any(|message| {
                message.contains(
                    "timed Source Text is represented by hold-segment editable TextLayers",
                )
            }));
        },
    );
    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn disabled_text_groups_omit_only_the_disabled_editable_controls() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();

    cases.run(
        "crates/aftereffects_file/tests/fixtures/text/import_text_enable_cases.aep",
        1,
        || {
            let project = pinned_project(&ENABLE_SOURCE);
            let animator = import_case(&project, &ENABLE_SOURCE, 1, "ANIMATOR_OFF");
            assert!(only_text(&animator).animators.is_empty());
            assert!(
                diagnostic_messages(&animator)
                    .iter()
                    .any(|message| message.contains("disabled ADBE Text Animator omitted"))
            );
        },
    );

    cases.run(
        "crates/aftereffects_file/tests/fixtures/text/import_text_enable_cases.aep",
        14,
        || {
            let project = pinned_project(&ENABLE_SOURCE);
            let range = import_case(&project, &ENABLE_SOURCE, 14, "RANGE_OFF");
            let text = only_text(&range);
            assert_eq!(text.animators.len(), 1);
            assert!(text.animators[0].selectors.is_empty());
            assert!(text.animators[0].wiggly_selectors.is_empty());
            assert!(text.animators[0].position.is_some());
            assert!(
                diagnostic_messages(&range)
                    .iter()
                    .any(|message| message.contains("disabled ADBE Text Selector omitted"))
            );
        },
    );

    cases.run(
        "crates/aftereffects_file/tests/fixtures/text/import_text_enable_cases.aep",
        27,
        || {
            let project = pinned_project(&ENABLE_SOURCE);
            let path = import_case(&project, &ENABLE_SOURCE, 27, "PATH_OPTIONS_OFF");
            let text = only_text(&path);
            assert!(text.path_options.is_none());
            assert_eq!(text.animators.len(), 1);
            assert!(
                diagnostic_messages(&path)
                    .iter()
                    .any(|message| message.contains("disabled ADBE Text Path Options omitted"))
            );
        },
    );

    cases.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn text_path_and_anchor_options_import_typed_editable_controls() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();
    for (composition_id, name, first_margin, last_margin, perpendicular, reverse, force) in [
        (1, "TEXT_PATH_PATH", 0.0, 0.0, true, false, false),
        (17, "TEXT_PATH_FIRST_MARGIN", 80.0, 0.0, true, false, false),
        (32, "TEXT_PATH_LAST_MARGIN", 0.0, 70.0, true, false, false),
        (47, "TEXT_PATH_PERPENDICULAR", 0.0, 0.0, true, false, false),
        (62, "TEXT_PATH_REVERSE", 0.0, 0.0, true, true, false),
        (77, "TEXT_PATH_FORCE_ALIGN", 0.0, 0.0, true, false, true),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/text/import_text_path_options.aep",
            composition_id,
            || {
                let project = pinned_project(&PATH_SOURCE);
                let converted = import_case(&project, &PATH_SOURCE, composition_id, name);
                let text = only_text(&converted);
                assert_eq!(text.source_text.text, "Text following a curved path");
                let options = text
                    .path_options
                    .as_ref()
                    .expect("authored Text Path Options");
                assert_ne!(options.path_layer, text.id);
                assert!(options.path_layer.value() > 0);
                assert_eq!(options.first_margin, first_margin);
                assert_eq!(options.last_margin, last_margin);
                assert_eq!(options.perpendicular_to_path, perpendicular);
                assert_eq!(options.reverse_path, reverse);
                assert_eq!(options.force_alignment, force);
            },
        );
    }

    for (composition_id, name, grouping) in [
        (92, "TEXT_ANCHOR_GROUPING_1", AnchorPointGrouping::Character),
        (107, "TEXT_ANCHOR_GROUPING_2", AnchorPointGrouping::Word),
        (122, "TEXT_ANCHOR_GROUPING_3", AnchorPointGrouping::Line),
        (137, "TEXT_ANCHOR_GROUPING_4", AnchorPointGrouping::All),
    ] {
        cases.run(
            "crates/aftereffects_file/tests/fixtures/text/import_text_path_options.aep",
            composition_id,
            || {
                let project = pinned_project(&PATH_SOURCE);
                let converted = import_case(&project, &PATH_SOURCE, composition_id, name);
                let text = only_text(&converted);
                assert_eq!(text.source_text.text, "Anchor grouping\nSecond line");
                let options = text
                    .anchor_options
                    .as_ref()
                    .expect("authored Text More Options");
                assert_eq!(options.anchor_point_grouping, grouping);
            },
        );
    }
    cases.finish();
}

fn animation_entry(
    converted: &StructuralConversion,
    item_id: fx_schema::FxItemId,
    property: &str,
) -> Value {
    let editable: Value =
        serde_json::from_slice(&converted.document.to_json_vec().unwrap()).unwrap();
    let matching = editable["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| {
            entry["target"]["kind"] == "fxItemProperty"
                && entry["target"]["itemId"] == item_id.value()
                && entry["target"]["propertyName"] == property
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        matching.len(),
        1,
        "expected one typed editable track for item {} property {property}",
        item_id.value()
    );
    matching.into_iter().next().unwrap()
}

fn assert_scalar_keys(entry: &Value, first: f64, second: f64) {
    let keys = entry["animator"]["keyframes"].as_array().unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0]["layerTime"], 500);
    assert_eq!(keys[0]["value"]["value"], first);
    assert_eq!(keys[1]["layerTime"], 2000);
    assert_eq!(keys[1]["value"]["value"], second);
    assert!(keys[0]["value"]["value"].is_number());
    assert!(keys[1]["value"]["value"].is_number());
}

struct SelectorTrackCase {
    composition_id: u32,
    name: &'static str,
    property: &'static str,
    first: f64,
    second: f64,
    units: SelectorUnits,
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn selector_numeric_animation_imports_unit_correct_typed_tracks() {
    let mut batch = crate::adobe_test_support::CaseBatch::new();
    let cases = [
        SelectorTrackCase {
            composition_id: 1,
            name: "SELECTOR_KEYED_PERCENT_START",
            property: "start",
            first: 0.0,
            second: 0.5,
            units: SelectorUnits::Percentage,
        },
        SelectorTrackCase {
            composition_id: 14,
            name: "SELECTOR_KEYED_PERCENT_END",
            property: "end",
            first: 0.25,
            second: 1.0,
            units: SelectorUnits::Percentage,
        },
        SelectorTrackCase {
            composition_id: 27,
            name: "SELECTOR_KEYED_PERCENT_OFFSET",
            property: "offset",
            first: -0.3,
            second: 0.3,
            units: SelectorUnits::Percentage,
        },
        SelectorTrackCase {
            composition_id: 40,
            name: "SELECTOR_KEYED_INDEX_START",
            property: "start",
            first: 0.0,
            second: 5.0,
            units: SelectorUnits::Index,
        },
        SelectorTrackCase {
            composition_id: 53,
            name: "SELECTOR_KEYED_INDEX_END",
            property: "end",
            first: 3.0,
            second: 12.0,
            units: SelectorUnits::Index,
        },
        SelectorTrackCase {
            composition_id: 66,
            name: "SELECTOR_KEYED_INDEX_OFFSET",
            property: "offset",
            first: 0.0,
            second: 4.0,
            units: SelectorUnits::Index,
        },
        SelectorTrackCase {
            composition_id: 79,
            name: "SELECTOR_KEYED_AMOUNT",
            property: "amount",
            first: 0.0,
            second: 1.0,
            units: SelectorUnits::Percentage,
        },
    ];

    for case in cases {
        batch.run(
            "crates/aftereffects_file/tests/fixtures/text/import_selector_animation.aep",
            case.composition_id,
            || {
                let project = pinned_project(&SELECTOR_ANIMATION_SOURCE);
                let converted = import_case(
                    &project,
                    &SELECTOR_ANIMATION_SOURCE,
                    case.composition_id,
                    case.name,
                );
                let text = only_text(&converted);
                assert_eq!(text.source_text.text, "Editable Selector");
                assert_eq!(text.animators.len(), 1);
                let selector = &text.animators[0].selectors[0];
                assert_eq!(selector.units, case.units);
                assert_scalar_keys(
                    &animation_entry(&converted, selector.id, case.property),
                    case.first,
                    case.second,
                );
            },
        );
    }
    batch.finish();
}

enum RemainingSelectorKind {
    Range {
        property: &'static str,
        first: f64,
        second: f64,
    },
    Wiggly {
        property: &'static str,
        first: f64,
        second: f64,
    },
}

struct RemainingSelectorCase {
    composition_id: u32,
    name: &'static str,
    kind: RemainingSelectorKind,
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn remaining_selector_animation_imports_typed_editable_tracks() {
    let mut batch = crate::adobe_test_support::CaseBatch::new();
    let cases = [
        RemainingSelectorCase {
            composition_id: 1,
            name: "KEYED_RANGE_EASE_HIGH",
            kind: RemainingSelectorKind::Range {
                property: "easeHigh",
                first: 0.0,
                second: 0.7,
            },
        },
        RemainingSelectorCase {
            composition_id: 17,
            name: "KEYED_RANGE_EASE_LOW",
            kind: RemainingSelectorKind::Range {
                property: "easeLow",
                first: 0.0,
                second: 0.7,
            },
        },
        RemainingSelectorCase {
            composition_id: 32,
            name: "KEYED_RANGE_RANDOM_SEED",
            kind: RemainingSelectorKind::Range {
                property: "randomSeed",
                first: 1.0,
                second: 23.0,
            },
        },
        RemainingSelectorCase {
            composition_id: 47,
            name: "KEYED_WIGGLY_SPEED",
            kind: RemainingSelectorKind::Wiggly {
                property: "speed",
                first: 1.0,
                second: 5.0,
            },
        },
        RemainingSelectorCase {
            composition_id: 62,
            name: "KEYED_WIGGLY_AMOUNT",
            kind: RemainingSelectorKind::Wiggly {
                property: "amount",
                first: 20.0,
                second: 80.0,
            },
        },
        RemainingSelectorCase {
            composition_id: 77,
            name: "KEYED_WIGGLY_SEED",
            kind: RemainingSelectorKind::Wiggly {
                property: "seed",
                first: 1.0,
                second: 23.0,
            },
        },
    ];

    for case in cases {
        batch.run(
            "crates/aftereffects_file/tests/fixtures/text/import_remaining_selector_animation.aep",
            case.composition_id,
            || {
                let project = pinned_project(&REMAINING_SELECTOR_SOURCE);
                let converted = import_case(
                    &project,
                    &REMAINING_SELECTOR_SOURCE,
                    case.composition_id,
                    case.name,
                );
                let text = only_text(&converted);
                assert_eq!(text.source_text.text, "Editable selector motion");
                assert_eq!(text.animators.len(), 1);
                let (item_id, property, first, second) = match case.kind {
                    RemainingSelectorKind::Range {
                        property,
                        first,
                        second,
                    } => {
                        let selector = &text.animators[0].selectors[0];
                        if property == "randomSeed" {
                            assert!(selector.randomize_order);
                        }
                        (selector.id, property, first, second)
                    }
                    RemainingSelectorKind::Wiggly {
                        property,
                        first,
                        second,
                    } => {
                        let selector: &WigglySelector = &text.animators[0].wiggly_selectors[0];
                        (selector.id, property, first, second)
                    }
                };
                assert_scalar_keys(
                    &animation_entry(&converted, item_id, property),
                    first,
                    second,
                );
            },
        );
    }
    batch.finish();
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn selector_order_preserves_supported_order_and_diagnoses_interleaving() {
    let mut cases = crate::adobe_test_support::CaseBatch::new();

    cases.run(
        "crates/aftereffects_file/tests/fixtures/text/import_selector_order_cases.aep",
        1,
        || {
            let project = pinned_project(&SELECTOR_ORDER_SOURCE);
            let two_ranges = import_case(&project, &SELECTOR_ORDER_SOURCE, 1, "TWO_RANGES");
            let animator = &only_text(&two_ranges).animators[0];
            assert_eq!(animator.selectors.len(), 2);
            assert!(animator.wiggly_selectors.is_empty());
            assert_eq!(
                (animator.selectors[0].start, animator.selectors[0].end),
                (0.0, 0.65)
            );
            assert_eq!(
                (animator.selectors[1].start, animator.selectors[1].end),
                (0.35, 1.0)
            );
        },
    );

    cases.run(
        "crates/aftereffects_file/tests/fixtures/text/import_selector_order_cases.aep",
        17,
        || {
            let project = pinned_project(&SELECTOR_ORDER_SOURCE);
            let range_then_wiggly =
                import_case(&project, &SELECTOR_ORDER_SOURCE, 17, "RANGE_THEN_WIGGLY");
            let animator = &only_text(&range_then_wiggly).animators[0];
            assert_eq!(animator.selectors.len(), 1);
            assert_eq!(animator.wiggly_selectors.len(), 1);
            assert!(
                !diagnostic_messages(&range_then_wiggly)
                    .iter()
                    .any(|message| message.contains("authored selector order is approximated"))
            );
        },
    );

    cases.run(
        "crates/aftereffects_file/tests/fixtures/text/import_selector_order_cases.aep",
        32,
        || {
            let project = pinned_project(&SELECTOR_ORDER_SOURCE);
            let wiggly_then_range =
                import_case(&project, &SELECTOR_ORDER_SOURCE, 32, "WIGGLY_THEN_RANGE");
            let animator = &only_text(&wiggly_then_range).animators[0];
            assert_eq!(animator.selectors.len(), 1);
            assert_eq!(animator.wiggly_selectors.len(), 1);
            assert!(diagnostic_messages(&wiggly_then_range).iter().any(|message| message.contains(
                "interleaves Range and Wiggly Selectors; the destination evaluates all ranges before all wigglies, so authored selector order is approximated"
            )));
        },
    );

    cases.finish();
}
