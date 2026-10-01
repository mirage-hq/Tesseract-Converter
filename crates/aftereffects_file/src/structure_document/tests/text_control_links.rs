// Supplementary storage mutation of a pinned AE-authored text layer.
//
// The base layer, its keyed linear 0→100 curve (0.5s→2.0s) and its Position
// storage are native records of `text/import_selector_animation.aep`
// composition 79. The Slider envelopes, expressions and every name are
// synthetic. Expected values are derived by hand from those controls, not from
// importer output. This is not independent Adobe evidence.
use super::*;

use fx_schema::TextLayer;
use fx_schema::text_animator::SelectorShape;

use crate::{
    properties::{runs, unique_list},
    rifx::Chunk,
};

const SOURCE: &[u8] = include_bytes!("../../../tests/fixtures/text/import_selector_animation.aep");
const SOURCE_SHA256: &str = "a5765c7fdbc4d6d323d40323ede6af41ee8d6a3d7dd9e1262d06d07ef242ae66";
const COMPOSITION: u32 = 79;

const START: &str = "100-effect(\"Span\")(1);";
const OFFSET: &str = "var p = effect(\"Progress\")(1);\r\nvar g = 100-effect(\"Span\")(1);\r\n\r\nlinear(p,0,100,-100,100-g);";
const SWEEP_OFFSET: &str = "Text Animator 1 Range Selector 1 ADBE Text Percent Offset";
const FOLLOW_OFFSET: &str = "Text Animator 2 Range Selector 1 ADBE Text Percent Offset";

fn data(id: &[u8; 4], value: impl Into<Vec<u8>>) -> Chunk {
    let mut value = value.into();
    if id == b"tdmn" {
        value.resize(40, 0);
    }
    Chunk::data(*id, value).unwrap()
}

fn display_name(value: &str) -> Chunk {
    let mut bytes = b"Utf8".to_vec();
    bytes.extend(u32::try_from(value.len()).unwrap().to_be_bytes());
    bytes.extend(value.as_bytes());
    data(b"tdsn", bytes)
}

/// Static storage; an expression chunk without the disable flag enables it.
fn stored(values: &[f64], expression: Option<&str>) -> Chunk {
    let mut meta = vec![0; 124];
    meta[..2].copy_from_slice(&[0xdb, 0x99]);
    meta[3] = u8::try_from(values.len()).unwrap();
    let mut chunks = vec![
        data(b"tdb4", meta),
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

fn with_expression(storage: &[Chunk], expression: &str) -> Chunk {
    let mut chunks = storage.to_vec();
    chunks.push(data(b"Utf8", expression));
    Chunk::list(*b"tdbs", chunks)
}

/// Native keyed storage with each 48-byte scalar key item edited in place.
fn edited_keys(storage: &[Chunk], edit: impl Fn(usize, &mut [u8])) -> Chunk {
    let mut chunks = storage.to_vec();
    let keys = chunks
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"list"))
        .and_then(Chunk::children_mut)
        .unwrap();
    let items = keys
        .iter_mut()
        .find(|chunk| chunk.id() == *b"ldat")
        .unwrap();
    let mut bytes = items.data_payload().unwrap().to_vec();
    for (index, item) in bytes.chunks_exact_mut(48).enumerate() {
        edit(index, item);
    }
    *items = Chunk::data(*b"ldat", bytes).unwrap();
    Chunk::list(*b"tdbs", chunks)
}

fn leaf(name: &str, storage: Chunk) -> Vec<Chunk> {
    vec![data(b"tdmn", name), storage]
}

fn group(name: &str, label: Option<&str>, children: Vec<Chunk>) -> Vec<Chunk> {
    let mut body: Vec<Chunk> = label.map(display_name).into_iter().collect();
    body.extend(children);
    vec![data(b"tdmn", name), Chunk::list(*b"tdgp", body)]
}

fn effect(kind: &str, label: &str, value: Chunk) -> Vec<Chunk> {
    let parameter = |index: u8| format!("{kind}-000{index}");
    vec![
        data(b"tdmn", kind),
        Chunk::list(
            *b"sspc",
            vec![Chunk::list(
                *b"tdgp",
                [
                    vec![display_name(label)],
                    leaf(&parameter(0), stored(&[0.0], None)),
                    leaf(&parameter(1), value),
                ]
                .concat(),
            )],
        ),
    ]
}

fn slider(label: &str, value: Chunk) -> Vec<Chunk> {
    effect("ADBE Slider Control", label, value)
}

fn root_children(layer: &Layer) -> &[Chunk] {
    layer
        .content
        .iter()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .and_then(Chunk::children)
        .unwrap()
}

fn named<'a>(children: &'a [Chunk], name: &str) -> &'a [Chunk] {
    let mut matches = runs(children)
        .unwrap()
        .into_iter()
        .filter(|(candidate, _)| *candidate == name);
    let (_, run) = matches.next().unwrap_or_else(|| panic!("missing {name}"));
    assert!(matches.next().is_none(), "duplicate {name}");
    run
}

/// Native `tdbs` storage of one leaf below the layer's property root.
fn native_storage(layer: &Layer, groups: &[&str], leaf: &str) -> Vec<Chunk> {
    let mut children = root_children(layer);
    for name in groups {
        children = unique_list(named(children, name), *b"tdgp").unwrap();
    }
    unique_list(named(children, leaf), *b"tdbs")
        .unwrap()
        .to_vec()
}

