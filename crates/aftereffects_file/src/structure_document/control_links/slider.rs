//! Bounded lowering for direct sibling Slider references, and the grammar of
//! the same-layer Source Text Slider percent binding.

use super::{Reference, display_name, finished, identifier, quoted, reference, token, unique_run};
use crate::{
    properties::{self, NumericProperty, NumericValueKind, PropertyError},
    structure::{Composition, Layer},
};

#[derive(Clone, Copy, Debug, PartialEq)]
struct SiblingReference<'a> {
    layer: &'a str,
    effect: &'a str,
    parameter: &'a str,
}

fn sibling_reference<'a>(text: &mut &'a str) -> Option<SiblingReference<'a>> {
    token(text, "thisComp.layer")?;
    token(text, "(")?;
    let layer = quoted(text)?;
    token(text, ")")?;
    token(text, ".")?;
    let reference = reference(text)?;
    Some(SiblingReference {
        layer,
        effect: reference.effect,
        parameter: reference.parameter,
    })
}

fn signed_scalar(mut text: &str) -> Option<(f64, SiblingReference<'_>)> {
    text = text.trim_start();
    let sign = if token(&mut text, "-").is_some() {
        -1.0
    } else {
        let _ = token(&mut text, "+");
        1.0
    };
    let reference = sibling_reference(&mut text)?;
    finished(text).then_some((sign, reference))
}

/// `[var] name =`: the start of one plain binding statement.
fn binding<'a>(text: &mut &'a str) -> Option<&'a str> {
    *text = text.trim_start();
    if let Some(rest) = text.strip_prefix("var")
        && rest.starts_with(char::is_whitespace)
    {
        *text = rest;
    }
    let name = identifier(text)?;
    token(text, "=")?;
    Some(name)
}

fn repeated_vector(mut text: &str) -> Option<SiblingReference<'_>> {
    let binding = binding(&mut text)?;
    let reference = sibling_reference(&mut text)?;
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
    finished(text).then_some(reference)
}

/// `[var] s = effect("…")("…"); Math.round(s).toLocaleString() + "%";`, where
/// `s` is one ASCII letter: a longer free name can resolve to an AE layer or
/// property attribute (`value`, `time`, `index`, ...) instead of the binding.
pub(super) fn percent_reference(mut text: &str) -> Option<Reference<'_>> {
    let name = binding(&mut text)?;
    if !matches!(name.as_bytes(), [letter] if letter.is_ascii_alphabetic()) {
        return None;
    }
    let reference = reference(&mut text)?;
    token(&mut text, ";")?;
    token(&mut text, "Math.round")?;
    token(&mut text, "(")?;
    if identifier(&mut text)? != name {
        return None;
    }
    token(&mut text, ")")?;
    token(&mut text, ".toLocaleString")?;
    token(&mut text, "(")?;
    token(&mut text, ")")?;
    token(&mut text, "+")?;
    if quoted(&mut text)? != "%" {
        return None;
    }
    finished(text).then_some(reference)
}

pub(super) fn lower_signed_scalar(
    owner: &Layer,
    composition: &Composition,
    expression: &str,
) -> Result<NumericProperty, PropertyError> {
    let (sign, reference) = signed_scalar(expression).ok_or(PropertyError::Layout(
        "not a complete signed sibling Slider reference",
    ))?;
    resolve(owner, composition, reference, sign)
}

pub(super) fn lower_repeated_vector(
    owner: &Layer,
    composition: &Composition,
    expression: &str,
) -> Result<NumericProperty, PropertyError> {
    let reference = repeated_vector(expression).ok_or(PropertyError::Layout(
        "not a repeated sibling Slider vector",
    ))?;
    let mut numeric = resolve(owner, composition, reference, 1.0)?;
    repeat_component(&mut numeric.values);
    for key in &mut numeric.keyframes {
        repeat_component(&mut key.values);
        repeat_component(&mut key.in_speed);
        repeat_component(&mut key.in_influence);
        repeat_component(&mut key.out_speed);
        repeat_component(&mut key.out_influence);
    }
    Ok(numeric)
}

