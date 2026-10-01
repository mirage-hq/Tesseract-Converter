//! Bounded lowering of text animator expressions to ordinary editable values.
//!
//! An admitted expression is an exact affine function of at most one
//! same-layer Slider curve. It may use finite decimal numbers, Slider values
//! (`effect(name)(1)` or a named value parameter), earlier `var` bindings that
//! are all used, unary and binary `+`/`-`, `value[i]` of a static vector owner,
//! `linear(t, tMin, tMax, v1, v2)` with static bounds, and
//! `text.animator(name).selector(name).field` aliases of percentage Range
//! Selector fields on the same text. The result is a static value, or the
//! Slider's own keys with mapped values and speeds in the shared layer clock.
//! Nothing is executed or sampled. Every other form, ambiguous name, cycle and
//! clamping case is rejected, so the caller keeps the stored value.
//!
//! Work is bounded before evaluation: an expression has at most
//! [`MAX_EXPRESSION_BYTES`] and [`MAX_BINDINGS`], `linear()` does not nest
//! (it is the grammar's only self-nesting form), and alias resolution stops
//! after [`MAX_ALIASES`]. Recursion depth therefore stays small.

use crate::{
    properties::{
        NumericProperty, NumericValueKind, PropertyError, read_numeric, root_runs, runs,
        unique_list,
    },
    rifx::Chunk,
};

use super::super::control_links::{
    display_name, expression, finished, finite_number, identifier, quoted, reference, resolve,
    token,
};
use super::range_default;

/// AE's stored display name for a group that keeps its default name.
const DEFAULT_NAME: &str = "-_0_/-";
/// Alias resolutions allowed while lowering one property.
const MAX_ALIASES: usize = 16;
/// Bytes of one expression that may be parsed.
const MAX_EXPRESSION_BYTES: usize = 4096;
/// `var` bindings allowed in one expression.
const MAX_BINDINGS: usize = 8;
/// Names a binding cannot take: JS reserved words, non-writable globals and
/// the names this grammar interprets itself.
const RESERVED: &[&str] = &[
    "Infinity",
    "NaN",
    "arguments",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "effect",
    "else",
    "enum",
    "eval",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "linear",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "super",
    "switch",
    "text",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "undefined",
    "value",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

/// Same-layer Slider Controls and Range Selectors that text expressions name.
pub(super) struct ExpressionLinks<'a> {
    effects: Result<Vec<(&'a str, &'a [Chunk])>, PropertyError>,
    animators: Result<Vec<Animator<'a>>, PropertyError>,
}

/// A native Text Animator, enabled or not: expressions can name either.
struct Animator<'a> {
    name: Option<&'a str>,
    selectors: Vec<Selector<'a>>,
}

struct Selector<'a> {
    name: Option<&'a str>,
    /// Native selector match name, such as `ADBE Text Selector`.
    kind: &'a str,
    /// Leaf runs of the selector and of its Range Advanced group.
    leaves: Vec<(&'a str, &'a [Chunk])>,
}

/// An exact affine image of at most one Slider curve, in source units.
#[derive(Clone, Debug)]
enum Scalar {
    /// A finite static value.
    Constant(f64),
    /// Finite scalar keys without a static value or expression.
    Curve(NumericProperty),
}

/// The value of a complete expression.
enum Value {
    Scalar(Scalar),
    /// Static components of a vector literal.
    Vector(Vec<f64>),
}

struct Binding<'a> {
    name: &'a str,
    value: Scalar,
    used: bool,
}

/// Range Selector fields being resolved for one lowered property.
#[derive(Default)]
struct Aliases {
    /// `(animator, selector, field)` addresses on the current alias path.
    path: Vec<(usize, usize, &'static str)>,
    /// Resolutions so far, bounded by [`MAX_ALIASES`].
    resolved: usize,
}

/// One expression's bindings and the property that owns it.
struct Evaluation<'l, 'a> {
    links: &'l ExpressionLinks<'a>,
    /// The owner's stored value, which `value[i]` reads.
    own: &'l NumericProperty,
    bindings: Vec<Binding<'a>>,
    aliases: &'l mut Aliases,
    /// Set while the arguments of a `linear()` call are parsed.
    in_linear: bool,
}

impl<'a> ExpressionLinks<'a> {
    /// Indexes the layer's Effect Parade and every animator of its text group.
    pub(super) fn new(content: &'a [Chunk], text_group: &'a [Chunk]) -> Self {
        Self {
            effects: effect_runs(content),
            animators: animators(text_group),
        }
    }

