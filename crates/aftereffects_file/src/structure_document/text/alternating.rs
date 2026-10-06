//! Lowering of AE's alternating-sign text Expression Selector.
//!
//! An enabled Expression Selector whose Amount is exactly
//! `if (textIndex % 2 == 0) { selectorValue; } else { -selectorValue; }`
//! keeps the preceding selector's weight `w` on characters with an even
//! one-based `textIndex` and negates it on odd ones. Adobe documents
//! `selectorValue` as the input from the selector above, and Adobe's own
//! presets store only Based On and Amount for an Expression Selector. The FX
//! runtime skips weights at or below zero but adds signed Position values, so
//! a negative Amount cannot carry the negation.
//!
//! Before conversion, an admitted animator (Position `P` on one Range
//! Selector) is replaced by itself without the Expression Selector, followed by
//! an editable correction animator with Position `-2P`. The correction's
//! Index/Square Add gates `[0, 1]`, `[2, 3]`, ... select the odd one-based
//! characters, and a copy of the Range Selector intersects them with `w`. Odd
//! characters move by `P*w - 2P*w = -P*w`, even ones by `P*w`. The copy keeps
//! the original values and keys, so the shared conversion mints its
//! identifiers and binds its tracks in the source-local clock like every other
//! selector.
//!
//! [`candidate`] admits only a profile for which this identity is exact; any
//! other source keeps the omission. Nothing is executed, and the expression is
//! not live: later text-length, Range Selector or Position edits can break the
//! alternation.
//!
//! The expansion is bounded before allocation: at most [`MAX_CHARACTERS`]
//! characters and [`MAX_SELECTORS`] expanded selectors on one text. Every
//! selector visits every character, so the expanded animators evaluate at most
//! 33,280 selector weights per frame.

use fx_schema::{LayerData as FxLayer, PropertyTarget, animator::AnimationGraphEntry};

use crate::{
    properties::{NumericProperty, NumericValueKind, PropertyError},
    rifx::Chunk,
};

use super::super::control_links::{expression, finished, identifier, token};
use super::{
    AnimatorSource, Key, NumericProperties, SelectorSource, SourceText, Value, at, editable_text,
    leaf_storage, numeric_has_keyframes,
};

/// Native match name of an Expression Selector.
pub(super) const EXPRESSION_SELECTOR: &str = "ADBE Text Expressible Selector";
/// Characters that an alternating text may have.
const MAX_CHARACTERS: usize = 256;
/// Selectors that the expanded animators of one text may have. At
/// [`MAX_CHARACTERS`], one animator takes its Range Selector, 128 gates and
/// the copy.
const MAX_SELECTORS: usize = 130;
const POSITION: &str = "ADBE Text Position 3D";
const MODE: &str = "ADBE Text Selector Mode";
/// Native Range Selector Mode codes, as `selector_mode` reads them.
const ADD: f64 = 1.0;
const INTERSECT: f64 = 3.0;

/// The admitted expansion of every alternating animator of one text.
pub(super) struct Candidate {
    /// The source with each alternating animator expanded.
    pub(super) source: SourceText,
    bases: Vec<Base>,
}

/// An expanded animator without its Expression Selector. Its correction
/// animator follows it.
struct Base {
    index: usize,
    /// Keyed Range Selector fields; each must become a track on both copies.
    keyed_fields: usize,
}

/// The Range Selector and static Position of an admitted animator.
struct Pair<'a> {
    range: &'a NumericProperties,
    position: [f64; 2],
}

/// Whether an Expression Selector, with leaf `group` decoded as `properties`,
/// alternates the sign of the preceding selection over Characters. Based On
/// may only keep its Characters default; no other field is admitted.
pub(super) fn alternates_sign(group: &[Chunk], properties: &NumericProperties) -> bool {
    let mut amounts = 0;
    for (name, property) in properties {
        let Ok(property) = property else {
            return false;
        };
        match name.as_str() {
            "ADBE Text Expressible Amount" if property.expression_enabled => amounts += 1,
            "ADBE Text Range Type2" if is_static(property, 1.0) => {}
            _ => return false,
        }
    }
    amounts == 1
        && leaf_storage(&[group], "ADBE Text Expressible Amount")
            .and_then(expression)
            .is_ok_and(alternating_sign_expression)
}

