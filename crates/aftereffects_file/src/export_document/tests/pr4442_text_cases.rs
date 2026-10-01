//! Text export contracts for edited FX documents.
//!
//! Inputs in this module are explicitly authored editable FX documents. Native
//! re-import and record checks are supplementary structural evidence only; no
//! generated project has been opened or rendered by Adobe.

use std::collections::{BTreeSet, HashSet};

use fx_schema::{
    FxItemId, PropertyAnimator, PropertyTarget, TextLayer,
    animator::{AnimatorData, KeyframeId, PropertyKeyframe, PropertyKeyframeTrack},
};
use sha2::{Digest, Sha256};

use super::*;

fn identity_transform() -> Value {
    json!({
        "anchorPoint": [0.0, 0.0],
        "position": [320.0, 180.0],
        "scale": [100.0, 100.0],
        "rotation": 0.0,
        "opacity": 100.0
    })
}

fn source_text(text: &str, box_text: bool) -> Value {
    let mut value = json!({
        "text": text,
        "fontFamily": "Inter-Regular",
        "fontStyle": "Regular",
        "fontSize": 42.0,
        "applyFill": true,
        "fillColor": [0.1, 0.2, 0.3, 0.9],
        "applyStroke": true,
        "strokeColor": [0.8, 0.7, 0.6, 0.5],
        "strokeWidth": 3.0,
        "strokeOverFill": true,
        "justification": "center",
        "tracking": 24.0,
        "leading": 54.0,
        "baselineShift": -2.0,
        "boxText": box_text,
        "allCaps": true
    });
    if box_text {
        value["boxSize"] = json!([420.0, 180.0]);
        value["boxPosition"] = json!([-210.0, -90.0]);
    }
    value
}

fn text_layer(id: u64, name: &str, source: Value) -> Value {
    json!({
        "type": "Text",
        "id": id,
        "name": name,
        "parent": null,
        "activeRange": {"start": 500, "duration": 2500},
        "transform": identity_transform(),
        "sourceText": source
    })
}

fn guide_layer(id: u64) -> Value {
    json!({
        "type": "Shape",
        "id": id,
        "name": "Fresh text path guide",
        "parent": null,
        "activeRange": {"start": 500, "duration": 2500},
        "transform": identity_transform(),
        "shape": {
            "path": {"commands": [
                {"type": "moveTo", "x": -160.0, "y": 0.0},
                {"type": "lineTo", "x": 160.0, "y": 0.0}
            ]}
        }
    })
}

fn explicit_document(
    layers: Vec<Value>,
    entries: Vec<AnimationGraphEntry>,
) -> EditableFxCompositionDocument {
    let mut value = imported();
    value["composition"]["layers"] = Value::Array(layers);
    value["composition"]["dynamics"] = json!({"entries": entries});
    EditableFxCompositionDocument::from_json_value(value).unwrap()
}

fn track(
    prefix: &str,
    values: Vec<(i64, PropertyValue)>,
    easing: PropertyKeyframeEasing,
) -> PropertyKeyframeTrack {
    PropertyKeyframeTrack::new(
        values
            .into_iter()
            .enumerate()
            .map(|(index, (time, value))| {
                PropertyKeyframe::new(
                    KeyframeId::new(format!("pr4442-text-{prefix}-{time}-{index}")),
                    fx_schema::TimeOffset::from_millis(time),
                    value,
                    easing,
                )
            })
            .collect(),
    )
    .unwrap()
}

