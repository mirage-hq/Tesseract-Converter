//! Typed variables on Groups and the composition, and links that read them.
//!
//! A Group or the composition defines `variables`. A layer property or a
//! Group variable can hold `{"type": "linked", "value": name}` instead of a
//! value. The link reads the nearest ancestor that defines
//! `name`, like React context: a property's lookup starts at its own layer, a
//! Group variable's lookup starts at the Group's parent, and the composition
//! is the last scope.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::de::{self, DeserializeOwned, Deserializer};
use serde::{Deserialize, Serialize, Serializer};
use ts_rs::TS;

use crate::{AssetId, ColorProperty, NonNegativeProperty, PositiveProperty, ScalarProperty};

/// Name of a variable, unique within the Group or composition defining it.
pub type VariableName = String;

/// Variables defined by one Group or composition, keyed by name.
pub type Variables = BTreeMap<VariableName, Linkable<VariableValue>>;

/// A variable's value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
#[ts(export_to = "project_types.d.ts")]
pub enum VariableValue {
    /// Text, read by Text layer `sourceText.text`.
    Text(String),
    /// Extended-sRGB RGB 0..64 and coverage alpha 0..1, read by fill and stroke colors.
    Color(#[serde(deserialize_with = "crate::deserialize_extended_color")] ColorProperty),
    /// Captions image asset id, read by an Image layer's source.
    Image(AssetId),
    /// A finite number, read by Text font size, stroke width and tracking.
    Number(#[serde(deserialize_with = "deserialize_finite")] f64),
    /// A font family name, read by Text font family. Each Text layer keeps
    /// its own font style.
    Font(String),
}

fn deserialize_finite<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    let value = f64::deserialize(deserializer)?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(de::Error::custom("a number variable must be finite"))
    }
}

impl VariableValue {
    /// Read a raw value, a string, a number or a `[r, g, b, a]` color, as a variable of
    /// type `variable_type`, with the same checks as loading a document.
    pub fn from_raw(
        variable_type: VariableType,
        value: serde_json::Value,
    ) -> Result<Self, serde_json::Error> {
        serde_json::from_value(serde_json::json!({"type": variable_type.label(), "value": value}))
    }

    /// This value's type.
    #[must_use]
    pub const fn variable_type(&self) -> VariableType {
        match self {
            Self::Text(_) => VariableType::Text,
            Self::Color(_) => VariableType::Color,
            Self::Image(_) => VariableType::Image,
            Self::Number(_) => VariableType::Number,
            Self::Font(_) => VariableType::Font,
        }
    }
}

/// The type of a variable, and of the properties that can link to one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableType {
    Text,
    Color,
    Image,
    Number,
    Font,
}

impl VariableType {
    /// The wire tag (`text`, `color`, `image`, `number` or `font`).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Color => "color",
            Self::Image => "image",
            Self::Number => "number",
            Self::Font => "font",
        }
    }
}

/// Wire shape of a link: `{"type": "linked", "value": name}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
#[ts(export_to = "project_types.d.ts")]
pub enum VariableLink {
    /// Reads the nearest ancestor variable with this name.
    Linked(VariableName),
}

/// A value, or a link to a variable that supplies it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Linkable<T> {
    Value(T),
    Linked(VariableName),
}

impl<T> Linkable<T> {
    /// The plain value, or `None` while this follows a variable.
    #[must_use]
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Linked(_) => None,
        }
    }

    /// Borrow the value, keeping a link as it is.
    #[must_use]
    pub fn as_ref(&self) -> Linkable<&T> {
        match self {
            Self::Value(value) => Linkable::Value(value),
            Self::Linked(name) => Linkable::Linked(name.clone()),
        }
    }

    /// The variable this follows, if any.
    #[must_use]
    pub fn link(&self) -> Option<&str> {
        match self {
            Self::Value(_) => None,
            Self::Linked(name) => Some(name),
        }
    }
}

impl<T: LinkTarget> Linkable<T> {
    /// The value this holds or links to in `scope`.
    #[must_use]
    pub fn resolve(&self, scope: &VariableScope) -> Option<T> {
        match self {
            Self::Value(value) => Some(value.clone()),
            Self::Linked(name) => scope.get(name).and_then(T::from_variable),
        }
    }

    /// Replace a link with the value it reads in `scope`. A link that does not
    /// resolve stays, which validation never lets a document reach.
    pub fn bake(&mut self, scope: &VariableScope) {
        if let Some(value) = self.link().and_then(|_| self.resolve(scope)) {
            *self = Self::Value(value);
        }
    }