/// Whether `text` is exactly `if (textIndex % 2 == 0) { selectorValue } else
/// { -selectorValue }`, with any whitespace and optional semicolons.
fn alternating_sign_expression(text: &str) -> bool {
    statement_rest(text).is_some_and(finished)
}

/// The text after the alternating-sign `if` statement.
fn statement_rest(mut text: &str) -> Option<&str> {
    fn word(text: &mut &str, expected: &str) -> Option<()> {
        (identifier(text)? == expected).then_some(())
    }
    fn block(text: &mut &str, negated: bool) -> Option<()> {
        token(text, "{")?;
        if negated {
            token(text, "-")?;
        }
        word(text, "selectorValue")?;
        let _ = token(text, ";");
        token(text, "}")
    }
    word(&mut text, "if")?;
    token(&mut text, "(")?;
    word(&mut text, "textIndex")?;
    for part in ["%", "2", "==", "0", ")"] {
        token(&mut text, part)?;
    }
    block(&mut text, false)?;
    word(&mut text, "else")?;
    block(&mut text, true)?;
    Some(text)
}

/// Every alternating animator of `source` expanded, `None` when it has none,
/// or why the expansion is not admitted.
pub(super) fn candidate(source: &SourceText) -> Result<Option<Candidate>, String> {
    if !source.animators.iter().any(alternating) {
        return Ok(None);
    }
    let characters = characters(source)?;
    let pairs = source
        .animators
        .iter()
        .map(pair)
        .collect::<Result<Vec<_>, _>>()?;
    let expanded = pairs.iter().flatten().count();
    let selectors = expanded.saturating_mul(characters.div_ceil(2) + 2);
    if selectors > MAX_SELECTORS {
        return Err(format!(
            "its {selectors} expanded selectors exceed the bound of {MAX_SELECTORS}"
        ));
    }
    let mut animators = Vec::with_capacity(source.animators.len() + expanded);
    let mut bases = Vec::with_capacity(expanded);
    for (animator, pair) in source.animators.iter().zip(&pairs) {
        let Some(pair) = pair else {
            animators.push(animator.clone());
            continue;
        };
        bases.push(Base {
            index: animators.len(),
            keyed_fields: pair
                .range
                .iter()
                .filter(|(_, field)| field.as_ref().is_ok_and(numeric_has_keyframes))
                .count(),
        });
        animators.push(AnimatorSource {
            name: animator.name.clone(),
            properties: animator.properties.clone(),
            selectors: vec![SelectorSource::Range {
                properties: pair.range.clone(),
            }],
            evaluated: animator.evaluated.clone(),
        });
        animators.push(correction(animator, pair, characters));
    }
    Ok(Some(Candidate {
        source: SourceText {
            animators,
            ..source.clone()
        },
        bases,
    }))
}

fn alternating(animator: &AnimatorSource) -> bool {
    animator
        .selectors
        .iter()
        .any(|selector| matches!(selector, SelectorSource::AlternatingSign { .. }))
}