fn group_children_mut<'a>(children: &'a mut [Chunk], name: &str) -> &'a mut Vec<Chunk> {
    let index = children
        .iter()
        .position(|chunk| {
            chunk.id() == *b"tdmn"
                && chunk.data_payload().is_some_and(|bytes| {
                    bytes.split(|byte| *byte == 0).next() == Some(name.as_bytes())
                })
        })
        .unwrap_or_else(|| panic!("missing {name}"));
    let group = &mut children[index + 1];
    assert_eq!(group.list_kind(), Some(*b"tdgp"), "{name} group");
    group.children_mut().unwrap()
}

const NATIVE_ANIMATOR: [&str; 3] = [
    "ADBE Text Properties",
    "ADBE Text Animators",
    "ADBE Text Animator",
];

/// The native 0→100 linear keys at layer-local 0.5s and 2.0s.
fn native_progress(native: &Layer) -> Vec<Chunk> {
    let groups = [
        &NATIVE_ANIMATOR[..],
        &[
            "ADBE Text Selectors",
            "ADBE Text Selector",
            "ADBE Text Range Advanced",
        ],
    ]
    .concat();
    native_storage(native, &groups, "ADBE Text Selector Max Amount")
}

fn alias(field: &str) -> String {
    format!("text.animator(\"Rise\").selector(\"Sweep\").{field};")
}

/// Rig choices that differ between cases; names deliberately differ from any
/// private source.
struct Spec {
    /// Rise/Sweep Percent Offset storage.
    sweep_offset: Chunk,
    /// Grow/Follow Percent Offset expression.
    follow_offset: String,
    /// Completion and width Slider value storage.
    progress: Chunk,
    span: Chunk,
}

/// One rig variant, built from the native layer.
type Variant = Box<dyn Fn(&Layer) -> Spec>;

impl Spec {
    fn standard(native: &Layer) -> Self {
        Self {
            sweep_offset: stored(&[-100.0], Some(OFFSET)),
            follow_offset: alias("offset"),
            progress: Chunk::list(*b"tdbs", native_progress(native)),
            span: stored(&[40.0], None),
        }
    }
}

/// Replaces the native animator with the two-animator rig and adds controls.
fn rig_layer(native: &Layer, spec: Spec) -> Layer {
    let ramp_up = || leaf("ADBE Text Range Shape", stored(&[2.0], None));
    let sweep = group(
        "ADBE Text Selector",
        Some("Sweep"),
        [
            leaf("ADBE Text Percent Start", stored(&[0.0], Some(START))),
            leaf("ADBE Text Percent Offset", spec.sweep_offset),
            group(
                "ADBE Text Range Advanced",
                None,
                [
                    ramp_up(),
                    leaf(
                        "ADBE Text Levels Max Ease",
                        stored(&[66.0], Some("effect(\"Ease Hi\")(1);")),
                    ),
                    leaf(
                        "ADBE Text Levels Min Ease",
                        stored(&[88.0], Some("effect(\"Ease Lo\")(1);")),
                    ),
                ]
                .concat(),
            ),
        ]
        .concat(),
    );
    let alternating = group(
        "ADBE Text Expressible Selector",
        Some("Alternate"),
        leaf(
            "ADBE Text Expressible Amount",
            stored(
                &[100.0, 100.0, 100.0],
                Some(
                    "if(textIndex%2 == 0){\r\n\tselectorValue;\r\n}else{\r\n\t-selectorValue;\r\n}",
                ),
            ),
        ),
    );
    // Stored values are deliberately stale: only resolved controls may win.
    let follow = group(
        "ADBE Text Selector",
        Some("Follow"),
        [
            leaf(
                "ADBE Text Percent Start",
                stored(&[0.0], Some(alias("start").as_str())),
            ),
            leaf(
                "ADBE Text Percent End",
                stored(&[100.0], Some(alias("end").as_str())),
            ),
            leaf(
                "ADBE Text Percent Offset",
                stored(&[45.0], Some(spec.follow_offset.as_str())),
            ),
            group(
                "ADBE Text Range Advanced",
                None,
                [
                    ramp_up(),
                    leaf(
                        "ADBE Text Levels Max Ease",
                        stored(&[66.0], Some(alias("advanced.easeHigh").as_str())),
                    ),
                    leaf(
                        "ADBE Text Levels Min Ease",
                        stored(&[88.0], Some(alias("advanced.easeLow").as_str())),
                    ),
                ]
                .concat(),
            ),
        ]
        .concat(),
    );
    let mode = |shown: &str, hidden: &str| {
        format!(
            "var d = effect(\"Mode\")(1);\r\n\r\nif (d == 1){{\r\n\t{shown};\r\n}}else{{\r\n\t{hidden};\r\n}}"
        )
    };
    let mut properties = NATIVE_ANIMATOR.to_vec();
    properties.push("ADBE Text Animator Properties");
    let position = native_storage(native, &properties, "ADBE Text Position 3D");
    let animators = [
        group(
            "ADBE Text Animator",
            Some("Rise"),
            [
                group("ADBE Text Selectors", None, [sweep, alternating].concat()),
                group(
                    "ADBE Text Animator Properties",
                    None,
                    leaf(
                        "ADBE Text Position 3D",
                        with_expression(
                            &position,
                            "var px = value[0];\r\nvar py = effect(\"Lift\")(1);\r\n\r\n[px,py];",
                        ),
                    ),
                ),
            ]
            .concat(),
        ),
        group(
            "ADBE Text Animator",
            Some("Grow"),
            [
                group("ADBE Text Selectors", None, follow),
                group(
                    "ADBE Text Animator Properties",
                    None,
                    [
                        leaf(
                            "ADBE Text Scale 3D",
                            stored(&[0.0, 0.0, 0.0], Some(mode("[0,0]", "[100,100]").as_str())),
                        ),
                        leaf(
                            "ADBE Text Opacity",
                            stored(&[100.0], Some(mode("100", "0").as_str())),
                        ),
                    ]
                    .concat(),
                ),
            ]
            .concat(),
        ),
    ]
    .concat();
    let effects = [
        slider("Progress", spec.progress),
        slider("Span", spec.span),
        slider("Lift", stored(&[60.0], None)),
        slider("Ease Hi", stored(&[-33.0], None)),
        slider("Ease Lo", stored(&[75.0], None)),
        effect("Pseudo/test mode", "Mode", stored(&[1.0], None)),
    ]
    .concat();

    let mut layer = native.clone();
    let root = layer
        .content
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .and_then(Chunk::children_mut)
        .unwrap();
    let text = group_children_mut(root, NATIVE_ANIMATOR[0]);
    *group_children_mut(text, NATIVE_ANIMATOR[1]) = animators;
    root.extend(group("ADBE Effect Parade", None, effects));
    // Layer start 0.75s: authored keys stay in the layer's own clock.
    set_layer_clock(&mut layer, [(18, 24), (0, 24), (54, 24)]);
    layer
}

