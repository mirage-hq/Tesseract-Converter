//! Fabricated generic `Keylight 906` records, imported through the real native
//! decoder and structural import. Supplementary CPU evidence only: no
//! Adobe-authored Keylight fixture or native render is pinned here, and these
//! tests do not execute the emitted WGSL.

use std::sync::Arc;

use serde_json::Value;

use super::{CATALOG, chunk_match_name, imported_case, named_group_mut};
use crate::{
    properties,
    rifx::{Chunk, Rifx},
    schema::layer_records::LayerRecord,
    structure::{ItemKind, Layer, SolidSource, StructuralProject, read_project},
    writer::effects::{effect_parade, new_effect},
};

const KEYLIGHT: &str = "Keylight 906";

/// Keylight (1.2) declaration types and first default payload words for
/// controls 0001..=0079, transcribed from a native `parT`.
#[rustfmt::skip]
const NATIVE_DECLARATIONS: [(u32, u32); 79] = [
    (9, 0), (7, 0x0000000b), (4, 0), (5, 0xff000000), (2, 0x00640000), (2, 0x00320000),
    (5, 0xff7f7f7f), (5, 0xff7f7f7f), (4, 0), (2, 0), (13, 0), (2, 0),
    (2, 0x00640000), (2, 0), (2, 0), (2, 0), (2, 0), (2, 0),
    (7, 0x00000004), (5, 0xff7f7f7f), (14, 0), (13, 0), (12, 0), (2, 0),
    (4, 0), (7, 0x00000002), (5, 0xff7f7f7f), (7, 0x00000003), (14, 0), (13, 0),
    (12, 0), (2, 0), (4, 0), (14, 0), (13, 0), (4, 0),
    (2, 0x00640000), (2, 0), (2, 0), (13, 0), (7, 0x00000001), (2, 0x00320000),
    (2, 0x00640000), (14, 0), (13, 0), (2, 0), (2, 0), (9, 0),
    (14, 0), (14, 0), (13, 0), (4, 0), (2, 0x00320000), (2, 0),
    (2, 0), (2, 0x00640000), (2, 0), (2, 0), (13, 0), (7, 0x00000001),
    (2, 0x00320000), (2, 0x00640000), (14, 0), (13, 0), (2, 0), (2, 0),
    (9, 0), (14, 0), (14, 0), (13, 0), (7, 0x00000001), (7, 0x00000001),
    (5, 0xff000000), (2, 0x00640000), (2, 0), (2, 0x00640000), (2, 0), (2, 0x00640000),
    (14, 0),
];

const GREEN: [f64; 3] = [26.0 / 255.0, 179.0 / 255.0, 64.0 / 255.0];

fn named(name: &str) -> Chunk {
    let mut bytes = vec![0; 40];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    Chunk::data(*b"tdmn", bytes).expect("40-byte match name")
}

/// Explicit control record; `tdb4` type flags are 1 (color) and 4 (integer).
fn record_with(type_flags: u8, values: &[f64], edit: impl FnOnce(&mut Vec<u8>)) -> Chunk {
    let mut meta = vec![0; 124];
    meta[..4].copy_from_slice(&[0xdb, 0x99, 0, values.len() as u8]);
    meta[59] = type_flags;
    edit(&mut meta);
    Chunk::list(
        *b"tdbs",
        vec![
            Chunk::data(*b"tdsb", [0, 0, 0, 1]).expect("flags"),
            Chunk::data(*b"tdb4", meta).expect("metadata"),
            Chunk::data(
                *b"cdat",
                values
                    .iter()
                    .flat_map(|value| value.to_be_bytes())
                    .collect::<Vec<_>>(),
            )
            .expect("values"),
        ],
    )
}

fn record(type_flags: u8, values: &[f64]) -> Chunk {
    record_with(type_flags, values, |_| {})
}

fn scalar(value: f64) -> Chunk {
    record(0, &[value])
}

/// Native slider records may carry integer metadata with a fractional value.
fn integer_flagged(value: f64) -> Chunk {
    record(4, &[value])
}

