//! Text-import fixture coverage with pinned sources and editable-value assertions.
//!
//! Ignored cases state their missing proof. These structural checks do not
//! establish Adobe render fidelity. Supplemental owner-clock rebasing does not
//! replace an independently authored segmented-Source-Text reference. The Slider
//! percent Source Text regressions run normally on derived storage.

use fx_schema::{PropertyTarget, PropertyValue};

use super::*;
use crate::rifx::Chunk;

struct SourcePin {
    bytes: &'static [u8],
    path: &'static str,
    len: usize,
    sha256: &'static str,
}

const TEXT_RANGES: SourcePin = SourcePin {
    bytes: include_bytes!("../../../tests/fixtures/text/text_ranges.aep"),
    path: "text/text_ranges.aep",
    len: 235_825,
    sha256: "bb169bce21c1bd31ca1651cef31c7d525d70cbf9d2e0eab2d99ebc665aadcc72",
};
const ANIMATOR_CHANNELS: SourcePin = SourcePin {
    bytes: include_bytes!("../../../tests/fixtures/text/import_text_animator_channels.aep"),
    path: "text/import_text_animator_channels.aep",
    len: 2_780_109,
    sha256: "56a8b0b4665989d833734a87ad344deedb5d83e7f2b24c9c7e718fd4c69ea289",
};
const SELECTOR_ANIMATION: SourcePin = SourcePin {
    bytes: include_bytes!("../../../tests/fixtures/text/import_selector_animation.aep"),
    path: "text/import_selector_animation.aep",
    len: 522_019,
    sha256: "a5765c7fdbc4d6d323d40323ede6af41ee8d6a3d7dd9e1262d06d07ef242ae66",
};
const REMAINING_SELECTOR_ANIMATION: SourcePin = SourcePin {
    bytes: include_bytes!("../../../tests/fixtures/text/import_remaining_selector_animation.aep"),
    path: "text/import_remaining_selector_animation.aep",
    len: 522_423,
    sha256: "ac04500b466a07be529cbfdc3c5fd95c412db68c53f8adc3bb74fc56c886d515",
};
const ADDITIONAL_CONTROLS: SourcePin = SourcePin {
    bytes: include_bytes!("../../../tests/fixtures/text/import_text_additional_controls.aep"),
    path: "text/import_text_additional_controls.aep",
    len: 704_985,
    sha256: "65a5929c222b2d44727adc40f18438adaa5e339a52568dab308f4e58c06b44e1",
};
const PATH_OPTIONS: SourcePin = SourcePin {
    bytes: include_bytes!("../../../tests/fixtures/text/import_text_path_options.aep"),
    path: "text/import_text_path_options.aep",
    len: 868_467,
    sha256: "019db8c748e3b306591bdbade1cbb83ed466481abf860c3488db10a0a6d485ec",
};

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

fn import(pin: &SourcePin, composition_id: u32, composition_name: &str) -> StructuralConversion {
    let project = pinned_project(pin);
    let source = composition(&project, composition_id);
    assert_eq!(
        source.frame_rate, 24.0,
        "{} comp {composition_id}",
        pin.path
    );
    let converted = to_structural_fx_document(&project, Some(composition_id))
        .unwrap_or_else(|error| panic!("{} comp {composition_id}: {error}", pin.path));
    assert_imported_canvas_matches_source(
        source,
        &converted,
        &format!("{} comp {composition_id}", pin.path),
    );
    assert_eq!(root(&converted).name, composition_name);
    converted
}

fn collect_text<'a>(layers: &'a [fx_schema::Layer], output: &mut Vec<&'a fx_schema::TextLayer>) {
    for layer in layers {
        match layer.data() {
            FxLayer::Text(text) => output.push(text),
            FxLayer::Group(group) => collect_text(&group.layers, output),
            FxLayer::BooleanOperation(boolean) => collect_text(&boolean.layers, output),
            _ => {}
        }
    }
}

fn only_text(converted: &StructuralConversion) -> &fx_schema::TextLayer {
    let mut texts = Vec::new();
    collect_text(root(converted).layers.as_slice(), &mut texts);
    assert_eq!(texts.len(), 1, "expected one editable Text layer");
    texts[0]
}

fn collect_layer_ids(layers: &[fx_schema::Layer], output: &mut HashSet<LayerId>) {
    for layer in layers {
        output.insert(layer.id());
        match layer.data() {
            FxLayer::Group(group) => collect_layer_ids(&group.layers, output),
            FxLayer::BooleanOperation(boolean) => collect_layer_ids(&boolean.layers, output),
            _ => {}
        }
    }
}

fn graph_entry<'a>(
    converted: &'a StructuralConversion,
    target: &PropertyTarget,
) -> &'a fx_schema::animator::AnimationGraphEntry {
    let matching = converted
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .filter(|entry| &entry.target == target)
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1, "one editable graph entry for {target:?}");
    matching[0]
}

fn assert_track(
    entry: &fx_schema::animator::AnimationGraphEntry,
    expected_times: [i64; 2],
    expected_values: [PropertyValue; 2],
) {
    let keys = entry.animator.keyframe_track().unwrap().keyframes();
    assert_eq!(keys.len(), 2);
    assert_eq!(
        keys.iter()
            .map(|key| key.layer_time().as_millis())
            .collect::<Vec<_>>(),
        expected_times
    );
    assert_eq!(keys[0].value(), &expected_values[0]);
    assert_eq!(keys[1].value(), &expected_values[1]);
}

fn source_text_layer(project: &StructuralProject, composition_id: u32) -> &Layer {
    composition(project, composition_id)
        .layers
        .iter()
        .find(|layer| layer.record.layer_type() == 3)
        .expect("pinned composition has a native Text layer")
}

fn rebased_entries(
    pin: &SourcePin,
    composition_id: u32,
    composition_name: &str,
) -> (
    fx_schema::TextLayer,
    Vec<fx_schema::animator::AnimationGraphEntry>,
) {
    let project = pinned_project(pin);
    let converted = to_structural_fx_document(&project, Some(composition_id)).unwrap();
    assert_eq!(root(&converted).name, composition_name);
    let mut text = only_text(&converted).clone();
    // Supplemental segment copy: the immutable source supplies the native
    // property records and exact values. Moving only the destination segment
    // start proves that copied tracks use the segment owner's signed clock.
    text.active_range = TimeRangeProperty::new(
        Time::from_millis(1_000),
        fx_schema::Duration::from_secs(2.0),
    );
    let imported = vec![FxLayer::Text(text.clone())];
    let (entries, warnings) = super::text::animation_entries(
        source_text_layer(&project, composition_id),
        &imported,
        &mut super::animation_budget::AnimationBudget::default(),
    );
    assert!(
        warnings.iter().all(|warning| !warning.contains("omitted")),
        "{warnings:?}"
    );
    (text, entries)
}