fn rig_project(spec: impl FnOnce(&Layer) -> Spec) -> StructuralProject {
    assert_eq!(format!("{:x}", Sha256::digest(SOURCE)), SOURCE_SHA256);
    let mut project = read_project(SOURCE).unwrap();
    let composition = composition_mut(&mut project, COMPOSITION);
    assert_eq!(composition.layers.len(), 1);
    let native = composition.layers[0].clone();
    composition.layers[0] = rig_layer(&native, spec(&native));
    project
}

fn convert(spec: impl FnOnce(&Layer) -> Spec) -> StructuralConversion {
    let converted = to_structural_fx_document(&rig_project(spec), Some(COMPOSITION)).unwrap();
    EditableFxCompositionDocument::from_json_slice(&converted.document.to_json_vec().unwrap())
        .unwrap();
    converted
}

/// The layer occurrence's source clock gate and its single editable text.
fn gate_and_text(converted: &StructuralConversion) -> (&GroupLayer, &TextLayer) {
    let occurrence = as_group(&root(converted).layers[0]);
    let gate = as_group(&occurrence.layers[0]);
    assert_eq!(gate.name, "Source content clock");
    let texts: Vec<_> = gate
        .layers
        .iter()
        .filter_map(|layer| match layer.data() {
            FxLayer::Text(text) => Some(text),
            _ => None,
        })
        .collect();
    let [text] = texts.as_slice() else {
        panic!("one editable TextLayer, not flattened media: {texts:?}");
    };
    (gate, text)
}

/// The rig's two native animators, Rise and Grow. An alternating correction
/// animator may follow Rise.
fn native_animators(text: &TextLayer) -> [&fx_schema::TextAnimator; 2] {
    ["Animator 1", "Animator 2"].map(|name| {
        let mut matches = text
            .animators
            .iter()
            .filter(|animator| animator.name == name);
        let animator = matches
            .next()
            .unwrap_or_else(|| panic!("missing {name}: {:?}", text.animators));
        assert!(matches.next().is_none(), "duplicate {name}");
        animator
    })
}

fn selectors(text: &TextLayer) -> [&fx_schema::RangeSelector; 2] {
    let [rise, grow] = native_animators(text);
    let ([sweep], [follow]) = (rise.selectors.as_slice(), grow.selectors.as_slice()) else {
        panic!("one Range Selector per native animator");
    };
    [sweep, follow]
}

/// `(layerTime ms, value, easing type)` of the unique track, if any.
fn track(
    converted: &StructuralConversion,
    item: fx_schema::FxItemId,
    property: &str,
) -> Option<Vec<(i64, f64, String)>> {
    let editable: Value =
        serde_json::from_slice(&converted.document.to_json_vec().unwrap()).unwrap();
    let entries: Vec<_> = editable["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| {
            entry["target"]["kind"] == "fxItemProperty"
                && entry["target"]["itemId"] == item.value()
                && entry["target"]["propertyName"] == property
        })
        .collect();
    assert!(entries.len() <= 1, "duplicate {property} tracks");
    let entry = entries.first()?;
    assert_eq!(
        entry["animator"]["type"], "keyframes",
        "ordinary keys, not a script"
    );
    Some(
        entry["animator"]["keyframes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|key| {
                (
                    key["layerTime"].as_i64().unwrap(),
                    key["value"]["value"].as_f64().unwrap(),
                    key["easing"]["type"].as_str().unwrap().to_owned(),
                )
            })
            .collect(),
    )
}

fn messages(converted: &StructuralConversion) -> Vec<&str> {
    converted
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect()
}

/// Whether one diagnostic contains every fragment.
fn mentions(converted: &StructuralConversion, fragments: &[&str]) -> bool {
    messages(converted)
        .iter()
        .any(|message| fragments.iter().all(|fragment| message.contains(fragment)))
}

fn close(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() < 1e-12
}

fn assert_keys(keys: Option<Vec<(i64, f64, String)>>, expected: [(i64, f64); 2], context: &str) {
    let keys = keys.unwrap_or_else(|| panic!("{context}: missing ordinary editable track"));
    assert_eq!(keys.len(), 2, "{context}: {keys:?}");
    for ((time, value, easing), (expected_time, expected_value)) in keys.iter().zip(expected) {
        assert_eq!(*time, expected_time, "{context}: owner-local clock once");
        assert!(close(*value, expected_value), "{context}: {keys:?}");
        assert_eq!(easing, "linear", "{context}");
    }
}