    /// Lowers the enabled expression in `storage`, whose stored value is `own`,
    /// to independent editable values or keys.
    pub(super) fn lower(
        &self,
        storage: &'a [Chunk],
        own: &NumericProperty,
    ) -> Result<NumericProperty, PropertyError> {
        let value = self.evaluate(expression(storage)?, own, &mut Aliases::default())?;
        value.into_property(own)
    }

    fn evaluate(
        &self,
        text: &'a str,
        own: &NumericProperty,
        aliases: &mut Aliases,
    ) -> Result<Value, PropertyError> {
        if text.len() > MAX_EXPRESSION_BYTES {
            return Err(PropertyError::Layout("expression exceeds the bounded size"));
        }
        Evaluation {
            links: self,
            own,
            bindings: Vec::new(),
            aliases,
            in_linear: false,
        }
        .program(text)
    }

    fn effects(&self) -> Result<&[(&'a str, &'a [Chunk])], PropertyError> {
        self.effects.as_deref().map_err(Clone::clone)
    }

    /// The resolved value of one Range Selector field.
    fn field(
        &self,
        selector: &Selector<'a>,
        field: &'static str,
        aliases: &mut Aliases,
    ) -> Result<Scalar, PropertyError> {
        let Some(storage) = selector.storage(field)? else {
            // A sparse selector keeps AE's native default; no curve is invented.
            return Ok(Scalar::Constant(range_default(field)));
        };
        let numeric = read_numeric(storage)?;
        if !numeric.expression_enabled {
            return Scalar::from_numeric(numeric);
        }
        match self.evaluate(expression(storage)?, &numeric, aliases)? {
            Value::Scalar(value) => Ok(value),
            Value::Vector(_) => Err(PropertyError::Layout(
                "aliased Range Selector field is not scalar",
            )),
        }
    }
}

impl<'a> Evaluation<'_, 'a> {
    /// `binding* (sum | vector) [;]`, consumed completely.
    fn program(&mut self, mut text: &'a str) -> Result<Value, PropertyError> {
        while let Some(name) = binding(&mut text) {
            if self.bindings.len() == MAX_BINDINGS {
                return Err(PropertyError::Layout(
                    "expression exceeds the bounded binding count",
                ));
            }
            if RESERVED.contains(&name) {
                return Err(PropertyError::Layout("reserved expression binding name"));
            }
            if self.bindings.iter().any(|binding| binding.name == name) {
                return Err(PropertyError::Layout("duplicate expression binding"));
            }
            let value = self.sum(&mut text)?;
            token(&mut text, ";").ok_or(PropertyError::Layout(
                "expression binding lacks its semicolon",
            ))?;
            self.bindings.push(Binding {
                name,
                value,
                used: false,
            });
        }
        let value = if token(&mut text, "[").is_some() {
            Value::Vector(self.vector(&mut text)?)
        } else {
            Value::Scalar(self.sum(&mut text)?)
        };
        if !finished(text) {
            return Err(PropertyError::Layout(
                "unsupported expression suffix or extra statement",
            ));
        }
        if self.bindings.iter().any(|binding| !binding.used) {
            return Err(PropertyError::Layout("unused expression binding"));
        }
        Ok(value)
    }

    /// The rest of `[a, b, ...]`: two to four static components.
    fn vector(&mut self, text: &mut &'a str) -> Result<Vec<f64>, PropertyError> {
        let mut values = Vec::new();
        loop {
            let value = self.sum(text)?.constant().ok_or(PropertyError::Layout(
                "vector expression components must be static",
            ))?;
            values.push(value);
            if token(text, "]").is_some() {
                break;
            }
            if values.len() == 4 || token(text, ",").is_none() {
                return Err(PropertyError::Layout("unsupported vector expression"));
            }
        }
        if values.len() < 2 {
            return Err(PropertyError::Layout("unsupported vector expression"));
        }
        Ok(values)
    }