fn entry_for(
    entries: &[fx_schema::animator::AnimationGraphEntry],
    target: PropertyTarget,
) -> &fx_schema::animator::AnimationGraphEntry {
    let matching = entries
        .iter()
        .filter(|entry| entry.target == target)
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1, "one copied entry");
    matching[0]
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_native_point_and_box_text_keep_exact_whole_document_fields() {
    let converted = import(&TEXT_RANGES, 1, "TestComp");
    let mut texts = Vec::new();
    collect_text(root(&converted).layers.as_slice(), &mut texts);
    assert_eq!(texts.len(), 8, "all authored text siblings remain editable");

    let point = texts
        .iter()
        .find(|text| text.source_text.text == "Hello World\nSecond Paragraph\nEnd")
        .unwrap();
    assert_eq!(point.source_text.font_family.as_ref(), "MyriadPro");
    assert_eq!(point.source_text.font_style.as_ref(), "Regular");
    assert_eq!(point.source_text.font_size.value(), 72.0);
    assert_eq!(point.source_text.fill_color, [1.0, 0.0, 0.0, 1.0]);
    assert!(!point.source_text.box_text);
    assert_eq!(point.source_text.box_size, None);
    assert_eq!(point.source_text.box_position, None);

    let boxed = texts
        .iter()
        .find(|text| text.source_text.box_text)
        .expect("native paragraph text remains box text");
    assert_eq!(boxed.source_text.box_size, Some([180.0, 300.0]));
    assert_eq!(boxed.source_text.box_position, Some([-90.0, -150.0]));
    assert!(
        converted
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("PostScript font identity")),
        "font identity approximation must remain explicit"
    );
    assert!(
        !converted
            .document
            .to_json_vec()
            .unwrap()
            .windows(8)
            .any(|window| window == b"JsScript"),
        "text import must not author JavaScript"
    );
}

struct AnimatorExpectation {
    composition_id: u32,
    composition_name: &'static str,
    property: &'static str,
    values: [PropertyValue; 2],
}