#[test]
fn renamed_control_rig_lowers_both_selectors_to_ordinary_editable_curves() {
    let converted = convert(Spec::standard);
    let (gate, text) = gate_and_text(&converted);
    assert_eq!(
        key_millis(playback(gate))[0],
        (750, 0),
        "the occurrence applies the 0.75s layer start once"
    );
    for (selector, name) in selectors(text)
        .into_iter()
        .zip(["Sweep", "Follow (aliases)"])
    {
        assert_eq!(selector.shape, SelectorShape::RampUp, "{name}");
        // Start = 100 - Span(40); the sparse End keeps AE's 100% default.
        assert!(
            close(selector.start, 0.6),
            "{name} start {}",
            selector.start
        );
        assert!(close(selector.end, 1.0), "{name} end {}", selector.end);
        assert!(close(selector.ease_high, -0.33), "{name} easeHigh");
        assert!(close(selector.ease_low, 0.75), "{name} easeLow");
        // linear(p, 0, 100, -100, 100 - (100 - Span)): slope 1.4, intercept -100,
        // on the native 0→100 keys at layer-local 0.5s and 2.0s.
        assert!(
            close(selector.offset, -1.0),
            "{name} offset {}",
            selector.offset
        );
        let keys = track(&converted, selector.id, "offset");
        assert_keys(keys.clone(), [(500, -1.0), (2_000, 0.4)], name);
        // At completion the Ramp Up range starts at the text end: zero weight.
        assert!(selector.start + keys.unwrap()[1].1 >= 1.0 - 1e-12, "{name}");
        for field in ["start", "end", "easeHigh", "easeLow"] {
            assert!(
                track(&converted, selector.id, field).is_none(),
                "{name} {field}"
            );
        }
    }
    let [rise, grow] = native_animators(text);
    // value[0] keeps the stored 0; Y is the Lift Slider, not the stored -90.
    assert_eq!(rise.position, Some([0.0, 60.0]));
    assert!(
        mentions(
            &converted,
            &[SWEEP_OFFSET, "lowered to independent editable"]
        ),
        "{:?}",
        messages(&converted)
    );
    // The Mode pseudo effect is not a Slider: stored Scale/Opacity stay, with a reason.
    assert_eq!(grow.scale, Some([0.0, 0.0]));
    assert_eq!(grow.opacity, Some(100.0));
    assert!(mentions(
        &converted,
        &[
            "Text Animator 2 ADBE Text Scale 3D",
            "not lowered",
            "non-Slider effect"
        ]
    ));
    assert!(mentions(
        &converted,
        &[
            "Text Animator 2 ADBE Text Opacity",
            "not lowered",
            "non-Slider effect"
        ]
    ));
    assert!(
        !mentions(&converted, &["ADBE Text Expressible Selector", "omitted"]),
        "the alternating selector is lowered: {:?}",
        messages(&converted)
    );
}

#[test]
fn unprovable_offset_links_keep_stored_values_with_a_reason() {
    let replaced = |text: String| {
        move |native: &Layer| Spec {
            sweep_offset: stored(&[-100.0], Some(text.as_str())),
            ..Spec::standard(native)
        }
    };
    let progress = |edit: fn(usize, &mut [u8])| {
        move |native: &Layer| Spec {
            progress: edited_keys(&native_progress(native), edit),
            ..Spec::standard(native)
        }
    };
    let suffix = "unsupported expression suffix or extra statement";
    let clamp = "linear() input may leave its range";
    let cases: [(&str, &str, Variant); 7] = [
        (
            "malformed tail",
            suffix,
            Box::new(replaced(format!("{OFFSET}\r\nfoo();"))),
        ),
        (
            "extra statement",
            suffix,
            Box::new(replaced(format!("{OFFSET}\r\n5;"))),
        ),
        (
            "duplicate binding",
            "duplicate expression binding",
            Box::new(replaced(
                OFFSET.replace("var g", "var p").replace("-g)", "-p)"),
            )),
        ),
        (
            "absent control",
            "Slider not found",
            Box::new(replaced(OFFSET.replace("Progress", "Missing"))),
        ),
        (
            "clamp crossing",
            clamp,
            Box::new(progress(|index, item| {
                if index == 1 {
                    item[8..16].copy_from_slice(&150.0_f64.to_be_bytes());
                }
            })),
        ),
        (
            "Bezier input",
            clamp,
            Box::new(progress(|_, item| {
                item[4] = 2;
                item[5] = 2;
            })),
        ),
        (
            "alias cycle",
            "cyclic Range Selector alias",
            Box::new(|native: &Layer| Spec {
                sweep_offset: stored(
                    &[-100.0],
                    Some("text.animator(\"Grow\").selector(\"Follow\").offset;"),
                ),
                ..Spec::standard(native)
            }),
        ),
    ];
    for (case, reason, spec) in cases {
        let converted = convert(|native| spec(native));
        let (_, text) = gate_and_text(&converted);
        let [sweep, follow] = selectors(text);
        // Only the offset link is unprovable: Start still resolves.
        assert!(close(sweep.start, 0.6), "{case}");
        assert!(close(sweep.offset, -1.0), "{case}: stored -100%");
        assert!(
            close(follow.offset, 0.45),
            "{case}: stored 45%, never a guess"
        );
        for selector in [sweep, follow] {
            assert!(track(&converted, selector.id, "offset").is_none(), "{case}");
        }
        for context in [SWEEP_OFFSET, FOLLOW_OFFSET] {
            assert!(
                mentions(&converted, &[context, "not lowered", reason]),
                "{case}: {context}: {:?}",
                messages(&converted)
            );
        }
    }
}