fn item_entry(
    id: FxItemId,
    property: &'static str,
    values: [(i64, PropertyValue); 2],
) -> AnimationGraphEntry {
    AnimationGraphEntry {
        target: PropertyTarget::fx_item(id, property),
        animator: PropertyAnimator::keyframes(track(
            &format!("item-{}-{property}", id.value()),
            values.into_iter().collect(),
            PropertyKeyframeEasing::Linear,
        )),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

fn layer_entry(
    id: LayerId,
    property: PropType,
    values: [(i64, PropertyValue); 2],
    easing: PropertyKeyframeEasing,
) -> AnimationGraphEntry {
    AnimationGraphEntry {
        target: PropertyTarget::layer(id, property),
        animator: PropertyAnimator::keyframes(track(
            &format!("layer-{}-{property:?}", id.value()),
            values.into_iter().collect(),
            easing,
        )),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

fn constant_layer_entry(
    id: LayerId,
    property: PropType,
    value: PropertyValue,
) -> AnimationGraphEntry {
    AnimationGraphEntry {
        target: PropertyTarget::layer(id, property),
        animator: PropertyAnimator::constant(value).unwrap(),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

fn disabled_layer_entry(
    id: LayerId,
    property: PropType,
    values: [(i64, PropertyValue); 2],
    disabled_value: PropertyValue,
) -> AnimationGraphEntry {
    let target = PropertyTarget::layer(id, property);
    let animator = PropertyAnimator::keyframes(track(
        &format!("disabled-{}-{property:?}", id.value()),
        values.into_iter().collect(),
        PropertyKeyframeEasing::Hold,
    ));
    let mut data = animator.data().clone();
    let AnimatorData::Keyframes {
        enabled,
        disabled_value: stored_disabled_value,
        ..
    } = &mut data
    else {
        panic!("keyed text fixture")
    };
    *enabled = false;
    *stored_disabled_value = Some(disabled_value);
    AnimationGraphEntry {
        target,
        animator: PropertyAnimator::from_data(&data).unwrap(),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

fn collect_text<'a>(layers: &'a [fx_schema::Layer], output: &mut Vec<&'a TextLayer>) {
    for layer in layers {
        match layer.data() {
            LayerData::Text(text) => output.push(text),
            LayerData::Group(group) => collect_text(&group.layers, output),
            LayerData::BooleanOperation(boolean) => collect_text(&boolean.layers, output),
            _ => {}
        }
    }
}

fn find_layer_named<'a>(
    layers: &'a [fx_schema::Layer],
    name: &str,
) -> Option<&'a fx_schema::Layer> {
    layers.iter().find_map(|layer| {
        if layer.data().name() == name {
            return Some(layer);
        }
        match layer.data() {
            LayerData::Group(group) => find_layer_named(&group.layers, name),
            LayerData::BooleanOperation(boolean) => find_layer_named(&boolean.layers, name),
            _ => None,
        }
    })
}

fn imported_texts(project: &StructuralProject) -> Vec<TextLayer> {
    let converted = to_structural_fx_document(project, Some(1)).unwrap();
    let mut texts = Vec::new();
    collect_text(converted.document.composition().layers(), &mut texts);
    texts.into_iter().cloned().collect()
}

fn utf16_hex(value: &str) -> Vec<u8> {
    let mut output = String::from("<FEFF");
    for unit in value.encode_utf16() {
        output.push_str(&format!("{unit:04X}"));
    }
    output.push('>');
    output.into_bytes()
}

fn occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_fresh_point_and_box_cos_text_preserves_whole_document_fields() {
    let document = explicit_document(
        vec![
            text_layer(5_001, "Fresh point", source_text("Point Ω\nSecond", false)),
            text_layer(5_002, "Fresh box", source_text("Box 世界\nSecond", true)),
        ],
        Vec::new(),
    );
    let output = to_aep(&document).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
    assert!(
        layers(&native)
            .iter()
            .all(|layer| { layer.record.layer_type() == 3 && layer.record.source_id() == 0 })
    );

    let texts = imported_texts(&native);
    assert_eq!(texts.len(), 2);
    let point = texts
        .iter()
        .find(|text| text.name == "Fresh point")
        .unwrap();
    assert_eq!(point.source_text.text, "Point Ω\nSecond");
    assert_eq!(point.source_text.font_family.as_ref(), "Inter");
    assert_eq!(point.source_text.font_style.as_ref(), "Regular");
    assert_eq!(point.source_text.font_size.value(), 42.0);
    assert_eq!(point.source_text.fill_color, [0.1, 0.2, 0.3, 0.9]);
    assert_eq!(point.source_text.stroke_color, Some([0.8, 0.7, 0.6, 0.5]));
    assert_eq!(point.source_text.stroke_width.value(), 3.0);
    assert!(point.source_text.stroke_over_fill);
    assert_eq!(point.source_text.tracking, 24.0);
    assert_eq!(point.source_text.leading.unwrap().value(), 54.0);
    assert_eq!(point.source_text.baseline_shift, -2.0);
    assert!(point.source_text.all_caps);
    assert!(!point.source_text.box_text);

    let boxed = texts.iter().find(|text| text.name == "Fresh box").unwrap();
    assert!(boxed.source_text.box_text);
    assert_eq!(boxed.source_text.box_size, Some([420.0, 180.0]));
    assert_eq!(boxed.source_text.box_position, Some([-210.0, -90.0]));
    assert!(!output.bytes.windows(8).any(|window| window == b"JsScript"));
}

/// Independently AE-authored All Caps sources store caps code 2 in field 12 of
/// the character style and normal text stores 0. The fresh export must write
/// those native bytes, not merely values our own reader accepts.
#[test]
fn fresh_all_caps_text_exports_the_native_caps_code_and_reimports_all_caps() {
    let mut normal = source_text("Normal case", false);
    normal["allCaps"] = json!(false);
    let document = explicit_document(
        vec![
            text_layer(5_011, "Fresh all caps", source_text("Mixed Case", false)),
            text_layer(5_012, "Fresh normal caps", normal),
        ],
        Vec::new(),
    );
    let output = to_aep(&document).unwrap();
    assert_eq!(occurrences(&output.bytes, b" /9 -2 /12 2 /53 "), 1);
    assert_eq!(occurrences(&output.bytes, b" /9 -2 /12 0 /53 "), 1);

    let texts = imported_texts(&read_project(&output.bytes).unwrap());
    let all_caps = |text: &str| {
        texts
            .iter()
            .find(|layer| layer.source_text.text == text)
            .unwrap_or_else(|| panic!("{text:?} was not reimported as editable Text"))
            .source_text
            .all_caps
    };
    assert!(all_caps("Mixed Case"));
    assert!(!all_caps("Normal case"));
}

/// The opaque COS payload of `layer`'s Source Text.
fn source_text_cos(layer: &crate::structure::Layer) -> &[u8] {
    fn find(chunks: &[crate::rifx::Chunk]) -> Option<&[u8]> {
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

/// Every font-set entry of the 76 public AE-authored Text fixtures is
/// `<< /0 << /99 /CoolTypeFont /0 << /0 (name) /2 n [/5 (version)] >> >> >>`,
/// and the reader takes the name from that depth. The former shallow
/// `<< /0 << /0 name >> >>` entry lost the font on reimport (`sans`/`serif`).
#[test]
fn fresh_font_entry_uses_the_native_cool_type_dictionary_and_reimports_its_name() {
    let bytes = include_bytes!(
        "../../../tests/fixtures/pr4442_native/sources/text_document_font_style.aep"
    );
    assert_eq!(bytes.len(), 94_035);
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "8ef0ba48c93f95bf4a50fdb1c73b4e3df172856b88535aa7909a084dd2dd1cba"
    );
    let source = read_project(bytes).unwrap();
    let [text] = layers(&source)
        .iter()
        .filter(|layer| layer.record.layer_type() == 3)
        .collect::<Vec<_>>()[..]
    else {
        panic!("one native Text layer");
    };
    let mut native_entry = b"<< /0 << /99 /CoolTypeFont /0 << /0 (\xFE\xFF".to_vec();
    native_entry.extend("Arial-BoldMT".encode_utf16().flat_map(u16::to_be_bytes));
    native_entry.extend(b") /2 1 /5 (");
    assert_eq!(occurrences(source_text_cos(text), &native_entry), 1);

    let imported = to_structural_fx_document(&source, Some(1)).unwrap();
    let output = to_aep(&imported.document).unwrap();
    let mut entry = b"[ << /0 << /99 /CoolTypeFont /0 << /0 ".to_vec();
    entry.extend(utf16_hex("Arial-BoldMT"));
    // The FX document holds no face format or version to write after it.
    entry.extend(b" >> >> >> ] >>");
    assert_eq!(
        occurrences(&output.bytes, &entry),
        1,
        "{:?}",
        output.diagnostics
    );

    let texts = imported_texts(&read_project(&output.bytes).unwrap());
    let fonts = texts
        .iter()
        .map(|text| {
            (
                text.source_text.font_family.as_ref(),
                text.source_text.font_style.as_ref(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(fonts, [("Arial", "BoldMT")]);
}

fn font_identities(texts: &[TextLayer]) -> Vec<(&str, &str)> {
    texts
        .iter()
        .map(|text| {
            (
                text.source_text.font_family.as_ref(),
                text.source_text.font_style.as_ref(),
            )
        })
        .collect()
}

/// A dash-less PostScript name has no style to split off. Import keeps it whole
/// with an empty style, so export writes the exact name instead of the guessed
/// `ArialMT-Regular`, and a reimport returns the same identity.
#[test]
fn dashless_postscript_font_round_trips_exactly_with_an_empty_style() {
    let bytes =
        include_bytes!("../../../tests/fixtures/pr4442_native/sources/text_document_point.aep");
    assert_eq!(bytes.len(), 93_997);
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "bc32cae3896c3aaf2e7a02f5283ce9ef31c9b0ef53f8c8a30a2b6c30b69eea34"
    );
    let source = read_project(bytes).unwrap();
    let [text] = layers(&source)
        .iter()
        .filter(|layer| layer.record.layer_type() == 3)
        .collect::<Vec<_>>()[..]
    else {
        panic!("one native Text layer");
    };
    let mut native_entry = b"<< /0 << /99 /CoolTypeFont /0 << /0 (\xFE\xFF".to_vec();
    native_entry.extend("ArialMT".encode_utf16().flat_map(u16::to_be_bytes));
    native_entry.extend(b") /2 1 /5 (");
    assert_eq!(occurrences(source_text_cos(text), &native_entry), 1);
    assert_eq!(font_identities(&imported_texts(&source)), [("ArialMT", "")]);

    let imported = to_structural_fx_document(&source, Some(1)).unwrap();
    let output = to_aep(&imported.document).unwrap();
    let mut entry = b"[ << /0 << /99 /CoolTypeFont /0 << /0 ".to_vec();
    entry.extend(utf16_hex("ArialMT"));
    entry.extend(b" >> >> >> ] >>");
    assert_eq!(occurrences(&output.bytes, &entry), 1);
    assert_eq!(occurrences(&output.bytes, &utf16_hex("ArialMT-Regular")), 0);
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("deterministic candidate")),
        "{:?}",
        output.diagnostics
    );
    let reimported = imported_texts(&read_project(&output.bytes).unwrap());
    assert_eq!(font_identities(&reimported), [("ArialMT", "")]);
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_hold_source_text_uses_signed_authored_time_union_and_deduplicated_fonts() {
    let id = LayerId::new(5_100);
    let layer = text_layer(
        id.value(),
        "Fresh Hold documents",
        source_text("base", false),
    );
    let entries = vec![
        layer_entry(
            id,
            PropType::TextContent,
            [
                (-500, PropertyValue::String("before".into())),
                (500, PropertyValue::String("after".into())),
            ],
            PropertyKeyframeEasing::Hold,
        ),
        layer_entry(
            id,
            PropType::FontFamily,
            [
                (-250, PropertyValue::String("Inter".into())),
                (750, PropertyValue::String("Roboto".into())),
            ],
            PropertyKeyframeEasing::Hold,
        ),
        layer_entry(
            id,
            PropType::FontStyle,
            [
                (-250, PropertyValue::String("Regular".into())),
                (750, PropertyValue::String("Bold".into())),
            ],
            PropertyKeyframeEasing::Hold,
        ),
        layer_entry(
            id,
            PropType::FontSize,
            [
                (0, PropertyValue::Float(42.0)),
                (1_000, PropertyValue::Float(64.0)),
            ],
            PropertyKeyframeEasing::Hold,
        ),
        constant_layer_entry(id, PropType::Tracking, PropertyValue::Float(18.0)),
        disabled_layer_entry(
            id,
            PropType::Leading,
            [
                (0, PropertyValue::Float(60.0)),
                (500, PropertyValue::Float(90.0)),
            ],
            PropertyValue::Float(54.0),
        ),
    ];
    let document = explicit_document(vec![layer], entries.clone());
    let LayerData::Text(text_layer) = document.composition().layers()[0].data() else {
        panic!("fresh Text input")
    };
    let lowered = text::lower(
        text_layer,
        &text_layer.transform,
        text_layer.id,
        &entries,
        None,
    )
    .unwrap();
    assert_eq!(
        lowered
            .spec
            .documents
            .keys
            .iter()
            .map(|key| key.time_millis)
            .collect::<Vec<_>>(),
        [-500, -250, 0, 500, 750, 1_000]
    );
    assert!(lowered.spec.documents.keyed);
    assert_eq!(lowered.spec.documents.keys[0].document.text, "before");
    assert_eq!(
        lowered.spec.documents.keys.last().unwrap().document.text,
        "after"
    );
    assert_eq!(lowered.spec.documents.keys[0].document.leading, Some(54.0));
    assert!(
        lowered
            .diagnostics
            .iter()
            .any(|message| message.contains("Disabled Leading"))
    );
    let fonts = lowered
        .spec
        .documents
        .keys
        .iter()
        .map(|key| key.document.font_postscript.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(fonts, BTreeSet::from(["Inter-Regular", "Roboto-Bold"]));

    let output = to_aep(&document).unwrap();
    assert_eq!(occurrences(&output.bytes, &utf16_hex("Inter-Regular")), 1);
    assert_eq!(occurrences(&output.bytes, &utf16_hex("Roboto-Bold")), 1);
    assert!(
        layers(&read_project(&output.bytes).unwrap())
            .iter()
            .all(|layer| { layer.record.layer_type() == 3 && layer.record.source_id() == 0 })
    );
}

fn all_channel_entries() -> Vec<AnimationGraphEntry> {
    let animator = FxItemId::new(5_201);
    let range = FxItemId::new(5_202);
    let wiggly = FxItemId::new(5_203);
    let more = FxItemId::new(5_204);
    let path = FxItemId::new(5_205);
    let mut entries = Vec::new();
    for (property, first, second) in [
        (
            "anchorPoint",
            PropertyValue::Vector2([1.0, 2.0]),
            PropertyValue::Vector2([3.0, 4.0]),
        ),
        (
            "position",
            PropertyValue::Vector2([5.0, 6.0]),
            PropertyValue::Vector2([7.0, 8.0]),
        ),
        (
            "scale",
            PropertyValue::Vector2([100.0, 100.0]),
            PropertyValue::Vector2([120.0, 80.0]),
        ),
        (
            "blur",
            PropertyValue::Vector2([0.0, 0.0]),
            PropertyValue::Vector2([4.0, 6.0]),
        ),
        (
            "fillColor",
            PropertyValue::Color([0.1, 0.2, 0.3, 1.0]),
            PropertyValue::Color([0.3, 0.4, 0.5, 1.0]),
        ),
        (
            "strokeColor",
            PropertyValue::Color([0.6, 0.5, 0.4, 1.0]),
            PropertyValue::Color([0.4, 0.3, 0.2, 1.0]),
        ),
    ] {
        entries.push(item_entry(
            animator,
            property,
            [(-250, first), (750, second)],
        ));
    }
    for (property, first, second) in [
        ("rotation", 0.0, 25.0),
        ("skew", 0.0, 12.0),
        ("skewAxis", 0.0, 45.0),
        ("tracking", 0.0, 30.0),
        ("strokeWidth", 1.0, 5.0),
        ("opacity", 100.0, 40.0),
        ("lineSpacing", 0.0, 20.0),
        ("lineAnchor", 0.0, 50.0),
        ("characterOffset", 0.0, 2.0),
        ("characterValue", 65.0, 90.0),
    ] {
        entries.push(item_entry(
            animator,
            property,
            [
                (-250, PropertyValue::Float(first)),
                (750, PropertyValue::Float(second)),
            ],
        ));
    }
    for (property, first, second) in [
        ("start", 0.1, 0.2),
        ("end", 0.9, 0.8),
        ("offset", 0.0, 0.25),
        ("amount", 1.0, 0.6),
        ("easeHigh", 0.0, 0.4),
        ("easeLow", 0.0, -0.3),
        ("randomSeed", 3.0, 7.0),
    ] {
        entries.push(item_entry(
            range,
            property,
            [
                (-250, PropertyValue::Float(first)),
                (750, PropertyValue::Float(second)),
            ],
        ));
    }
    for (property, first, second) in [
        ("speed", 2.0, 4.0),
        ("amount", 100.0, 55.0),
        ("seed", 1.0, 9.0),
    ] {
        entries.push(item_entry(
            wiggly,
            property,
            [
                (-250, PropertyValue::Float(first)),
                (750, PropertyValue::Float(second)),
            ],
        ));
    }
    entries.push(item_entry(
        more,
        "groupingAlignment",
        [
            (-250, PropertyValue::Vector2([0.0, 0.0])),
            (750, PropertyValue::Vector2([40.0, -30.0])),
        ],
    ));
    for (property, second) in [("firstMargin", 120.0), ("lastMargin", 80.0)] {
        entries.push(item_entry(
            path,
            property,
            [
                (-250, PropertyValue::Float(0.0)),
                (750, PropertyValue::Float(second)),
            ],
        ));
    }
    entries
}

fn all_channel_text() -> Value {
    let mut text = text_layer(
        5_200,
        "Fresh all Text controls",
        source_text("Editable", false),
    );
    text["animators"] = json!([{
        "id": 5201,
        "name": "All sixteen",
        "selectors": [{
            "id": 5202, "start": 0.1, "end": 0.9, "offset": 0.0,
            "units": "percentage", "basedOn": "words", "mode": "intersect",
            "amount": 1.0, "shape": "triangle", "easeHigh": 0.0,
            "easeLow": 0.0, "randomizeOrder": true, "randomSeed": 3.0
        }],
        "wigglySelectors": [{
            "id": 5203, "mode": "subtract", "speed": 2.0,
            "amount": 100.0, "seed": 1.0
        }],
        "anchorPoint": [1.0, 2.0], "position": [5.0, 6.0],
        "scale": [100.0, 100.0], "rotation": 0.0, "skew": 0.0,
        "skewAxis": 0.0, "tracking": 0.0, "strokeWidth": 1.0,
        "blur": [0.0, 0.0], "opacity": 100.0,
        "fillColor": [0.1, 0.2, 0.3, 1.0],
        "strokeColor": [0.6, 0.5, 0.4, 1.0],
        "lineSpacing": 0.0, "lineAnchor": 0.0,
        "characterOffset": 0.0, "characterValue": 65.0
    }]);
    text["anchorOptions"] = json!({
        "id": 5204, "anchorPointGrouping": "word",
        "groupingAlignment": [0.0, 0.0]
    });
    text["pathOptions"] = json!({
        "id": 5205, "pathLayer": 5299, "firstMargin": 0.0,
        "lastMargin": 0.0, "perpendicularToPath": true,
        "reversePath": true, "forceAlignment": true
    });
    text
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_all_animator_selector_more_and_path_channels_export_signed_native_keys() {
    let entries = all_channel_entries();
    assert_eq!(entries.len(), 29);
    let expected_entries = entries.clone();
    let document = explicit_document(vec![guide_layer(5_299), all_channel_text()], entries);
    let output = to_aep(&document).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(
        layers(&native).len(),
        1,
        "guide is consumed into Text mask: {:?}",
        output.diagnostics
    );
    assert_eq!(layers(&native)[0].record.layer_type(), 3);
    assert_eq!(layers(&native)[0].record.source_id(), 0);

    let converted = to_structural_fx_document(&native, Some(1)).unwrap();
    let mut texts = Vec::new();
    collect_text(converted.document.composition().layers(), &mut texts);
    let [text] = texts.as_slice() else {
        panic!("one fresh Text layer")
    };
    let animator = &text.animators[0];
    assert_eq!(text.animators.len(), 1);
    assert_eq!(animator.selectors.len(), 1);
    assert_eq!(animator.wiggly_selectors.len(), 1);
    // Fresh native import remints identities; compare controls independently of IDs.
    let mut animator_json = serde_json::to_value(animator).unwrap();
    animator_json["id"] = json!(5201);
    // Check the authored name separately, after all channel assertions run.
    animator_json["name"] = json!("All sixteen");
    animator_json["selectors"][0]["id"] = json!(5202);
    animator_json["wigglySelectors"][0]["id"] = json!(5203);
    assert_eq!(
        animator_json,
        json!({
            "id": 5201,
            "name": "All sixteen",
            "selectors": [{
                "id": 5202, "start": 0.1, "end": 0.9, "offset": 0.0,
                "units": "percentage", "basedOn": "words", "mode": "intersect",
                "amount": 1.0, "shape": "triangle", "easeHigh": 0.0,
                "easeLow": 0.0, "randomizeOrder": true, "randomSeed": 3.0
            }],
            "wigglySelectors": [{
                "id": 5203, "mode": "subtract", "speed": 2.0,
                "amount": 100.0, "seed": 1.0
            }],
            "anchorPoint": [1.0, 2.0], "position": [5.0, 6.0],
            "scale": [100.0, 100.0], "rotation": 0.0, "skew": 0.0,
            "skewAxis": 0.0, "tracking": 0.0, "strokeWidth": 1.0,
            "blur": [0.0, 0.0], "opacity": 100.0,
            "fillColor": [0.1, 0.2, 0.3, 1.0],
            "strokeColor": [0.6, 0.5, 0.4, 1.0],
            "lineSpacing": 0.0, "lineAnchor": 0.0,
            "characterOffset": 0.0, "characterValue": 65.0
        })
    );
    let more = text.anchor_options.as_ref().unwrap();
    let mut more_json = serde_json::to_value(more).unwrap();
    more_json["id"] = json!(5204);
    assert_eq!(
        more_json,
        json!({
            "id": 5204,
            "anchorPointGrouping": "word",
            "groupingAlignment": [0.0, 0.0]
        })
    );
    let path = text.path_options.as_ref().unwrap();
    let guide = find_layer_named(
        converted.document.composition().layers(),
        "Fresh all Text controls — Mask 1 guide",
    )
    .expect("fresh native import retains the named text path guide");
    assert_eq!(path.path_layer, guide.id());
    let mut path_json = serde_json::to_value(path).unwrap();
    path_json["id"] = json!(5205);
    path_json["pathLayer"] = json!(5299);
    assert_eq!(
        path_json,
        json!({
            "id": 5205, "pathLayer": 5299, "firstMargin": 0.0,
            "lastMargin": 0.0, "perpendicularToPath": true,
            "reversePath": true, "forceAlignment": true
        })
    );

    let imported_ids = [
        (FxItemId::new(5201), animator.id),
        (FxItemId::new(5202), animator.selectors[0].id),
        (FxItemId::new(5203), animator.wiggly_selectors[0].id),
        (FxItemId::new(5204), more.id),
        (FxItemId::new(5205), path.id),
    ];
    let graph = converted.document.composition().dynamics().entries();
    assert_eq!(graph.len(), expected_entries.len());
    for expected in &expected_entries {
        let PropertyTarget::FxItemProperty(expected_target) = &expected.target else {
            panic!("all-channel fixture uses item-property tracks")
        };
        let (_, imported_id) = imported_ids
            .iter()
            .find(|(authored, _)| *authored == expected_target.item_id())
            .expect("known authored item");
        let matches = graph
            .iter()
            .filter(|entry| {
                matches!(
                    &entry.target,
                    PropertyTarget::FxItemProperty(target)
                        if target.item_id() == *imported_id
                            && target.property_name() == expected_target.property_name()
                )
            })
            .collect::<Vec<_>>();
        let [actual] = matches.as_slice() else {
            panic!(
                "expected one fresh native Text track for item {} property {}, got {}",
                expected_target.item_id(),
                expected_target.property_name(),
                matches.len()
            )
        };
        let expected_keys = expected.animator.keyframe_track().unwrap().keyframes();
        let actual_keys = actual.animator.keyframe_track().unwrap().keyframes();
        assert_eq!(actual_keys.len(), expected_keys.len());
        for (actual, expected) in actual_keys.iter().zip(expected_keys) {
            assert_eq!(actual.layer_time(), expected.layer_time());
            assert_eq!(actual.value(), expected.value());
            assert_eq!(actual.easing(), expected.easing());
        }
    }
    assert!(!output.bytes.windows(8).any(|window| window == b"JsScript"));
    assert_eq!(animator.name, "All sixteen", "authored Animator name");
}

#[test]
#[ignore = "Adobe-native proof backlog; see docs/after-effects-support.md"]
fn pr4442_unsupported_text_layout_is_diagnosed_while_supported_sibling_exports() {
    let mut unsupported = text_layer(5_400, "Approximated text", source_text("kept", true));
    unsupported["sourceText"]["fontVariations"] = json!({"id": 5401, "axes": {"wght": 700.0}});
    unsupported["sourceText"]["underline"] = json!(true);
    unsupported["sourceText"]["strikethrough"] = json!(true);
    unsupported["sourceText"]["verticalAlign"] = json!("center");
    unsupported["sourceText"]["scaleBoxTextWithTransform"] = json!(true);
    let sibling = text_layer(
        5_402,
        "Supported text sibling",
        source_text("sibling", false),
    );
    let entries = vec![layer_entry(
        LayerId::new(5_400),
        PropType::TextContent,
        [
            (0, PropertyValue::String("first".into())),
            (500, PropertyValue::String("continuous".into())),
        ],
        PropertyKeyframeEasing::Linear,
    )];
    let document = explicit_document(vec![unsupported, sibling], entries);
    let output = to_aep(&document).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 2, "{:?}", output.diagnostics);
    let messages = output
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>();
    for expected in [
        "Variable-font axes",
        "Underline/strikethrough",
        "Box vertical alignment",
        "scaleBoxTextWithTransform",
        "continuous interpolation",
    ] {
        assert!(
            messages.iter().any(|message| message.contains(expected)),
            "{expected}: {messages:?}"
        );
    }
    let names = layers(&native)
        .iter()
        .map(|layer| layer.name.as_ref())
        .collect::<HashSet<_>>();
    assert!(names.contains("Approximated text"));
    assert!(names.contains("Supported text sibling"));
    assert!(
        layers(&native)
            .iter()
            .all(|layer| layer.record.source_id() == 0)
    );
    // Multi-style character/paragraph runs have no explicit FX input shape;
    // unlike the diagnosed fields above, they remain a missing export oracle
    // rather than being silently claimed by this structural case.
}

fn zero_transform() -> Value {
    let mut transform = identity_transform();
    transform["position"] = json!([0.0, 0.0]);
    transform
}

fn identity_playback(span: u64) -> Value {
    affine_playback([0, span], [0, span])
}

fn affine_playback(
    [input_start, input_end]: [u64; 2],
    [output_start, output_end]: [u64; 2],
) -> Value {
    let input = json!({"start": input_start, "duration": input_end - input_start});
    json!({
        "type": "windowed",
        "inputRange": input,
        "mapping": {
            "type": "linear",
            "input": input,
            "output": {"start": output_start, "duration": output_end - output_start}
        },
        "inputOffsetMs": 0
    })
}

fn linear_playback(keys: &[(u64, u64)]) -> Value {
    let start = keys.first().unwrap().0;
    let end = keys.last().unwrap().0;
    json!({
        "type": "windowed",
        "inputRange": {"start": start, "duration": end - start},
        "mapping": {"type": "timeRemap", "property": {
            "before": "inactive",
            "after": "inactive",
            "keyframes": keys
                .iter()
                .enumerate()
                .map(|(index, (time, value))| json!({
                    "id": format!("held-text-clock-{index}"),
                    "time": time,
                    "value": value,
                    "easing": {"type": "linear"}
                }))
                .collect::<Vec<_>>()
        }},
        "inputOffsetMs": 0
    })
}

/// The importer's shape for one AE text layer with held Source Text: a layer
/// Group (`id`) holding a "Source content clock" Group (`id + 1`) with
/// `playback`, holding one Text per held value.
fn held_text_layer(id: u64, name: &str, playback: Value, segments: &[(&str, u64, u64)]) -> Value {
    let clock = id + 1;
    let texts = segments
        .iter()
        .zip(clock + 1..)
        .map(|((text, start, duration), text_id)| {
            let mut layer =
                text_layer(text_id, &format!("{name} {text}"), source_text(text, false));
            layer["parent"] = json!(clock);
            layer["activeRange"] = json!({"start": start, "duration": duration});
            layer["transform"] = zero_transform();
            layer
        })
        .collect::<Vec<_>>();
    json!({
        "type": "Group",
        "id": id,
        "name": name,
        "parent": null,
        "playback": identity_playback(2000),
        "transform": identity_transform(),
        "layers": [{
            "type": "Group",
            "id": clock,
            "name": "Source content clock",
            "parent": id,
            "transform": zero_transform(),
            "playback": playback,
            "layers": texts
        }]
    })
}

fn two_second_document(layers: Vec<Value>) -> EditableFxCompositionDocument {
    let mut value = imported();
    value["duration"] = json!(2.0);
    value["composition"]["layers"] = Value::Array(layers);
    value["composition"]["dynamics"] = json!({"entries": []});
    EditableFxCompositionDocument::from_json_value(value).unwrap()
}

fn native_named<'a>(project: &'a StructuralProject, name: &str) -> &'a crate::structure::Layer {
    layers(project)
        .iter()
        .find(|layer| layer.name.as_ref() == name)
        .unwrap_or_else(|| panic!("native layer {name:?}"))
}

/// Comp-time `(start, end)` of one native AV layer, from its exact record
/// fields: `start + point * stretch`.
fn comp_interval(layer: &crate::structure::Layer) -> (f64, f64) {
    let record = &layer.record;
    let start = record.start_time().unwrap();
    let stretch = record.stretch().unwrap();
    (
        start + record.in_point().unwrap() * stretch,
        start + record.out_point().unwrap() * stretch,
    )
}

/// An identity-clock AE text layer holds several held segments. Its single
/// child is a text-only branch, so Null parenting keeps each segment as an
/// editable native Text layer instead of omitting the subtree for want of
/// glyph bounds.
#[test]
fn identity_wrapper_of_held_text_segments_exports_each_segment_under_null_parents() {
    let document = two_second_document(vec![held_text_layer(
        5_600,
        "Held",
        linear_playback(&[(0, 0), (2_000, 2_000)]),
        &[("0%", 0, 1_500), ("100%", 1_500, 999_999_998_500)],
    )]);
    let output = to_aep(&document).unwrap();
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("subtree omitted")),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let texts = layers(&native)
        .iter()
        .filter(|layer| layer.record.layer_type() == 3)
        .map(|layer| (layer.name.as_ref(), comp_interval(layer)))
        .collect::<Vec<_>>();
    assert_eq!(texts, [("Held 0%", (0.0, 1.5)), ("Held 100%", (1.5, 2.0))]);
    for text in ["0%\r", "100%\r"] {
        assert_eq!(occurrences(&output.bytes, &utf16_hex(text)), 1, "{text:?}");
    }
    let clock = native_named(&native, "Source content clock");
    let wrapper = native_named(&native, "Held");
    assert!(clock.record.flags().null_layer && wrapper.record.flags().null_layer);
    assert_eq!(clock.record.parent_id(), wrapper.record.id());
    for text in ["Held 0%", "Held 100%"] {
        assert_eq!(
            native_named(&native, text).record.parent_id(),
            clock.record.id()
        );
    }
}

/// Null parenting stays limited by its existing guards: a Null's blend mode,
/// opacity and motion blur do not reach its children, so such a text-only
/// wrapper still needs a precomposition and remains a diagnosed omission.
#[test]
fn text_only_wrapper_keeps_the_null_parent_guards() {
    for (field, value) in [
        ("blendMode", json!("multiply")),
        ("motionBlur", json!(true)),
        ("opacity", json!(50.0)),
    ] {
        let mut wrapper = held_text_layer(
            5_700,
            "Guarded",
            linear_playback(&[(0, 0), (2_000, 2_000)]),
            &[("0%", 0, 1_500), ("100%", 1_500, 999_999_998_500)],
        );
        if field == "opacity" {
            wrapper["transform"]["opacity"] = value;
        } else {
            wrapper[field] = value;
        }
        let output = to_aep(&two_second_document(vec![wrapper])).unwrap();
        assert!(
            output.diagnostics.iter().any(|diagnostic| {
                diagnostic.layer_id == Some(LayerId::new(5_700))
                    && diagnostic.message.contains("glyph bounds are not known")
            }),
            "{field}: {:?}",
            output.diagnostics
        );
        let native = read_project(&output.bytes).unwrap();
        assert!(
            layers(&native)
                .iter()
                .all(|layer| layer.record.layer_type() != 3),
            "{field}"
        );
    }
}

/// One occurrence of a nested composition that holds only a held-text layer,
/// in the importer's shape: a full-span Group (`id`) holding its source-clock
/// Group (`id + 1`), whose two linear keys on its active interval map the
/// parent to the 2 s source. The text layer takes `id + 10` onwards.
fn text_occurrence(id: u64, name: &str, clock: [(u64, u64); 2]) -> Value {
    let mut text = held_text_layer(
        id + 10,
        &format!("{name} text"),
        linear_playback(&[(0, 0), (2_000, 2_000)]),
        &[("0%", 0, 1_500), ("100%", 1_500, 999_999_998_500)],
    );
    text["parent"] = json!(id + 1);
    json!({
        "type": "Group",
        "id": id,
        "name": name,
        "parent": null,
        "playback": identity_playback(2000),
        "transform": identity_transform(),
        "layers": [{
            "type": "Group",
            "id": id + 1,
            "name": format!("{name} clock"),
            "parent": id,
            "transform": zero_transform(),
            "playback": linear_playback(&clock),
            "layers": [text]
        }]
    })
}

fn composition_layers(project: &StructuralProject, id: u32) -> &[crate::structure::Layer] {
    let ItemKind::Composition(composition) = &project.item(id).unwrap().kind else {
        panic!("composition {id}")
    };
    &composition.layers
}

/// Text must precompose under an offset or negative-start source clock, but
/// has no glyph bounds to size a source canvas. The precomposition uses native
/// collapse transformations on the root canvas; both occurrences keep their
/// exact clocks and every held segment. Adobe rendering is unverified.
#[test]
fn clocked_text_occurrences_export_as_collapsed_precompositions_with_exact_clocks() {
    let document = two_second_document(vec![
        text_occurrence(5_800, "Late", [(500, 0), (2_000, 1_500)]),
        text_occurrence(5_900, "Early", [(0, 500), (1_500, 2_000)]),
    ]);
    let dimensions = document.to_json_value().unwrap()["dimensions"].clone();
    let output = to_aep(&document).unwrap();
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("subtree omitted")),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    for (name, id, clock) in [
        ("Late", 5_801, [(1, 2), (0, 1), (3, 2), (1, 1)]),
        ("Early", 5_901, [(-1, 2), (1, 2), (2, 1), (1, 1)]),
    ] {
        assert!(output.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(id))
                && diagnostic.message.contains("no FX glyph bounds")
        }));
        let occurrence = native_named(&native, &format!("{name} clock"));
        assert_eq!(
            occurrence.record.parent_id(),
            native_named(&native, name).record.id()
        );
        assert!(occurrence.record.flags().collapse_transformation, "{name}");
        let record = &occurrence.record;
        assert_eq!(
            [
                record.start_time_fraction(),
                record.in_point_fraction(),
                record.out_point_fraction(),
                record.stretch_fraction(),
            ],
            clock,
            "{name}"
        );
        let ItemKind::Composition(source) = &native.item(record.source_id()).unwrap().kind else {
            panic!("{name}: occurrence source");
        };
        assert_eq!(
            json!({"width": source.width, "height": source.height}),
            dimensions,
            "{name}: nominal root canvas"
        );
        let held = composition_layers(&native, record.source_id())
            .iter()
            .filter(|layer| layer.record.layer_type() == 3)
            .map(|layer| (layer.name.as_ref().to_owned(), comp_interval(layer)))
            .collect::<Vec<_>>();
        assert_eq!(
            held,
            [
                (format!("{name} text 0%"), (0.0, 1.5)),
                (format!("{name} text 100%"), (1.5, 2.0)),
            ]
        );
    }
}

/// Collapse admits plain 2D Text only. Mixed content keeps the vector rules,
/// and occurrence masks, child blend modes, motion blur or 3D would rasterize
/// or re-blend the collapsed layer, so each stays a diagnosed omission.
#[test]
fn clocked_text_collapse_rejects_mixed_masked_blended_blurred_or_3d_content() {
    let base = || text_occurrence(5_800, "Late", [(500, 0), (2_000, 1_500)]);
    let mut mixed = base();
    let mut vector = rect(&imported(), 5_890);
    vector["parent"] = json!(5_801);
    mixed["layers"][0]["layers"]
        .as_array_mut()
        .unwrap()
        .push(vector);
    let mut masked = base();
    masked["layers"][0]["masks"] = json!([{
        "id": 5_891, "mode": "add", "opacity": 100,
        "path": {"commands": [
            {"type": "moveTo", "x": 0, "y": 0},
            {"type": "lineTo", "x": 100, "y": 0},
            {"type": "lineTo", "x": 100, "y": 100},
            {"type": "close"}
        ]}
    }]);
    let segment = |edit: &dyn Fn(&mut Value)| {
        let mut value = base();
        edit(&mut value["layers"][0]["layers"][0]["layers"][0]["layers"][0]);
        value
    };
    for (case, occurrence) in [
        ("mixed Text and vector", mixed),
        ("occurrence mask", masked),
        (
            "child blend mode",
            segment(&|text| text["blendMode"] = json!("multiply")),
        ),
        (
            "child motion blur",
            segment(&|text| text["motionBlur"] = json!(true)),
        ),
        (
            "child 3D rotation",
            segment(&|text| {
                text["transform"]["rotationX"] = json!(20.0);
            }),
        ),
    ] {
        let output = to_aep(&two_second_document(vec![occurrence])).unwrap();
        // 3D already rules out the occurrence's own Null parent.
        assert!(
            output.diagnostics.iter().any(|diagnostic| {
                [Some(LayerId::new(5_800)), Some(LayerId::new(5_801))]
                    .contains(&diagnostic.layer_id)
                    && diagnostic.message.contains("subtree omitted")
            }),
            "{case}: {:?}",
            output.diagnostics
        );
        let native = read_project(&output.bytes).unwrap();
        assert!(
            native.items.iter().all(|item| match &item.kind {
                ItemKind::Composition(composition) => composition
                    .layers
                    .iter()
                    .all(|layer| layer.record.layer_type() != 3),
                _ => true,
            }),
            "{case}"
        );
    }
}

/// A document on its own canvas and duration.
fn canvas_document(
    [width, height]: [u32; 2],
    duration_millis: u64,
    layers: Vec<Value>,
) -> EditableFxCompositionDocument {
    let mut value = imported();
    value["dimensions"] = json!({"width": width, "height": height});
    value["duration"] = json!(duration_millis as f64 / 1_000.0);
    value["composition"]["layers"] = Value::Array(layers);
    value["composition"]["dynamics"] = json!({"entries": []});
    EditableFxCompositionDocument::from_json_value(value).unwrap()
}

/// [`held_text_layer`] over a `span`-millisecond source whose "Source content
/// clock" keeps the importer's explicit identity keys.
fn held_text_source(id: u64, name: &str, span: u64, segments: &[(&str, u64, u64)]) -> Value {
    let mut layer = held_text_layer(id, name, linear_playback(&[(0, 0), (span, span)]), segments);
    layer["playback"] = identity_playback(span);
    layer
}

/// A wrapper Group (`id`) over `span` holding one occurrence Group (`id + 1`)
/// that maps its `[start, end)` parent interval through `playback` into
/// `content`.
fn clocked_occurrence(
    id: u64,
    name: &str,
    span: u64,
    [start, end]: [u64; 2],
    playback: Value,
    mut content: Value,
) -> Value {
    content["parent"] = json!(id + 1);
    assert_eq!(
        playback["inputRange"],
        json!({"start": start, "duration": end - start})
    );
    json!({
        "type": "Group",
        "id": id,
        "name": name,
        "parent": null,
        "playback": identity_playback(span),
        "transform": identity_transform(),
        "layers": [{
            "type": "Group",
            "id": id + 1,
            "name": format!("{name} clock"),
            "parent": id,
            "transform": zero_transform(),
            "playback": playback,
            "layers": [content]
        }]
    })
}

/// A hidden plain Group (`id`) holding one Text per segment from `id + 1`. A
/// hidden Group cannot use a Null parent, so its Text must precompose.
fn hidden_text_owner(
    id: u64,
    name: &str,
    playback: &Value,
    segments: &[(&str, u64, u64)],
) -> Value {
    let texts = segments
        .iter()
        .zip(id + 1..)
        .map(|((text, start, duration), text_id)| {
            let mut layer =
                text_layer(text_id, &format!("{name} {text}"), source_text(text, false));
            layer["parent"] = json!(id);
            layer["activeRange"] = json!({"start": start, "duration": duration});
            layer["transform"] = zero_transform();
            layer
        })
        .collect::<Vec<_>>();
    json!({
        "type": "Group",
        "id": id,
        "name": name,
        "parent": null,
        "isHidden": true,
        "playback": playback,
        "transform": identity_transform(),
        "layers": texts
    })
}

/// The collapsed occurrence named `name` in `composition` and its source
/// composition, whose nominal size must be the root `canvas`.
fn collapsed_occurrence<'a>(
    project: &'a StructuralProject,
    composition: u32,
    name: &str,
    canvas: [u32; 2],
) -> (&'a crate::structure::Layer, u32) {
    let occurrence = composition_layers(project, composition)
        .iter()
        .find(|layer| layer.name.as_ref() == name)
        .unwrap_or_else(|| panic!("native occurrence {name:?} in composition {composition}"));
    let record = &occurrence.record;
    assert!(record.flags().collapse_transformation, "{name}");
    let ItemKind::Composition(source) = &project.item(record.source_id()).unwrap().kind else {
        panic!("{name}: occurrence source");
    };
    assert_eq!(
        [u32::from(source.width), u32::from(source.height)],
        canvas,
        "{name}: nominal root canvas"
    );
    (occurrence, record.source_id())
}

/// Exact native record clock `[start, in, out, stretch]`.
fn record_clock(layer: &crate::structure::Layer) -> [(i32, u32); 4] {
    let record = &layer.record;
    [
        record.start_time_fraction(),
        record.in_point_fraction(),
        record.out_point_fraction(),
        record.stretch_fraction(),
    ]
}

/// Native Text layers of `composition` as `(name, comp-time interval)`.
fn held_texts(project: &StructuralProject, composition: u32) -> Vec<(String, (f64, f64))> {
    composition_layers(project, composition)
        .iter()
        .filter(|layer| layer.record.layer_type() == 3)
        .map(|layer| (layer.name.as_ref().to_owned(), comp_interval(layer)))
        .collect()
}

/// The authored source intervals of the held `segments` of `name`.
fn authored_intervals(name: &str, segments: &[(&str, u64, u64)]) -> Vec<(String, (f64, f64))> {
    segments
        .iter()
        .map(|&(text, start, duration)| {
            (
                format!("{name} {text}"),
                (start as f64 / 1_000.0, (start + duration) as f64 / 1_000.0),
            )
        })
        .collect()
}

fn omitted_subtree(output: &ExportedDocument, id: u64) -> bool {
    output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(id))
            && diagnostic.message.contains("subtree omitted")
    })
}