/// Native colors store ARGB in 0..255.
fn color([r, g, b]: [f64; 3]) -> Chunk {
    record(1, &[255.0, r * 255.0, g * 255.0, b * 255.0])
}

/// A 148-byte `pard`: type at bytes 12..16 and default payload from byte 56.
fn pard(kind: u32, payload: &[u32]) -> Chunk {
    let mut bytes = vec![0; 148];
    bytes[12..16].copy_from_slice(&kind.to_be_bytes());
    for (index, word) in payload.iter().enumerate() {
        bytes[56 + 4 * index..60 + 4 * index].copy_from_slice(&word.to_be_bytes());
    }
    Chunk::data(*b"pard", bytes).expect("declaration")
}

/// One effect occurrence: its own declaration table and explicit controls.
#[derive(Clone)]
struct Instance {
    match_name: &'static str,
    enabled: bool,
    declarations: Vec<(String, Chunk)>,
    explicit: Vec<(String, Chunk)>,
}

impl Instance {
    /// AE's sparse layer-side layout: an empty `parT`, explicit records for the
    /// group markers and buttons, and an authored Screen Colour.
    fn sparse(screen: [f64; 3]) -> Self {
        let mut explicit: Vec<_> = [
            "0001", "0011", "0021", "0022", "0029", "0030", "0034", "0035", "0040", "0044", "0045",
            "0048", "0049", "0050", "0051", "0059", "0063", "0064", "0067", "0068", "0069", "0070",
            "0079",
        ]
        .into_iter()
        .map(|suffix| (suffix.to_owned(), scalar(0.0)))
        .collect();
        explicit.push(("0004".into(), color(screen)));
        Self {
            match_name: KEYLIGHT,
            enabled: true,
            declarations: Vec::new(),
            explicit,
        }
    }

    /// The complete native declaration table, as stored by other instances.
    fn declared(mut self) -> Self {
        self.declarations = NATIVE_DECLARATIONS
            .iter()
            .enumerate()
            .map(|(index, &(kind, word))| (format!("{:04}", index + 1), pard(kind, &[word])))
            .collect();
        self
    }

    fn declare(mut self, suffix: &str, declaration: Chunk) -> Self {
        self.declarations.retain(|(existing, _)| existing != suffix);
        self.declarations.push((suffix.into(), declaration));
        self
    }

    fn set(mut self, suffix: &str, record: Chunk) -> Self {
        self.explicit.retain(|(existing, _)| existing != suffix);
        self.explicit.push((suffix.into(), record));
        self
    }

    fn chunks(&self) -> [Chunk; 2] {
        let mut table = vec![Chunk::data(*b"parn", [0, 0, 0, 0]).expect("count")];
        for (suffix, declaration) in &self.declarations {
            table.push(named(&format!("{}-{suffix}", self.match_name)));
            table.push(declaration.clone());
        }
        let mut explicit = vec![
            Chunk::data(*b"tdsb", [0, 0, 0, u8::from(self.enabled)]).expect("enable flags"),
            named(&format!("{}-0000", self.match_name)),
            integer_flagged(0.0),
        ];
        for (suffix, leaf) in &self.explicit {
            explicit.push(named(&format!("{}-{suffix}", self.match_name)));
            explicit.push(leaf.clone());
        }
        explicit.extend([
            named("ADBE Effect Built In Params"),
            Chunk::list(*b"tdgp", vec![named("ADBE Group End")]),
            named("ADBE Group End"),
        ]);
        [
            named(self.match_name),
            Chunk::list(
                *b"sspc",
                vec![
                    Chunk::list(*b"parT", table),
                    Chunk::list(*b"tdgp", explicit),
                ],
            ),
        ]
    }
}

/// A writer-generated catalog effect occurrence (match name and plugin).
fn catalog_effect(match_name: &str) -> [Chunk; 2] {
    let effect = new_effect(match_name, true, [120.0, 80.0]).expect("catalog effect");
    let parade = effect_parade(&[effect], 1, [120.0, 80.0]).expect("writer Effect Parade");
    let children = parade.children().expect("parade group");
    let start = children
        .iter()
        .position(|chunk| chunk.id() == *b"tdmn")
        .expect("occurrence match name");
    [children[start].clone(), children[start + 1].clone()]
}