fn animator_expectations() -> Vec<AnimatorExpectation> {
    use PropertyValue::{Color, Float, Vector2};
    vec![
        AnimatorExpectation {
            composition_id: 17,
            composition_name: "ANIMATOR_ANCHOR_KEYED",
            property: "anchorPoint",
            values: [Vector2([0.0, 0.0]), Vector2([25.0, 15.0])],
        },
        AnimatorExpectation {
            composition_id: 47,
            composition_name: "ANIMATOR_POSITION_KEYED",
            property: "position",
            values: [Vector2([0.0, 0.0]), Vector2([0.0, -60.0])],
        },
        AnimatorExpectation {
            composition_id: 77,
            composition_name: "ANIMATOR_SCALE_KEYED",
            property: "scale",
            values: [Vector2([100.0, 100.0]), Vector2([135.0, 70.0])],
        },
        AnimatorExpectation {
            composition_id: 107,
            composition_name: "ANIMATOR_ROTATION_KEYED",
            property: "rotation",
            values: [Float(0.0), Float(25.0)],
        },
        AnimatorExpectation {
            composition_id: 137,
            composition_name: "ANIMATOR_SKEW_KEYED",
            property: "skew",
            values: [Float(0.0), Float(20.0)],
        },
        AnimatorExpectation {
            composition_id: 167,
            composition_name: "ANIMATOR_SKEW_AXIS_KEYED",
            property: "skewAxis",
            values: [Float(0.0), Float(45.0)],
        },
        AnimatorExpectation {
            composition_id: 197,
            composition_name: "ANIMATOR_TRACKING_KEYED",
            property: "tracking",
            values: [Float(0.0), Float(30.0)],
        },
        AnimatorExpectation {
            composition_id: 227,
            composition_name: "ANIMATOR_STROKE_WIDTH_KEYED",
            property: "strokeWidth",
            values: [Float(0.0), Float(8.0)],
        },
        AnimatorExpectation {
            composition_id: 257,
            composition_name: "ANIMATOR_BLUR_KEYED",
            property: "blur",
            values: [Vector2([0.0, 0.0]), Vector2([8.0, 12.0])],
        },
        AnimatorExpectation {
            composition_id: 287,
            composition_name: "ANIMATOR_OPACITY_KEYED",
            property: "opacity",
            values: [Float(100.0), Float(30.0)],
        },
        AnimatorExpectation {
            composition_id: 317,
            composition_name: "ANIMATOR_FILL_COLOR_KEYED",
            property: "fillColor",
            values: [Color([0.9, 0.7, 0.2, 1.0]), Color([0.2, 0.9, 0.4, 1.0])],
        },
        AnimatorExpectation {
            composition_id: 347,
            composition_name: "ANIMATOR_STROKE_COLOR_KEYED",
            property: "strokeColor",
            values: [Color([0.2, 0.6, 1.0, 1.0]), Color([1.0, 0.2, 0.5, 1.0])],
        },
        AnimatorExpectation {
            composition_id: 377,
            composition_name: "ANIMATOR_LINE_SPACING_KEYED",
            property: "lineSpacing",
            values: [Float(0.0), Float(30.0)],
        },
        AnimatorExpectation {
            composition_id: 407,
            composition_name: "ANIMATOR_LINE_ANCHOR_KEYED",
            property: "lineAnchor",
            values: [Float(0.0), Float(75.0)],
        },
        AnimatorExpectation {
            composition_id: 437,
            composition_name: "ANIMATOR_CHAR_OFFSET_KEYED",
            property: "characterOffset",
            values: [Float(0.0), Float(3.0)],
        },
        AnimatorExpectation {
            composition_id: 467,
            composition_name: "ANIMATOR_CHAR_REPLACE_KEYED",
            property: "characterValue",
            values: [Float(65.0), Float(90.0)],
        },
    ]
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_all_sixteen_animator_families_copy_exact_values_on_signed_segment_clocks() {
    for case in animator_expectations() {
        let converted = import(
            &ANIMATOR_CHANNELS,
            case.composition_id,
            case.composition_name,
        );
        let text = only_text(&converted);
        let animator = &text.animators[0];
        assert_eq!(text.source_text.text, "Editable motion\nSecond line");
        assert_eq!(text.source_text.font_family.as_ref(), "ArialMT");
        assert_track(
            graph_entry(
                &converted,
                &PropertyTarget::fx_item(animator.id, case.property),
            ),
            [500, 2_000],
            case.values.clone(),
        );

        let (rebased_text, entries) = rebased_entries(
            &ANIMATOR_CHANNELS,
            case.composition_id,
            case.composition_name,
        );
        let copied_animator = &rebased_text.animators[0];
        assert_track(
            entry_for(
                &entries,
                PropertyTarget::fx_item(copied_animator.id, case.property),
            ),
            [-500, 1_000],
            case.values,
        );
    }
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_range_wiggly_more_and_path_tracks_share_the_signed_segment_owner_clock() {
    let cases = [
        (
            &SELECTOR_ANIMATION,
            1,
            "SELECTOR_KEYED_PERCENT_START",
            "range",
            "start",
            [PropertyValue::Float(0.0), PropertyValue::Float(0.25)],
        ),
        (
            &REMAINING_SELECTOR_ANIMATION,
            47,
            "KEYED_WIGGLY_SPEED",
            "wiggly",
            "speed",
            [PropertyValue::Float(2.0), PropertyValue::Float(4.0)],
        ),
        (
            &ADDITIONAL_CONTROLS,
            47,
            "FIRST_MARGIN_KEYED",
            "path",
            "firstMargin",
            [PropertyValue::Float(0.0), PropertyValue::Float(150.0)],
        ),
    ];
    for (pin, composition_id, name, family, property, values) in cases {
        let (text, entries) = rebased_entries(pin, composition_id, name);
        let item_id = match family {
            "range" => text.animators[0].selectors[0].id,
            "wiggly" => text.animators[0].wiggly_selectors[0].id,
            "path" => text.path_options.as_ref().unwrap().id,
            _ => unreachable!(),
        };
        assert_track(
            entry_for(&entries, PropertyTarget::fx_item(item_id, property)),
            [-500, 1_000],
            values,
        );
    }

    let (text, entries) = rebased_entries(&ADDITIONAL_CONTROLS, 32, "ANCHOR_ALIGNMENT_KEYED");
    let options = text.anchor_options.as_ref().unwrap();
    assert_track(
        entry_for(
            &entries,
            PropertyTarget::fx_item(options.id, "groupingAlignment"),
        ),
        [-500, 1_000],
        [
            PropertyValue::Vector2([0.0, 0.0]),
            PropertyValue::Vector2([40.0, -30.0]),
        ],
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_native_text_path_resolves_real_mask_guide_identity_not_native_ordinal() {
    let converted = import(&PATH_OPTIONS, 1, "TEXT_PATH_PATH");
    let text = only_text(&converted);
    let options = text
        .path_options
        .as_ref()
        .expect("native mask index 1 becomes editable TextPathOptions");
    let mut ids = HashSet::new();
    collect_layer_ids(root(&converted).layers.as_slice(), &mut ids);
    assert!(ids.contains(&options.path_layer));
    assert_ne!(options.path_layer, text.id);
    assert_eq!(options.first_margin, 0.0);
    assert_eq!(options.last_margin, 0.0);
    assert!(
        converted
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.message.contains("no imported editable mask")),
        "real native mask index must not become a dangling numeric layer id"
    );
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_unsupported_text_semantics_are_diagnosed_without_losing_text_siblings() {
    let converted = import(&TEXT_RANGES, 1, "TestComp");
    let mut texts = Vec::new();
    collect_text(root(&converted).layers.as_slice(), &mut texts);
    assert_eq!(texts.len(), 8);
    assert!(texts.iter().all(|text| !text.source_text.text.is_empty()));
    assert!(
        converted
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("PostScript font identity")),
        "host font resolution limitation must be contextual"
    );
    assert!(texts.iter().all(|text| {
        text.source_text.font_variations.is_none()
            && !text.source_text.underline
            && !text.source_text.strikethrough
    }));
    // The native fixture has no independently recorded non-default variable
    // axes/decorations oracle. Their absence is therefore a proof gap, not a
    // fabricated successful import assertion; supported text siblings remain.
    assert!(!converted.document.to_json_vec().unwrap().is_empty());
}

// Slider percent Source Text: enabled regressions on derived storage. Each case
// combines unchanged Adobe-authored parts of pinned sources; the combination is
// not an Adobe-authored expression oracle or render proof.

const TEXT_POINT: SourcePin = SourcePin {
    bytes: include_bytes!("../../../tests/fixtures/pr4442_native/sources/text_document_point.aep"),
    path: "pr4442_native/sources/text_document_point.aep",
    len: 93_997,
    sha256: "bc32cae3896c3aaf2e7a02f5283ce9ef31c9b0ef53f8c8a30a2b6c30b69eea34",
};
const TEXT_CONTENT_HOLD: SourcePin = SourcePin {
    bytes: include_bytes!(
        "../../../tests/fixtures/pr4442_native/sources/text_document_content_hold.aep"
    ),
    path: "pr4442_native/sources/text_document_content_hold.aep",
    len: 95_581,
    sha256: "058b58952ab9d970f482878b7d68c5ee599d83805176c3908596f4be485f1e3a",
};
/// Layer 16's Fill Opacity: native Hold keys 0 at 0.25 s and 100 at 1.5 s.
const HOLD_KEYS: SourcePin = SourcePin {
    bytes: include_bytes!("../../../tests/fixtures/pr4442_native/sources/paint_fill_enable.aep"),
    path: "pr4442_native/sources/paint_fill_enable.aep",
    len: 85_747,
    sha256: "decd5c9bce7d7c1ea7542c82ab97e50d5c5bfee95b6722bea17e0f12f650504c",
};
/// Layer 28's native expression controls keep AE's default names: display
/// name `-_0_/-` and descriptor `fnam` "Slider Control" / "Angle Control".
const DEFAULT_CONTROLS: SourcePin = SourcePin {
    bytes: include_bytes!("../../../tests/fixtures/geometry/geometry_probe.aep"),
    path: "geometry/geometry_probe.aep",
    len: 388_549,
    sha256: "a7f4830e5e37ba1fbbcb897cfe7a40b6eff3496267c2ca911763cc96971260b2",
};
/// Layer 91's Rect Roundness: native Hold keys 2, 10 and 19 at 0, 1 and 2 s.
const THREE_HOLD_KEYS: SourcePin = SourcePin {
    bytes: include_bytes!(
        "../../../tests/fixtures/implemented_additions/native_transform_keys.aep"
    ),
    path: "implemented_additions/native_transform_keys.aep",
    len: 462_845,
    sha256: "09674f30f1d4544885f355a210c89bb51c05c4cbb1e875fa545c955f1097d43e",
};

/// A complete Source Text percent expression, with AE's CR statement separator.
const PERCENT: &str =
    "s = effect(\"Slider Control\")(\"Slider\");\rMath.round(s).toLocaleString() + \"%\";";

fn native_layer(pin: &SourcePin, id: u32) -> Layer {
    pinned_project(pin)
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Composition(composition) => composition
                .layers
                .iter()
                .find(|layer| layer.record.id() == id)
                .cloned(),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{} layer {id}", pin.path))
}

fn is_named(chunk: &Chunk, name: &str) -> bool {
    chunk.id() == *b"tdmn"
        && chunk
            .data_payload()
            .and_then(|payload| payload.split(|byte| *byte == 0).next())
            == Some(name.as_bytes())
}

/// The `kind` LIST that follows the first `name` match name under `chunks`.
fn named_list<'a>(chunks: &'a mut [Chunk], name: &str, kind: [u8; 4]) -> Option<&'a mut Chunk> {
    if let Some(index) = chunks
        .windows(2)
        .position(|pair| is_named(&pair[0], name) && pair[1].list_kind() == Some(kind))
    {
        return chunks.get_mut(index + 1);
    }
    chunks.iter_mut().find_map(|chunk| {
        chunk
            .children_mut()
            .and_then(|children| named_list(children, name, kind))
    })
}

fn match_name(name: &str) -> Chunk {
    let mut bytes = name.as_bytes().to_vec();
    bytes.resize(40, 0);
    Chunk::data(*b"tdmn", bytes).unwrap()
}

fn utf8_name(id: [u8; 4], value: &str) -> Chunk {
    let mut bytes = b"Utf8".to_vec();
    bytes.extend(u32::try_from(value.len()).unwrap().to_be_bytes());
    bytes.extend(value.as_bytes());
    Chunk::data(id, bytes).unwrap()
}

/// The native Hold keys of [`HOLD_KEYS`], relabelled as a Slider value leaf.
fn native_hold_leaf() -> Vec<Chunk> {
    slider_leaf(&HOLD_KEYS, 16, "ADBE Vector Fill Opacity")
}

/// Layer `id`'s native `property` keys, relabelled as a Slider value leaf.
fn slider_leaf(pin: &SourcePin, id: u32, property: &str) -> Vec<Chunk> {
    let mut layer = native_layer(pin, id);
    let mut leaf = named_list(&mut layer.content, property, *b"tdbs")
        .and_then(|leaf| leaf.children().map(<[Chunk]>::to_vec))
        .unwrap_or_else(|| panic!("{} layer {id} native {property} keys", pin.path));
    for chunk in &mut leaf {
        if chunk.id() == *b"tdsn" {
            *chunk = utf8_name(*b"tdsn", "Slider");
        }
    }
    leaf
}

/// [`DEFAULT_CONTROLS`]' native `kind` effect, displayed as `display`, with
/// `value` grafted in as its `-0001` leaf.
fn native_control(kind: &str, display: &str, value: Vec<Chunk>) -> Vec<Chunk> {
    let mut layer = native_layer(&DEFAULT_CONTROLS, 28);
    let mut descriptor = named_list(&mut layer.content, kind, *b"sspc")
        .expect("native expression control")
        .clone();
    let controls = descriptor
        .children_mut()
        .unwrap()
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .and_then(Chunk::children_mut)
        .unwrap();
    for chunk in controls.iter_mut() {
        if chunk.id() == *b"tdsn" {
            *chunk = utf8_name(*b"tdsn", display);
        }
    }
    let at = controls
        .iter()
        .position(|chunk| is_named(chunk, "ADBE Effect Built In Params"))
        .unwrap();
    controls.splice(
        at..at,
        [
            match_name(&format!("{kind}-0001")),
            Chunk::list(*b"tdbs", value),
        ],
    );
    vec![match_name(kind), descriptor]
}

fn default_slider() -> Vec<Chunk> {
    native_control("ADBE Slider Control", "-_0_/-", native_hold_leaf())
}

/// `text` with `effects` in a new Effect Parade and `expression` stored on
/// its Source Text, disabled through tdb4 byte 119 when `disabled`.
fn with_expression(text: &Layer, effects: Vec<Chunk>, expression: &str, disabled: bool) -> Layer {
    let mut layer = text.clone();
    let root = layer
        .content
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .and_then(Chunk::children_mut)
        .unwrap();
    let at = root
        .iter()
        .position(|chunk| is_named(chunk, "ADBE Transform Group"))
        .unwrap();
    root.splice(
        at..at,
        [
            match_name("ADBE Effect Parade"),
            Chunk::list(*b"tdgp", effects),
        ],
    );
    let descriptor = named_list(&mut layer.content, "ADBE Text Document", *b"btds")
        .and_then(Chunk::children_mut)
        .unwrap()
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdbs"))
        .and_then(Chunk::children_mut)
        .unwrap();
    let flags = descriptor
        .iter_mut()
        .find(|chunk| chunk.id() == *b"tdb4")
        .unwrap();
    let mut bytes = flags.data_payload().unwrap().to_vec();
    assert_eq!(bytes[119..121], [0, 0], "unchanged native flags");
    bytes[120] = 1;
    bytes[119] = u8::from(disabled);
    *flags = Chunk::data(*b"tdb4", bytes).unwrap();
    descriptor.push(Chunk::data(*b"Utf8", expression.as_bytes().to_vec()).unwrap());
    layer
}

fn text_layer(pin: &SourcePin) -> Layer {
    source_text_layer(&pinned_project(pin), 1).clone()
}

/// A fresh import of `pin` composition 1 with its native Text layer replaced.
fn import_text(pin: &SourcePin, text: Layer) -> StructuralConversion {
    let mut project = pinned_project(pin);
    let composition = composition_mut(&mut project, 1);
    let slot = composition
        .layers
        .iter_mut()
        .find(|layer| layer.record.layer_type() == 3)
        .unwrap();
    *slot = text;
    to_structural_fx_document(&project, Some(1)).unwrap_or_else(|error| panic!("{error}"))
}

fn text_segments(converted: &StructuralConversion) -> Vec<fx_schema::TextLayer> {
    let mut texts = Vec::new();
    collect_text(root(converted).layers.as_slice(), &mut texts);
    texts.into_iter().cloned().collect()
}

fn diagnostics_containing<'a>(converted: &'a StructuralConversion, text: &str) -> Vec<&'a str> {
    converted
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .filter(|message| message.contains(text))
        .collect()
}