/// Every occurrence clock the writer represents exactly keeps a plain-Text
/// occurrence collapsed. Each expected record follows by hand from AE's
/// `parent = start + source * stretch` for the authored FX map, and each held
/// value keeps its authored source interval. Text boundaries are multiples of
/// 125 ms, which native 1/24576 s ticks store exactly.
#[test]
fn clocked_text_collapse_keeps_each_supported_occurrence_clock_exact() {
    struct Case {
        canvas: [u32; 2],
        duration: u64,
        id: u64,
        name: &'static str,
        active: [u64; 2],
        playback: Value,
        span: u64,
        segments: &'static [(&'static str, u64, u64)],
        clock: [(i32, u32); 4],
    }
    let cases = [
        // Offset: 750 -> 0 and 3000 -> 2250, so stretch 1 and start 0.75 s.
        Case {
            canvas: [1280, 720],
            duration: 3_000,
            id: 6_100,
            name: "Countdown",
            active: [750, 3_000],
            playback: linear_playback(&[(750, 0), (3_000, 2_250)]),
            span: 2_250,
            segments: &[("Three", 0, 750), ("Two", 750, 750), ("One", 1_500, 750)],
            clock: [(3, 4), (0, 1), (9, 4), (1, 1)],
        },
        // Negative start with a trimmed head: 0 -> 600 and 1400 -> 2000, so
        // stretch 1 and start 0 - 0.6 = -0.6 s.
        Case {
            canvas: [1080, 1920],
            duration: 2_000,
            id: 6_200,
            name: "Ticker",
            active: [0, 1_400],
            playback: linear_playback(&[(0, 600), (1_400, 2_000)]),
            span: 2_000,
            segments: &[("Alpha", 0, 875), ("Beta", 875, 1_125)],
            clock: [(-3, 5), (3, 5), (2, 1), (1, 1)],
        },
        // Trimmed at both ends: 200 -> 450 and 1700 -> 1950, so stretch 1 and
        // start 0.2 - 0.45 = -0.25 s.
        Case {
            canvas: [640, 360],
            duration: 2_500,
            id: 6_300,
            name: "Lower third",
            active: [200, 1_700],
            playback: linear_playback(&[(200, 450), (1_700, 1_950)]),
            span: 2_500,
            segments: &[("first line", 0, 1_250), ("second line", 1_250, 1_250)],
            clock: [(-1, 4), (9, 20), (39, 20), (1, 1)],
        },
        // Slow keys: 0 -> 0 and 2000 -> 800, so stretch 2000/800 = 5/2.
        Case {
            canvas: [1920, 1080],
            duration: 2_000,
            id: 6_400,
            name: "Slow title",
            active: [0, 2_000],
            playback: linear_playback(&[(0, 0), (2_000, 800)]),
            span: 2_000,
            segments: &[("Dawn", 0, 375), ("Dusk", 375, 1_625)],
            clock: [(0, 1), (0, 1), (4, 5), (5, 2)],
        },
        // Fast keys: 500 -> 0 and 1500 -> 2000, so stretch 1000/2000 = 1/2 and
        // start 0.5 s.
        Case {
            canvas: [720, 720],
            duration: 2_000,
            id: 6_500,
            name: "Fast badge",
            active: [500, 1_500],
            playback: linear_playback(&[(500, 0), (1_500, 2_000)]),
            span: 2_000,
            segments: &[("left", 0, 1_000), ("right", 1_000, 1_000)],
            clock: [(1, 2), (0, 1), (2, 1), (1, 2)],
        },
        // Half rate from 400 ms: 1600 ms of parent time reaches source 800 ms,
        // so stretch 1600/800 = 2 and start 0.4 s.
        Case {
            canvas: [800, 600],
            duration: 2_000,
            id: 6_600,
            name: "Half rate",
            active: [400, 2_000],
            playback: affine_playback([400, 2_000], [0, 800]),
            span: 1_625,
            segments: &[("half one", 0, 500), ("half two", 500, 1_125)],
            clock: [(2, 5), (0, 1), (4, 5), (2, 1)],
        },
        // Double rate from 0: 900 ms of parent time reaches source 1800 ms, so
        // stretch 900/1800 = 1/2.
        Case {
            canvas: [1024, 768],
            duration: 2_000,
            id: 6_700,
            name: "Double rate",
            active: [0, 900],
            playback: affine_playback([0, 900], [0, 1_800]),
            span: 1_875,
            segments: &[("double one", 0, 1_000), ("double two", 1_000, 875)],
            clock: [(0, 1), (0, 1), (9, 5), (1, 2)],
        },
    ];
    for case in cases {
        let text = format!("{} text", case.name);
        let document = canvas_document(
            case.canvas,
            case.duration,
            vec![clocked_occurrence(
                case.id,
                case.name,
                case.duration,
                case.active,
                case.playback,
                held_text_source(case.id + 10, &text, case.span, case.segments),
            )],
        );
        let output = to_aep(&document).unwrap();
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("subtree omitted")),
            "{}: {:?}",
            case.name,
            output.diagnostics
        );
        let native = read_project(&output.bytes).unwrap();
        let (occurrence, source) =
            collapsed_occurrence(&native, 1, &format!("{} clock", case.name), case.canvas);
        assert_eq!(record_clock(occurrence), case.clock, "{}", case.name);
        assert_eq!(
            held_texts(&native, source),
            authored_intervals(&text, case.segments),
            "{}",
            case.name
        );
        for (value, ..) in case.segments {
            assert_eq!(
                occurrences(&output.bytes, &utf16_hex(&format!("{value}\r"))),
                1,
                "{}: {value:?}",
                case.name
            );
        }
    }
}