#[test]
fn disabled_offset_expression_keeps_authored_keys_for_itself_and_its_alias() {
    let converted = convert(|native| {
        let mut storage = native_progress(native);
        let meta = storage
            .iter_mut()
            .find(|chunk| chunk.id() == *b"tdb4")
            .unwrap();
        let mut bytes = meta.data_payload().unwrap().to_vec();
        bytes[119] |= 1;
        *meta = Chunk::data(*b"tdb4", bytes).unwrap();
        Spec {
            sweep_offset: with_expression(&storage, OFFSET),
            ..Spec::standard(native)
        }
    });
    let (_, text) = gate_and_text(&converted);
    for (selector, name) in selectors(text).into_iter().zip(["Sweep", "Follow"]) {
        // The authored 0→100% keys, not the disabled linear() result.
        assert_keys(
            track(&converted, selector.id, "offset"),
            [(500, 0.0), (2_000, 1.0)],
            name,
        );
    }
    assert!(!mentions(&converted, &[SWEEP_OFFSET, "lowered"]));
}

#[test]
fn animated_span_lowers_start_but_rejects_its_linear_bound() {
    let converted = convert(|native| Spec {
        span: Chunk::list(*b"tdbs", native_progress(native)),
        ..Spec::standard(native)
    });
    let (_, text) = gate_and_text(&converted);
    for (selector, name) in selectors(text).into_iter().zip(["Sweep", "Follow"]) {
        // 100 - Span is exact on the Span keys: 100% → 0%.
        assert_keys(
            track(&converted, selector.id, "start"),
            [(500, 1.0), (2_000, 0.0)],
            name,
        );
        assert!(track(&converted, selector.id, "offset").is_none(), "{name}");
    }
    assert!(mentions(&converted, &[SWEEP_OFFSET, "not lowered"]));
}

#[test]
fn animation_budget_denial_keeps_resolved_static_selector_values() {
    let converted = to_structural_fx_document_with_animation_limit(
        &rig_project(Spec::standard),
        Some(COMPOSITION),
        0,
    )
    .unwrap();
    let (_, text) = gate_and_text(&converted);
    for selector in selectors(text) {
        assert!(track(&converted, selector.id, "offset").is_none());
        // The first resolved key, not the stale stored 45% of the alias.
        assert!(close(selector.offset, -1.0), "{}", selector.offset);
        assert!(close(selector.start, 0.6));
    }
    assert!(
        messages(&converted)
            .iter()
            .any(|message| message.contains("animation budget")),
        "{:?}",
        messages(&converted)
    );
}

// Alternating textIndex Expression Selector. The FX text runtime is outside
// this workspace, so `displacements` evaluates the emitted editable controls
// with the destination contract that `fx_schema` documents. Expected values
// come from AE's one-based textIndex and the imported linear Ramp Up range.

/// Linear sample of an ordinary editable track at layer time `ms`, holding
/// outside its keys; `None` without a track.
fn sampled(
    converted: &StructuralConversion,
    item: fx_schema::FxItemId,
    property: &str,
    ms: i64,
) -> Option<f64> {
    let keys = track(converted, item, property)?;
    let (first, last) = (&keys[0], &keys[keys.len() - 1]);
    if ms <= first.0 {
        return Some(first.1);
    }
    if ms >= last.0 {
        return Some(last.1);
    }
    let pair = keys.windows(2).find(|pair| pair[1].0 >= ms)?;
    let ((from_ms, from, easing), (to_ms, to, _)) = (&pair[0], &pair[1]);
    assert_eq!(easing, "linear", "the model samples linear keys only");
    Some(from + (to - from) * (ms - from_ms) as f64 / (to_ms - from_ms) as f64)
}

/// Per-character Position displacement `[x, y]` of `text` at layer time `ms`.
/// An animator's Range Selectors fold in order: the first is used directly,
/// Add sums and Intersect multiplies, each step clamped to [-1, 1]. Only a
/// positive final weight moves a character, additively.
fn displacements(converted: &StructuralConversion, text: &TextLayer, ms: i64) -> Vec<[f64; 2]> {
    use fx_schema::text_animator::{SelectorBasis, SelectorMode, SelectorUnits};

    let count = text.source_text.text.chars().count();
    let mut moved = vec![[0.0; 2]; count];
    for animator in &text.animators {
        let Some(position) = animator.position else {
            continue;
        };
        let mut combined = vec![1.0; count];
        for (index, selector) in animator.selectors.iter().enumerate() {
            assert_eq!(selector.based_on, SelectorBasis::Characters);
            assert!(!selector.randomize_order);
            let field = |name, value| sampled(converted, selector.id, name, ms).unwrap_or(value);
            let scale = match selector.units {
                SelectorUnits::Percentage => count as f64,
                SelectorUnits::Index => 1.0,
            };
            let offset = field("offset", selector.offset);
            let [start, end] = [field("start", selector.start), field("end", selector.end)]
                .map(|bound| (bound + offset) * scale);
            let (lo, hi) = (start.min(end), start.max(end));
            let amount = field("amount", selector.amount);
            for (unit, slot) in combined.iter_mut().enumerate() {
                let left = unit as f64;
                let center = left + 0.5;
                let shaped = match selector.shape {
                    SelectorShape::Square => ((left + 1.0).min(hi) - left.max(lo)).clamp(0.0, 1.0),
                    SelectorShape::RampUp if hi > lo => ((center - lo) / (hi - lo)).clamp(0.0, 1.0),
                    SelectorShape::RampUp => f64::from(u8::from(center > lo)),
                    shape => panic!("{shape:?} is outside this model"),
                };
                let weight = shaped * amount;
                *slot = match selector.mode {
                    SelectorMode::Add if index == 0 => weight,
                    SelectorMode::Add => *slot + weight,
                    SelectorMode::Intersect if index > 0 => *slot * weight,
                    mode => panic!("{mode:?} selector {index} is outside this model"),
                }
                .clamp(-1.0, 1.0);
            }
        }
        for (slot, weight) in moved.iter_mut().zip(combined) {
            if weight > 0.0 {
                slot[0] += position[0] * weight;
                slot[1] += position[1] * weight;
            }
        }
    }
    moved
}