    /// Rewrite a link to a name in `substitutions` with that entry: a value
    /// replaces the link and a link re-points it.
    pub fn substitute(&mut self, substitutions: &Variables) {
        let Some(substitution) = self.link().and_then(|name| substitutions.get(name)) else {
            return;
        };
        match substitution {
            Linkable::Value(variable) => {
                if let Some(value) = T::from_variable(variable) {
                    *self = Self::Value(value);
                }
            }
            Linkable::Linked(target) => *self = Self::Linked(target.clone()),
        }
    }
}

/// A number property a `number` variable can supply.
pub trait NumberProperty: LinkTarget {
    /// The plain number.
    fn number(&self) -> f64;
}

impl NumberProperty for ScalarProperty {
    fn number(&self) -> f64 {
        *self
    }
}

impl NumberProperty for PositiveProperty {
    fn number(&self) -> f64 {
        self.value()
    }
}

impl NumberProperty for NonNegativeProperty {
    fn number(&self) -> f64 {
        self.value()
    }
}

impl<T: NumberProperty> Linkable<T> {
    /// The plain number, or `None` while this follows a variable.
    #[must_use]
    pub fn number(&self) -> Option<f64> {
        self.value().map(T::number)
    }
}

/// One linkable property, visited without naming its value type.
pub trait LinkSlot {
    /// See [`Linkable::bake`].
    fn bake(&mut self, scope: &VariableScope);
    /// See [`Linkable::substitute`].
    fn substitute(&mut self, substitutions: &Variables);
    /// Check that a link reads a variable in `scope` this property accepts:
    /// one of its type, and for a number one in the property's range.
    fn check(&self, scope: &VariableScope, owner: &dyn Fn() -> String)
        -> Result<(), VariableError>;
}

impl<T: LinkTarget> LinkSlot for Linkable<T> {
    fn bake(&mut self, scope: &VariableScope) {
        Linkable::bake(self, scope);
    }

    fn substitute(&mut self, substitutions: &Variables) {
        Linkable::substitute(self, substitutions);
    }

    fn check(
        &self,
        scope: &VariableScope,
        owner: &dyn Fn() -> String,
    ) -> Result<(), VariableError> {
        let Self::Linked(name) = self else {
            return Ok(());
        };
        if let Some(expected) = T::VARIABLE_TYPE {
            check_link(scope, name, expected, owner)?;
        }
        match scope.get(name).map(T::from_variable) {
            None => Err(VariableError::Unresolved {
                owner: owner(),
                name: name.clone(),
            }),
            Some(None) => Err(VariableError::OutOfRange {
                owner: owner(),
                name: name.clone(),
            }),
            Some(Some(_)) => Ok(()),
        }
    }
}

impl<T> From<T> for Linkable<T> {
    fn from(value: T) -> Self {
        Self::Value(value)
    }
}

impl From<&str> for Linkable<String> {
    fn from(value: &str) -> Self {
        Self::Value(value.to_owned())
    }
}

/// A plain value equals the same value and never a link.
impl<T: PartialEq> PartialEq<T> for Linkable<T> {
    fn eq(&self, other: &T) -> bool {
        self.value() == Some(other)
    }
}

impl PartialEq<str> for Linkable<String> {
    fn eq(&self, other: &str) -> bool {
        self.value().is_some_and(|value| value == other)
    }
}

impl PartialEq<&str> for Linkable<String> {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

impl From<&str> for Linkable<Arc<str>> {
    fn from(value: &str) -> Self {
        Self::Value(Arc::from(value))
    }
}

impl From<String> for Linkable<Arc<str>> {
    fn from(value: String) -> Self {
        Self::Value(Arc::from(value))
    }
}

impl PartialEq<str> for Linkable<Arc<str>> {
    fn eq(&self, other: &str) -> bool {
        self.value().is_some_and(|value| &**value == other)
    }
}

impl PartialEq<&str> for Linkable<Arc<str>> {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

impl<T: Default> Default for Linkable<T> {
    fn default() -> Self {
        Self::Value(T::default())
    }
}

impl<T: Serialize> Serialize for Linkable<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Value(value) => value.serialize(serializer),
            Self::Linked(name) => VariableLink::Linked(name.clone()).serialize(serializer),
        }
    }
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for Linkable<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        read_linkable(serde_json::Value::deserialize(deserializer)?, |value| {
            serde_json::from_value(value)
        })
        .map_err(de::Error::custom)
    }
}

fn read_linkable<T>(
    value: serde_json::Value,
    read_value: impl FnOnce(serde_json::Value) -> Result<T, serde_json::Error>,
) -> Result<Linkable<T>, serde_json::Error> {
    if value.get("type").and_then(serde_json::Value::as_str) == Some("linked") {
        let VariableLink::Linked(name) = serde_json::from_value(value)?;
        return Ok(Linkable::Linked(name));
    }
    read_value(value).map(Linkable::Value)
}