/// A clocked Text occurrence inside another keeps both clocks: each clocked
/// Group is classified on its own, so each collapses with its own record.
/// Outer: 1000 -> 0 and 3000 -> 2000, so start 1 s. Inner, in the outer
/// source: 0 -> 500 and 1000 -> 1500, so start -0.5 s. Both have stretch 1.
#[test]
fn nested_clocked_text_occurrences_each_keep_their_own_exact_clock() {
    const SEGMENTS: &[(&str, u64, u64)] = &[("Inner first", 0, 625), ("Inner second", 625, 875)];
    let inner = clocked_occurrence(
        6_850,
        "Inner",
        2_000,
        [0, 1_000],
        linear_playback(&[(0, 500), (1_000, 1_500)]),
        held_text_source(6_860, "Inner text", 1_500, SEGMENTS),
    );
    let document = canvas_document(
        [1600, 900],
        3_000,
        vec![clocked_occurrence(
            6_800,
            "Outer",
            3_000,
            [1_000, 3_000],
            linear_playback(&[(1_000, 0), (3_000, 2_000)]),
            inner,
        )],
    );
    let output = to_aep(&document).unwrap();
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("subtree omitted")),
        "{:?}",
        output.diagnostics
    );
    let native = read_project(&output.bytes).unwrap();
    let (outer, outer_source) = collapsed_occurrence(&native, 1, "Outer clock", [1600, 900]);
    assert_eq!(record_clock(outer), [(1, 1), (0, 1), (2, 1), (1, 1)]);
    let (inner, inner_source) =
        collapsed_occurrence(&native, outer_source, "Inner clock", [1600, 900]);
    assert_eq!(record_clock(inner), [(-1, 2), (1, 2), (3, 2), (1, 1)]);
    assert!(held_texts(&native, outer_source).is_empty());
    assert_eq!(
        held_texts(&native, inner_source),
        authored_intervals("Inner text", SEGMENTS)
    );
}

