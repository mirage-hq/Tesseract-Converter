//! Occurrence-local numeric expressions with diagnosed random approximations.
//! Never emits FX script code.

mod model;
mod syntax;
#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashMap};

use boa_engine::{JsValue, js_string, object::JsObject};
use fx_keyframe_bake::script::{ScriptError, ScriptRuntime};
use thiserror::Error;

use crate::{
    expression_samples::{EvaluatedProperty, ExpressionEvaluationError, ExpressionSamples},
    structure::{Composition, ProjectItem},
};
use model::Model;

const SHIM: &str = include_str!("shim.js");
const RANDOM: &str = include_str!("random.js");
const LOOP_ITERATION_LIMIT: u64 = 1_000_000;

/// Random APIs used by this owner, including evaluated numeric dependencies,
/// and approximations applied to the owner's own native keys before evaluation.
#[derive(Debug)]
pub(crate) struct Approximation {
    pub(crate) layer_id: u32,
    pub(crate) property: crate::expression_samples::PropertyIdentity,
    pub(crate) apis: BTreeSet<String>,
    pub(crate) key_notes: Vec<String>,
}

#[derive(Debug, Error)]
enum EvaluationError {
    #[error("expression syntax: {0}")]
    Syntax(#[from] boa_parser::Error),
    #[error("expression is not admitted: {0}")]
    Unsupported(String),
    #[error("expression runtime: {0}")]
    Script(#[from] ScriptError),
    #[error("expression model serialization: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Snapshot after Essential overrides; caller owns lifetime and original sidecars.
/// Samples are only source-frame observations, not Adobe-native evidence.
#[cfg(test)]
pub(crate) fn evaluate_occurrence(
    items: &HashMap<u32, &ProjectItem>,
    comp_id: u32,
    comp: &Composition,
    captured: &ExpressionSamples,
    has_overrides: bool,
) -> ExpressionSamples {
    evaluate_occurrence_with_diagnostics(
        items,
        comp_id,
        comp,
        captured,
        has_overrides,
        &mut Vec::new(),
    )
}

/// Evaluate one overridden occurrence without changing serialized Adobe sidecars.
/// Successful random samples report approximation separately from fatal failures.
pub(crate) fn evaluate_occurrence_with_diagnostics(
    items: &HashMap<u32, &ProjectItem>,
    comp_id: u32,
    comp: &Composition,
    captured: &ExpressionSamples,
    has_overrides: bool,
    approximations: &mut Vec<Approximation>,
) -> ExpressionSamples {
    let mut result = ExpressionSamples::default();
    if !has_overrides {
        // Only this occurrence's composition is queried by its lowering context.
        // Never clone an entire all-compositions capture at every recursive level.
        result.properties.extend(
            captured
                .properties()
                .iter()
                .filter(|sample| sample.composition_id() == comp_id)
                .cloned(),
        );
        result.errors.extend(
            captured
                .errors()
                .iter()
                .filter(|error| error.composition_id() == comp_id)
                .cloned(),
        );
    }
    // Source-ID-only Adobe records cannot describe an overridden occurrence.
    let model = Model::new(items, comp_id, comp, !has_overrides);
    let enabled: Vec<_> = model
        .properties
        .iter()
        .enumerate()
        .filter(|(_, p)| p.comp_id == comp_id && p.expression_enabled)
        .filter(|(_, p)| {
            result.lookup(comp_id, p.layer_id, &p.identity).is_none()
                && !result.errors().iter().any(|error| {
                    error.composition_id() == comp_id
                        && error.layer_id() == p.layer_id
                        && error.property() == &p.identity
                })
        })
        .map(|(slot, p)| (slot, p.layer_id, p.identity.clone(), p.notes.clone()))
        .collect();
    if enabled.is_empty() {
        return result;
    }
    let prepared = prepare(&model);
    let mut prepared = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            for (_, layer_id, property, _) in enabled {
                result.errors.push(ExpressionEvaluationError {
                    composition_id: comp_id,
                    layer_id,
                    property,
                    message: error.to_string(),
                });
            }
            return result;
        }
    };
    let grid = frame_grid(comp.duration_secs, comp.frame_rate).and_then(|times| {
        if times.len().saturating_mul(enabled.len()) > 250_000 {
            return Err("occurrence expression sample budget exceeds 250000 vectors".into());
        }
        Ok(times)
    });
    for (slot, layer_id, property, key_notes) in enabled {
        if matches!(
            property,
            crate::expression_samples::PropertyIdentity::SourceText {}
        ) {
            let texts = grid
                .as_ref()
                .map_err(|error| EvaluationError::Unsupported(error.clone()))
                .and_then(|times| evaluate_text_grid(&mut prepared, slot, times));
            match texts {
                Ok(texts) => {
                    if !prepared.approximated.is_empty() {
                        approximations.push(Approximation {
                            layer_id,
                            property: property.clone(),
                            apis: prepared.approximated.clone(),
                            key_notes,
                        });
                    }
                    result.texts.push(crate::expression_samples::EvaluatedText {
                        composition_id: comp_id,
                        layer_id,
                        sample_times_seconds: grid.as_ref().cloned().unwrap_or_default(),
                        texts,
                    });
                }
                Err(error) => result.errors.push(ExpressionEvaluationError {
                    composition_id: comp_id,
                    layer_id,
                    property,
                    message: error.to_string(),
                }),
            }
            continue;
        }
        let evaluated = grid
            .as_ref()
            .map_err(|error| EvaluationError::Unsupported(error.clone()))
            .and_then(|times| evaluate_grid(&mut prepared, slot, times));
        match evaluated {
            Ok(values) => {
                if !prepared.approximated.is_empty() || !key_notes.is_empty() {
                    approximations.push(Approximation {
                        layer_id,
                        property: property.clone(),
                        apis: prepared.approximated.clone(),
                        key_notes,
                    });
                }
                result.properties.push(EvaluatedProperty {
                    composition_id: comp_id,
                    layer_id,
                    property,
                    start_ms: 0,
                    sample_times_seconds: grid.as_ref().cloned().unwrap_or_default(),
                    values,
                    frame_sampled: true,
                });
            }
            Err(error) => result.errors.push(ExpressionEvaluationError {
                composition_id: comp_id,
                layer_id,
                property,
                message: error.to_string(),
            }),
        }
    }
    result
}

struct Prepared {
    runtime: ScriptRuntime,
    input: JsObject,
    dimensions: Vec<usize>,
    shim: String,
    approximated: BTreeSet<String>,
}
fn prepare(model: &Model) -> Result<Prepared, EvaluationError> {
    let mut runtime = ScriptRuntime::new()?;
    // Admitted loops are bounded per evaluation; a runaway loop is an error.
    runtime
        .context_mut()
        .runtime_limits_mut()
        .set_loop_iteration_limit(LOOP_ITERATION_LIMIT);
    let serialized = serde_json::to_value(model)?;
    let value =
        JsValue::from_json(&serialized, runtime.context_mut()).map_err(ScriptError::from)?;
    let input = JsObject::with_null_proto();
    // ScriptRuntime protects FX's structured time input. AE's scalar clock lives
    // inside the shim, never by coercing this object or changing shared playback.
    input
        .set(
            js_string!("time"),
            JsObject::with_null_proto(),
            true,
            runtime.context_mut(),
        )
        .map_err(ScriptError::from)?;
    input
        .set(js_string!("model"), value, true, runtime.context_mut())
        .map_err(ScriptError::from)?;
    Ok(Prepared {
        runtime,
        input,
        dimensions: model.properties.iter().map(|p| p.initial.len()).collect(),
        shim: format!("{RANDOM}\n{SHIM}"),
        approximated: BTreeSet::new(),
    })
}
fn evaluate_grid(
    prepared: &mut Prepared,
    slot: usize,
    times: &[f64],
) -> Result<Vec<Vec<f64>>, EvaluationError> {
    prepared.approximated.clear();
    // Property-slot indices come from a resident Vec and are exactly representable.
    if slot as u128 > (1_u128 << 53) {
        return Err(EvaluationError::Unsupported(
            "property index exceeds JS integer precision".into(),
        ));
    }
    prepared
        .input
        .set(
            js_string!("slot"),
            slot as f64,
            true,
            prepared.runtime.context_mut(),
        )
        .map_err(ScriptError::from)?;
    times
        .iter()
        .map(|seconds| {
            prepared
                .input
                .set(
                    js_string!("seconds"),
                    *seconds,
                    true,
                    prepared.runtime.context_mut(),
                )
                .map_err(ScriptError::from)?;
            let value = prepared
                .runtime
                .call(&prepared.shim, prepared.input.clone().into())?;
            let json = value
                .to_json(prepared.runtime.context_mut())
                .map_err(ScriptError::from)?
                .ok_or_else(|| {
                    EvaluationError::Unsupported("expression returned no numeric value".into())
                })?;
            let apis = json
                .get("approximated")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| {
                    EvaluationError::Unsupported("missing expression approximation report".into())
                })?;
            for api in apis {
                let api = api.as_str().ok_or_else(|| {
                    EvaluationError::Unsupported("invalid expression approximation report".into())
                })?;
                prepared.approximated.insert(api.to_owned());
            }
            let json = json.get("value").ok_or_else(|| {
                EvaluationError::Unsupported("missing expression numeric result".into())
            })?;
            let values = if let Some(number) = json.as_f64() {
                vec![number]
            } else if let Some(array) = json.as_array() {
                array
                    .iter()
                    .map(|v| {
                        v.as_f64().ok_or_else(|| {
                            EvaluationError::Unsupported("nonnumeric expression component".into())
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                return Err(EvaluationError::Unsupported(
                    "expression returned a nonnumeric value".into(),
                ));
            };
            if values.is_empty()
                || values.len() > 4
                || values.len() != prepared.dimensions[slot]
                || values.iter().any(|v| !v.is_finite())
            {
                return Err(EvaluationError::Unsupported(
                    "invalid expression dimensions/value".into(),
                ));
            }
            Ok(values)
        })
        .collect()
}
/// Source Text samples: the shim returns the expression's string result.
fn evaluate_text_grid(
    prepared: &mut Prepared,
    slot: usize,
    times: &[f64],
) -> Result<Vec<String>, EvaluationError> {
    prepared.approximated.clear();
    prepared
        .input
        .set(
            js_string!("slot"),
            slot as f64,
            true,
            prepared.runtime.context_mut(),
        )
        .map_err(ScriptError::from)?;
    times
        .iter()
        .map(|seconds| {
            prepared
                .input
                .set(
                    js_string!("seconds"),
                    *seconds,
                    true,
                    prepared.runtime.context_mut(),
                )
                .map_err(ScriptError::from)?;
            let value = prepared
                .runtime
                .call(&prepared.shim, prepared.input.clone().into())?;
            let json = value
                .to_json(prepared.runtime.context_mut())
                .map_err(ScriptError::from)?
                .ok_or_else(|| {
                    EvaluationError::Unsupported("Source Text returned no value".into())
                })?;
            for api in json
                .get("approximated")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
            {
                prepared.approximated.insert(api.to_owned());
            }
            json.get("value")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    EvaluationError::Unsupported("Source Text result is not text".into())
                })
        })
        .collect()
}

fn frame_grid(duration: f64, fps: f64) -> Result<Vec<f64>, String> {
    if !duration.is_finite() || duration < 0.0 || !fps.is_finite() || fps <= 0.0 {
        return Err("invalid composition expression clock".into());
    }
    let frames = (duration * fps).ceil();
    if !frames.is_finite() || frames >= usize::MAX as f64 {
        return Err("composition sample count exceeds platform integer range".into());
    }
    if frames >= 250_000.0 {
        return Err("composition expression sample budget exceeds 250000 vectors".into());
    }
    // The checked frame count is nonnegative and below usize::MAX; include endpoint.
    let mut times: Vec<_> = (0..frames as usize)
        .map(|frame| frame as f64 / fps)
        .collect();
    times.push(duration);
    Ok(times)
}
