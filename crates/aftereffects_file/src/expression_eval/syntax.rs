//! Closed-world numeric expression admission and vector arithmetic rewriting.

use std::{
    collections::{HashMap, HashSet},
    ops::ControlFlow,
};

use boa_ast::{
    Spanned,
    declaration::{Binding, Variable},
    expression::{
        Call, Expression, Identifier,
        access::{PropertyAccess, PropertyAccessField},
        literal::{LiteralKind, PropertyDefinition},
        operator::{
            assign::{Assign, AssignOp, AssignTarget},
            binary::{ArithmeticOp, BinaryOp},
            unary::UnaryOp,
        },
    },
    function::FunctionDeclaration,
    scope::Scope,
    visitor::{VisitWith, Visitor, VisitorMut},
};
use boa_interner::{Interner, ToIndentedString, ToInternedString};
use boa_parser::{Parser, Source};

use super::EvaluationError;

const GLOBALS: &[&str] = &[
    "undefined",
    "createPath",
    "points",
    "inTangents",
    "outTangents",
    "isClosed",
    "pointOnPath",
    "tangentOnPath",
    "normalOnPath",
    "value",
    "time",
    "thisLayer",
    "thisComp",
    "thisProperty",
    "transform",
    "index",
    "inPoint",
    "outPoint",
    "effect",
    "comp",
    "valueAtTime",
    "velocityAtTime",
    "sourceRectAtTime",
    "substring",
    "substr",
    "linear",
    "clamp",
    "Math",
    "posterizeTime",
    "length",
    "normalize",
    "add",
    "sub",
    "mul",
    "div",
    "framesToTime",
    "timeToFrames",
    "loopIn",
    "loopOut",
    "loop_in",
    "loop_out",
    "key",
    "nearestKey",
    "numKeys",
    "seedRandom",
    "random",
    "gaussRandom",
    "wiggle",
    "noise",
    "ease",
    "easeIn",
    "easeOut",
    "degreesToRadians",
    "degrees_to_radians",
    "radians_to_degrees",
    "radiansToDegrees",
    "rgbToHsl",
    "hslToRgb",
    "loopInDuration",
    "loopOutDuration",
    "lookAt",
    "dot",
    "cross",
    "toComp",
    "fromComp",
    "toWorld",
    "fromWorld",
    "toWorldVec",
    "fromWorldVec",
    "fromCompToSurface",
    "velocity",
    "speed",
    "width",
    "height",
    "name",
    "active",
    "content",
    "mask",
    "smooth",
    "hasParent",
    "parent",
    "anchorPoint",
    "anchor_point",
    "xRotation",
    "yRotation",
    "zRotation",
    "orientation",
    "xPosition",
    "yPosition",
    "zPosition",
    "position",
    "scale",
    "rotation",
    "opacity",
    "text",
    "String",
    "Number",
    "parseInt",
    "parseFloat",
    "isNaN",
    "isFinite",
];
/// Extended layer/helper API names that user code may still declare as locals.
const SHADOWABLE: &[&str] = &[
    "ease",
    "easeIn",
    "easeOut",
    "degreesToRadians",
    "degrees_to_radians",
    "radians_to_degrees",
    "radiansToDegrees",
    "rgbToHsl",
    "hslToRgb",
    "loopInDuration",
    "loopOutDuration",
    "lookAt",
    "dot",
    "cross",
    "toComp",
    "fromComp",
    "toWorld",
    "fromWorld",
    "toWorldVec",
    "fromWorldVec",
    "fromCompToSurface",
    "velocity",
    "speed",
    "width",
    "height",
    "name",
    "active",
    "content",
    "mask",
    "smooth",
    "hasParent",
    "parent",
    "anchorPoint",
    "anchor_point",
    "xRotation",
    "yRotation",
    "zRotation",
    "orientation",
    "xPosition",
    "yPosition",
    "zPosition",
    "position",
    "scale",
    "rotation",
    "opacity",
    "text",
    "String",
    "Number",
    "parseInt",
    "parseFloat",
    "isNaN",
    "isFinite",
];
const FUNCTIONS: &[&str] = &[
    "createPath",
    "points",
    "inTangents",
    "outTangents",
    "isClosed",
    "pointOnPath",
    "tangentOnPath",
    "normalOnPath",
    "effect",
    "comp",
    "valueAtTime",
    "velocityAtTime",
    "sourceRectAtTime",
    "substring",
    "substr",
    "linear",
    "clamp",
    "posterizeTime",
    "length",
    "normalize",
    "add",
    "sub",
    "mul",
    "div",
    "framesToTime",
    "timeToFrames",
    "loopIn",
    "loopOut",
    "loop_in",
    "loop_out",
    "key",
    "nearestKey",
    "seedRandom",
    "random",
    "gaussRandom",
    "wiggle",
    "noise",
    "ease",
    "easeIn",
    "easeOut",
    "degreesToRadians",
    "degrees_to_radians",
    "radians_to_degrees",
    "radiansToDegrees",
    "rgbToHsl",
    "hslToRgb",
    "loopInDuration",
    "loopOutDuration",
    "lookAt",
    "dot",
    "cross",
    "toComp",
    "fromComp",
    "toWorld",
    "fromWorld",
    "toWorldVec",
    "fromWorldVec",
    "fromCompToSurface",
    "content",
    "mask",
    "smooth",
    "String",
    "Number",
    "parseInt",
    "parseFloat",
    "isNaN",
    "isFinite",
];
/// String/number methods admitted on plain (non-API) values.
const VALUE_METHODS: &[&str] = &[
    "exec",
    "match",
    "toFixed",
    "toString",
    "toPrecision",
    "toUpperCase",
    "toLowerCase",
    "substr",
    "substring",
    "slice",
    "split",
    "join",
    "replace",
    "indexOf",
    "lastIndexOf",
    "charAt",
    "trim",
    "padStart",
    "padEnd",
    "concat",
    "repeat",
    "includes",
    "startsWith",
    "endsWith",
    "toLocaleString",
];
fn random_api_helper(name: &str) -> Option<&'static str> {
    match name {
        "seedRandom" => Some("__aeSeedRandom"),
        "random" => Some("__aeRandomValue"),
        "gaussRandom" => Some("__aeGaussRandom"),
        "wiggle" => Some("__aeWiggle"),
        "noise" => Some("__aeNoise"),
        _ => None,
    }
}