/// Hidden plain-Text owners, at the root and inside a 0.5 s-late occurrence
/// source, both carrying `clocks`. Each must precompose and collapse with an
/// identity record, keeping its held values.
fn assert_hidden_text_owners_collapse(clocks: [Value; 2]) {
    const ROOT: &[(&str, u64, u64)] = &[("Standby", 0, 1_250), ("On air", 1_250, 1_250)];
    const NESTED: &[(&str, u64, u64)] = &[("Relay open", 0, 500), ("Relay closed", 500, 1_500)];
    let [root_clock, nested_clock] = clocks;
    let mut relay = clocked_occurrence(
        7_200,
        "Relay",
        2_500,
        [500, 2_500],
        linear_playback(&[(500, 0), (2_500, 2_000)]),
        hidden_text_owner(7_210, "Relay monitor", &nested_clock, NESTED),
    );
    // Hidden-only content has no render bounds, so the clocked source also
    // holds a visible caption.
    let mut caption = text_layer(7_220, "Relay caption", source_text("Relay caption", false));
    caption["parent"] = json!(7_201);
    caption["activeRange"] = json!({"start": 0, "duration": 2_000});
    caption["transform"] = zero_transform();
    relay["layers"][0]["layers"]
        .as_array_mut()
        .unwrap()
        .push(caption);
    let document = canvas_document(
        [1366, 768],
        2_500,
        vec![hidden_text_owner(7_100, "Studio", &root_clock, ROOT), relay],
    );
    let output = to_aep(&document).unwrap();
    let omitted = [7_100, 7_201, 7_210]
        .into_iter()
        .filter(|&id| omitted_subtree(&output, id))
        .collect::<Vec<_>>();
    assert!(omitted.is_empty(), "{omitted:?}: {:?}", output.diagnostics);
    let native = read_project(&output.bytes).unwrap();
    let (studio, studio_source) = collapsed_occurrence(&native, 1, "Studio", [1366, 768]);
    assert!(!studio.record.flags().enabled, "hidden root owner");
    assert_eq!(comp_interval(studio), (0.0, 2.5));
    assert_eq!(
        held_texts(&native, studio_source),
        authored_intervals("Studio", ROOT)
    );
    let (relay, relay_source) = collapsed_occurrence(&native, 1, "Relay clock", [1366, 768]);
    assert_eq!(record_clock(relay), [(1, 2), (0, 1), (2, 1), (1, 1)]);
    assert_eq!(
        held_texts(&native, relay_source),
        [("Relay caption".to_owned(), (0.0, 2.0))]
    );
    let (monitor, monitor_source) =
        collapsed_occurrence(&native, relay_source, "Relay monitor", [1366, 768]);
    assert!(!monitor.record.flags().enabled, "hidden nested owner");
    assert_eq!(comp_interval(monitor), (0.0, 2.0));
    assert_eq!(
        held_texts(&native, monitor_source),
        authored_intervals("Relay monitor", NESTED)
    );
}