/// Asserts AE's alternation at range start, middle and completion: a
/// character with an even one-based index moves by `position * w`, an odd one
/// by `-position * w`, where `w` is the imported Ramp Up weight of the Range
/// Selector spanning [`start`, 100%] + offset.
fn assert_alternates(
    converted: &StructuralConversion,
    position: [f64; 2],
    start: f64,
    offsets: [(i64, f64); 3],
) {
    let (_, text) = gate_and_text(converted);
    let count = text.source_text.text.chars().count() as f64;
    for (ms, offset) in offsets {
        let (lo, hi) = ((start + offset) * count, (1.0 + offset) * count);
        for (unit, moved) in displacements(converted, text, ms).into_iter().enumerate() {
            let weight = ((unit as f64 + 0.5 - lo) / (hi - lo)).clamp(0.0, 1.0);
            let sign = if (unit + 1) % 2 == 0 { 1.0 } else { -1.0 };
            for axis in 0..2 {
                let expected = sign * position[axis] * weight;
                assert!(
                    (moved[axis] - expected).abs() < 1e-9,
                    "{ms} ms: character {unit} ({:?}) axis {axis} moved {} instead of {expected}",
                    text.source_text.text.chars().nth(unit),
                    moved[axis]
                );
            }
        }
    }
}

#[test]
fn alternating_selector_moves_odd_characters_opposite_to_even_ones() {
    let converted = convert(Spec::standard);
    let (_, text) = gate_and_text(&converted);
    // Zero-based 8 is a space: Characters counts it, as one-based odd index 9.
    assert_eq!(text.source_text.text, "Editable Selector");
    // Offset keys -100% at 500 ms and 40% at 2000 ms; -30% halfway.
    assert_alternates(
        &converted,
        [0.0, 60.0],
        0.6,
        [(500, -1.0), (1_250, -0.3), (2_000, 0.4)],
    );
}

#[test]
fn alternating_correction_is_an_editable_animator_beside_untouched_siblings() {
    use fx_schema::text_animator::SelectorMode;
    use fx_schema::{RangeSelector, TextAnimator};

    let converted = convert(Spec::standard);
    let (_, text) = gate_and_text(&converted);
    let [rise, correction, grow] = text.animators.as_slice() else {
        panic!("Rise, its correction and Grow: {:?}", text.animators);
    };
    assert_eq!(
        [&rise.name, &correction.name, &grow.name],
        [
            "Animator 1",
            "Animator 1 alternating position",
            "Animator 2"
        ]
    );
    assert_eq!(rise.position, Some([0.0, 60.0]));
    assert_eq!(
        *correction,
        TextAnimator {
            id: correction.id,
            name: correction.name.clone(),
            selectors: correction.selectors.clone(),
            position: Some([0.0, -120.0]),
            ..TextAnimator::default()
        },
        "-2x Position is the correction's only property"
    );
    let [sweep] = rise.selectors.as_slice() else {
        panic!("Rise keeps only its Range Selector");
    };
    let (copy, gates) = correction.selectors.split_last().unwrap();
    assert_eq!(gates.len(), 9, "17 characters, nine odd one-based indices");
    assert_eq!(copy.mode, SelectorMode::Intersect);
    assert_eq!(
        RangeSelector {
            id: sweep.id,
            mode: sweep.mode,
            ..copy.clone()
        },
        *sweep,
        "the copy differs only by its identity and mode"
    );
    // Both copies keep the same ordinary keys in the layer's own clock.
    let keys = track(&converted, sweep.id, "offset");
    assert_keys(keys.clone(), [(500, -1.0), (2_000, 0.4)], "Sweep");
    assert_eq!(track(&converted, copy.id, "offset"), keys, "copy");
    for gate in gates {
        for property in RangeSelector::ANIMATABLE_PROPERTIES {
            assert!(track(&converted, gate.id, property).is_none());
        }
    }
    // The later sibling still targets its own selector.
    let [follow] = grow.selectors.as_slice() else {
        panic!("Grow keeps one Range Selector");
    };
    assert_keys(
        track(&converted, follow.id, "offset"),
        [(500, -1.0), (2_000, 0.4)],
        "Follow",
    );
    assert!(
        mentions(
            &converted,
            &[
                "Text Animator 1 alternating textIndex Expression Selector lowered",
                "\"Animator 1 alternating position\"",
                "not live",
                "text-length",
            ]
        ),
        "{:?}",
        messages(&converted)
    );
    assert!(!mentions(
        &converted,
        &["ADBE Text Expressible Selector", "omitted"]
    ));
}

const TEXT_RANGES: &[u8] = include_bytes!("../../../tests/fixtures/text/text_ranges.aep");
const TEXT_ANIMATOR: &[u8] = include_bytes!("../../../tests/fixtures/text/text_animator.aep");