fn repeat_component(values: &mut Vec<f64>) {
    if let [value] = values.as_slice() {
        values.push(*value);
    }
}

fn resolve(
    owner: &Layer,
    composition: &Composition,
    reference: SiblingReference<'_>,
    sign: f64,
) -> Result<NumericProperty, PropertyError> {
    let mut matches = composition
        .layers
        .iter()
        .filter(|candidate| candidate.name.as_ref() == reference.layer);
    let controller = matches
        .next()
        .ok_or(PropertyError::Layout("Slider controller layer missing"))?;
    if matches.next().is_some() {
        return Err(PropertyError::Layout("ambiguous Slider controller layer"));
    }

    let (Some(owner_start), Some(stretch)) = (owner.record.start_time(), owner.record.stretch())
    else {
        return Err(PropertyError::Layout("invalid Slider owner clock"));
    };
    let controller_start = controller
        .record
        .start_time()
        .ok_or(PropertyError::Layout("invalid Slider controller clock"))?;
    if !owner_start.is_finite()
        || !controller_start.is_finite()
        || !stretch.is_finite()
        || stretch <= 0.0
        || controller.record.stretch() != Some(stretch)
    {
        return Err(PropertyError::Layout(
            "Slider binding requires equal positive source stretch",
        ));
    }

    let root = properties::root_runs(&controller.content)?;
    let parade = unique_run(&root, "ADBE Effect Parade")?;
    let effects = properties::runs(properties::unique_list(parade, *b"tdgp")?)?;
    let mut matches = effects.iter().filter_map(|(kind, run)| {
        let plugin = properties::unique_list(run, *b"sspc").ok()?;
        let body = properties::unique_list(plugin, *b"tdgp").ok()?;
        (display_name(body) == Some(reference.effect)).then_some((*kind, body))
    });
    let (kind, body) = matches
        .next()
        .ok_or(PropertyError::Layout("Slider control missing"))?;
    if matches.next().is_some() || kind != "ADBE Slider Control" {
        return Err(PropertyError::Layout(
            "ambiguous or mismatched Slider control kind",
        ));
    }
    let parameters = properties::runs(body)?;
    let mut matches = parameters.iter().filter_map(|(name, run)| {
        let body = properties::unique_list(run, *b"tdbs").ok()?;
        (*name == reference.parameter || display_name(body) == Some(reference.parameter))
            .then_some((*name, body))
    });
    let (name, body) = matches
        .next()
        .ok_or(PropertyError::Layout("Slider parameter missing"))?;
    if matches.next().is_some() || name != "ADBE Slider Control-0001" {
        return Err(PropertyError::Layout(
            "ambiguous or non-value Slider parameter",
        ));
    }

    let mut numeric = properties::read_numeric(body)?;
    if numeric.expression_enabled
        || numeric.dimensions_separated
        || numeric.value_kind == NumericValueKind::Color
        || (numeric.animated && numeric.keyframes.is_empty())
        || (!numeric.animated && numeric.values.len() != 1)
        || (!numeric.values.is_empty() && numeric.values.len() != 1)
        || numeric.keyframes.iter().any(|key| {
            key.values.len() != 1 || !key.spatial_in.is_empty() || !key.spatial_out.is_empty()
        })
    {
        return Err(PropertyError::Layout("unsupported Slider scalar curve"));
    }
    for value in numeric.values.iter_mut().chain(
        numeric
            .keyframes
            .iter_mut()
            .flat_map(|key| key.values.iter_mut()),
    ) {
        *value *= sign;
        if !value.is_finite() {
            return Err(PropertyError::Layout("nonfinite Slider value"));
        }
    }
    let offset = (controller_start - owner_start) / stretch;
    if !offset.is_finite() {
        return Err(PropertyError::Layout("invalid Slider clock offset"));
    }
    for key in &mut numeric.keyframes {
        key.time_secs += offset;
        if !key.time_secs.is_finite() {
            return Err(PropertyError::Layout("invalid rebased Slider key"));
        }
        for speed in key.in_speed.iter_mut().chain(&mut key.out_speed) {
            *speed *= sign;
        }
    }
    numeric.value_kind = NumericValueKind::Continuous;
    numeric.expression_enabled = false;
    numeric.expression_present = false;
    Ok(numeric)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIRECT: &str = "thisComp.layer(\"Circle_cntrl_01\").effect(\"Separation\")(\"Slider\")";
    const VECTOR: &str =
        "temp = thisComp.layer(\"Circle_cntrl_01\").effect(\"Size\")(\"Slider\");\r[temp, temp]";
    /// A complete Source Text percent expression, with AE's CR statement separator.
    const PERCENT: &str =
        "s = effect(\"Slider Control\")(\"Slider\");\rMath.round(s).toLocaleString() + \"%\";";

    #[test]
    fn percent_grammar_is_complete_and_binds_one_letter() {
        let slider = |effect| {
            Some(Reference {
                effect,
                parameter: "Slider",
            })
        };
        for (accepted, expected) in [
            (PERCENT.to_owned(), slider("Slider Control")),
            (format!("var {PERCENT}"), slider("Slider Control")),
            (PERCENT.replace('"', "'"), slider("Slider Control")),
            (
                PERCENT.trim_end_matches(';').to_owned(),
                slider("Slider Control"),
            ),
            (
                PERCENT
                    .replace("s = ", "v = ")
                    .replace("(s)", "(v)")
                    .replace("Slider Control", "Progress"),
                slider("Progress"),
            ),
            (
                " s = effect ( 'Slider Control' ) ( 'Slider' ) ;\n Math.round ( s ) .toLocaleString ( ) + '%' ;"
                    .to_owned(),
                slider("Slider Control"),
            ),
        ] {
            assert_eq!(percent_reference(&accepted), expected, "{accepted}");
        }
        for rejected in [
            PERCENT.replace("round(s)", "round(t)"),
            PERCENT
                .replace("s = ", "slider = ")
                .replace("(s)", "(slider)"),
            PERCENT.replace("s = ", "Math = ").replace("(s)", "(Math)"),
            PERCENT.replace("s = ", "$ = ").replace("(s)", "($)"),
            PERCENT.replace("s = ", "_ = ").replace("(s)", "(_)"),
            PERCENT.replace("Math.round", "Math.floor"),
            PERCENT.replace("Math.round(s)", "s"),
            PERCENT.replace(".toLocaleString()", ".toFixed(0)"),
            PERCENT.replace(".toLocaleString()", ""),
            PERCENT.replace("\"%\"", "\" %\""),
            PERCENT.replace("\"%\"", "\"\\u0025\""),
            PERCENT.replace("(\"Slider\");", "(\"Slider\").value;"),
            PERCENT.replace("effect(", "thisComp.layer(\"Controller\").effect("),
            PERCENT.replace("effect(\"Slider Control\")", "effect(1)"),
            PERCENT.replace("s = ", ""),
            format!("{PERCENT} s"),
            format!("{PERCENT}\rs * 2;"),
        ] {
            assert_eq!(percent_reference(&rejected), None, "{rejected}");
        }
    }

    #[test]
    fn exact_scalar_and_repeated_vector_grammar_rejects_executable_suffixes() {
        assert_eq!(signed_scalar(DIRECT).unwrap().0, 1.0);
        assert_eq!(signed_scalar(&format!("-{DIRECT}")).unwrap().0, -1.0);
        assert!(repeated_vector(VECTOR).is_some());
        assert!(repeated_vector(&format!("var {VECTOR}")).is_some());
        for expression in [
            format!("{DIRECT} + 1"),
            format!("{DIRECT}.value"),
            format!("{DIRECT}; execute()"),
            VECTOR.replace("[temp, temp]", "[temp, other]"),
            VECTOR.replace("[temp, temp]", "[temp, temp][0]"),
        ] {
            assert!(
                signed_scalar(&expression).is_none() && repeated_vector(&expression).is_none(),
                "{expression}"
            );
        }
    }
}