    /// `[-] operand ((+|-) operand)*`.
    fn sum(&mut self, text: &mut &'a str) -> Result<Scalar, PropertyError> {
        *text = text.trim_start();
        let negative = text.starts_with('-') && !text.starts_with("--");
        if negative {
            *text = &text[1..];
        }
        let mut value = self.operand(text)?;
        if negative {
            value = value.affine(-1.0, 0.0)?;
        }
        loop {
            let sign = if token(text, "+").is_some() {
                1.0
            } else if token(text, "-").is_some() {
                -1.0
            } else {
                return Ok(value);
            };
            // `a--b` and `a++b` are not sums in JS; signed operands are not needed.
            if text.trim_start().starts_with(['+', '-']) {
                return Err(PropertyError::Layout("unsupported adjacent sign operators"));
            }
            let operand = self.operand(text)?;
            value = value.add(operand, sign)?;
        }
    }

    fn operand(&mut self, text: &mut &'a str) -> Result<Scalar, PropertyError> {
        *text = text.trim_start();
        if text.starts_with(|character: char| character.is_ascii_digit() || character == '.') {
            return number(text)
                .map(Scalar::Constant)
                .ok_or(PropertyError::Layout("unsupported numeric literal"));
        }
        let start = *text;
        let name =
            identifier(text).ok_or(PropertyError::Layout("unsupported expression operand"))?;
        match name {
            "effect" => {
                *text = start;
                let link = reference(text)
                    .ok_or(PropertyError::Layout("not a complete Slider reference"))?;
                Scalar::from_numeric(resolve(self.links.effects()?, link)?)
            }
            "text" => {
                *text = start;
                let field = selector_field(text).ok_or(PropertyError::Layout(
                    "not a complete Range Selector field alias",
                ))?;
                self.alias(field)
            }
            "linear" => {
                if self.in_linear {
                    return Err(PropertyError::Layout(
                        "nested linear() calls are not supported",
                    ));
                }
                self.in_linear = true;
                let value = self.linear(text);
                self.in_linear = false;
                value
            }
            "value" => self.own_component(text),
            _ => {
                let binding = self
                    .bindings
                    .iter_mut()
                    .find(|binding| binding.name == name)
                    .ok_or(PropertyError::Layout("unknown expression identifier"))?;
                binding.used = true;
                Ok(binding.value.clone())
            }
        }
    }

    /// The rest of `linear(t, tMin, tMax, v1, v2)` after its name.
    fn linear(&mut self, text: &mut &'a str) -> Result<Scalar, PropertyError> {
        const ARGUMENTS: PropertyError =
            PropertyError::Layout("linear() needs exactly five arguments");
        token(text, "(").ok_or(ARGUMENTS)?;
        let input = self.sum(text)?;
        let mut bounds = [0.0; 4];
        for bound in &mut bounds {
            token(text, ",").ok_or(ARGUMENTS)?;
            *bound = self
                .sum(text)?
                .constant()
                .ok_or(PropertyError::Layout("linear() bounds must be static"))?;
        }
        token(text, ")").ok_or(ARGUMENTS)?;
        input.linear(bounds)
    }

    /// The rest of `value[i]`: one component of a static vector owner.
    fn own_component(&self, text: &mut &'a str) -> Result<Scalar, PropertyError> {
        const INDEX: PropertyError = PropertyError::Layout("unsupported value component access");
        token(text, "[").ok_or(INDEX)?;
        *text = text.trim_start();
        let index = text
            .chars()
            .next()
            .and_then(|digit| digit.to_digit(10))
            .ok_or(INDEX)?;
        *text = &text[1..];
        token(text, "]").ok_or(INDEX)?;
        if self.own.animated || self.own.values.len() < 2 {
            return Err(PropertyError::Layout(
                "value[i] needs a static vector property",
            ));
        }
        let index = usize::try_from(index).map_err(|_| INDEX)?;
        self.own
            .values
            .get(index)
            .copied()
            .map(Scalar::Constant)
            .ok_or(INDEX)
    }