#[test]
fn slider_percent_source_text_holds_each_native_slider_key() {
    let authored = text_layer(&TEXT_POINT);
    let [plain] = text_segments(&import_text(&TEXT_POINT, authored.clone()))
        .try_into()
        .unwrap();
    let converted = import_text(
        &TEXT_POINT,
        with_expression(&authored, default_slider(), PERCENT, false),
    );
    let segments = text_segments(&converted);
    let held: Vec<_> = segments
        .iter()
        .map(|text| {
            (
                text.source_text.text.as_str(),
                text.active_range.start.as_millis(),
                text.active_range.end().as_millis(),
            )
        })
        .collect();
    // Before the first key AE holds its value, and Hold keeps each value
    // until the next key.
    assert_eq!(held, [("0%", 0, 1_500), ("100%", 1_500, 1_000_000_000_000)]);
    for text in &segments {
        let mut expected = plain.source_text.clone();
        expected.text.clone_from(&text.source_text.text);
        assert_eq!(text.source_text, expected, "cached style and layout kept");
        assert_eq!(text.parent, segments[0].parent);
        assert_eq!(text.transform, plain.transform);
        assert_eq!(text.animators.len(), plain.animators.len());
    }
    assert_eq!(
        diagnostics_containing(&converted, "Slider percent expression lowered to 2"),
        [
            "Source Text: enabled same-layer Slider percent expression lowered to 2 independent editable Hold text segment(s); the live Slider linkage is not retained"
        ]
    );
    assert!(diagnostics_containing(&converted, "not lowered").is_empty());
    assert!(
        !converted
            .document
            .to_json_vec()
            .unwrap()
            .windows(8)
            .any(|window| window == b"JsScript"),
        "no JavaScript is authored"
    );

    // A custom display name is referenced by that name, not the default.
    let custom = native_control("ADBE Slider Control", "Progress", native_hold_leaf());
    let expression = PERCENT.replace("Slider Control", "Progress");
    let converted = import_text(
        &TEXT_POINT,
        with_expression(&authored, custom, &expression, false),
    );
    assert_eq!(
        text_segments(&converted)
            .iter()
            .map(|text| text.source_text.text.as_str())
            .collect::<Vec<_>>(),
        ["0%", "100%"]
    );
}