/// The rig with `edit` applied to its layer. `edit` also gets the native
/// layer, whose storages it may reuse.
fn convert_edited(
    spec: impl FnOnce(&Layer) -> Spec,
    edit: impl FnOnce(&Layer, &mut Layer),
) -> StructuralConversion {
    let mut project = rig_project(spec);
    let native = read_project(SOURCE).unwrap();
    edit(
        &composition(&native, COMPOSITION).layers[0],
        &mut composition_mut(&mut project, COMPOSITION).layers[0],
    );
    let converted = to_structural_fx_document(&project, Some(COMPOSITION)).unwrap();
    EditableFxCompositionDocument::from_json_slice(&converted.document.to_json_vec().unwrap())
        .unwrap();
    converted
}

/// The children of the group at `path` below the rig's Rise animator.
fn rise_group<'a>(layer: &'a mut Layer, path: &[&str]) -> &'a mut Vec<Chunk> {
    let mut children = layer
        .content
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .and_then(Chunk::children_mut)
        .unwrap();
    for name in NATIVE_ANIMATOR.iter().chain(path) {
        children = group_children_mut(children, name);
    }
    children
}

const RISE_SWEEP: [&str; 2] = ["ADBE Text Selectors", "ADBE Text Selector"];
const RISE_ALTERNATE: [&str; 2] = ["ADBE Text Selectors", "ADBE Text Expressible Selector"];

/// Replaces the rigged Source Text with the native document of the layer in
/// `fixture` that shows `text`.
fn donor_text(layer: &mut Layer, fixture: &[u8], text: &str) {
    fn payload(chunks: &[Chunk]) -> Option<&[u8]> {
        chunks.iter().find_map(|chunk| {
            if chunk.list_kind() == Some(*b"btdk") {
                chunk.opaque_payload()
            } else {
                chunk.children().and_then(payload)
            }
        })
    }
    let units: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
    let project = read_project(fixture).unwrap();
    let donor = project
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Composition(composition) => Some(&composition.layers),
            _ => None,
        })
        .flatten()
        .find(|layer| {
            payload(&layer.content)
                .is_some_and(|bytes| bytes.windows(units.len()).any(|window| window == units))
        })
        .unwrap_or_else(|| panic!("no native layer shows {text:?}"));
    let document = |children: &[Chunk]| {
        children
            .iter()
            .position(|chunk| {
                chunk.id() == *b"tdmn"
                    && chunk.data_payload().is_some_and(|bytes| {
                        bytes.split(|byte| *byte == 0).next() == Some(b"ADBE Text Document")
                    })
            })
            .unwrap()
            + 1
    };
    let donor_text =
        unique_list(named(root_children(donor), NATIVE_ANIMATOR[0]), *b"tdgp").unwrap();
    let native = donor_text[document(donor_text)].clone();
    let root = layer
        .content
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .and_then(Chunk::children_mut)
        .unwrap();
    let rigged = group_children_mut(root, NATIVE_ANIMATOR[0]);
    let index = document(rigged);
    rigged[index] = native;
}

#[test]
fn alternation_follows_text_length_position_span_and_clock() {
    // "Animate Me": ten characters, an even count with a space at one-based
    // index 8, from another native fixture. Span 25 and a static Position
    // replace the rig's controls; the layer starts at 0.25s instead of 0.75s.
    let converted = convert_edited(
        |native| Spec {
            span: stored(&[25.0], None),
            ..Spec::standard(native)
        },
        |native, layer| {
            donor_text(layer, TEXT_ANIMATOR, "Animate Me\r");
            let mut properties = NATIVE_ANIMATOR.to_vec();
            properties.push("ADBE Text Animator Properties");
            let position = native_storage(native, &properties, "ADBE Text Position 3D");
            *rise_group(layer, &["ADBE Text Animator Properties"]) = leaf(
                "ADBE Text Position 3D",
                with_expression(&position, "[7, -45];"),
            );
            set_layer_clock(layer, [(6, 24), (0, 24), (54, 24)]);
        },
    );
    let (gate, text) = gate_and_text(&converted);
    assert_eq!(text.source_text.text, "Animate Me");
    assert_eq!(key_millis(playback(gate))[0], (250, 0), "layer start once");
    // Start 75%; offset -100% at 500 ms and 25% at 2000 ms.
    assert_alternates(
        &converted,
        [7.0, -45.0],
        0.75,
        [(500, -1.0), (1_250, -0.375), (2_000, 0.25)],
    );
}