/// The character count of one static line in one paragraph and character
/// style run, after the importer's terminal-return normalization. The text
/// must be printable ASCII, spaces included, so FX character indices equal
/// AE's one-based Characters `textIndex` minus one.
fn characters(source: &SourceText) -> Result<usize, String> {
    const KEYED: &str = "Source Text is keyed or expression-driven";
    let [document] = source.documents.as_slice() else {
        return Err(KEYED.into());
    };
    if source.percent.is_some() || !source.document_is_static || !source.document_starts.is_empty()
    {
        return Err(KEYED.into());
    }
    let raw = at(document, &[Key::Name("0"), Key::Name("0")])
        .and_then(Value::as_str)
        .unwrap_or_default();
    let units = raw.encode_utf16().count() as f64;
    let one_run = |key| {
        at(document, &[Key::Name("0"), Key::Name(key), Key::Name("0")])
            .and_then(Value::as_array)
            .is_some_and(
                |runs| matches!(runs, [run] if run.get("1").and_then(Value::as_f64) == Some(units)),
            )
    };
    if !one_run("5") || !one_run("6") {
        return Err("the text has more than one paragraph or character style run".into());
    }
    let text = editable_text(raw);
    if text.is_empty()
        || text.len() > MAX_CHARACTERS
        || !text.bytes().all(|byte| matches!(byte, b' '..=b'~'))
    {
        return Err(format!(
            "the text is not one line of 1 to {MAX_CHARACTERS} printable ASCII characters"
        ));
    }
    Ok(text.len())
}

/// The admitted Range Selector and Position of an alternating animator, or
/// `None` for an animator without an alternating Expression Selector.
fn pair(animator: &AnimatorSource) -> Result<Option<Pair<'_>>, String> {
    if !alternating(animator) {
        return Ok(None);
    }
    let rejected = |reason: &str| format!("Text {} {reason}", animator.name);
    let [
        SelectorSource::Range { properties: range },
        SelectorSource::AlternatingSign { .. },
    ] = animator.selectors.as_slice()
    else {
        return Err(rejected(
            "does not have one Range Selector directly followed by the Expression Selector",
        ));
    };
    let position = static_position(&animator.properties)
        .ok_or_else(|| rejected("does not animate only a static, finite 2D Position"))?;
    ordinary_range(range).map_err(rejected)?;
    Ok(Some(Pair { range, position }))
}

/// `P` when the only property is a static Position with a zero Z, and both
/// `P` and `-2P` are finite.
fn static_position(properties: &NumericProperties) -> Option<[f64; 2]> {
    let [(name, Ok(position))] = properties.as_slice() else {
        return None;
    };
    let (x, y, z) = match position.values[..] {
        [x, y] => (x, y, 0.0),
        [x, y, z] => (x, y, z),
        _ => return None,
    };
    let fixed = name == POSITION
        && !position.animated
        && position.keyframes.is_empty()
        && !position.expression_enabled
        && !position.dimensions_separated;
    (fixed
        && z == 0.0
        && [x, y, -2.0 * x, -2.0 * y]
            .iter()
            .all(|value| value.is_finite()))
    .then_some([x, y])
}

/// A nonrandom Characters Add Range Selector with a static Amount from 0 to
/// 100% keeps every weight in [0, 1], as the identity requires. Its fields
/// must be decoded and have no unlowered expression.
fn ordinary_range(range: &NumericProperties) -> Result<(), &'static str> {
    if !range
        .iter()
        .all(|(_, field)| field.as_ref().is_ok_and(|field| !field.expression_enabled))
    {
        return Err("Range Selector has a malformed field or an unlowered expression");
    }
    let field = |name: &str| {
        range
            .iter()
            .find(|(candidate, _)| candidate == name)
            .and_then(|(_, field)| field.as_ref().ok())
    };
    let keeps = |name, value| field(name).is_none_or(|field| is_static(field, value));
    if !(keeps(MODE, ADD)
        && keeps("ADBE Text Range Type2", 1.0)
        && keeps("ADBE Text Randomize Order", 0.0))
    {
        return Err("Range Selector is not a nonrandom Characters Add selection");
    }
    let amount = field("ADBE Text Selector Max Amount").map_or(Some(100.0), static_value);
    if !amount.is_some_and(|amount| (0.0..=100.0).contains(&amount)) {
        return Err("Range Selector Amount is not static from 0 to 100%");
    }
    Ok(())
}

fn static_value(property: &NumericProperty) -> Option<f64> {
    match property.values[..] {
        [value]
            if !property.animated
                && property.keyframes.is_empty()
                && !property.expression_enabled =>
        {
            Some(value)
        }
        _ => None,
    }
}