#[test]
fn source_text_expressions_outside_the_slider_percent_form_keep_authored_text() {
    let authored = text_layer(&TEXT_POINT);
    let [plain] = text_segments(&import_text(&TEXT_POINT, authored.clone()))
        .try_into()
        .unwrap();
    let keep_authored = |converted: &StructuralConversion, context: &str| {
        let [text] = text_segments(converted)
            .try_into()
            .unwrap_or_else(|texts: Vec<_>| panic!("{context}: {} Text layers", texts.len()));
        assert_eq!(text.source_text, plain.source_text, "{context}");
        assert_eq!(text.active_range, plain.active_range, "{context}");
        assert!(
            diagnostics_containing(converted, "lowered to").is_empty(),
            "{context}"
        );
    };

    // A disabled expression leaves AE showing the authored text.
    let converted = import_text(
        &TEXT_POINT,
        with_expression(&authored, default_slider(), PERCENT, true),
    );
    keep_authored(&converted, "disabled");
    assert!(diagnostics_containing(&converted, "Source Text:").is_empty());

    let angle = native_control("ADBE Angle Control", "Slider Control", native_hold_leaf());
    let custom = native_control("ADBE Slider Control", "Slider Control", native_hold_leaf());
    let renamed = native_control("ADBE Slider Control", "Progress", native_hold_leaf());
    let not_lowered: [(&str, Vec<Chunk>, String); 7] = [
        (
            "other syntax",
            default_slider(),
            PERCENT.replace("Math.round", "Math.floor"),
        ),
        (
            "wrong property",
            default_slider(),
            PERCENT.replace("(\"Slider\")", "(\"Angle\")"),
        ),
        (
            "wrong effect",
            default_slider(),
            PERCENT.replace("Slider Control", "Slider"),
        ),
        (
            "default/custom collision",
            [default_slider(), custom].concat(),
            PERCENT.to_owned(),
        ),
        ("custom name is authoritative", renamed, PERCENT.to_owned()),
        ("non-Slider effect", angle, PERCENT.to_owned()),
        (
            "sibling hop",
            default_slider(),
            PERCENT.replace("effect(", "thisComp.layer(\"Controller\").effect("),
        ),
    ];
    for (context, effects, expression) in not_lowered {
        let converted = import_text(
            &TEXT_POINT,
            with_expression(&authored, effects, &expression, false),
        );
        keep_authored(&converted, context);
        assert_eq!(
            diagnostics_containing(&converted, "enabled expression not lowered").len(),
            1,
            "{context}: {:?}",
            converted.diagnostics
        );
    }

    // Keyed Source Text keeps its native Hold documents.
    let keyed = text_layer(&TEXT_CONTENT_HOLD);
    let authored_keys: Vec<_> = text_segments(&import_text(&TEXT_CONTENT_HOLD, keyed.clone()))
        .into_iter()
        .map(|text| (text.source_text, text.active_range))
        .collect();
    assert_eq!(authored_keys.len(), 2);
    let converted = import_text(
        &TEXT_CONTENT_HOLD,
        with_expression(&keyed, default_slider(), PERCENT, false),
    );
    let kept: Vec<_> = text_segments(&converted)
        .into_iter()
        .map(|text| (text.source_text, text.active_range))
        .collect();
    assert_eq!(kept, authored_keys);
    assert_eq!(
        diagnostics_containing(&converted, "keyed Source Text documents are not combined").len(),
        1
    );
}

// Whole-expansion admission. Composition 32 of `ADDITIONAL_CONTROLS` keeps its
// Adobe-authored Text layer 46, whose keyed Grouping Alignment gives every held
// segment one copied control track, and its native background solid.