/// Deserialize a linkable extended-sRGB color, checking a plain value's range.
pub fn deserialize_linkable_extended_color<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Linkable<ColorProperty>, D::Error> {
    read_linkable(serde_json::Value::deserialize(deserializer)?, |value| {
        crate::deserialize_extended_color(value)
    })
    .map_err(de::Error::custom)
}

/// Deserialize an optional linkable color without changing absent/null semantics.
pub fn deserialize_optional_linkable_extended_color<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Linkable<ColorProperty>>, D::Error> {
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::Null => Ok(None),
        value => read_linkable(value, crate::deserialize_extended_color)
            .map(Some)
            .map_err(de::Error::custom),
    }
}

/// A property value a variable can supply.
pub trait LinkTarget: Clone {
    /// The type of variable this property reads, or `None` where any type
    /// can (a Group variable that links to another).
    const VARIABLE_TYPE: Option<VariableType>;
    /// This value read from `variable`, or `None` when the types differ or
    /// the number is out of this property's range.
    fn from_variable(variable: &VariableValue) -> Option<Self>;
}

impl LinkTarget for String {
    const VARIABLE_TYPE: Option<VariableType> = Some(VariableType::Text);
    fn from_variable(variable: &VariableValue) -> Option<Self> {
        match variable {
            VariableValue::Text(text) => Some(text.clone()),
            _ => None,
        }
    }
}

impl LinkTarget for ColorProperty {
    const VARIABLE_TYPE: Option<VariableType> = Some(VariableType::Color);
    fn from_variable(variable: &VariableValue) -> Option<Self> {
        match variable {
            VariableValue::Color(color) => Some(*color),
            _ => None,
        }
    }
}

impl LinkTarget for AssetId {
    const VARIABLE_TYPE: Option<VariableType> = Some(VariableType::Image);
    fn from_variable(variable: &VariableValue) -> Option<Self> {
        match variable {
            VariableValue::Image(asset_id) => Some(asset_id.clone()),
            _ => None,
        }
    }
}

impl LinkTarget for Arc<str> {
    const VARIABLE_TYPE: Option<VariableType> = Some(VariableType::Font);
    fn from_variable(variable: &VariableValue) -> Option<Self> {
        match variable {
            VariableValue::Font(family) => Some(Arc::from(family.as_str())),
            _ => None,
        }
    }
}

impl LinkTarget for ScalarProperty {
    const VARIABLE_TYPE: Option<VariableType> = Some(VariableType::Number);
    fn from_variable(variable: &VariableValue) -> Option<Self> {
        match variable {
            VariableValue::Number(number) => Some(*number),
            _ => None,
        }
    }
}

impl LinkTarget for PositiveProperty {
    const VARIABLE_TYPE: Option<VariableType> = Some(VariableType::Number);
    fn from_variable(variable: &VariableValue) -> Option<Self> {
        Self::new(ScalarProperty::from_variable(variable)?)
    }
}

impl LinkTarget for NonNegativeProperty {
    const VARIABLE_TYPE: Option<VariableType> = Some(VariableType::Number);
    fn from_variable(variable: &VariableValue) -> Option<Self> {
        Self::new(ScalarProperty::from_variable(variable)?)
    }
}

impl LinkTarget for VariableValue {
    const VARIABLE_TYPE: Option<VariableType> = None;
    fn from_variable(variable: &VariableValue) -> Option<Self> {
        Some(variable.clone())
    }
}

/// The variables visible at one point of the layer tree, resolved to values.
///
/// Cheap to clone: a Group without variables shares its parent's scope.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VariableScope(Arc<BTreeMap<VariableName, VariableValue>>);

impl VariableScope {
    /// The composition's scope, the root of every lookup. A composition
    /// variable cannot link, so links here are left out.
    #[must_use]
    pub fn root(variables: &Variables) -> Self {
        Self::default().child(variables)
    }

    /// The scope inside a Group defining `variables`. Its linked variables
    /// read this scope, which is the Group's parent.
    #[must_use]
    pub fn child(&self, variables: &Variables) -> Self {
        if variables.is_empty() {
            return self.clone();
        }
        let mut values = (*self.0).clone();
        for (name, variable) in variables {
            match variable {
                Linkable::Value(value) => {
                    values.insert(name.clone(), value.clone());
                }
                Linkable::Linked(target) => match self.get(target) {
                    Some(value) => {
                        values.insert(name.clone(), value.clone());
                    }
                    None => {
                        values.remove(name);
                    }
                },
            }
        }
        Self(Arc::new(values))
    }