fn parade(occurrences: impl IntoIterator<Item = [Chunk; 2]>) -> Vec<Chunk> {
    let mut children = vec![Chunk::data(*b"tdsb", [0, 0, 0, 1]).expect("parade flags")];
    children.extend(occurrences.into_iter().flatten());
    children.push(named("ADBE Group End"));
    children
}

fn project_with(parade: Vec<Chunk>, edit_owner: impl FnOnce(&mut Layer)) -> StructuralProject {
    let mut project = read_project(CATALOG).expect("pinned Adobe-native effect catalog");
    let item = project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .expect("pinned owner composition");
    let ItemKind::Composition(composition) = &mut item.kind else {
        panic!("composition 1")
    };
    let owner = composition.layers.first_mut().expect("native owner layer");
    *named_group_mut(&mut owner.content, "ADBE Effect Parade").expect("native Effect Parade") =
        parade;
    edit_owner(owner);
    project
}

/// Replace the native catalog owner's Effect Parade, keeping its owner layer.
fn import(parade: Vec<Chunk>) -> (Value, Vec<String>) {
    imported_case(&project_with(parade, |_| {}), 1)
}

fn import_one(instance: &Instance) -> (Value, Vec<String>) {
    import(parade([instance.chunks()]))
}

/// Every imported effect record, in document order.
fn effect_records(node: &Value) -> Vec<&Value> {
    let mut records = Vec::new();
    if let Some(effects) = node.get("effects").and_then(Value::as_array) {
        records.extend(effects);
    }
    if let Some(composition) = node.get("composition") {
        records.extend(effect_records(composition));
    }
    for layer in node
        .get("layers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        records.extend(effect_records(layer));
    }
    records
}

fn shaders(document: &Value) -> Vec<&Value> {
    effect_records(document)
        .into_iter()
        .filter(|record| record["effect"]["type"] == "customShader")
        .collect()
}

fn only_shader<'a>(document: &'a Value, warnings: &[String]) -> &'a Value {
    let shaders = shaders(document);
    let [shader] = shaders.as_slice() else {
        panic!("one editable Keylight shader expected: {warnings:?}")
    };
    shader
}

fn params(shader: &Value) -> Vec<(String, f64)> {
    shader["effect"]["params"]
        .as_array()
        .expect("shader params")
        .iter()
        .map(|param| {
            (
                param["name"].as_str().expect("param name").to_owned(),
                param["default"].as_f64().expect("param value"),
            )
        })
        .collect()
}

fn assert_params(shader: &Value, expected: [f64; 6]) {
    let actual = params(shader);
    let names: Vec<_> = actual.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "screen.colorR",
            "screen.colorG",
            "screen.colorB",
            "screenGain",
            "screenBalance",
            "clipWhite"
        ]
    );
    for ((name, value), expected) in actual.iter().zip(expected) {
        assert!(
            (value - expected).abs() < 1e-12,
            "{name}: expected {expected}, got {value}; {actual:?}"
        );
    }
}

fn defaults_note(warnings: &[String]) -> Option<&String> {
    warnings
        .iter()
        .find(|warning| warning.contains(KEYLIGHT) && warning.contains("plugin defaults"))
}

#[test]
fn sparse_keylight_without_declarations_uses_plugin_defaults() {
    let (document, warnings) = import_one(&Instance::sparse(GREEN));
    let shader = only_shader(&document, &warnings);
    assert_eq!(shader["enabled"], true);
    assert_params(shader, [GREEN[0], GREEN[1], GREEN[2], 100.0, 50.0, 100.0]);
    let note = defaults_note(&warnings).expect("absent controls are reported");
    for label in [
        "View",
        "Screen Gain",
        "Screen Balance",
        "Clip White",
        "Inside Mask",
    ] {
        assert!(note.contains(label), "{note}");
    }
    assert!(
        !note.contains("Screen Colour"),
        "authored, not defaulted: {note}"
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("CustomShader approximation")),
        "{warnings:?}"
    );
}