/// A default-named Slider holding [`THREE_HOLD_KEYS`]: three Hold segments.
fn three_key_slider() -> Vec<Chunk> {
    native_control(
        "ADBE Slider Control",
        "-_0_/-",
        slider_leaf(&THREE_HOLD_KEYS, 91, "ADBE Vector Rect Roundness"),
    )
}

/// Orders `(text, copy, solid)` as the layers of composition 32, in import order.
type Arrangement = fn(Layer, Layer, Layer) -> Vec<Layer>;

/// Composition 32 with the layers `arrange` returns from `text` in place of
/// Text layer 46, an unchanged copy of layer 46 under a fresh id, and the
/// native background solid.
fn alignment_composition(text: Layer, arrange: Arrangement) -> StructuralProject {
    let mut project = pinned_project(&ADDITIONAL_CONTROLS);
    let layers = &mut composition_mut(&mut project, 32).layers;
    let [authored, solid]: [Layer; 2] = std::mem::take(layers)
        .try_into()
        .unwrap_or_else(|layers: Vec<_>| panic!("{} layers", layers.len()));
    assert_eq!(authored.record.id(), 46);
    let mut copy = authored;
    patch(&mut copy, 0, &1_046_u32.to_be_bytes());
    *layers = arrange(text, copy, solid);
    project
}

/// A Dynamic Link picture of composition 32 whose identities start at `first_id`.
fn linked_at(project: &StructuralProject, first_id: u64) -> StructuralConversion {
    to_linked_picture(
        project,
        32,
        &mut |_| MediaResolution::Unavailable,
        Destination::LinkedPicture {
            parent: None,
            first_id,
            asset_namespace: AssetNamespace::STANDALONE,
        },
    )
    .unwrap_or_else(|error| panic!("identities from {first_id}: {error}"))
}

fn document_json(converted: &StructuralConversion) -> String {
    String::from_utf8(converted.document.to_json_vec().unwrap()).unwrap()
}

#[test]
fn slider_percent_expansion_over_the_animation_allowance_keeps_authored_text_and_siblings() {
    let authored = native_layer(&ADDITIONAL_CONTROLS, 46);
    let arrange: Arrangement = |text, copy, solid| vec![text, copy, solid];
    let plain = alignment_composition(authored.clone(), arrange);
    let expression = alignment_composition(
        with_expression(&authored, three_key_slider(), PERCENT, false),
        arrange,
    );
    let unlimited = to_structural_fx_document(&plain, Some(32)).unwrap();
    assert_eq!(
        unlimited.document.composition().dynamics().entries().len(),
        2,
        "only each Text layer's Grouping Alignment animates"
    );
    // Room for the authored layer's track and its later sibling's, not for
    // three segment copies.
    let allowance = unlimited.animation_budget_used;
    let limited =
        to_structural_fx_document_with_animation_limit(&expression, Some(32), allowance).unwrap();
    assert_eq!(
        document_json(&limited),
        document_json(&unlimited),
        "the authored text, its identities, tracks and later sibling are imported as if the expression were not lowered"
    );
    assert_eq!(
        limited.animation_budget_used, allowance,
        "the rejected expansion's reservations are released"
    );
    assert_eq!(
        diagnostics_containing(&limited, "enabled expression not lowered"),
        [
            "Source Text: enabled expression not lowered (the copied control tracks of its 3 Hold segments exceed the generated-animation allowance); authored Source Text retained without evaluating it"
        ]
    );
    assert!(diagnostics_containing(&limited, "lowered to").is_empty());
}

#[test]
fn slider_percent_expansion_past_the_identifier_space_keeps_authored_text_and_siblings() {
    let authored = native_layer(&ADDITIONAL_CONTROLS, 46);
    // Only the authored composition's identities remain. Admission is in
    // import order, so what follows the text must leave the three segments
    // short: here only the background solid, or nothing.
    let arrangements: [(&str, Arrangement); 2] = [
        ("later solid", |text, _, solid| vec![text, solid]),
        ("text last", |text, copy, solid| vec![copy, solid, text]),
    ];
    for (context, arrange) in arrangements {
        let plain = alignment_composition(authored.clone(), arrange);
        let expression = alignment_composition(
            with_expression(&authored, three_key_slider(), PERCENT, false),
            arrange,
        );
        let first_id = u64::MAX - (linked_at(&plain, 1).next_id - 1);
        let authored_picture = linked_at(&plain, first_id);
        assert_eq!(authored_picture.next_id, u64::MAX, "{context}");
        let exhausted = linked_at(&expression, first_id);
        assert_eq!(
            document_json(&exhausted),
            document_json(&authored_picture),
            "{context}: no partial percentage sequence; the authored text and siblings are kept"
        );
        assert_eq!(
            exhausted.next_id,
            u64::MAX,
            "{context}: only the authored identities are consumed"
        );
        assert_eq!(
            diagnostics_containing(&exhausted, "enabled expression not lowered"),
            [
                "Source Text: enabled expression not lowered (its 3 Hold segments exceed the remaining generated identifier space); authored Source Text retained without evaluating it"
            ],
            "{context}"
        );
    }
}

#[test]
fn slider_percent_expansion_above_former_identifier_ceilings_is_admitted_whole() {
    // Past the removed 10,000-object cutoff and 1,000,000-identifier ceiling.
    const FIRST_ID: u64 = 1_000_001;
    let arrange: Arrangement = |text, copy, solid| vec![text, copy, solid];
    let authored = native_layer(&ADDITIONAL_CONTROLS, 46);
    let plain = linked_at(&alignment_composition(authored.clone(), arrange), FIRST_ID);
    let [authored_text, _] = text_segments(&plain)
        .try_into()
        .unwrap_or_else(|texts: Vec<_>| panic!("{} authored Text layers", texts.len()));
    // A TextLayer holds its own identity and one per text control.
    let segment_ids = 1
        + authored_text.animators.len()
        + authored_text
            .animators
            .iter()
            .map(|animator| animator.selectors.len() + animator.wiggly_selectors.len())
            .sum::<usize>()
        + usize::from(authored_text.anchor_options.is_some())
        + usize::from(authored_text.path_options.is_some());
    let expanded = linked_at(
        &alignment_composition(
            with_expression(&authored, three_key_slider(), PERCENT, false),
            arrange,
        ),
        FIRST_ID,
    );
    let (percent, others): (Vec<_>, Vec<_>) = text_segments(&expanded)
        .into_iter()
        .partition(|text| text.source_text.text.ends_with('%'));
    let held: Vec<_> = percent
        .iter()
        .map(|text| {
            (
                text.source_text.text.as_str(),
                text.active_range.start.as_millis(),
                text.active_range.end().as_millis(),
            )
        })
        .collect();
    assert_eq!(
        held,
        [
            ("2%", 0, 1_000),
            ("10%", 1_000, 2_000),
            ("19%", 2_000, 1_000_000_000_000)
        ]
    );
    let [sibling] = others
        .try_into()
        .unwrap_or_else(|texts: Vec<_>| panic!("{} later siblings", texts.len()));
    assert_eq!(sibling.source_text, authored_text.source_text);
    assert!(percent.iter().all(|text| text.id.value() > 1_000_000));
    assert_eq!(
        expanded.document.composition().dynamics().entries().len(),
        4,
        "every segment and the sibling keep their Grouping Alignment track"
    );
    assert_eq!(
        expanded.next_id - plain.next_id,
        2 * u64::try_from(segment_ids).unwrap(),
        "exactly two more segments' identities"
    );
    assert_eq!(
        diagnostics_containing(&expanded, "lowered to 3 independent editable Hold").len(),
        1
    );
    assert!(diagnostics_containing(&expanded, "not lowered").is_empty());
}