#[test]
fn hidden_text_owner_with_canonical_identity_collapses_at_the_root_and_in_a_clocked_source() {
    assert_hidden_text_owners_collapse([identity_playback(2_500), identity_playback(2_000)]);
}

/// Text collapse uses the owner clock its export caller already validated,
/// independently of the mapping domain's endpoints. Both identity mappings
/// cover more than each owner's visible window.
#[test]
fn hidden_text_owner_with_a_wider_identity_mapping_collapses_like_canonical_identity() {
    let mut root = identity_playback(5_000);
    root["inputRange"]["duration"] = json!(2_500);
    let mut nested = identity_playback(4_000);
    nested["inputRange"]["duration"] = json!(2_000);
    assert_hidden_text_owners_collapse([root, nested]);
}

/// Identity keys over each owner's whole span keep the same native record as
/// the canonical linear identity mapping.
#[test]
fn hidden_text_owner_with_explicit_identity_keys_collapses_like_canonical_identity() {
    assert_hidden_text_owners_collapse([
        linear_playback(&[(0, 0), (2_500, 2_500)]),
        linear_playback(&[(0, 0), (2_000, 2_000)]),
    ]);
}

/// Occurrence effects and masks would rasterize a collapsed Text layer, so
/// those clocked occurrences stay diagnosed omissions while their supported
/// sibling keeps its exact clock and held value.
#[test]
fn clocked_text_exclusions_keep_a_supported_sibling_collapsed() {
    const KEPT: &[(&str, u64, u64)] = &[("kept value", 0, 1_750)];
    // 250 -> 0 and 2000 -> 1750, so stretch 1 and start 0.25 s.
    let occurrence = |id: u64, name: &str, value: &'static str| {
        clocked_occurrence(
            id,
            name,
            2_000,
            [250, 2_000],
            linear_playback(&[(250, 0), (2_000, 1_750)]),
            held_text_source(
                id + 10,
                &format!("{name} text"),
                1_750,
                &[(value, 0, 1_750)],
            ),
        )
    };
    let mut exposed = occurrence(6_920, "Exposed", "exposed value");
    exposed["layers"][0]["effects"] = json!([{"type": "exposure", "exposure": 1.0}]);
    let mut masked = occurrence(6_940, "Masked", "masked value");
    masked["layers"][0]["masks"] = json!([{
        "id": 6_959, "mode": "add", "opacity": 100,
        "path": {"commands": [
            {"type": "moveTo", "x": 0, "y": 0},
            {"type": "lineTo", "x": 200, "y": 0},
            {"type": "lineTo", "x": 200, "y": 100},
            {"type": "close"}
        ]}
    }]);
    let document = canvas_document(
        [960, 540],
        2_000,
        vec![exposed, occurrence(6_900, "Kept", "kept value"), masked],
    );
    let output = to_aep(&document).unwrap();
    assert!(omitted_subtree(&output, 6_921), "{:?}", output.diagnostics);
    assert!(
        output.diagnostics.iter().any(|diagnostic| {
            diagnostic.layer_id == Some(LayerId::new(6_941))
                && diagnostic.message.contains(
                    "Collapsed Text source requires a 2D occurrence without masks or matte consumers",
                )
        }),
        "{:?}",
        output.diagnostics
    );
    assert!(!omitted_subtree(&output, 6_901), "{:?}", output.diagnostics);
    let native = read_project(&output.bytes).unwrap();
    let (kept, source) = collapsed_occurrence(&native, 1, "Kept clock", [960, 540]);
    assert_eq!(record_clock(kept), [(1, 4), (0, 1), (7, 4), (1, 1)]);
    assert_eq!(
        held_texts(&native, source),
        authored_intervals("Kept text", KEPT)
    );
    for value in ["exposed value\r", "masked value\r"] {
        assert_eq!(
            occurrences(&output.bytes, &utf16_hex(value)),
            0,
            "{value:?}"
        );
    }
}