    /// A field of a uniquely named Range Selector on the same text.
    fn alias(
        &mut self,
        (animator, selector, field): (&'a str, &'a str, &'static str),
    ) -> Result<Scalar, PropertyError> {
        let animators = self.links.animators.as_ref().map_err(Clone::clone)?;
        let mut matches = animators
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.name == Some(animator));
        let (animator_index, animator) = matches
            .next()
            .ok_or(PropertyError::Layout("aliased text animator is absent"))?;
        if matches.next().is_some() {
            return Err(PropertyError::Layout("ambiguous text animator name"));
        }
        let mut matches = animator
            .selectors
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.name == Some(selector));
        let (selector_index, selector) = matches
            .next()
            .ok_or(PropertyError::Layout("aliased Range Selector is absent"))?;
        if matches.next().is_some() {
            return Err(PropertyError::Layout("ambiguous Range Selector name"));
        }
        if selector.kind != "ADBE Text Selector" {
            return Err(PropertyError::Layout(
                "aliased selector is not a Range Selector",
            ));
        }
        selector.require_percentage_units()?;
        let address = (animator_index, selector_index, field);
        if self.aliases.path.contains(&address) {
            return Err(PropertyError::Layout("cyclic Range Selector alias"));
        }
        self.aliases.resolved += 1;
        if self.aliases.resolved > MAX_ALIASES {
            return Err(PropertyError::Layout(
                "Range Selector aliases exceed the bounded work",
            ));
        }
        self.aliases.path.push(address);
        let value = self.links.field(selector, field, self.aliases);
        self.aliases.path.pop();
        value
    }
}

impl<'a> Selector<'a> {
    fn read(kind: &'a str, run: &'a [Chunk]) -> Result<Self, PropertyError> {
        let group = unique_list(run, *b"tdgp")?;
        let mut leaves = Vec::new();
        for (name, run) in runs(group)? {
            match unique_list(run, *b"tdgp") {
                Ok(advanced) if name == "ADBE Text Range Advanced" => {
                    leaves.extend(runs(advanced)?)
                }
                _ => leaves.push((name, run)),
            }
        }
        Ok(Self {
            name: label(group),
            kind,
            leaves,
        })
    }

    fn storage(&self, match_name: &str) -> Result<Option<&'a [Chunk]>, PropertyError> {
        let mut matches = self.leaves.iter().filter(|(name, _)| *name == match_name);
        let Some((_, run)) = matches.next() else {
            return Ok(None);
        };
        if matches.next().is_some() {
            return Err(PropertyError::Layout("duplicate Range Selector field"));
        }
        unique_list(run, *b"tdbs").map(Some)
    }

    /// Field aliases name percentage values; Index units are not admitted.
    fn require_percentage_units(&self) -> Result<(), PropertyError> {
        let Some(storage) = self.storage("ADBE Text Range Units")? else {
            return Ok(());
        };
        let units = read_numeric(storage)?;
        if units.expression_enabled || units.animated || units.values != [1.0] {
            return Err(PropertyError::Layout(
                "aliased Range Selector does not use percentage units",
            ));
        }
        Ok(())
    }
}

impl Scalar {
    /// A scalar Slider value or Range Selector field without an enabled expression.
    fn from_numeric(mut numeric: NumericProperty) -> Result<Self, PropertyError> {
        if numeric.expression_enabled
            || numeric.dimensions_separated
            || numeric.value_kind == NumericValueKind::Color
        {
            return Err(PropertyError::Layout("unsupported linked control value"));
        }
        if !numeric.animated {
            let [value] = numeric.values[..] else {
                return Err(PropertyError::Layout("linked control is not scalar"));
            };
            return finite(value).map(Self::Constant);
        }
        let scalar = |values: &[f64]| values.len() <= 1 && values.iter().all(|v| v.is_finite());
        let unsupported = numeric.keyframes.is_empty()
            || numeric.keyframes.iter().any(|key| {
                key.values.len() != 1
                    || !scalar(&key.values)
                    || !key.time_secs.is_finite()
                    || !scalar(&key.in_speed)
                    || !scalar(&key.out_speed)
                    || !scalar(&key.in_influence)
                    || !scalar(&key.out_influence)
                    || !key.spatial_in.is_empty()
                    || !key.spatial_out.is_empty()
                    || !matches!(key.in_interpolation, 1..=3)
                    || !matches!(key.out_interpolation, 1..=3)
            });
        if unsupported {
            return Err(PropertyError::Layout("unsupported linked control curve"));
        }
        numeric.values.clear();
        numeric.expression_present = false;
        numeric.value_kind = NumericValueKind::Continuous;
        Ok(Self::Curve(numeric))
    }