/// The opaque COS payload of `layer`'s Source Text.
fn source_text_cos(layer: &Layer) -> &[u8] {
    fn find(chunks: &[Chunk]) -> Option<&[u8]> {
        chunks.iter().find_map(|chunk| {
            if chunk.list_kind() == Some(*b"btdk") {
                chunk.opaque_payload()
            } else {
                chunk.children().and_then(find)
            }
        })
    }
    find(&layer.content).expect("Source Text COS payload")
}

fn native_document_text(layer: &Layer) -> String {
    let value = super::super::text::cos::parse(source_text_cos(layer)).unwrap();
    value
        .get("1")
        .unwrap()
        .get("1")
        .unwrap()
        .index(0)
        .unwrap()
        .get("0")
        .unwrap()
        .get("0")
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}

/// A fresh export of the derived percent source: each held segment stays a
/// native Text layer with its own text and interval, parented through Nulls
/// for the identity source clock and the AE layer.
#[test]
fn slider_percent_segments_export_as_native_text_under_null_parents() {
    let converted = import_text(
        &TEXT_POINT,
        with_expression(&text_layer(&TEXT_POINT), default_slider(), PERCENT, false),
    );
    let output = crate::export_document::to_aep(&converted.document).unwrap();
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("subtree omitted")),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let ItemKind::Composition(composition) = &native.item(1).unwrap().kind else {
        panic!("root composition");
    };
    let named = |name: &str| {
        composition
            .layers
            .iter()
            .find(|layer| layer.name.as_ref() == name)
            .unwrap_or_else(|| panic!("native layer {name:?}"))
    };
    let texts = composition
        .layers
        .iter()
        .filter(|layer| layer.record.layer_type() == 3)
        .collect::<Vec<_>>();
    let held = texts
        .iter()
        .map(|layer| {
            let record = &layer.record;
            let start = record.start_time().unwrap();
            let stretch = record.stretch().unwrap();
            (
                layer.name.as_ref(),
                start + record.in_point().unwrap() * stretch,
                start + record.out_point().unwrap() * stretch,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        held,
        [
            ("Source content clock (Source Text 1)", 0.0, 1.5),
            ("Source content clock (Source Text 2)", 1.5, 2.0),
        ]
    );
    let clock = named("Source content clock");
    let layer = named("text_document_point");
    assert!(clock.record.flags().null_layer && layer.record.flags().null_layer);
    assert_eq!(clock.record.parent_id(), layer.record.id());
    for (text, value) in texts.iter().zip(["0%\r", "100%\r"]) {
        assert_eq!(text.record.parent_id(), clock.record.id());
        assert_eq!(native_document_text(text), value);
    }
}

/// Retimes the two native keys of the `tdbs` children `keys` to `ticks` of
/// `timebase` per second, keeping every other stored byte.
fn retime_keys(keys: &mut [Chunk], timebase: u32, ticks: [i32; 2]) {
    let descriptor = keys
        .iter_mut()
        .find(|chunk| chunk.id() == *b"tdb4")
        .unwrap();
    let mut bytes = descriptor.data_payload().unwrap().to_vec();
    bytes[12..16].copy_from_slice(&timebase.to_be_bytes());
    *descriptor = Chunk::data(*b"tdb4", bytes).unwrap();
    let list = keys
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"list"))
        .and_then(Chunk::children_mut)
        .expect("native key list");
    let stride = list
        .iter()
        .find(|chunk| chunk.id() == *b"lhd3")
        .and_then(Chunk::data_payload)
        .map(|header| usize::from(u16::from_be_bytes([header[18], header[19]])))
        .unwrap();
    let items = list
        .iter_mut()
        .find(|chunk| chunk.id() == *b"ldat")
        .unwrap();
    let mut bytes = items.data_payload().unwrap().to_vec();
    assert_eq!(bytes.len(), 2 * stride, "two native keys");
    for (item, ticks) in bytes.chunks_exact_mut(stride).zip(ticks) {
        item[0..4].copy_from_slice(&ticks.to_be_bytes());
    }
    *items = Chunk::data(*b"ldat", bytes).unwrap();
}

/// A 55% Hold key at native tick 11264 of 30720 per second lies at 30 fps
/// frame 11, between milliseconds 366 and 367.
const FRAME_ELEVEN_TIMEBASE: u32 = 30_720;
const FRAME_ELEVEN_TICKS: i32 = 11_264;

fn frame_eleven_percent_text() -> Layer {
    let mut leaf = native_hold_leaf();
    retime_keys(
        &mut leaf,
        FRAME_ELEVEN_TIMEBASE,
        [7_680, FRAME_ELEVEN_TICKS],
    );
    let slider = native_control("ADBE Slider Control", "-_0_/-", leaf);
    with_expression(&text_layer(&TEXT_POINT), slider, PERCENT, false)
}

fn held_ranges(converted: &StructuralConversion) -> Vec<(String, u64, u64)> {
    text_segments(converted)
        .iter()
        .map(|text| {
            (
                text.source_text.text.clone(),
                text.active_range.start.as_millis(),
                text.active_range.end().as_millis(),
            )
        })
        .collect()
}

