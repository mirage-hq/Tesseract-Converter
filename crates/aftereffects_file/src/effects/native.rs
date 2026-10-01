//! Bounded decoding of AE's native Effect Parade, including sparse plugin defaults.
//! A project's EfdG contains definitions; a layer-side `sspc` repeats its `parT`.

mod levels;

use super::definitions::{self, ParameterDefinition};
use crate::{
    properties::{self, NumericProperty, NumericValueKind, PropertyError},
    rifx::Chunk,
};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DecodedEffect {
    pub match_name: String,
    /// One-based occurrence in the native Effect Parade, including omitted siblings.
    pub index: usize,
    pub enabled: bool,
    /// This instance's own `parT` declaration table.
    pub declarations: Declarations,
    pub parameters: Vec<DecodedParameter>,
}

/// The state of an instance's own `sspc/parT` declaration table. Controls
/// without a usable declaration fall back to the checked-in catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Declarations {
    /// AE stored no table.
    Missing,
    /// One table whose declaration names decode; it can be empty.
    Readable,
    /// Duplicated tables or undecodable declaration names.
    Unreadable,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DecodedParameter {
    pub match_name: String,
    /// Native `pard` parameter type from this instance's own declaration, or
    /// `Ok(None)` without one. An error means the control is declared more
    /// than once or its `pard` is missing, duplicated or malformed.
    pub declared_kind: Result<Option<u32>, PropertyError>,
    pub numeric: Result<NumericProperty, PropertyError>,
    /// No explicit record exists and the instance's one declaration is a
    /// type-12 mask path whose default payload is all zero: no path is
    /// selected. `numeric` stays an error because a path is not a number.
    pub unset_path: bool,
}

