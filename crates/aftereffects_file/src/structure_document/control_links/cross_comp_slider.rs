//! Exact repeated XY Scale binding to a static cross-composition Slider.
//! No expression execution, clock conversion, or live controller linkage.

use super::{
    cross_comp, expression, finished, identifier, quoted, reference, resolve, scale_vector, token,
    unique_run,
};
use crate::{
    properties::{self, NumericProperty, NumericValueKind, PropertyError},
    structure::Layer,
};

#[derive(Debug)]
struct Link<'a> {
    composition: &'a str,
    layer: &'a str,
    slider: super::Reference<'a>,
}

fn parse(mut text: &str) -> Option<Link<'_>> {
    let binding = super::slider::binding(&mut text)?;
    // Hoisted/reserved/global names can change the meaning of the RHS call.
    const RESERVED: &str = "Infinity NaN arguments await break case catch class comp const continue debugger default delete do effect else enum eval export extends false finally for function if implements import in instanceof interface let new null package private protected public return static super switch this throw true try typeof undefined var void while with yield";
    if RESERVED
        .split_ascii_whitespace()
        .any(|name| name == binding)
    {
        return None;
    }
    token(&mut text, "comp")?;
    token(&mut text, "(")?;
    let composition = quoted(&mut text)?;
    token(&mut text, ")")?;
    token(&mut text, ".layer")?;
    token(&mut text, "(")?;
    let layer = quoted(&mut text)?;
    token(&mut text, ")")?;
    token(&mut text, ".")?;
    let slider = reference(&mut text)?;
    token(&mut text, ";")?;
    token(&mut text, "[")?;
    if identifier(&mut text)? != binding {
        return None;
    }
    token(&mut text, ",")?;
    if identifier(&mut text)? != binding {
        return None;
    }
    token(&mut text, "]")?;
    finished(text).then_some(Link {
        composition,
        layer,
        slider,
    })
}

pub(super) fn lower(
    layer: &Layer,
    context: cross_comp::Context<'_>,
    base: &NumericProperty,
) -> Option<Result<NumericProperty, PropertyError>> {
    let root = properties::root_runs(&layer.content).ok()?;
    let transform = unique_run(&root, "ADBE Transform Group").ok()?;
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp").ok()?).ok()?;
    let scale = properties::unique_list(unique_run(&leaves, "ADBE Scale").ok()?, *b"tdbs").ok()?;
    let link = parse(expression(scale).ok()?)?;
    Some((|| {
        let dimensions = super::numeric_dimensions(base)?;
        if !matches!(dimensions, 2 | 3) {
            return Err(PropertyError::Layout(
                "cross-composition Scale destination requires two or three native components",
            ));
        }
        cross_comp::validate_destination(base, dimensions)?;
        let source = cross_comp::source(
            context,
            cross_comp::Reference {
                composition: link.composition,
                layer: link.layer,
                member: cross_comp::Member::Scale,
            },
        )?;
        let root = properties::root_runs(&source.layer.content)?;
        let parade = unique_run(&root, "ADBE Effect Parade")?;
        let effects = properties::runs(properties::unique_list(parade, *b"tdgp")?)?;
        static_xy(resolve(&effects, link.slider)?)
    })())
}

fn static_xy(scalar: NumericProperty) -> Result<NumericProperty, PropertyError> {
    if scalar.animated
        || !scalar.keyframes.is_empty()
        || scalar.expression_enabled
        || scalar.value_kind != NumericValueKind::Continuous
        || scalar.dimensions_separated
        || !matches!(scalar.values.as_slice(), [value] if value.is_finite())
    {
        return Err(PropertyError::Layout(
            "cross-composition Scale requires a static finite scalar Slider",
        ));
    }
    scale_vector(scalar, 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_profile_rejects_animated_separated_executable_and_non_scalar_controls() {
        let scalar = NumericProperty {
            values: vec![129.8462],
            animated: false,
            expression_enabled: false,
            expression_present: false,
            dimensions_separated: false,
            keyframes: Vec::new(),
            value_kind: NumericValueKind::Continuous,
        };
        assert_eq!(
            static_xy(scalar.clone()).unwrap().values,
            vec![129.8462 * 0.01; 2]
        );
        let mut animated = scalar.clone();
        animated.animated = true;
        assert!(static_xy(animated).is_err());
        let mut separated = scalar.clone();
        separated.dimensions_separated = true;
        assert!(static_xy(separated).is_err());
        let mut executable = scalar.clone();
        executable.expression_enabled = true;
        assert!(static_xy(executable).is_err());
        for values in [vec![], vec![1.0, 2.0], vec![f64::NAN], vec![f64::INFINITY]] {
            let mut invalid = scalar.clone();
            invalid.values = values;
            assert!(static_xy(invalid).is_err());
        }
    }

    const EXPRESSION: &str = "temp = comp(\"Render\").layer(\"Controls\").effect(\"Text Scale\")(\"ADBE Slider Control-0001\"); [temp, temp]";

    #[test]
    fn grammar_requires_one_complete_repeated_xy_binding() {
        assert!(parse(EXPRESSION).is_some());
        assert!(parse(&format!("var {EXPRESSION};")).is_some());
        assert!(parse(&EXPRESSION.replace("\"ADBE Slider Control-0001\"", "1")).is_some());
        for invalid in [
            format!("{EXPRESSION}; evil()"),
            EXPRESSION.replace("temp", "comp"),
            EXPRESSION.replace("temp", "var"),
            EXPRESSION.replace("temp", "Infinity"),
            EXPRESSION.replace("[temp, temp]", "[temp, other]"),
            EXPRESSION.replace("[temp, temp]", "[temp, temp, temp]"),
            EXPRESSION.replace("[temp, temp]", "[temp * time, temp]"),
            EXPRESSION.replace("; [", "; unused = 1; ["),
            EXPRESSION.replace("comp(\"Render\")", "comp(\"Render\", time)"),
        ] {
            assert!(parse(&invalid).is_none(), "{invalid}");
        }
    }
}