#[test]
fn explicit_and_declared_values_take_precedence_over_profile_defaults() {
    // A complete declaration table and AE's integer-flagged fractional sliders.
    let authored = Instance::sparse(GREEN)
        .declared()
        .set("0005", integer_flagged(120.0))
        .set("0006", integer_flagged(58.000_000_000_000_02))
        .set("0013", integer_flagged(82.700_000_000_000_2));
    let (document, warnings) = import_one(&authored);
    let shader = only_shader(&document, &warnings);
    assert_params(
        shader,
        [
            GREEN[0],
            GREEN[1],
            GREEN[2],
            120.0,
            58.000_000_000_000_02,
            82.700_000_000_000_2,
        ],
    );
    assert_eq!(defaults_note(&warnings), None, "every control is stored");
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.contains("-0023") || warning.contains("-0031")),
        "declared unset mask paths are decoded, not unsupported: {warnings:?}"
    );

    // A declaration default wins over the profile default per control.
    let declared = Instance::sparse(GREEN)
        .declare("0005", pard(2, &[110 << 16]))
        .declare("0013", pard(2, &[90 << 16]));
    let (document, warnings) = import_one(&declared);
    assert_params(
        only_shader(&document, &warnings),
        [GREEN[0], GREEN[1], GREEN[2], 110.0, 50.0, 90.0],
    );
    let note = defaults_note(&warnings).expect("partially declared");
    assert!(
        note.contains("Screen Balance") && !note.contains("Screen Gain"),
        "{note}"
    );
}

#[test]
fn sparse_defaults_are_resolved_per_occurrence() {
    let authored = Instance::sparse([0.1, 0.2, 0.9])
        .declared()
        .set("0005", integer_flagged(120.0))
        .set("0006", integer_flagged(58.0));
    let sparse = Instance::sparse(GREEN);
    let (document, warnings) = import(parade([authored.chunks(), sparse.chunks()]));
    let shaders = shaders(&document);
    assert_eq!(shaders.len(), 2, "{warnings:?}");
    assert_params(shaders[0], [0.1, 0.2, 0.9, 120.0, 58.0, 100.0]);
    assert_params(
        shaders[1],
        [GREEN[0], GREEN[1], GREEN[2], 100.0, 50.0, 100.0],
    );
    assert_eq!(shaders[0]["effect"]["name"], "Keylight (AE effect 1)");
    assert_eq!(shaders[1]["effect"]["name"], "Keylight (AE effect 2)");
}

#[test]
fn any_uniquely_dominant_screen_primary_is_keyed() {
    for screen in [[0.8, 0.1, 0.25], GREEN, [0.05, 0.3, 0.9]] {
        let (document, warnings) = import_one(&Instance::sparse(screen));
        assert_params(
            only_shader(&document, &warnings),
            [screen[0], screen[1], screen[2], 100.0, 50.0, 100.0],
        );
    }
    for screen in [[0.6, 0.6, 0.1], [0.5, 0.5, 0.5], [0.0; 3]] {
        let (document, warnings) = import_one(&Instance::sparse(screen));
        assert!(shaders(&document).is_empty(), "{screen:?}");
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("no unique dominant channel")),
            "{warnings:?}"
        );
    }
}

/// The owner's imported effects with Keylight between two catalog siblings.
struct Siblings {
    types: Vec<String>,
    ids: Vec<u64>,
    enabled: Vec<bool>,
    warnings: Vec<String>,
}