#[test]
fn alternation_outside_the_admitted_profile_keeps_the_omission() {
    type Edit = Box<dyn Fn(&Layer, &mut Layer)>;
    let sweep_leaf = |name: &'static str, value: f64| -> Edit {
        Box::new(move |_, layer| {
            rise_group(layer, &RISE_SWEEP).extend(leaf(name, stored(&[value], None)));
        })
    };
    let alternate = |expression: &'static str, based_on: Option<f64>| -> Edit {
        Box::new(move |_, layer| {
            let mut leaves = leaf(
                "ADBE Text Expressible Amount",
                stored(&[100.0, 100.0, 100.0], Some(expression)),
            );
            if let Some(based_on) = based_on {
                leaves.extend(leaf("ADBE Text Range Type2", stored(&[based_on], None)));
            }
            *rise_group(layer, &RISE_ALTERNATE) =
                [vec![display_name("Alternate")], leaves].concat();
        })
    };
    let native_expression =
        "if(textIndex%2 == 0){\r\n\tselectorValue;\r\n}else{\r\n\t-selectorValue;\r\n}";
    let cases: [(&str, Option<&str>, Edit); 6] = [
        (
            "Subtract range",
            Some("Text Animator 1 Range Selector is not a nonrandom Characters Add selection"),
            sweep_leaf("ADBE Text Selector Mode", 2.0),
        ),
        (
            "keyed Smoothness without a destination track",
            Some("a keyed Range Selector field has no editable track on both copies"),
            Box::new(|native, layer| {
                rise_group(layer, &RISE_SWEEP).extend(leaf(
                    "ADBE Text Selector Smoothness",
                    Chunk::list(*b"tdbs", native_progress(native)),
                ));
            }),
        ),
        (
            "three character-style runs",
            Some("the text has more than one paragraph or character style run"),
            Box::new(|_, layer| donor_text(layer, TEXT_RANGES, "AVAWAY\r")),
        ),
        (
            "explicit Characters basis",
            None,
            alternate(native_expression, Some(1.0)),
        ),
        (
            "Characters Excluding Spaces basis",
            Some("omitted"),
            alternate(native_expression, Some(2.0)),
        ),
        (
            "other parity",
            Some("omitted"),
            alternate(
                "if(textIndex%2 == 1){\r\n\tselectorValue;\r\n}else{\r\n\t-selectorValue;\r\n}",
                None,
            ),
        ),
    ];
    for (case, outcome, edit) in cases {
        let converted = convert_edited(Spec::standard, |native, layer| edit(native, layer));
        let (_, text) = gate_and_text(&converted);
        let names: Vec<_> = text
            .animators
            .iter()
            .map(|animator| animator.name.as_str())
            .collect();
        let omitted = mentions(
            &converted,
            &[
                "Text Animator 1 selector ADBE Text Expressible Selector",
                "omitted",
            ],
        );
        match outcome {
            // Explicitly stored native defaults change nothing.
            None => {
                assert_eq!(names.len(), 3, "{case}: lowered");
                assert!(!omitted, "{case}");
            }
            // Not an alternating selector: the plain omission, no lowering attempt.
            Some("omitted") => {
                assert_eq!(names, ["Animator 1", "Animator 2"], "{case}");
                assert!(omitted, "{case}");
                assert!(!mentions(&converted, &["alternating textIndex"]), "{case}");
            }
            Some(reason) => {
                assert_eq!(names, ["Animator 1", "Animator 2"], "{case}");
                assert!(omitted, "{case}");
                assert!(
                    mentions(
                        &converted,
                        &[
                            "alternating textIndex Expression Selector not lowered",
                            reason
                        ]
                    ),
                    "{case}: {:?}",
                    messages(&converted)
                );
            }
        }
        // The rest of the rig is the unexpanded import.
        let [sweep, follow] = selectors(text);
        for (selector, name) in [(sweep, "Sweep"), (follow, "Follow")] {
            assert_keys(
                track(&converted, selector.id, "offset"),
                [(500, -1.0), (2_000, 0.4)],
                &format!("{case}: {name}"),
            );
        }
    }
}

#[test]
fn denied_or_unaddressable_expansion_rolls_back_to_the_omission() {
    use super::animation_budget::AnimationBudget;

    let converted = convert(Spec::standard);
    let (occurrence, _) = gate_and_text(&converted);
    let project = rig_project(Spec::standard);
    let layer = &composition(&project, COMPOSITION).layers[0];
    let import = |next_id: &mut u64, budget: &mut AnimationBudget| {
        text::import_with_mask_guides(layer, occurrence, &[], next_id, budget)
    };
    let animators = |imported: &text::TextImport| -> Vec<String> {
        let [FxLayer::Text(text)] = imported.layers.as_slice() else {
            panic!("one editable TextLayer");
        };
        text.animators
            .iter()
            .map(|animator| animator.name.clone())
            .collect()
    };
    let rejected = |imported: &text::TextImport, reason: &str| {
        assert_eq!(animators(imported), ["Animator 1", "Animator 2"]);
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains(&format!(
                    "alternating textIndex Expression Selector not lowered ({reason}"
                ))),
            "{:?}",
            imported.warnings
        );
    };

    let mut budget = AnimationBudget::default();
    let mut cursor = 1;
    let expanded = import(&mut cursor, &mut budget);
    assert_eq!(animators(&expanded).len(), 3);
    assert_eq!(expanded.animations.len(), 3, "Sweep, its copy and Follow");
    let expanded_ids = cursor - 1;

    // One byte short of all three tracks: the copy's track cannot be admitted
    // alone, so both offset tracks of the unexpanded import remain.
    let mut limited = AnimationBudget::with_limit(budget.used() - 1);
    let mut cursor = 1;
    let denied = import(&mut cursor, &mut limited);
    rejected(
        &denied,
        "its copied Range Selector tracks exceed the generated-animation allowance",
    );
    assert_eq!(
        denied.animations.len(),
        2,
        "Sweep and Follow keep their tracks"
    );
    let unexpanded_ids = cursor - 1;
    assert!(unexpanded_ids < expanded_ids);

    // Identifiers for the unexpanded import but not the expansion: none of the
    // expansion's identifiers are consumed.
    let first = u64::MAX - (expanded_ids - 1);
    let mut cursor = first;
    let exhausted = import(&mut cursor, &mut AnimationBudget::default());
    rejected(
        &exhausted,
        "its correction animators exceed the remaining generated identifier space",
    );
    assert_eq!(cursor - first, unexpanded_ids);
    assert_eq!(exhausted.animations.len(), 2);
}