/// Values of `texts` shown at `frame` of 30 fps by Tesseract, which samples FX at
/// the nearest whole millisecond.
fn fx_shown_at(texts: &[fx_schema::TextLayer], frame: u32) -> Vec<String> {
    let time = fx_schema::Time::from_secs(f64::from(frame) / 30.0);
    texts
        .iter()
        .filter(|text| text.active_range.start <= time && time < text.active_range.end())
        .map(|text| text.source_text.text.clone())
        .collect()
}

/// Values of the native Text layers of `layers` that After Effects shows at
/// exact time `time` of their composition: `start + [in, out) * stretch`.
fn native_shown_at(layers: &[Layer], time: f64) -> Vec<&'static str> {
    layers
        .iter()
        .filter(|layer| layer.record.layer_type() == 3)
        .filter(|layer| {
            let record = &layer.record;
            let start = record.start_time().unwrap();
            let stretch = record.stretch().unwrap();
            start + record.in_point().unwrap() * stretch <= time
                && time < start + record.out_point().unwrap() * stretch
        })
        .map(|layer| match native_document_text(layer).as_str() {
            "100%\r" => "100%",
            "0%\r" => "0%",
            unexpected => panic!("unexpected native Source Text {unexpected:?}"),
        })
        .collect()
}

/// A Hold key between two milliseconds must be shown from its own frame.
/// Tesseract rounds its sample time to the nearest millisecond, but After Effects
/// samples an export at exact frame times: a boundary rounded up to 367 ms hid
/// the new value at frame 11 (11/30 s) of a 30 fps export.
#[test]
fn hold_key_between_milliseconds_is_shown_from_its_own_frame() {
    let converted = import_text(&TEXT_POINT, frame_eleven_percent_text());
    let segments = text_segments(&converted);
    let output = crate::export_document::to_aep_with_fps(&converted.document, 30.0).unwrap();
    let native = read_project(&output.bytes).unwrap();
    let ItemKind::Composition(composition) = &native.item(1).unwrap().kind else {
        panic!("root composition");
    };
    for (frame, value) in [(10, "0%"), (11, "100%"), (12, "100%")] {
        assert_eq!(fx_shown_at(&segments, frame), [value], "FX frame {frame}");
        assert_eq!(
            native_shown_at(&composition.layers, f64::from(frame) / 30.0),
            [value],
            "exported frame {frame}"
        );
    }
    // Contiguous: the new value starts at the millisecond at or before its key.
    assert_eq!(
        held_ranges(&converted),
        [
            ("0%".into(), 0, 366),
            ("100%".into(), 366, 1_000_000_000_000)
        ]
    );
}

/// The same held text placed 0.1 s early, as an occurrence of its composition:
/// the key's own parent frame (8 = 11 - 3) shows the new value.
#[test]
fn hold_key_between_milliseconds_is_shown_from_its_own_frame_under_an_occurrence_clock() {
    let mut value = import_text(&TEXT_POINT, frame_eleven_percent_text())
        .document
        .to_json_value()
        .unwrap();
    let root = &mut value["composition"]["layers"][0];
    let (root_id, occurrence_id, clock_id) = (root["id"].clone(), 90_000, 90_001);
    let mut text = root["layers"][0].take();
    text["parent"] = serde_json::json!(clock_id);
    root["layers"][0] = serde_json::json!({
        "type": "Group",
        "id": occurrence_id,
        "name": "Early occurrence",
        "parent": root_id,
        "playback": {
            "type": "windowed",
            "inputRange": {"start": 0, "duration": 2000},
            "mapping": {"type": "linear",
                "input": {"start": 0, "duration": 2000},
                "output": {"start": 0, "duration": 2000}},
            "inputOffsetMs": 0
        },
        "transform": text["transform"].clone(),
        "layers": [{
            "type": "Group",
            "id": clock_id,
            "name": "Early occurrence clock",
            "parent": occurrence_id,
            "transform": root["transform"].clone(),
            "playback": {"type": "windowed",
                "inputRange": {"start": 0, "duration": 1900},
                "inputOffsetMs": 0,
                "mapping": {"type": "timeRemap", "property": {
                "before": "inactive", "after": "inactive", "keyframes": [
                {"id": "early-0", "time": 0, "value": 100, "easing": {"type": "linear"}},
                {"id": "early-1", "time": 1900, "value": 2000, "easing": {"type": "linear"}}
            ]}}},
            "layers": [text]
        }]
    });
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let output = crate::export_document::to_aep_with_fps(&document, 30.0).unwrap();
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("subtree omitted")),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let ItemKind::Composition(root) = &native.item(1).unwrap().kind else {
        panic!("root composition");
    };
    let occurrence = root
        .layers
        .iter()
        .find(|layer| layer.name.as_ref() == "Early occurrence clock")
        .unwrap();
    let ItemKind::Composition(source) = &native.item(occurrence.record.source_id()).unwrap().kind
    else {
        panic!("occurrence source");
    };
    let record = &occurrence.record;
    let (start, stretch) = (record.start_time().unwrap(), record.stretch().unwrap());
    for (frame, value) in [(7, "0%"), (8, "100%"), (9, "100%")] {
        // AE shows source time (parent - start) / stretch of the occurrence.
        let time = (f64::from(frame) / 30.0 - start) / stretch;
        assert!((record.in_point().unwrap()..record.out_point().unwrap()).contains(&time));
        assert_eq!(
            native_shown_at(&source.layers, time),
            [value],
            "parent frame {frame}"
        );
    }
}

/// Authored keyed Source Text, and the fallback that keeps it when an
/// expression over it is not lowered, use the same boundary rule.
#[test]
fn keyed_source_text_between_milliseconds_starts_at_the_millisecond_before_its_key() {
    let mut authored = text_layer(&TEXT_CONTENT_HOLD);
    let keys = named_list(&mut authored.content, "ADBE Text Document", *b"btds")
        .and_then(Chunk::children_mut)
        .unwrap()
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdbs"))
        .and_then(Chunk::children_mut)
        .unwrap();
    retime_keys(keys, FRAME_ELEVEN_TIMEBASE, [0, FRAME_ELEVEN_TICKS]);
    for text in [
        authored.clone(),
        with_expression(&authored, default_slider(), PERCENT, false),
    ] {
        assert_eq!(
            held_ranges(&import_text(&TEXT_CONTENT_HOLD, text)),
            [
                ("FIRST editable".into(), 0, 366),
                ("SECOND native".into(), 366, 1_000_000_000_000)
            ]
        );
    }
}