fn import_between_siblings(keylight: [Chunk; 2], edit_owner: impl FnOnce(&mut Layer)) -> Siblings {
    let project = project_with(
        parade([
            catalog_effect("ADBE Tint"),
            keylight,
            catalog_effect("ADBE Gaussian Blur 2"),
        ]),
        edit_owner,
    );
    let (document, warnings) = imported_case(&project, 1);
    let records = effect_records(&document);
    Siblings {
        types: records
            .iter()
            .map(|record| record["effect"]["type"].as_str().expect("type").to_owned())
            .collect(),
        ids: records
            .iter()
            .map(|record| record["id"].as_u64().expect("effect id"))
            .collect(),
        enabled: records
            .iter()
            .map(|record| record["enabled"].as_bool().expect("enabled"))
            .collect(),
        warnings,
    }
}

#[test]
fn keylight_keeps_its_parade_position_switches_and_identity() {
    let admitted = import_between_siblings(Instance::sparse(GREEN).chunks(), |_| {});
    assert_eq!(
        admitted.types,
        ["tintTritone", "customShader", "gaussianBlur"],
        "{:?}",
        admitted.warnings
    );
    let first = admitted.ids[0];
    assert_eq!(admitted.ids, [first, first + 1, first + 2]);
    assert_eq!(admitted.enabled, [true, true, true]);

    let disabled = Instance {
        enabled: false,
        ..Instance::sparse(GREEN)
    };
    let disabled = import_between_siblings(disabled.chunks(), |_| {});
    assert_eq!(
        disabled.types,
        ["tintTritone", "customShader", "gaussianBlur"]
    );
    assert_eq!(
        disabled.enabled,
        [true, false, true],
        "the native effect switch is kept"
    );

    let switched_off = import_between_siblings(Instance::sparse(GREEN).chunks(), |owner| {
        let mut bytes = owner.record.encode();
        bytes[39] &= !(1 << 2);
        owner.record = LayerRecord::decode(&bytes).expect("layer record");
        assert!(!owner.record.flags().effects_active);
    });
    assert_eq!(
        switched_off.enabled,
        [false, false, false],
        "the layer Effects switch is kept"
    );
}

#[test]
fn keylight_stays_before_a_following_fractal_blend() {
    fn prefix(node: &Value) -> Option<&Value> {
        if node["name"] == "Source before Fractal blend" {
            return Some(node);
        }
        match node {
            Value::Object(fields) => fields.values().find_map(prefix),
            Value::Array(values) => values.iter().find_map(prefix),
            _ => None,
        }
    }

    for enabled in [true, false] {
        let mut project = read_project(include_bytes!(
            "../../../tests/fixtures/effects/shape_owner_gaussian.aep"
        ))
        .unwrap();
        let fixture = Rifx::parse_with(
            include_bytes!("../../../tests/fixtures/effects/native-fractal-noise-controls.rifx"),
            |_| false,
        )
        .unwrap();
        let ItemKind::Composition(comp) = &mut project
            .items
            .iter_mut()
            .find(|item| item.id == 1)
            .unwrap()
            .kind
        else {
            panic!("composition 1")
        };
        let mut owner = comp.layers[0].clone();
        owner.content = fixture.chunks()[0].children().unwrap().to_vec();
        owner.record =
            LayerRecord::decode(properties::data(&owner.content, *b"ldta").unwrap()).unwrap();
        let source_id = owner.record.source_id();
        let effects = named_group_mut(&mut owner.content, "ADBE Effect Parade").unwrap();
        // The third native occurrence is Basic/Spline noise with Multiply blending.
        let fractal_index = effects
            .iter()
            .enumerate()
            .filter(|(_, chunk)| chunk_match_name(chunk) == Some("ADBE Fractal Noise"))
            .nth(2)
            .unwrap()
            .0;
        let fractal = [
            effects[fractal_index].clone(),
            effects[fractal_index + 1].clone(),
        ];
        *effects = parade([
            Instance {
                enabled,
                ..Instance::sparse(GREEN)
            }
            .chunks(),
            fractal,
        ]);
        comp.layers = vec![owner];
        comp.width = 1920;
        comp.height = 1080;
        comp.duration_secs = 6.;
        let mut source = project.items[0].clone();
        source.id = source_id;
        source.kind = ItemKind::Unknown(0);
        source.solid = Some(Ok(SolidSource {
            width: 1280,
            height: 720,
            pixel_aspect: (1, 1),
            color: [0.; 3],
        }));
        project.items.push(source);

        let (document, warnings) = imported_case(&project, 1);
        let prefix = prefix(&document).unwrap_or_else(|| panic!("Fractal blend: {warnings:?}"));
        let shader = only_shader(prefix, &warnings);
        assert_eq!(shader["enabled"], enabled);
        assert_eq!(shaders(&document).len(), 1);
    }
}