    fn constant(&self) -> Option<f64> {
        match self {
            Self::Constant(value) => Some(*value),
            Self::Curve(_) => None,
        }
    }

    /// `scale * self + offset`; key speeds scale with the values.
    fn affine(self, scale: f64, offset: f64) -> Result<Self, PropertyError> {
        match self {
            Self::Constant(value) => finite(scale * value + offset).map(Self::Constant),
            Self::Curve(mut curve) => {
                for key in &mut curve.keyframes {
                    for value in &mut key.values {
                        *value = finite(scale * *value + offset)?;
                    }
                    for speed in key.in_speed.iter_mut().chain(&mut key.out_speed) {
                        *speed = finite(scale * *speed)?;
                    }
                }
                Ok(Self::Curve(curve))
            }
        }
    }

    /// `self + sign * other`, for at most one animated operand.
    fn add(self, other: Self, sign: f64) -> Result<Self, PropertyError> {
        match (self, other) {
            (Self::Constant(left), Self::Constant(right)) => {
                finite(left + sign * right).map(Self::Constant)
            }
            (curve @ Self::Curve(_), Self::Constant(right)) => curve.affine(1.0, sign * right),
            (Self::Constant(left), curve @ Self::Curve(_)) => curve.affine(sign, left),
            (Self::Curve(_), Self::Curve(_)) => Err(PropertyError::Layout(
                "expression combines more than one animated control",
            )),
        }
    }

    /// AE `linear(t, tMin, tMax, v1, v2)`. A curve is admitted only when its
    /// Linear/Hold segments keep every key inside the input range, so the
    /// clamp never applies and the mapping is exactly affine.
    fn linear(self, [t_min, t_max, from, to]: [f64; 4]) -> Result<Self, PropertyError> {
        if t_min >= t_max {
            return Err(PropertyError::Layout(
                "linear() needs an increasing input range",
            ));
        }
        let slope = finite((to - from) / (t_max - t_min))?;
        match self {
            Self::Constant(input) => {
                finite(from + (input.clamp(t_min, t_max) - t_min) * slope).map(Self::Constant)
            }
            Self::Curve(curve) => {
                let confined = curve.keyframes.windows(2).all(|pair| {
                    pair[0].out_interpolation == 3
                        || (pair[0].out_interpolation == 1 && pair[1].in_interpolation == 1)
                }) && curve
                    .keyframes
                    .iter()
                    .all(|key| (t_min..=t_max).contains(&key.values[0]));
                if !confined {
                    return Err(PropertyError::Layout(
                        "linear() input may leave its range or overshoot between keys",
                    ));
                }
                Self::Curve(curve).affine(slope, from - t_min * slope)
            }
        }
    }
}

impl Value {
    fn into_property(self, own: &NumericProperty) -> Result<NumericProperty, PropertyError> {
        let dimensions = if own.animated {
            own.keyframes.first().map(|key| key.values.len())
        } else {
            Some(own.values.len())
        };
        match (self, dimensions) {
            (Self::Scalar(Scalar::Constant(value)), Some(1)) => Ok(static_property(vec![value])),
            (Self::Scalar(Scalar::Curve(curve)), Some(1)) => Ok(curve),
            (Self::Vector(values), Some(dimensions))
                if (2..=dimensions).contains(&values.len()) =>
            {
                Ok(static_property(values))
            }
            _ => Err(PropertyError::Layout(
                "expression result does not match the property dimensions",
            )),
        }
    }
}