/// Extract all named native effect controls. Unsupported controls remain as
/// explicit errors, never silently become scalar zeroes.
pub(crate) fn read_effects(content: &[Chunk], size: [f64; 2]) -> (Vec<DecodedEffect>, Vec<String>) {
    let mut warnings = Vec::new();
    let Ok(root) = properties::root_runs(content) else {
        return (
            Vec::new(),
            vec!["effect layer has malformed property names".into()],
        );
    };
    let mut parades = root
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Effect Parade");
    let Some((_, parade)) = parades.next() else {
        return (Vec::new(), warnings);
    };
    if parades.next().is_some() {
        return (
            Vec::new(),
            vec!["duplicate Effect Parade roots; ambiguous effects omitted".into()],
        );
    }
    let Ok(groups) = properties::unique_list(parade, *b"tdgp") else {
        return (Vec::new(), vec!["effect parade group missing".into()]);
    };
    let Ok(instances) = properties::runs(groups) else {
        return (Vec::new(), vec!["effect parade names malformed".into()]);
    };
    let mut effects = Vec::with_capacity(instances.len());
    for (index, (match_name, run)) in instances.into_iter().enumerate() {
        let Some(sspc) = run
            .iter()
            .find(|chunk| chunk.list_kind() == Some(*b"sspc"))
            .and_then(Chunk::children)
        else {
            warnings.push(format!("{match_name}: native plugin descriptor missing"));
            continue;
        };
        let Some(body) = sspc
            .iter()
            .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
            .and_then(Chunk::children)
        else {
            warnings.push(format!("{match_name}: native plugin controls missing"));
            continue;
        };
        let enabled = properties::group_enabled_or_warn(sspc, match_name, &mut warnings);
        let explicit = match properties::runs(body) {
            Ok(explicit) => explicit,
            Err(error) => {
                warnings.push(format!(
                    "{match_name}: explicit control table malformed ({error}); effect omitted"
                ));
                continue;
            }
        };
        let packed_levels = if match_name == "ADBE Easy Levels2" {
            match levels::read(&explicit, &mut warnings) {
                Ok(master) => master,
                Err(error) => {
                    warnings.push(format!(
                        "{match_name}: packed Levels {error}; effect omitted, siblings retained"
                    ));
                    continue;
                }
            }
        } else {
            None
        };
        let tables: Vec<_> = sspc
            .iter()
            .filter(|chunk| chunk.list_kind() == Some(*b"parT"))
            .filter_map(Chunk::children)
            .collect();
        let (declarations, local_definitions) = match tables.as_slice() {
            [] => {
                warnings.push(format!(
                    "{match_name}: native parameter definitions missing"
                ));
                (Declarations::Missing, Vec::new())
            }
            [table] => match properties::runs(table) {
                Ok(definitions) => (Declarations::Readable, definitions),
                Err(error) => {
                    warnings.push(format!(
                        "{match_name}: native parameter definitions malformed: {error}"
                    ));
                    (Declarations::Unreadable, Vec::new())
                }
            },
            _ => {
                warnings.push(format!(
                    "{match_name}: duplicate native parameter definition tables"
                ));
                (Declarations::Unreadable, Vec::new())
            }
        };
        let canonical = definitions::definition(match_name);
        let mut names = Vec::new();
        for name in local_definitions
            .iter()
            .map(|(name, _)| *name)
            .chain(canonical.into_iter().flat_map(|effect| {
                effect
                    .parameters
                    .iter()
                    .map(|parameter| parameter.match_name.as_str())
            }))
            .chain(explicit.iter().map(|(name, _)| *name))
            .chain(super::toner::parameter_names(match_name).iter().copied())
        {
            if packed_levels.is_some() && name == levels::HISTOGRAM {
                continue;
            }
            if name != "ADBE Effect Built In Params"
                && !name.ends_with("-0000")
                && !names.contains(&name)
            {
                names.push(name);
            }
        }
        let parameters = names
            .into_iter()
            .map(|name| {
                let mut declared = local_definitions
                    .iter()
                    .filter(|(candidate, _)| *candidate == name)
                    .map(|(_, definition)| *definition);
                let local = declared.next();
                let first_kind = local.map(declaration_kind);
                let declared_kind = if declared.next().is_some() {
                    Err(PropertyError::Layout("duplicate effect declaration"))
                } else {
                    first_kind.clone().transpose()
                };
                if let Some(numeric) = packed_levels
                    .as_ref()
                    .and_then(|master| master.numeric(name))
                {
                    // UI slots describe a selected channel, not the render master.
                    // Their cache values/type hints cannot override packed floats.
                    return DecodedParameter {
                        match_name: name.to_owned(),
                        declared_kind,
                        numeric: Ok(numeric),
                        unset_path: false,
                    };
                }
                let canonical = canonical.and_then(|effect| {
                    effect
                        .parameters
                        .iter()
                        .find(|parameter| parameter.match_name == name)
                });
                // Rounding and point rules keep using the first well-formed declaration.
                let kind = first_kind
                    .and_then(Result::ok)
                    .or_else(|| canonical.map(|parameter| parameter.kind));
                let explicit_run = explicit.iter().find(|(id, _)| *id == name);
                let duplicate = duplicate_record_error(match_name)
                    .filter(|_| explicit.iter().filter(|(id, _)| *id == name).count() > 1);
                let unset_path = explicit_run.is_none()
                    && declared_kind.is_ok()
                    && local.is_some_and(unset_path_default);
                let mut relative_point = false;
                let numeric = if let Some(error) = duplicate {
                    Err(PropertyError::Layout(error))
                } else if let Some((_, run)) = explicit_run {
                    match properties::unique_list(run, *b"tdbs") {
                        Ok(leaf) => {
                            relative_point = (kind == Some(6)
                                || (kind.is_none()
                                    && match_name == "ADBE Geometry2"
                                    && matches!(
                                        name,
                                        "ADBE Geometry2-0001" | "ADBE Geometry2-0002"
                                    )))
                                && properties::data(leaf, *b"tdb4")
                                    .is_ok_and(|meta| meta.len() == 124 && meta[59] == 4);
                            if relative_point {
                                properties::read_effect_point(leaf)
                            } else {
                                properties::read_numeric(leaf)
                            }
                        }
                        Err(error) => Err(error),
                    }
                } else if let Some(definition) = local {
                    default_numeric(definition, size)
                } else {
                    canonical_numeric(canonical, size)
                        .or_else(|| super::toner::default_parameter(match_name, name).map(Ok))
                        .unwrap_or(Err(PropertyError::Layout(
                            "effect parameter definition missing",
                        )))
                };
                let mut numeric = numeric;
                // Adobe's plugin Points (type flag 4) store fractions of source
                // bounds. Legacy independent-component records already use pixels.
                if relative_point && let Ok(point) = &mut numeric {
                    scale_relative_point(point, size);
                }
                // PF_Param_SLIDER exposes integral values in Adobe even when its
                // key record contains a fractional double supplied by scripting.
                if kind == Some(1)
                    && let Ok(numeric) = &mut numeric
                {
                    round_integer_values(numeric);
                }
                if let Err(error) = &numeric
                    && !unset_path
                {
                    warnings.push(format!("{match_name}/{name}: {error}"));
                }
                DecodedParameter {
                    match_name: name.to_owned(),
                    declared_kind,
                    numeric,
                    unset_path,
                }
            })
            .collect();
        effects.push(DecodedEffect {
            match_name: match_name.to_owned(),
            index: index + 1,
            enabled,
            declarations,
            parameters,
        });
    }
    (effects, warnings)
}

/// Bounded plugin profiles reject a control with duplicate explicit records
/// instead of silently using the first one. Each keeps its own diagnostic.
fn duplicate_record_error(match_name: &str) -> Option<&'static str> {
    match match_name {
        super::keylight::MATCH_NAME => Some("duplicate effect control record"),
        "CC Toner" => Some("duplicate Toner control"),
        _ => None,
    }
}