fn assert_omitted(instance: Instance, reason: &str) {
    assert_occurrence_omitted(instance.chunks(), reason);
}

fn assert_occurrence_omitted(occurrence: [Chunk; 2], reason: &str) {
    let omitted = import_between_siblings(occurrence, |_| {});
    let warnings = &omitted.warnings;
    assert_eq!(
        omitted.types,
        ["tintTritone", "gaussianBlur"],
        "{reason}: {warnings:?}"
    );
    assert_eq!(
        omitted.ids[1],
        omitted.ids[0] + 1,
        "{reason}: an omitted occurrence consumes no identity"
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.starts_with("Effect Keylight 906:")
                && warning.contains(reason)
                && warning.ends_with("effect omitted, owner and other effects retained")),
        "{reason}: {warnings:?}"
    );
}

#[test]
fn unsupported_modes_are_omitted_with_their_control() {
    let base = || Instance::sparse(GREEN);
    for (suffix, record, reason) in [
        ("0002", scalar(1.0), "View (0002)"),
        ("0003", scalar(1.0), "Unpremultiply Result (0003)"),
        ("0007", color([0.6, 0.5, 0.5]), "Despill Bias (0007)"),
        ("0010", scalar(2.0), "Screen Pre-blur (0010)"),
        ("0012", scalar(10.0), "Clip Black (0012)"),
        ("0019", scalar(2.0), "Replace Method (0019)"),
        ("0020", color([0.2, 0.4, 0.6]), "Replace Colour (0020)"),
        ("0025", scalar(1.0), "Inside Mask Invert (0025)"),
        ("0028", scalar(1.0), "Source Alpha (0028)"),
        ("0036", scalar(1.0), "Enable Colour Correction (0036)"),
        ("0075", scalar(5.0), "Crop Left (0075)"),
    ] {
        assert_omitted(base().set(suffix, record), reason);
    }
}

#[test]
fn out_of_range_or_degenerate_editable_values_are_omitted() {
    let base = || Instance::sparse(GREEN);
    assert_omitted(
        base().set("0005", scalar(5000.5)),
        "Screen Gain (0005) = 5000.5 is outside",
    );
    assert_omitted(
        base().set("0006", scalar(-1.0)),
        "Screen Balance (0006) = -1 is outside",
    );
    assert_omitted(
        base().set("0013", scalar(0.0)),
        "does not exceed the supported Clip Black 0",
    );
    assert_omitted(
        base().set("0013", scalar(100.5)),
        "Clip White (0013) = 100.5 is outside",
    );
    assert_omitted(
        base().set("0004", color([0.1, 1.5, 0.2])),
        "Screen Colour (0004): malformed colour",
    );
}

#[test]
fn malformed_duplicate_unknown_and_animated_controls_are_omitted() {
    let base = || Instance::sparse(GREEN);
    let mut duplicate = base();
    duplicate.explicit.push(("0004".into(), color(GREEN)));
    assert_omitted(
        duplicate,
        "Screen Colour (0004): unsupported or malformed property: duplicate effect control record",
    );
    let mut duplicate_declaration = base().declare("0006", pard(2, &[50 << 16]));
    duplicate_declaration
        .declarations
        .push(("0006".into(), pard(2, &[50 << 16])));
    assert_omitted(
        duplicate_declaration,
        "Screen Balance (0006): unreadable declaration (unsupported or malformed property: duplicate effect declaration)",
    );
    assert_omitted(
        base().set("0005", record_with(0, &[120.0], |meta| meta.truncate(100))),
        "Screen Gain (0005): unsupported or malformed property",
    );
    assert_omitted(
        base().set("0080", scalar(0.0)),
        "Keylight 906-0080: control outside the supported Keylight 906 profile",
    );
    assert_omitted(
        base().declare("0080", pard(2, &[0])),
        "Keylight 906-0080: control outside the supported Keylight 906 profile",
    );
    assert_omitted(
        base().declare("0005", pard(10, &[0])),
        "Screen Gain (0005): declared parameter type 10 is incompatible with the supported type 2",
    );
    assert_omitted(
        base().set("0005", record_with(0, &[120.0], |meta| meta[68] = 1)),
        "Screen Gain (0005): animated or expression-driven",
    );
    assert_omitted(
        base().set("0013", record_with(0, &[90.0], |meta| meta[120] = 1)),
        "Clip White (0013): animated or expression-driven",
    );
}