const MATH_FUNCTIONS: &[&str] = &[
    "abs", "acos", "acosh", "asin", "asinh", "atan", "atanh", "atan2", "ceil", "cos", "cosh",
    "exp", "expm1", "floor", "hypot", "log", "log1p", "log2", "log10", "max", "min", "pow",
    "round", "sign", "sin", "sinh", "sqrt", "tan", "tanh", "trunc",
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ApiKind {
    Comp,
    Layer,
    Transform,
    EffectSelector,
    Property,
    TextStyle,
    /// A sourceRectAtTime snapshot contains numeric data, not live API receivers.
    Bounds,
    /// A local shadows an ambient name without becoming an API receiver.
    Value,
    Key,
    /// A Shape Contents item, Mask or nested member resolved by name at runtime.
    Group,
    Ambiguous,
}

fn api_kind(
    expression: &Expression,
    interner: &Interner,
    aliases: &HashMap<String, ApiKind>,
) -> Option<ApiKind> {
    match expression.flatten() {
        Expression::Identifier(name) => {
            let name = name.to_interned_string(interner);
            match name.as_str() {
                "thisComp" => Some(ApiKind::Comp),
                "thisLayer" => Some(ApiKind::Layer),
                "thisProperty" => Some(ApiKind::Property),
                "transform" => Some(ApiKind::Transform),
                "parent" if !aliases.contains_key(&name) => Some(ApiKind::Layer),
                "text" if !aliases.contains_key(&name) => Some(ApiKind::Group),
                "anchorPoint" | "anchor_point" | "position" | "scale" | "rotation"
                | "zRotation" | "xRotation" | "yRotation" | "orientation" | "xPosition"
                | "yPosition" | "zPosition" | "opacity"
                    if !aliases.contains_key(&name) =>
                {
                    Some(ApiKind::Property)
                }
                _ => aliases
                    .get(&name)
                    .copied()
                    .filter(|kind| *kind != ApiKind::Value),
            }
        }
        Expression::Call(call) => match call.function().flatten() {
            Expression::Identifier(name) if name.to_interned_string(interner) == "comp" => {
                Some(ApiKind::Comp)
            }
            Expression::Identifier(name) if name.to_interned_string(interner) == "effect" => {
                Some(ApiKind::EffectSelector)
            }
            Expression::Identifier(name)
                if name.to_interned_string(interner) == "sourceRectAtTime"
                    && !aliases.contains_key("sourceRectAtTime") =>
            {
                Some(ApiKind::Bounds)
            }
            Expression::Identifier(name)
                if matches!(
                    name.to_interned_string(interner).as_str(),
                    "content" | "mask"
                ) && !aliases.contains_key(&name.to_interned_string(interner)) =>
            {
                Some(ApiKind::Group)
            }
            Expression::Identifier(name)
                if matches!(
                    name.to_interned_string(interner).as_str(),
                    "key" | "nearestKey"
                ) =>
            {
                Some(ApiKind::Key)
            }
            Expression::PropertyAccess(PropertyAccess::Simple(access)) => {
                let PropertyAccessField::Const(field) = access.field() else {
                    return None;
                };
                match (
                    field.to_interned_string(interner).as_str(),
                    api_kind(access.target(), interner, aliases),
                ) {
                    ("getStyleAt", Some(ApiKind::Property)) => Some(ApiKind::TextStyle),
                    ("layer", Some(ApiKind::Comp)) => Some(ApiKind::Layer),
                    ("transform", Some(ApiKind::Layer)) => Some(ApiKind::Property),
                    ("text", Some(ApiKind::Layer)) => Some(ApiKind::Property),
                    ("effect", Some(ApiKind::Layer)) => Some(ApiKind::EffectSelector),
                    ("key" | "nearestKey", Some(ApiKind::Property | ApiKind::Group)) => {
                        Some(ApiKind::Key)
                    }
                    ("sourceRectAtTime", Some(ApiKind::Layer)) => Some(ApiKind::Bounds),
                    ("content", Some(ApiKind::Layer | ApiKind::Group))
                    | ("mask", Some(ApiKind::Layer)) => Some(ApiKind::Group),
                    _ => None,
                }
            }
            target => match api_kind(target, interner, aliases) {
                Some(ApiKind::EffectSelector | ApiKind::Transform) => Some(ApiKind::Property),
                Some(ApiKind::Comp) => Some(ApiKind::Layer),
                Some(ApiKind::Layer | ApiKind::Group) => Some(ApiKind::Group),
                _ => None,
            },
        },
        Expression::PropertyAccess(PropertyAccess::Simple(access)) => {
            let PropertyAccessField::Const(field) = access.field() else {
                return None;
            };
            match (
                field.to_interned_string(interner).as_str(),
                api_kind(access.target(), interner, aliases),
            ) {
                ("transform", Some(ApiKind::Layer)) => Some(ApiKind::Transform),
                ("parent", Some(ApiKind::Layer)) => Some(ApiKind::Layer),
                ("text" | "marker", Some(ApiKind::Layer)) => Some(ApiKind::Group),
                // Source Text's `.value` is plain data, not a nested API group.
                ("sourceText", Some(ApiKind::Group)) => Some(ApiKind::Property),
                ("style", Some(ApiKind::Property | ApiKind::Group)) => Some(ApiKind::TextStyle),
                (
                    "anchorPoint" | "anchor_point" | "position" | "scale" | "rotation"
                    | "zRotation" | "xRotation" | "yRotation" | "orientation" | "xPosition"
                    | "yPosition" | "zPosition" | "opacity",
                    Some(ApiKind::Layer | ApiKind::Transform),
                ) => Some(ApiKind::Property),
                // Members of named Contents/Mask items resolve at runtime.
                (_, Some(ApiKind::Group)) => Some(ApiKind::Group),
                _ => None,
            }
        }
        Expression::Conditional(conditional) => {
            let a = api_kind(conditional.if_true(), interner, aliases);
            let b = api_kind(conditional.if_false(), interner, aliases);
            if a == b {
                a
            } else if a.is_some() || b.is_some() {
                Some(ApiKind::Ambiguous)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum PathArray {
    Outer,
    Row,
}

fn path_array(
    expression: &Expression,
    interner: &Interner,
    arrays: &HashMap<String, PathArray>,
) -> Option<PathArray> {
    match expression.flatten() {
        Expression::Identifier(name) => arrays.get(&name.to_interned_string(interner)).copied(),
        Expression::Call(call) => match call.function().flatten() {
            Expression::Identifier(name)
                if matches!(
                    name.to_interned_string(interner).as_str(),
                    "points" | "inTangents" | "outTangents"
                ) =>
            {
                Some(PathArray::Outer)
            }
            Expression::PropertyAccess(PropertyAccess::Simple(access)) if matches!(access.field(), PropertyAccessField::Const(field) if matches!(field.to_interned_string(interner).as_str(), "points" | "inTangents" | "outTangents")) => {
                Some(PathArray::Outer)
            }
            _ => None,
        },
        Expression::PropertyAccess(PropertyAccess::Simple(access))
            if matches!(access.field(), PropertyAccessField::Expr(_))
                && matches!(
                    path_array(access.target(), interner, arrays),
                    Some(PathArray::Outer)
                ) =>
        {
            Some(PathArray::Row)
        }
        _ => None,
    }
}

pub(super) fn compile(source: &str) -> Result<String, EvaluationError> {
    compile_with_implicit_aliases(source, true)
}

pub(super) fn compile_text(source: &str) -> Result<String, EvaluationError> {
    // Keep Source Text's separate bounded expression profiles unchanged.
    compile_with_implicit_aliases(source, false)
}

// Legacy AE accepts a numeric consequent immediately before `else` without `;`.
// Admit only a complete numeric-branch conditional; the condition still passes all AST guards.
fn legacy_numeric_conditional(source: &str) -> Option<String> {
    let source = source.trim().trim_end_matches(';').trim_end();
    let rest = source.strip_prefix("if")?.trim_start();
    let rest = rest.strip_prefix('(')?;
    let (consequent, alternate) = rest.rsplit_once("else")?;
    let (condition, consequent) = consequent.rsplit_once(')')?;
    let consequent = consequent.trim();
    let alternate = alternate.trim();
    let numeric = |value: &str| {
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || b".+-eE".contains(&byte))
            && value.parse::<f64>().is_ok_and(f64::is_finite)
    };
    if !(numeric(consequent) && numeric(alternate)) {
        return None;
    }
    Some(format!(
        "if ({condition}) {{ {consequent}; }} else {{ {alternate}; }}"
    ))
}

fn compile_with_implicit_aliases(
    source: &str,
    allow_implicit_aliases: bool,
) -> Result<String, EvaluationError> {
    let source = source.trim_end_matches('\0');
    let legacy = allow_implicit_aliases
        .then(|| legacy_numeric_conditional(source))
        .flatten();
    let source = legacy.as_deref().unwrap_or(source);
    let mut interner = Interner::default();
    let mut script = Parser::new(Source::from_bytes(source))
        .parse_script(&Scope::new_global(), &mut interner)?;
    let mut bindings = Bindings {
        interner: &interner,
        names: HashSet::new(),
        data_locals: HashSet::new(),
        aliases: HashMap::new(),
        arrays: HashMap::new(),
        mutated: HashSet::new(),
        implicit_aliases: HashSet::new(),
        allow_implicit_aliases,
        root_names: HashSet::new(),
        functions: HashSet::new(),
        current_function: None,
        calls: HashMap::new(),
    };
    if let ControlFlow::Break(reason) = script.visit_with(&mut bindings) {
        return Err(EvaluationError::Unsupported(reason));
    }
    if bindings.mutated.iter().any(|name| {
        bindings
            .aliases
            .get(name)
            .is_some_and(|kind| !matches!(kind, ApiKind::Bounds | ApiKind::Value))
            || bindings.arrays.contains_key(name)
    }) {
        return Err(EvaluationError::Unsupported(
            "mutable AE API aliases are not admitted".into(),
        ));
    }
    if bindings
        .functions
        .iter()
        .any(|name| bindings.mutated.contains(name))
    {
        return Err(EvaluationError::Unsupported(
            "helper reassignment is not admitted".into(),
        ));
    }
    for name in &bindings.functions {
        let mut pending = vec![name.as_str()];
        let mut seen = HashSet::new();
        while let Some(caller) = pending.pop() {
            if !seen.insert(caller) {
                continue;
            }
            for target in bindings.calls.get(caller).into_iter().flatten() {
                if target == name {
                    return Err(EvaluationError::Unsupported(
                        "recursive helpers are not admitted".into(),
                    ));
                }
                if bindings.functions.contains(target) {
                    pending.push(target);
                }
            }
        }
    }
    let mut implicit: Vec<_> = bindings
        .mutated
        .iter()
        .filter(|name| !bindings.root_names.contains(*name))
        .chain(bindings.implicit_aliases.iter())
        .cloned()
        .collect();
    implicit.sort();
    if implicit
        .iter()
        .any(|name| GLOBALS.contains(&name.as_str()) || name.starts_with("__"))
    {
        return Err(EvaluationError::Unsupported(
            "writes to the AE API are not admitted".into(),
        ));
    }
    let aliases = bindings.aliases;
    let mut locals = bindings.names;
    locals.extend(implicit.iter().cloned());
    let functions = bindings.functions;
    let arrays = bindings.arrays;
    let data_locals = bindings.data_locals;
    let mut visitor = Admission {
        interner: &mut interner,
        locals,
        data_locals,
        aliases,
        arrays,
        functions,
    };
    if let ControlFlow::Break(reason) = script.visit_with_mut(&mut visitor) {
        return Err(EvaluationError::Unsupported(reason));
    }
    let declarations = if implicit.is_empty() {
        String::new()
    } else {
        format!("var {};\n", implicit.join(","))
    };
    Ok(format!(
        "{declarations}{}",
        script.to_indented_string(&interner, 0)
    ))
}

struct Bindings<'a> {
    interner: &'a Interner,
    names: HashSet<String>,
    data_locals: HashSet<String>,
    aliases: HashMap<String, ApiKind>,
    arrays: HashMap<String, PathArray>,
    mutated: HashSet<String>,
    implicit_aliases: HashSet<String>,
    allow_implicit_aliases: bool,
    root_names: HashSet<String>,
    functions: HashSet<String>,
    current_function: Option<String>,
    calls: HashMap<String, Vec<String>>,
}
impl<'ast> Visitor<'ast> for Bindings<'_> {
    type BreakTy = String;
    fn visit_variable(&mut self, variable: &'ast Variable) -> ControlFlow<String> {
        let Binding::Identifier(name) = variable.binding() else {
            return ControlFlow::Break("destructuring is not admitted".into());
        };
        let name = name.to_interned_string(self.interner);
        if (GLOBALS.contains(&name.as_str())
            && random_api_helper(&name).is_none()
            && !SHADOWABLE.contains(&name.as_str()))
            || name.starts_with("__")
        {
            return ControlFlow::Break("shadowing the AE API is not admitted".into());
        }
        if self.current_function.is_none() {
            self.root_names.insert(name.clone());
        }
        if !self.names.insert(name.clone()) {
            // JavaScript permits var redeclarations, including helper parameters.
            // Treat them conservatively as writes so API/Path aliases and helper
            // identities still cannot be rebound through a second declaration.
            self.mutated.insert(name.clone());
        }
        if variable
            .init()
            .is_some_and(|expression| api_kind(expression, self.interner, &self.aliases).is_none())
        {
            self.data_locals.insert(name.clone());
        }
        if let Some(kind) = variable
            .init()
            .and_then(|expression| path_array(expression, self.interner, &self.arrays))
        {
            self.arrays.insert(name.clone(), kind);
        }
        if let Some(kind) = variable
            .init()
            .and_then(|expression| api_kind(expression, self.interner, &self.aliases))
        {
            self.aliases.insert(name, kind);
        } else if SHADOWABLE.contains(&name.as_str()) {
            self.aliases.entry(name).or_insert(ApiKind::Value);
        }
        variable.visit_with(self)
    }
    fn visit_expression(&mut self, expression: &'ast Expression) -> ControlFlow<String> {
        if let Expression::Assign(assign) = expression
            && let AssignTarget::Identifier(name) = assign.lhs()
        {
            let name = name.to_interned_string(self.interner);
            let kind = api_kind(assign.rhs(), self.interner, &self.aliases);
            // A first top-level assignment is an implicit local initializer;
            // subsequent writes must still invalidate live receiver aliases.
            if self.allow_implicit_aliases
                && self.current_function.is_none()
                && assign.op() == AssignOp::Assign
                && !self.names.contains(&name)
                && !self.mutated.contains(&name)
                && let Some(kind) = kind
                && !matches!(kind, ApiKind::Bounds | ApiKind::Value | ApiKind::Ambiguous)
            {
                self.names.insert(name.clone());
                self.implicit_aliases.insert(name.clone());
                self.aliases.insert(name, kind);
            } else {
                self.mutated.insert(name.clone());
                // Data snapshots can be rebound, unlike live receivers.
                if kind == Some(ApiKind::Bounds) && !self.aliases.contains_key(&name) {
                    self.aliases.insert(name, ApiKind::Bounds);
                }
            }
        }
        if let Expression::Call(call) = expression
            && let Expression::Identifier(target) = call.function().flatten()
            && let Some(caller) = &self.current_function
        {
            self.calls
                .entry(caller.clone())
                .or_default()
                .push(target.to_interned_string(self.interner));
        }
        if let Expression::Update(update) = expression
            && let boa_ast::expression::operator::update::UpdateTarget::Identifier(name) =
                update.target()
        {
            self.mutated.insert(name.to_interned_string(self.interner));
        }
        expression.visit_with(self)
    }
    fn visit_catch(&mut self, catch: &'ast boa_ast::statement::Catch) -> ControlFlow<String> {
        if let Some(binding) = catch.parameter() {
            let Binding::Identifier(name) = binding else {
                return ControlFlow::Break("catch destructuring is not admitted".into());
            };
            let name = name.to_interned_string(self.interner);
            if GLOBALS.contains(&name.as_str()) || name.starts_with("__") {
                return ControlFlow::Break("catch shadowing the AE API is not admitted".into());
            }
            if !self.names.insert(name.clone()) {
                self.mutated.insert(name);
            }
        }
        catch.visit_with(self)
    }
    fn visit_function_declaration(
        &mut self,
        function: &'ast FunctionDeclaration,
    ) -> ControlFlow<String> {
        let name = function.name().to_interned_string(self.interner);
        if self.current_function.is_some()
            || self.functions.len() >= 8
            || !function.parameters().is_simple()
            || function.parameters().has_duplicates()
            || GLOBALS.contains(&name.as_str())
            || name.starts_with("__")
            || !self.functions.insert(name.clone())
            || !self.names.insert(name.clone())
        {
            return ControlFlow::Break("helper declaration is outside the bounded profile".into());
        }
        self.current_function = Some(name);
        let result = function.visit_with(self);
        self.current_function = None;
        result
    }
}

struct Admission<'a> {
    interner: &'a mut Interner,
    locals: HashSet<String>,
    data_locals: HashSet<String>,
    aliases: HashMap<String, ApiKind>,
    arrays: HashMap<String, PathArray>,
    functions: HashSet<String>,
}
impl Admission<'_> {
    fn path_index(&self, index: &Expression) -> bool {
        match index.flatten() {
            Expression::Identifier(name) => {
                let name = name.to_interned_string(self.interner);
                self.locals.contains(&name)
                    && !self.aliases.contains_key(&name)
                    && !self.arrays.contains_key(&name)
                    && !self.functions.contains(&name)
            }
            Expression::Literal(_) => index
                .to_interned_string(self.interner)
                .parse::<usize>()
                .is_ok(),
            _ => false,
        }
    }
    fn is_api_reference(&self, expression: &Expression) -> bool {
        // Native Key records are data, not live AE API receivers.
        matches!(
            api_kind(expression, self.interner, &self.aliases),
            Some(
                ApiKind::Comp
                    | ApiKind::Layer
                    | ApiKind::Transform
                    | ApiKind::EffectSelector
                    | ApiKind::Property
                    | ApiKind::TextStyle
                    | ApiKind::Group
                    | ApiKind::Ambiguous
            )
        )
    }
    fn helper(&mut self, name: &str, args: Vec<Expression>, span: boa_ast::Span) -> Expression {
        Call::new(
            Identifier::new(self.interner.get_or_intern(name), span).into(),
            args.into_boxed_slice(),
            span,
        )
        .into()
    }
    fn field(&self, access: &PropertyAccess) -> Option<String> {
        let PropertyAccess::Simple(access) = access else {
            return None;
        };
        match access.field() {
            PropertyAccessField::Const(name) => Some(name.to_interned_string(self.interner)),
            PropertyAccessField::Expr(_) => None,
        }
    }
    fn call_allowed(&self, target: &Expression) -> bool {
        match target.flatten() {
            Expression::Identifier(name) => {
                let name = name.to_interned_string(self.interner);
                ((!self.locals.contains(&name) && FUNCTIONS.contains(&name.as_str()))
                    || self.functions.contains(&name))
                    || api_kind(target, self.interner, &self.aliases).is_some_and(|kind| {
                        matches!(
                            kind,
                            ApiKind::EffectSelector
                                | ApiKind::Comp
                                | ApiKind::Layer
                                | ApiKind::Transform
                                | ApiKind::Group
                        )
                    })
            }
            Expression::PropertyAccess(access) => {
                let Some(field) = self.field(access) else {
                    return false;
                };
                let PropertyAccess::Simple(access) = access else {
                    return false;
                };
                if access.target().to_interned_string(self.interner) == "Math" {
                    MATH_FUNCTIONS.contains(&field.as_str())
                } else if api_kind(access.target(), self.interner, &self.aliases).is_none()
                    && VALUE_METHODS.contains(&field.as_str())
                {
                    true
                } else {
                    matches!(
                        (
                            field.as_str(),
                            api_kind(access.target(), self.interner, &self.aliases)
                        ),
                        ("setText", Some(ApiKind::TextStyle))
                            | ("getStyleAt", Some(ApiKind::Property))
                            | ("layer", Some(ApiKind::Comp))
                            | ("effect" | "transform" | "text", Some(ApiKind::Layer))
                            | (
                                "valueAtTime"
                                    | "key"
                                    | "nearestKey"
                                    | "velocityAtTime"
                                    | "speedAtTime"
                                    | "smooth"
                                    | "loopIn"
                                    | "loopOut"
                                    | "loop_in"
                                    | "loop_out"
                                    | "loopInDuration"
                                    | "loopOutDuration"
                                    | "substring"
                                    | "substr"
                                    | "points"
                                    | "inTangents"
                                    | "outTangents"
                                    | "isClosed"
                                    | "pointOnPath"
                                    | "tangentOnPath"
                                    | "normalOnPath",
                                Some(ApiKind::Property | ApiKind::Group)
                            )
                            | (
                                "toComp"
                                    | "fromComp"
                                    | "toWorld"
                                    | "fromWorld"
                                    | "toWorldVec"
                                    | "fromWorldVec"
                                    | "fromCompToSurface"
                                    | "sourceRectAtTime"
                                    | "content"
                                    | "mask",
                                Some(ApiKind::Layer)
                            )
                            | ("content", Some(ApiKind::Group))
                    )
                }
            }
            // The second call selects a parameter on an admitted effect accessor.
            Expression::Call(_) => {
                api_kind(target, self.interner, &self.aliases).is_some_and(|kind| {
                    matches!(
                        kind,
                        ApiKind::EffectSelector
                            | ApiKind::Comp
                            | ApiKind::Layer
                            | ApiKind::Transform
                            | ApiKind::Group
                    )
                })
            }
            Expression::Conditional(choice) => {
                self.call_allowed(choice.if_true()) && self.call_allowed(choice.if_false())
            }
            _ => false,
        }
    }
}
impl<'ast> VisitorMut<'ast> for Admission<'_> {
    type BreakTy = String;
    fn visit_statement_mut(
        &mut self,
        statement: &'ast mut boa_ast::Statement,
    ) -> ControlFlow<String> {
        match statement {
            boa_ast::Statement::Return(returned)
                if returned
                    .target()
                    .is_some_and(|target| self.is_api_reference(target)) =>
            {
                return ControlFlow::Break(
                    "helpers returning AE API references are not admitted".into(),
                );
            }
            // Explicit throws can transport live API references into untyped catch
            // bindings. Reject them regardless of reachability or payload; retain
            // try/catch for recoverable native Key/range errors only.
            boa_ast::Statement::Throw(_) => {
                return ControlFlow::Break("explicit throw statements are not admitted".into());
            }
            boa_ast::Statement::Block(_)
            | boa_ast::Statement::Var(_)
            | boa_ast::Statement::Empty
            | boa_ast::Statement::Expression(_)
            | boa_ast::Statement::Return(_)
            | boa_ast::Statement::Try(_)
            | boa_ast::Statement::Switch(_)
            | boa_ast::Statement::Break(_)
            | boa_ast::Statement::Continue(_)
            // Loops are bounded by the evaluator's runtime iteration limit.
            | boa_ast::Statement::ForLoop(_)
            | boa_ast::Statement::WhileLoop(_)
            | boa_ast::Statement::DoWhileLoop(_) => {}
            boa_ast::Statement::If(branch)
                if !matches!(
                    api_kind(branch.cond(), self.interner, &self.aliases),
                    Some(ApiKind::Property | ApiKind::Ambiguous)
                ) => {}
            _ => {
                return ControlFlow::Break(
                    "statement or implicit Property truthiness is not admitted".into(),
                );
            }
        }
        statement.visit_with_mut(self)
    }
    fn visit_declaration_mut(
        &mut self,
        declaration: &'ast mut boa_ast::Declaration,
    ) -> ControlFlow<String> {
        if !matches!(
            declaration,
            boa_ast::Declaration::Lexical(_) | boa_ast::Declaration::FunctionDeclaration(_)
        ) {
            return ControlFlow::Break("only variable declarations are admitted".into());
        }
        declaration.visit_with_mut(self)
    }
    fn visit_expression_mut(&mut self, expression: &'ast mut Expression) -> ControlFlow<String> {
        let span = expression.span();
        if let Expression::Assign(assign) = expression
            && assign.op() == AssignOp::Assign
            && let AssignTarget::Identifier(name) = assign.lhs()
            && self
                .data_locals
                .contains(&name.to_interned_string(self.interner))
            && !self
                .aliases
                .contains_key(&name.to_interned_string(self.interner))
            && !self
                .arrays
                .contains_key(&name.to_interned_string(self.interner))
            && !matches!(assign.rhs().flatten(), Expression::Identifier(_))
            && api_kind(assign.rhs(), self.interner, &self.aliases) == Some(ApiKind::Property)
        {
            // Mutable data locals capture the current value, never a live receiver.
            let mut rhs = assign.rhs().clone();
            self.visit_expression_mut(&mut rhs)?;
            let rhs = self.helper("__aeUnwrap", vec![rhs], span);
            *expression = Assign::new(AssignOp::Assign, assign.lhs().clone(), rhs).into();
            return ControlFlow::Continue(());
        }
        // Rewrite copied-Path accesses before walking children: an assignment LHS
        // must never be transformed into a read call by the generic visitor.
        if let Expression::Assign(assign) = expression
            && let AssignTarget::Access(PropertyAccess::Simple(access)) = assign.lhs()
            && path_array(access.target(), self.interner, &self.arrays).is_some()
        {
            let PropertyAccessField::Expr(index) = access.field() else {
                return ControlFlow::Break("Path array property writes are not admitted".into());
            };
            if !self.path_index(index) {
                return ControlFlow::Break("unsupported Path array index".into());
            }
            let operation = match assign.op() {
                AssignOp::Assign => None,
                AssignOp::Add => Some("__aeAdd"),
                AssignOp::Sub => Some("__aeSub"),
                AssignOp::Mul => Some("__aeMul"),
                AssignOp::Div => Some("__aeDiv"),
                _ => return ControlFlow::Break("unsupported Path array assignment".into()),
            };
            let mut target = access.target().clone();
            let mut index = index.as_ref().clone();
            let mut rhs = assign.rhs().clone();
            self.visit_expression_mut(&mut target)?;
            self.visit_expression_mut(&mut index)?;
            self.visit_expression_mut(&mut rhs)?;
            let reference = self.helper("__aePathSlot", vec![target, index], span);
            let helper = match operation {
                None => "__aePathSet",
                Some("__aeAdd") => "__aePathSetAdd",
                Some("__aeSub") => "__aePathSetSub",
                Some("__aeMul") => "__aePathSetMul",
                _ => "__aePathSetDiv",
            };
            *expression = self.helper(helper, vec![reference, rhs], span);
            return ControlFlow::Continue(());
        }
        if let Expression::PropertyAccess(PropertyAccess::Simple(access)) = expression
            && path_array(access.target(), self.interner, &self.arrays).is_some()
            && let PropertyAccessField::Expr(index) = access.field()
        {
            if !self.path_index(index) {
                return ControlFlow::Break("unsupported Path array index".into());
            }
            let mut target = access.target().clone();
            let mut index = index.as_ref().clone();
            self.visit_expression_mut(&mut target)?;
            self.visit_expression_mut(&mut index)?;
            *expression = self.helper("__aePathGet", vec![target, index], span);
            return ControlFlow::Continue(());
        }
        match expression {
            Expression::Identifier(name) => {
                let name = name.to_interned_string(self.interner);
                if !GLOBALS.contains(&name.as_str()) && !self.locals.contains(&name) {
                    return ControlFlow::Break(format!("unknown AE identifier {name}"));
                }
            }
            Expression::Call(call)
                if (matches!(call.function().flatten(), Expression::Identifier(name)
                    if self.functions.contains(&name.to_interned_string(self.interner)))
                    || matches!(call.function().flatten(), Expression::Conditional(_)))
                    && call
                        .args()
                        .iter()
                        .any(|argument| self.is_api_reference(argument)) =>
            {
                return ControlFlow::Break(
                    "passing AE API references to helpers is not admitted".into(),
                );
            }
            Expression::ArrayLiteral(array)
                if array
                    .as_ref()
                    .iter()
                    .flatten()
                    .any(|element| self.is_api_reference(element)) =>
            {
                return ControlFlow::Break(
                    "arrays carrying AE API references are not admitted".into(),
                );
            }
            Expression::Call(call) if !self.call_allowed(call.function()) => {
                return ControlFlow::Break(format!(
                    "unimplemented function {}",
                    call.function().to_interned_string(self.interner)
                ));
            }
            Expression::PropertyAccess(access) => {
                let field = self.field(access);
                let PropertyAccess::Simple(simple) = access else {
                    return ControlFlow::Break("private/super access is not admitted".into());
                };
                if let Some(field) = field {
                    let math = simple.target().to_interned_string(self.interner) == "Math";
                    let allowed = if math {
                        MATH_FUNCTIONS.contains(&field.as_str())
                            || matches!(
                                field.as_str(),
                                "PI" | "E"
                                    | "LN2"
                                    | "LN10"
                                    | "LOG2E"
                                    | "LOG10E"
                                    | "SQRT1_2"
                                    | "SQRT2"
                            )
                    } else {
                        // An absent shim member is undefined, so a logical/conditional
                        // fallback could otherwise turn unsupported AE reads into values.
                        match api_kind(simple.target(), self.interner, &self.aliases) {
                            Some(ApiKind::Comp) => matches!(
                                field.as_str(),
                                "layer"
                                    | "name"
                                    | "width"
                                    | "height"
                                    | "duration"
                                    | "frameDuration"
                                    | "displayStartTime"
                                    | "_name"
                                    | "numLayers"
                            ),
                            Some(ApiKind::Layer) => matches!(
                                field.as_str(),
                                "effect"
                                    | "transform"
                                    | "index"
                                    | "inPoint"
                                    | "outPoint"
                                    | "anchorPoint"
                                    | "anchor_point"
                                    | "xRotation"
                                    | "yRotation"
                                    | "zRotation"
                                    | "orientation"
                                    | "xPosition"
                                    | "yPosition"
                                    | "zPosition"
                                    | "position"
                                    | "scale"
                                    | "rotation"
                                    | "opacity"
                                    | "width"
                                    | "height"
                                    | "name"
                                    | "_name"
                                    | "active"
                                    | "parent"
                                    | "hasParent"
                                    | "text"
                                    | "sourceRectAtTime"
                                    | "content"
                                    | "mask"
                                    | "marker"
                                    | "toComp"
                                    | "fromComp"
                                    | "toWorld"
                                    | "fromWorld"
                                    | "toWorldVec"
                                    | "fromWorldVec"
                                    | "fromCompToSurface"
                            ),
                            Some(ApiKind::Transform) => matches!(
                                field.as_str(),
                                "anchorPoint"
                                    | "anchor_point"
                                    | "position"
                                    | "scale"
                                    | "rotation"
                                    | "zRotation"
                                    | "xRotation"
                                    | "yRotation"
                                    | "orientation"
                                    | "xPosition"
                                    | "yPosition"
                                    | "zPosition"
                                    | "opacity"
                            ),
                            Some(ApiKind::Property) => matches!(
                                field.as_str(),
                                "value"
                                    | "valueAtTime"
                                    | "numKeys"
                                    | "length"
                                    | "key"
                                    | "nearestKey"
                                    | "velocity"
                                    | "speed"
                                    | "velocityAtTime"
                                    | "speedAtTime"
                                    | "smooth"
                                    | "loopIn"
                                    | "loopOut"
                                    | "loop_in"
                                    | "loop_out"
                                    | "loopInDuration"
                                    | "loopOutDuration"
                                    | "style"
                                    | "getStyleAt"
                                    | "points"
                                    | "inTangents"
                                    | "outTangents"
                                    | "isClosed"
                                    | "pointOnPath"
                                    | "tangentOnPath"
                                    | "normalOnPath"
                                    | "substring"
                                    | "substr"
                            ),
                            Some(ApiKind::TextStyle) => {
                                matches!(field.as_str(), "fillColor" | "setText")
                            }
                            Some(ApiKind::Bounds) => {
                                matches!(field.as_str(), "left" | "top" | "width" | "height")
                            }
                            // The runtime object rejects members it cannot resolve.
                            Some(ApiKind::Group) => true,
                            Some(ApiKind::Key) => {
                                matches!(field.as_str(), "time" | "index" | "value")
                            }
                            // Effect groups expose enablement, but not Property value/numKeys probes.
                            Some(ApiKind::EffectSelector) => {
                                matches!(field.as_str(), "numKeys" | "value" | "active")
                            }
                            Some(ApiKind::Ambiguous | ApiKind::Value) => false,
                            None => {
                                matches!(
                                    field.as_str(),
                                    "length" | "text" | "start" | "index" | "lastIndex" | "message"
                                ) || VALUE_METHODS.contains(&field.as_str())
                            }
                        }
                    };
                    if !allowed {
                        return ControlFlow::Break(format!("unimplemented member {field}"));
                    }
                } else if let PropertyAccessField::Expr(index) = simple.field() {
                    let printed = index.to_interned_string(self.interner);
                    if printed.parse::<usize>().is_err() {
                        if api_kind(simple.target(), self.interner, &self.aliases).is_some() {
                            return ControlFlow::Break(
                                "dynamic API indices are not admitted".into(),
                            );
                        }
                        let mut target = simple.target().clone();
                        let mut index = index.as_ref().clone();
                        self.visit_expression_mut(&mut target)?;
                        self.visit_expression_mut(&mut index)?;
                        *expression = self.helper("__aeDataIndex", vec![target, index], span);
                        return ControlFlow::Continue(());
                    }
                }
            }
            Expression::Assign(assign) => {
                let AssignTarget::Identifier(name) = assign.lhs() else {
                    return ControlFlow::Break("property writes are not admitted".into());
                };
                if api_kind(assign.rhs(), self.interner, &self.aliases).is_some_and(|kind| {
                    kind != ApiKind::Bounds
                        && self.aliases.get(&name.to_interned_string(self.interner)) != Some(&kind)
                }) {
                    return ControlFlow::Break(
                        "assigning AE API references is not admitted".into(),
                    );
                }
                if !matches!(
                    assign.op(),
                    AssignOp::Assign
                        | AssignOp::Add
                        | AssignOp::Sub
                        | AssignOp::Mul
                        | AssignOp::Div
                ) || !self
                    .locals
                    .contains(&name.to_interned_string(self.interner))
                {
                    return ControlFlow::Break(
                        "only assignments to declared local variables are admitted".into(),
                    );
                }
            }
            Expression::Binary(binary)
                if matches!(binary.op(), BinaryOp::Logical(_))
                    && (api_kind(binary.lhs(), self.interner, &self.aliases).is_some()
                        || api_kind(binary.rhs(), self.interner, &self.aliases).is_some()) =>
            {
                return ControlFlow::Break("implicit Property truthiness is not admitted".into());
            }
            Expression::Binary(binary)
                if matches!(
                    binary.op(),
                    BinaryOp::Relational(
                        boa_ast::expression::operator::binary::RelationalOp::StrictEqual
                            | boa_ast::expression::operator::binary::RelationalOp::StrictNotEqual
                    )
                ) && (api_kind(binary.lhs(), self.interner, &self.aliases).is_some()
                    || api_kind(binary.rhs(), self.interner, &self.aliases).is_some()) =>
            {
                return ControlFlow::Break(
                    "strict Property identity comparisons are not admitted".into(),
                );
            }
            Expression::Conditional(conditional)
                if api_kind(conditional.condition(), self.interner, &self.aliases).is_some() =>
            {
                return ControlFlow::Break("implicit Property truthiness is not admitted".into());
            }
            Expression::Unary(unary)
                if unary.op() == UnaryOp::Not
                    && api_kind(unary.target(), self.interner, &self.aliases).is_some() =>
            {
                return ControlFlow::Break("implicit Property truthiness is not admitted".into());
            }
            Expression::Unary(unary)
                if !matches!(unary.op(), UnaryOp::Minus | UnaryOp::Plus | UnaryOp::Not) =>
            {
                return ControlFlow::Break("unimplemented unary operator".into());
            }
            Expression::Update(update) => {
                let boa_ast::expression::operator::update::UpdateTarget::Identifier(name) =
                    update.target()
                else {
                    return ControlFlow::Break("property updates are not admitted".into());
                };
                if !self
                    .locals
                    .contains(&name.to_interned_string(self.interner))
                {
                    return ControlFlow::Break("only local updates are admitted".into());
                }
            }
            Expression::ObjectLiteral(object) => {
                for property in object.properties() {
                    let PropertyDefinition::Property(name, value) = property else {
                        return ControlFlow::Break("only text data records are admitted".into());
                    };
                    if !matches!(
                        name.to_interned_string(self.interner).as_str(),
                        "text" | "start"
                    ) || self.is_api_reference(value)
                        || !matches!(
                            value.flatten(),
                            Expression::Literal(_)
                                | Expression::Identifier(_)
                                | Expression::PropertyAccess(_)
                                | Expression::Call(_)
                        )
                    {
                        return ControlFlow::Break(
                            "only plain text/start data fields are admitted".into(),
                        );
                    }
                }
            }
            Expression::RegExpLiteral(_) => {}
            Expression::Literal(literal) => {
                if let LiteralKind::String(symbol) = literal.kind_mut() {
                    // Boa's AST printer quotes strings without escaping their contents.
                    let escaped = self
                        .interner
                        .resolve_expect(*symbol)
                        .to_string()
                        .chars()
                        .flat_map(char::escape_default)
                        .collect::<String>();
                    *symbol = self.interner.get_or_intern(escaped.as_str());
                }
            }
            Expression::TemplateLiteral(_)
            | Expression::ArrayLiteral(_)
            | Expression::Parenthesized(_)
            | Expression::Binary(_)
            | Expression::Unary(_)
            | Expression::Conditional(_)
            | Expression::Call(_) => {}
            _ => {
                return ControlFlow::Break(
                    "expression syntax is outside the numeric profile".into(),
                );
            }
        }
        expression.visit_with_mut(self)?;
        let span = expression.span();
        match expression {
            Expression::Call(call) => {
                if let Expression::PropertyAccess(PropertyAccess::Simple(access)) =
                    call.function().flatten()
                    && matches!(access.field(), PropertyAccessField::Const(name) if name.to_interned_string(self.interner)=="substr")
                    && api_kind(access.target(), self.interner, &self.aliases).is_none()
                {
                    let mut args = vec![access.target().clone()];
                    args.extend_from_slice(call.args());
                    *expression = self.helper("__aeSubstr", args, span);
                    return ControlFlow::Continue(());
                }
                if let Expression::Identifier(name) = call.function().flatten() {
                    let name = name.to_interned_string(self.interner);
                    if !self.locals.contains(&name)
                        && let Some(helper) = random_api_helper(&name)
                    {
                        *expression = self.helper(helper, call.args().to_vec(), span);
                    }
                }
            }
            Expression::Binary(binary) => {
                let helper = match binary.op() {
                    BinaryOp::Arithmetic(ArithmeticOp::Add) => Some("__aeAdd"),
                    BinaryOp::Arithmetic(ArithmeticOp::Sub) => Some("__aeSub"),
                    BinaryOp::Arithmetic(ArithmeticOp::Mul) => Some("__aeMul"),
                    BinaryOp::Arithmetic(ArithmeticOp::Div) => Some("__aeDiv"),
                    BinaryOp::Arithmetic(ArithmeticOp::Mod) => Some("__aeMod"),
                    BinaryOp::Arithmetic(_) => {
                        return ControlFlow::Break("unimplemented arithmetic operator".into());
                    }
                    _ => None,
                };
                if let Some(helper) = helper {
                    *expression = self.helper(
                        helper,
                        vec![binary.lhs().clone(), binary.rhs().clone()],
                        span,
                    );
                }
            }
            Expression::Assign(assign)
                if matches!(
                    assign.op(),
                    AssignOp::Add | AssignOp::Sub | AssignOp::Mul | AssignOp::Div
                ) =>
            {
                let AssignTarget::Identifier(name) = assign.lhs() else {
                    unreachable!()
                };
                let helper = match assign.op() {
                    AssignOp::Add => "__aeAdd",
                    AssignOp::Sub => "__aeSub",
                    AssignOp::Mul => "__aeMul",
                    _ => "__aeDiv",
                };
                let rhs = self.helper(
                    helper,
                    vec![Expression::Identifier(*name), assign.rhs().clone()],
                    span,
                );
                *expression = Assign::new(AssignOp::Assign, assign.lhs().clone(), rhs).into();
            }
            Expression::Unary(unary) if unary.op() == UnaryOp::Minus => {
                *expression = self.helper("__aeNeg", vec![unary.target().clone()], span);
            }
            Expression::Unary(unary) if unary.op() == UnaryOp::Plus => {
                *expression = self.helper("__aeUnwrap", vec![unary.target().clone()], span);
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
#[path = "path_array_tests.rs"]
mod path_array_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn walks_unreachable_calls_and_rejects_ambient_random_and_escape_routes() {
        for code in [
            "if(false) temporalWiggle(1,2); value",
            "Math.random()",
            "effect('a')('b').constructor('x')",
            "thisLayer['effect']('x')",
            "easeInOutQuad(time)",
            "var x=value; x %= 2",
            "var time=0; time",
            "globalThis",
            "eval('value')",
            "if(false) 'not a comp'.layer(1); value",
            "var c=thisComp; c=1; c.layer(1)",
            "var p=0; p=thisProperty; value",
            "if(thisProperty) 1; else 0",
            "var p=thisProperty; p ? 1 : 0",
            "thisProperty === 1",
            "function f() { return f(); } f()",
            "function f() { return g(); } function g() { return f(); } f()",
            "function f() { if(false) return value.constructor('x'); return value; } f()",
            "for (var k in thisLayer) {}",
        ] {
            assert!(compile(code).is_err(), "{code}");
        }
    }
    #[test]
    fn expression_receiver_fields_reject_unsupported_layer_fallbacks() {
        for (code, field) in [
            ("thisLayer.duration || 1", "duration"),
            ("thisLayer.frameDuration || 1", "frameDuration"),
            ("thisLayer.displayStartTime || 1", "displayStartTime"),
            ("thisLayer.numKeys || 1", "numKeys"),
            ("thisComp.index || 1", "index"),
            ("transform.width || 1", "width"),
            ("thisProperty.width || 1", "width"),
        ] {
            let error = compile(code).expect_err(code);
            assert!(
                matches!(error, EvaluationError::Unsupported(ref reason) if reason.contains(&format!("unimplemented member {field}"))),
                "{code}: {error}"
            );
        }
    }

    #[test]
    fn marker_key_time_receivers_are_admitted() {
        compile("var n=thisComp.layer('controller').marker.nearestKey(time).index; thisProperty.valueAtTime(time-thisComp.layer('controller').marker.key(n).time)").unwrap();
    }

    #[test]
    fn expression_receiver_fields_keep_supported_members() {
        for code in [
            "thisComp.width || 1",
            "thisComp.height ? 100 : 0",
            "thisComp.duration + thisComp.frameDuration + thisComp.displayStartTime",
            "thisLayer.index + thisLayer.inPoint + thisLayer.outPoint",
            "transform.position.valueAtTime(time)",
            "thisLayer.position.value",
            "thisProperty.numKeys + thisProperty.length",
            "var l=thisComp.layer(1); l.transform.opacity.value",
            "var p=thisComp.layer(1).effect(1)(1); p.value",
            "value.length",
            "[1,2].length",
            "Math.PI + Math.sin(time)",
            "thisLayer.width + thisComp.layer(1).height",
            "ease(time,0,1,0,100) + easeIn(time,0,100) + easeOut(time,0,1,[0,0],[1,1])[0]",
            "var x=value; x += [1,2]; x",
            "var s=0; for (var i=0; i<3; i++) { s += i; } s",
            "var n=0; while (n < 2) { n++; } n",
            "content('Group 1').transform.position.value",
            "thisLayer.content('Group 1').content('Ellipse Path 1').size",
            "mask('Mask 1').maskOpacity",
            "toComp(anchorPoint) + fromComp([0,0]) + thisLayer.toWorld([0,0])",
            "velocity + speed + transform.position.velocityAtTime(time)",
            "loopOutDuration('cycle', 1) + smooth(0.2, 5)",
            "time % 1 + degreesToRadians(90) + radiansToDegrees(1)",
            "var width = 3; width",
            "thisLayer.parent.transform.position + hasParent",
        ] {
            assert!(compile(code).is_ok(), "{code}");
        }
    }

    #[test]
    fn expression_helpers_reject_api_references_carried_through_exceptions() {
        for code in [
            "function ref(x){ try { throw thisLayer; } catch(x) { return x; } } ref(0).length || 1;",
            "function ref(x){ try { throw thisProperty; } catch(x) { return x; } } ref(0) || 1;",
            "function ref(x){ try { throw linear; } catch(x) { return x; } } ref(0).length || 1;",
            "function ref(x){ if(false) { throw x; } return x; } ref(1);",
            "try { throw 1; } catch(e) { 7; }",
        ] {
            assert!(compile(code).is_err(), "admitted explicit throw: {code}");
        }
    }

    #[test]
    fn transforms_ast_not_strings_or_comments() {
        let code = compile("var x=[1,2]; /* value+random() */ x = x * 2 + [3,4]; x;").unwrap();
        assert!(code.contains("__aeAdd") && code.contains("__aeMul"));
        assert!(!code.contains("random"));
        assert!(
            compile("thisComp.layer('controller').effect('a+b')('Slider')")
                .unwrap()
                .contains("a+b")
        );
    }
}