fn is_static(property: &NumericProperty, value: f64) -> bool {
    static_value(property) == Some(value)
}

/// `-2P` on the odd one-based characters, intersected with a copy of the
/// Range Selector.
fn correction(animator: &AnimatorSource, pair: &Pair<'_>, characters: usize) -> AnimatorSource {
    let mut copy: NumericProperties = pair
        .range
        .iter()
        .filter(|(name, _)| name != MODE)
        .cloned()
        .collect();
    copy.push(native(MODE, &[INTERSECT]));
    let mut selectors: Vec<_> = (0..characters).step_by(2).map(odd_gate).collect();
    selectors.push(SelectorSource::Range { properties: copy });
    let [x, y] = pair.position;
    AnimatorSource {
        name: format!("{} alternating position", animator.name),
        // Adding zero keeps a zero component unsigned.
        properties: vec![native(POSITION, &[-2.0 * x + 0.0, -2.0 * y + 0.0, 0.0])],
        selectors,
        evaluated: Vec::new(),
    }
}

/// An Index/Square Add gate that fully selects zero-based character `index`
/// alone: AE's odd one-based `textIndex` `index + 1`.
fn odd_gate(index: usize) -> SelectorSource {
    let start = index as f64;
    SelectorSource::Range {
        properties: vec![
            native("ADBE Text Range Units", &[2.0]),
            native("ADBE Text Range Type2", &[1.0]),
            native(MODE, &[ADD]),
            native("ADBE Text Range Shape", &[1.0]),
            native("ADBE Text Index Start", &[start]),
            native("ADBE Text Index End", &[start + 1.0]),
        ],
    }
}

/// A static decoded native leaf.
fn native(name: &str, values: &[f64]) -> (String, Result<NumericProperty, PropertyError>) {
    (
        name.to_owned(),
        Ok(NumericProperty {
            values: values.to_vec(),
            animated: false,
            expression_enabled: false,
            expression_present: false,
            dimensions_separated: false,
            keyframes: Vec::new(),
            value_kind: NumericValueKind::Continuous,
        }),
    )
}

impl Candidate {
    /// Whether every keyed field of each expanded Range Selector became one
    /// editable track on both copies. Otherwise a correction could stay
    /// frozen while its base animates.
    pub(super) fn tracks_complete(
        &self,
        layers: &[FxLayer],
        entries: &[AnimationGraphEntry],
    ) -> bool {
        let [FxLayer::Text(text)] = layers else {
            return false;
        };
        let tracks = |selector: &fx_schema::RangeSelector| {
            entries
                .iter()
                .filter(|entry| {
                    matches!(&entry.target, PropertyTarget::FxItemProperty(target)
                        if target.item_id() == selector.id)
                })
                .count()
        };
        self.bases.iter().all(|base| {
            let range = text
                .animators
                .get(base.index)
                .and_then(|animator| animator.selectors.first());
            let copy = text
                .animators
                .get(base.index + 1)
                .and_then(|correction| correction.selectors.last());
            range.zip(copy).is_some_and(|(range, copy)| {
                tracks(range) == base.keyed_fields && tracks(copy) == base.keyed_fields
            })
        })
    }

    /// One diagnostic for each lowered animator.
    pub(super) fn lowered(&self) -> impl Iterator<Item = String> + '_ {
        self.bases.iter().map(|base| {
            let animators = &self.source.animators;
            format!(
                "Text {} alternating textIndex Expression Selector lowered to the editable correction animator {:?} (-2x Position on odd one-based characters, intersected with a Range Selector copy); the expression is not live, so text-length, Range Selector or Position edits can break the alternation",
                animators[base.index].name,
                animators[base.index + 1].name
            )
        })
    }
}

/// The diagnostic for an alternating Expression Selector that is not lowered.
pub(super) fn not_lowered(reason: &str) -> String {
    format!(
        "alternating textIndex Expression Selector not lowered ({reason}); it is omitted, so every character moves in the same direction"
    )
}

#[cfg(test)]
mod tests;