/// The plugin descriptor (`sspc`) children of one occurrence.
fn plugin(occurrence: &mut [Chunk; 2]) -> &mut Vec<Chunk> {
    occurrence[1].children_mut().expect("plugin descriptor")
}

#[test]
fn unreadable_or_ambiguous_declarations_are_omitted_despite_explicit_values() {
    const TABLE: &str = "its own parameter declaration table is duplicated or malformed";
    let mut duplicated = Instance::sparse(GREEN).chunks();
    let table = plugin(&mut duplicated)
        .iter()
        .find(|chunk| chunk.list_kind() == Some(*b"parT"))
        .cloned()
        .expect("declaration table");
    plugin(&mut duplicated).push(table);
    assert_occurrence_omitted(duplicated, TABLE);

    let mut misnamed = Instance::sparse(GREEN)
        .declare("0005", pard(2, &[100 << 16]))
        .chunks();
    let table = plugin(&mut misnamed)
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"parT"))
        .and_then(Chunk::children_mut)
        .expect("declaration table");
    let name = table
        .iter()
        .position(|chunk| chunk.id() == *b"tdmn")
        .expect("declaration name");
    table[name] = Chunk::data(*b"tdmn", vec![0xff, 0xff]).expect("short name");
    assert_occurrence_omitted(misnamed, TABLE);

    // A valid explicit value does not hide a malformed or ambiguous declaration.
    assert_omitted(
        Instance::sparse(GREEN)
            .declare(
                "0005",
                Chunk::data(*b"pard", vec![0; 12]).expect("short pard"),
            )
            .set("0005", scalar(120.0)),
        "Screen Gain (0005): unreadable declaration",
    );
    assert_omitted(
        Instance::sparse(GREEN)
            .declare("0006", Chunk::data(*b"pdnm", vec![0; 4]).expect("no pard"))
            .set("0006", scalar(50.0)),
        "Screen Balance (0006): unreadable declaration",
    );
    let mut twice = Instance::sparse(GREEN).declare("0011", pard(13, &[0]));
    twice.declarations.push(("0011".into(), pard(13, &[0])));
    assert_omitted(twice, "Screen Matte (0011): unreadable declaration");
}

#[test]
fn a_missing_declaration_table_still_uses_plugin_defaults() {
    let mut missing = Instance::sparse(GREEN).chunks();
    plugin(&mut missing).retain(|chunk| chunk.list_kind() != Some(*b"parT"));
    let (document, warnings) = import(parade([missing]));
    assert_params(
        only_shader(&document, &warnings),
        [GREEN[0], GREEN[1], GREEN[2], 100.0, 50.0, 100.0],
    );
    assert!(defaults_note(&warnings).is_some(), "{warnings:?}");
}

#[test]
fn mask_selectors_are_admitted_only_when_proven_unset() {
    // Absent (sparse) and declared all-zero path defaults select no mask.
    let declared = Instance::sparse(GREEN)
        .declare("0023", pard(12, &[0]))
        .declare("0031", pard(12, &[0]));
    for instance in [Instance::sparse(GREEN), declared.clone()] {
        let (document, warnings) = import_one(&instance);
        only_shader(&document, &warnings);
        assert!(
            !warnings.iter().any(|warning| warning.contains("-0023")),
            "{warnings:?}"
        );
    }
    let reason = "Inside Mask (0023): an explicit or undecodable mask selector";
    assert_omitted(declared.clone().set("0023", scalar(0.0)), reason);
    assert_omitted(
        declared.clone().declare("0023", pard(12, &[0, 0, 1])),
        reason,
    );
    assert_omitted(
        declared.declare("0031", pard(2, &[0])),
        "Outside Mask (0031): declared parameter type 2 is incompatible with the supported type 12",
    );
}