    /// The value `name` resolves to here.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&VariableValue> {
        self.0.get(name)
    }

    /// Every visible name with its resolved value.
    #[must_use]
    pub fn values(&self) -> &BTreeMap<VariableName, VariableValue> {
        &self.0
    }

    /// Whether no variable is visible.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Replace every link in a composition's JSON with the value it reads: a
/// string, a color array or an asset id. Variables stay, unread.
pub(crate) fn bake_composition_json(
    composition: &mut serde_json::Value,
    variables: &Variables,
) -> Result<(), serde_json::Error> {
    let Some(layers) = composition
        .get_mut("layers")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return Ok(());
    };
    bake_layers_json(layers, &VariableScope::root(variables))
}

fn bake_layers_json(
    layers: &mut [serde_json::Value],
    scope: &VariableScope,
) -> Result<(), serde_json::Error> {
    for layer in layers {
        let Some(layer) = layer.as_object_mut() else {
            continue;
        };
        let inner = match layer.get("variables") {
            Some(variables) => scope.child(&serde_json::from_value(variables.clone())?),
            None => scope.clone(),
        };
        for (field, value) in layer.iter_mut() {
            if field != "layers" && field != "variables" {
                bake_links_json(value, &inner, &|variable| match variable {
                    VariableValue::Text(text) => serde_json::Value::from(text.as_str()),
                    VariableValue::Color(color) => serde_json::json!(color),
                    VariableValue::Image(asset_id) => serde_json::Value::from(asset_id.as_str()),
                    VariableValue::Number(number) => serde_json::Value::from(*number),
                    VariableValue::Font(family) => serde_json::Value::from(family.as_str()),
                });
            }
        }
        if let Some(children) = layer
            .get_mut("layers")
            .and_then(serde_json::Value::as_array_mut)
        {
            bake_layers_json(children, &inner)?;
        }
    }
    Ok(())
}

fn bake_links_json(
    value: &mut serde_json::Value,
    scope: &VariableScope,
    baked: &dyn Fn(&VariableValue) -> serde_json::Value,
) {
    if value.get("type").and_then(serde_json::Value::as_str) == Some("linked") {
        if let Some(variable) = value
            .get("value")
            .and_then(serde_json::Value::as_str)
            .and_then(|name| scope.get(name))
        {
            *value = baked(variable);
        }
        return;
    }
    match value {
        serde_json::Value::Object(object) => object
            .values_mut()
            .for_each(|value| bake_links_json(value, scope, baked)),
        serde_json::Value::Array(items) => items
            .iter_mut()
            .for_each(|value| bake_links_json(value, scope, baked)),
        _ => {}
    }
}

/// Why a variable or link breaks the variable invariant.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VariableError {
    /// A link names a variable no ancestor defines.
    #[error(
        "{owner} links to variable {name:?}, which no enclosing Group or the composition defines"
    )]
    Unresolved { owner: String, name: String },
    /// A link reads a variable of another type.
    #[error("{owner} needs a {expected} variable, but {name:?} is {actual}")]
    TypeMismatch {
        owner: String,
        name: String,
        expected: &'static str,
        actual: &'static str,
    },
    /// A number link reads a value outside its property's range.
    #[error("{owner} links to variable {name:?}, whose number is out of range for that property")]
    OutOfRange { owner: String, name: String },
}

/// Check that `name` resolves in `scope` to a variable of type `expected`.
pub fn check_link(
    scope: &VariableScope,
    name: &str,
    expected: VariableType,
    owner: impl FnOnce() -> String,
) -> Result<(), VariableError> {
    let Some(value) = scope.get(name) else {
        return Err(VariableError::Unresolved {
            owner: owner(),
            name: name.to_owned(),
        });
    };
    let actual = value.variable_type();
    if actual != expected {
        return Err(VariableError::TypeMismatch {
            owner: owner(),
            name: name.to_owned(),
            expected: expected.label(),
            actual: actual.label(),
        });
    }
    Ok(())
}

