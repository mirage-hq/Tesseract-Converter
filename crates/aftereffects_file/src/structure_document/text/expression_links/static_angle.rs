//! Exact direct static Angle Control aliases for scalar text animator controls.
//! This is not an additional operand in the affine Slider-expression grammar.

use super::{
    NumericProperty, PropertyError, finished, quoted, read_numeric, runs, token, unique_list,
};
use crate::{
    rifx::Chunk,
    structure_document::control_links::{display_name, effect_name},
};

struct Access<'a> {
    effect: &'a str,
    parameter: Option<&'a str>,
}

fn parse(mut text: &str) -> Option<Access<'_>> {
    token(&mut text, "effect")?;
    token(&mut text, "(")?;
    let effect = quoted(&mut text)?;
    token(&mut text, ")")?;
    token(&mut text, "(")?;
    let parameter = if let Some(parameter) = quoted(&mut text) {
        Some(parameter)
    } else {
        // AE effect(1) addresses the visible value, not hidden parameter 0000.
        token(&mut text, "1")?;
        None
    };
    token(&mut text, ")")?;
    finished(text).then_some(Access { effect, parameter })
}

pub(super) fn lower(
    text: &str,
    own: &NumericProperty,
    effects: Result<&[(&str, &[Chunk])], PropertyError>,
) -> Option<Result<NumericProperty, PropertyError>> {
    let access = parse(text)?;
    let effects = match effects {
        Ok(effects) => effects,
        Err(error) => return Some(Err(error)),
    };
    let named = || {
        effects.iter().filter_map(|(kind, run)| {
            let plugin = unique_list(run, *b"sspc").ok()?;
            let body = unique_list(plugin, *b"tdgp").ok()?;
            (effect_name(plugin, body) == Some(access.effect)).then_some((*kind, body))
        })
    };
    // Leave Sliders and other controls to the unchanged existing grammar.
    if !named().any(|(kind, _)| kind == "ADBE Angle Control") {
        return None;
    }
    Some((|| {
        if own.values.len() != 1 || own.dimensions_separated {
            return Err(PropertyError::Layout(
                "static Angle alias requires a scalar owner",
            ));
        }
        let mut candidates = named();
        let (_, body) = candidates
            .next()
            .ok_or(PropertyError::Layout("Angle Control missing"))?;
        if candidates.next().is_some() {
            return Err(PropertyError::Layout("ambiguous Angle Control effect"));
        }
        let parameters = runs(body)?;
        let mut candidates = parameters.iter().filter_map(|(name, run)| {
            let body = unique_list(run, *b"tdbs").ok()?;
            let matches = match access.parameter {
                Some(parameter) => *name == parameter || display_name(body) == Some(parameter),
                None => *name == "ADBE Angle Control-0001",
            };
            matches.then_some((*name, body))
        });
        let (name, body) = candidates
            .next()
            .ok_or(PropertyError::Layout("Angle Control value missing"))?;
        if candidates.next().is_some() || name != "ADBE Angle Control-0001" {
            return Err(PropertyError::Layout(
                "ambiguous or non-value Angle Control parameter",
            ));
        }
        let angle = read_numeric(body)?;
        if angle.expression_enabled
            || angle.animated
            || !angle.keyframes.is_empty()
            || angle.values.len() != 1
            || !angle.values[0].is_finite()
        {
            return Err(PropertyError::Layout(
                "Angle alias requires a finite static scalar control",
            ));
        }
        Ok(super::static_property(angle.values))
    })())
}