fn static_property(values: Vec<f64>) -> NumericProperty {
    NumericProperty {
        values,
        animated: false,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: Vec::new(),
        value_kind: NumericValueKind::Continuous,
    }
}

fn finite(value: f64) -> Result<f64, PropertyError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(PropertyError::NonFinite)
    }
}

/// `[var] name =` at the start of a statement; `==` is not an assignment.
fn binding<'a>(text: &mut &'a str) -> Option<&'a str> {
    let mut rest = text.trim_start();
    if let Some(after) = rest.strip_prefix("var")
        && after.starts_with(char::is_whitespace)
    {
        rest = after;
    }
    let name = identifier(&mut rest)?;
    token(&mut rest, "=")?;
    if rest.starts_with('=') {
        return None;
    }
    *text = rest;
    Some(name)
}

/// An unsigned decimal literal. JS reads `010` as legacy octal, and a
/// literal directly followed by a name character is not a number.
fn number(text: &mut &str) -> Option<f64> {
    let bytes = text.as_bytes();
    if bytes.first() == Some(&b'0') && bytes.get(1).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let value = finite_number(text)?;
    let suffix = text.starts_with(|character: char| {
        character.is_ascii_alphanumeric() || matches!(character, '_' | '$' | '.')
    });
    (!suffix).then_some(value)
}

/// `text.animator(name).selector(name).field` for the percentage fields that
/// bounded rigs alias, as `(animator, selector, field match name)`.
fn selector_field<'a>(text: &mut &'a str) -> Option<(&'a str, &'a str, &'static str)> {
    for part in ["text", ".", "animator", "("] {
        token(text, part)?;
    }
    let animator = quoted(text)?;
    for part in [")", ".", "selector", "("] {
        token(text, part)?;
    }
    let selector = quoted(text)?;
    token(text, ")")?;
    token(text, ".")?;
    let field = match identifier(text)? {
        "start" => "ADBE Text Percent Start",
        "end" => "ADBE Text Percent End",
        "offset" => "ADBE Text Percent Offset",
        "advanced" => {
            token(text, ".")?;
            match identifier(text)? {
                "easeHigh" => "ADBE Text Levels Max Ease",
                "easeLow" => "ADBE Text Levels Min Ease",
                _ => return None,
            }
        }
        _ => return None,
    };
    Some((animator, selector, field))
}

fn label(group: &[Chunk]) -> Option<&str> {
    display_name(group).filter(|name| *name != DEFAULT_NAME)
}

fn effect_runs(content: &[Chunk]) -> Result<Vec<(&str, &[Chunk])>, PropertyError> {
    let root = root_runs(content)?;
    let mut parades = root
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Effect Parade");
    let Some((_, parade)) = parades.next() else {
        return Ok(Vec::new());
    };
    if parades.next().is_some() {
        return Err(PropertyError::Layout("duplicate Effect Parade"));
    }
    runs(unique_list(parade, *b"tdgp")?)
}

fn animators(text_group: &[Chunk]) -> Result<Vec<Animator<'_>>, PropertyError> {
    let mut animators = Vec::new();
    for (name, run) in runs(text_group)? {
        match name {
            "ADBE Text Animators" => {
                for (name, run) in runs(unique_list(run, *b"tdgp")?)? {
                    if name == "ADBE Text Animator" {
                        animators.push(Animator::read(run)?);
                    }
                }
            }
            // Mirrors the reader's tolerance for a directly materialized Animator.
            "ADBE Text Animator" => animators.push(Animator::read(run)?),
            _ => {}
        }
    }
    Ok(animators)
}

impl<'a> Animator<'a> {
    fn read(run: &'a [Chunk]) -> Result<Self, PropertyError> {
        let group = unique_list(run, *b"tdgp")?;
        let mut selectors = Vec::new();
        for (name, run) in runs(group)? {
            if name == "ADBE Text Selectors" {
                for (kind, run) in runs(unique_list(run, *b"tdgp")?)? {
                    selectors.push(Selector::read(kind, run)?);
                }
            }
        }
        Ok(Self {
            name: label(group),
            selectors,
        })
    }
}

#[cfg(test)]
mod tests;