/// Check that each linked variable in `variables` resolves in `parent`, the
/// scope enclosing the Group that defines them.
pub fn check_variable_links(
    parent: &VariableScope,
    variables: &Variables,
    owner: impl Fn() -> String,
) -> Result<(), VariableError> {
    for (name, variable) in variables {
        if let Linkable::Linked(target) = variable {
            if parent.get(target).is_none() {
                return Err(VariableError::Unresolved {
                    owner: format!("{} variable {name:?}", owner()),
                    name: target.clone(),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn variables(value: serde_json::Value) -> Variables {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn links_read_the_nearest_definition_and_linked_variables_read_upward() {
        let composition = variables(json!({
            "title": {"type": "text", "value": "DOUBLE"},
            "accent": {"type": "color", "value": [1, 0, 0, 1]}
        }));
        let group = variables(json!({
            "headline": {"type": "linked", "value": "title"},
            "accent": {"type": "color", "value": [0, 0, 1, 1]}
        }));
        let root = VariableScope::root(&composition);
        let inner = root.child(&group);

        assert_eq!(
            inner.get("headline"),
            Some(&VariableValue::Text("DOUBLE".into()))
        );
        assert_eq!(
            inner.get("accent"),
            Some(&VariableValue::Color([0.0, 0.0, 1.0, 1.0]))
        );
        assert_eq!(
            root.get("accent"),
            Some(&VariableValue::Color([1.0, 0.0, 0.0, 1.0]))
        );
        assert_eq!(inner.get("missing"), None);
    }

    #[test]
    fn a_group_variable_does_not_read_itself() {
        let group = variables(json!({"headline": {"type": "linked", "value": "headline"}}));
        let root = VariableScope::default();
        assert_eq!(root.child(&group).get("headline"), None);
        assert!(check_variable_links(&root, &group, || "Group 1".into()).is_err());
    }

    #[test]
    fn linkable_round_trips_both_forms() {
        let text: Linkable<String> = serde_json::from_value(json!("DOUBLE")).unwrap();
        assert_eq!(text, Linkable::Value("DOUBLE".into()));
        let link: Linkable<String> =
            serde_json::from_value(json!({"type": "linked", "value": "headline"})).unwrap();
        assert_eq!(link, Linkable::Linked("headline".into()));
        assert_eq!(
            serde_json::to_value(&link).unwrap(),
            json!({"type": "linked", "value": "headline"})
        );
    }

    #[test]
    fn linked_colors_skip_and_plain_colors_keep_the_range_check() {
        let read = |value: serde_json::Value| {
            deserialize_linkable_extended_color(value).map_err(|error| error.to_string())
        };
        assert_eq!(
            read(json!({"type": "linked", "value": "accent"})),
            Ok(Linkable::Linked("accent".into()))
        );
        assert!(read(json!([2, 0, 0, 1.5])).is_err());
    }

    #[test]
    fn baking_a_composition_writes_every_link_as_its_value() {
        let transform = json!({"anchorPoint": [0, 0], "position": [0, 0], "scale": [100, 100], "rotation": 0, "opacity": 100});
        let mut document = json!({
            "id": "card",
            "name": "Card",
            "variables": {
                "title": {"type": "text", "value": "DOUBLE"},
                "accent": {"type": "color", "value": [1, 1, 0.18, 1]}
            },
            "layers": [{
                "type": "Group", "id": 1, "name": "Card", "transform": transform,
                "playback": {"type": "windowed", "inputRange": {"start": 0, "duration": 1000},
                    "mapping": {"type": "linear", "input": {"start": 0, "duration": 1000}, "output": {"start": 0, "duration": 1000}},
                    "inputOffsetMs": 0},
                "variables": {"headline": {"type": "linked", "value": "title"}},
                "fills": [{"paint": {"type": "solid", "color": {"type": "linked", "value": "accent"}}, "fillRule": "nonZeroWinding", "blendMode": "normal", "opacity": 1}],
                "layers": [{
                    "type": "Text", "id": 2, "name": "Headline", "parent": 1,
                    "activeRange": {"start": 0, "duration": 1000}, "transform": transform,
                    "sourceText": {"text": {"type": "linked", "value": "headline"}, "fontFamily": "Inter", "fontSize": 96, "fillColor": [1, 1, 1, 1]}
                }]
            }]
        });
        let composition: crate::FXComposition = serde_json::from_value(document.clone()).unwrap();
        let baked = serde_json::to_value(composition.bake_variables().unwrap()).unwrap();
        document["layers"][0]["fills"][0]["paint"]["color"] = json!([1.0, 1.0, 0.18, 1.0]);
        document["layers"][0]["layers"][0]["sourceText"]["text"] = json!("DOUBLE");
        assert_eq!(baked, document);
    }

    #[test]
    fn baking_replaces_a_link_with_its_value() {
        let composition = variables(json!({"photo": {"type": "image", "value": "img_1"}}));
        let scope = VariableScope::root(&composition);
        let mut source: Linkable<AssetId> = Linkable::Linked("photo".into());
        source.bake(&scope);
        assert_eq!(source, Linkable::Value(AssetId::from_trusted("img_1")));
    }
}