/// The parameter type of one well-formed 148-byte `pard` declaration.
fn declaration_kind(definition: &[Chunk]) -> Result<u32, PropertyError> {
    let bytes = properties::data(definition, *b"pard")?;
    if bytes.len() != 148 {
        return Err(PropertyError::Layout("effect pard length"));
    }
    Ok(u32::from_be_bytes(
        bytes[12..16].try_into().expect("four-byte kind"),
    ))
}

/// A `pard` type-12 (mask path) declaration whose whole default payload is
/// zero selects no path. Any other payload remains undecoded.
fn unset_path_default(definition: &[Chunk]) -> bool {
    properties::data(definition, *b"pard").is_ok_and(|bytes| {
        bytes.len() == 148
            && bytes[12..16] == 12_u32.to_be_bytes()
            && bytes[56..].iter().all(|byte| *byte == 0)
    })
}

fn canonical_numeric(
    definition: Option<&ParameterDefinition>,
    size: [f64; 2],
) -> Option<Result<NumericProperty, PropertyError>> {
    let definition = definition?;
    let values = definition.values(size)?;
    let value_kind = match definition.kind {
        1 | 4 | 7 => NumericValueKind::Integer,
        5 => NumericValueKind::Color,
        _ => NumericValueKind::Continuous,
    };
    Some(Ok(NumericProperty {
        values,
        animated: false,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: Vec::new(),
        value_kind,
    }))
}

fn scale_relative_point(point: &mut NumericProperty, size: [f64; 2]) {
    for (component, value) in point.values.iter_mut().enumerate() {
        *value *= size[component];
    }
    for key in &mut point.keyframes {
        for (component, value) in key.values.iter_mut().enumerate() {
            *value *= size[component];
        }
        for (component, value) in key.in_speed.iter_mut().enumerate() {
            *value *= size[component];
        }
        for (component, value) in key.out_speed.iter_mut().enumerate() {
            *value *= size[component];
        }
    }
}

fn round_integer_values(numeric: &mut NumericProperty) {
    for value in &mut numeric.values {
        *value = value.round();
    }
    for key in &mut numeric.keyframes {
        for value in &mut key.values {
            *value = value.round();
        }
    }
    numeric.value_kind = NumericValueKind::Integer;
}

/// `pard` is a 148-byte native parameter declaration, not a `tdb4` leaf.
/// Its value slots use plugin-specific storage; point defaults need layer size.
fn default_numeric(definition: &[Chunk], size: [f64; 2]) -> Result<NumericProperty, PropertyError> {
    let bytes = properties::data(definition, *b"pard")?;
    if bytes.len() != 148 {
        return Err(PropertyError::Layout("effect pard length"));
    }
    let kind = u32::from_be_bytes(
        bytes[12..16]
            .try_into()
            .map_err(|_| PropertyError::Layout("effect pard kind"))?,
    );
    let value = u32::from_be_bytes(
        bytes[56..60]
            .try_into()
            .map_err(|_| PropertyError::Layout("effect pard default"))?,
    );
    let (values, value_kind) = match kind {
        2 | 3 => {
            // AE plugin PF_Fixed and PF_Angle defaults are signed 16:16,
            // including small nonzero fractions (not whole-number integers).
            let raw = i32::from_be_bytes(
                bytes[56..60]
                    .try_into()
                    .map_err(|_| PropertyError::Layout("effect slider default"))?,
            );
            (
                vec![f64::from(raw) / 65_536.0],
                NumericValueKind::Continuous,
            )
        }
        1 | 4 | 7 => (vec![f64::from(value)], NumericValueKind::Integer),
        6 => {
            if size.iter().any(|v| !v.is_finite() || *v <= 0.0) {
                return Err(PropertyError::Layout(
                    "effect point default requires source dimensions",
                ));
            }
            let x = i32::from_be_bytes(
                bytes[56..60]
                    .try_into()
                    .map_err(|_| PropertyError::Layout("point x default"))?,
            );
            let y = i32::from_be_bytes(
                bytes[60..64]
                    .try_into()
                    .map_err(|_| PropertyError::Layout("point y default"))?,
            );
            (
                vec![
                    f64::from(x) / 65_536.0 * size[0],
                    f64::from(y) / 65_536.0 * size[1],
                ],
                NumericValueKind::Continuous,
            )
        }
        5 => {
            let rgba = [bytes[57], bytes[58], bytes[59], bytes[56]];
            (
                rgba.into_iter()
                    .map(|channel| f64::from(channel) / 255.0)
                    .collect(),
                NumericValueKind::Color,
            )
        }
        10 => (
            vec![f64::from_be_bytes(bytes[56..64].try_into().map_err(
                |_| PropertyError::Layout("effect floating default"),
            )?)],
            NumericValueKind::Continuous,
        ),
        _ => return Err(PropertyError::Layout("unsupported effect default kind")),
    };
    if values.iter().any(|v| !v.is_finite()) {
        return Err(PropertyError::NonFinite);
    }
    Ok(NumericProperty {
        values,
        animated: false,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: Vec::new(),
        value_kind,
    })
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;

#[cfg(test)]
mod levels_tests;