#[test]
fn inert_controls_of_disabled_features_do_not_block_admission() {
    let instance = Instance::sparse(GREEN)
        .set("0009", scalar(1.0))
        .set("0024", scalar(40.0))
        .set("0037", scalar(20.0))
        .set("0041", scalar(3.0))
        .set("0071", scalar(2.0))
        .set("0053", record_with(0, &[50.0], |meta| meta[68] = 1));
    let (document, warnings) = import_one(&instance);
    assert_params(
        only_shader(&document, &warnings),
        [GREEN[0], GREEN[1], GREEN[2], 100.0, 50.0, 100.0],
    );
}

#[test]
fn other_match_names_keep_the_unmapped_omission() {
    let other = Instance {
        match_name: "Keylight 905",
        ..Instance::sparse(GREEN)
    };
    let (document, warnings) = import_one(&other);
    assert!(shaders(&document).is_empty());
    assert!(
        warnings.iter().any(|warning| warning
            == "Effect Keylight 905: no current native FX counterpart/mapping; effect omitted, owner and other effects retained"),
        "{warnings:?}"
    );
}

#[test]
fn project_and_layer_names_or_item_order_do_not_change_the_conversion() {
    let keylight = Instance::sparse(GREEN);
    let baseline = project_with(parade([keylight.chunks()]), |_| {});
    let mut renamed = project_with(parade([keylight.chunks()]), |owner| {
        owner.name = Arc::from("renamed owner");
    });
    for item in &mut renamed.items {
        item.name = format!("renamed {}", item.id);
    }
    renamed.items.reverse();
    let (expected, _) = imported_case(&baseline, 1);
    let (actual, warnings) = imported_case(&renamed, 1);
    assert_eq!(
        only_shader(&actual, &warnings)["effect"],
        only_shader(&expected, &warnings)["effect"]
    );
}

#[test]
fn emitted_shader_declares_parameters_in_uniform_order() {
    let (document, warnings) = import_one(&Instance::sparse(GREEN));
    let effect = &only_shader(&document, &warnings)["effect"];
    let wgsl = effect["wgsl"].as_str().expect("complete WGSL module");
    let fields: Vec<_> = wgsl
        .split("struct Params {")
        .nth(1)
        .and_then(|rest| rest.split("};").next())
        .expect("Params uniform")
        .lines()
        .filter_map(|line| line.trim().strip_suffix(": f32,"))
        .collect();
    assert_eq!(
        fields,
        [
            "screen_r",
            "screen_g",
            "screen_b",
            "screen_gain",
            "screen_balance",
            "clip_white",
            "_pad0",
            "_pad1"
        ],
        "one f32 per parameter in order, padded to a multiple of four"
    );
    assert_eq!(params(only_shader(&document, &warnings)).len(), 6);
    for entry in [
        "@vertex\nfn vs_main(",
        "@fragment\nfn fs_main(",
        "@group(0) @binding(0) var t_texture: texture_2d<f32>;",
        "@group(0) @binding(1) var s_sampler: sampler;",
        "@group(0) @binding(2) var<uniform> params: Params;",
    ] {
        assert!(wgsl.contains(entry), "{entry}");
    }
    assert!(!wgsl.contains("@binding(3)"), "single-texture layout");
    assert!(
        effect.get("textureInputs").is_none(),
        "no consumed input layer"
    );
    assert!(
        effect["description"]
            .as_str()
            .is_some_and(|text| text.contains("not Keylight's unpublished algorithm")
                && text.contains("SDR")),
        "{effect}"
    );
}
