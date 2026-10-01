//! Non-mutating scalar/Path script preparation for the archive export route.
//!
//! Graph/clock support is deliberately explicit. Unsupported scripts stay in the
//! copy for the lowerer's contextual omission policy; a resource-budget failure
//! aborts the entire preparation before media staging or publication.

mod path;
mod seed;
#[cfg(test)]
mod tests;

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
};

use boa_engine::JsValue;
use fx_conv::Progress;
use fx_keyframe_bake::{
    curve_fit::{FittedCurve, FittedEasing, fit_scalar_curve},
    identity::{conversion_identity_seed, converted_keyframe_id, random_seed},
    script::{ScriptError, ScriptRuntime, install_reference_tables},
};
use fx_schema::{
    EditableFxCompositionDocument, EffectData, FxItemId, Layer, LayerData, LayerId, PropType,
    PropertyAnimator, PropertyKeyframeEasing, PropertyTarget, PropertyValue,
    animator::{
        AnimationGraphEntry, AnimatorData, PropertyKeyframe, PropertyKeyframeError,
        PropertyKeyframeTrack,
    },
    time::TimeOffset,
};
use serde_json::json;

use crate::{export_document::ExportDiagnostic, writer::AepWriteError};

// JavaScript Number must distinguish adjacent millisecond timestamps. This is
// a representation bound, not a duration/work quota. Per-call VM loop/stack
// protection remains in ScriptRuntime; project totals do not limit preparation.
const MAX_EXACT_SCRIPT_MILLIS: u64 = (1 << 53) - 1;

/// Provision the existing empirical native-parser stack margin without a source
/// size quota. This is not isolation: Boa's recursive parser and dynamic runtime
/// code can still exhaust a native stack. Overflow or thread allocation failure
/// aborts preparation, rather than dropping the affected animation.
fn stack_bytes(longest_source: usize) -> Result<usize, AepWriteError> {
    longest_source
        .checked_mul(32 << 10)
        .map(|bytes| bytes.max(16 << 20))
        .ok_or_else(|| {
            AepWriteError::InvalidDocument(
                "script evaluation stack size exceeds host address space".into(),
            )
        })
}

pub(super) struct Prepared<'a> {
    pub(super) document: Cow<'a, EditableFxCompositionDocument>,
    pub(super) diagnostics: Vec<ExportDiagnostic>,
}

#[derive(Debug, thiserror::Error)]
enum BakeError {
    #[error("script bake budget exceeded: {0}")]
    Budget(&'static str),
    #[error("{0}")]
    Unsupported(&'static str),
    #[error(transparent)]
    Script(#[from] ScriptError),
    #[error("script must return a finite scalar number at layer time {0}ms")]
    NonScalar(u64),
    #[error(
        "script is history-dependent or failed fresh dense playback validation at layer time {0}ms"
    )]
    Validation(u64),
    #[error(transparent)]
    Keyframe(#[from] PropertyKeyframeError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Default)]
struct Budget {
    calls: usize,
    probes: usize,
    keys: usize,
}

impl Budget {
    fn sample(&mut self) -> Result<(), BakeError> {
        self.calls = self
            .calls
            .checked_add(1)
            .ok_or(BakeError::Budget("evaluation counter overflow"))?;
        Ok(())
    }

    fn probe(&mut self) -> Result<(), BakeError> {
        self.probes = self
            .probes
            .checked_add(1)
            .ok_or(BakeError::Budget("probe counter overflow"))?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Owner {
    id: LayerId,
    duration_ms: u64,
    unsupported_clock: bool,
}

#[cfg(test)]
pub(super) fn prepare(
    document: &EditableFxCompositionDocument,
) -> Result<Prepared<'_>, AepWriteError> {
    prepare_with_progress(document, Progress::default())
}

pub(super) fn prepare_with_progress<'a>(
    document: &'a EditableFxCompositionDocument,
    progress: Progress<'_>,
) -> Result<Prepared<'a>, AepWriteError> {
    prepare_layers_with_progress(document, document.composition().layers(), false, progress)
}

#[cfg(test)]
pub(super) fn prepare_layers<'a>(
    document: &'a EditableFxCompositionDocument,
    roots: &[fx_schema::Layer],
    selected: bool,
) -> Result<Prepared<'a>, AepWriteError> {
    prepare_layers_with_progress(document, roots, selected, Progress::default())
}

pub(super) fn prepare_layers_with_progress<'a>(
    document: &'a EditableFxCompositionDocument,
    roots: &[fx_schema::Layer],
    selected: bool,
    progress: Progress<'_>,
) -> Result<Prepared<'a>, AepWriteError> {
    let entries = document.composition().dynamics().entries();
    if !entries.iter().any(|entry| entry.animator.is_js_script()) {
        return Ok(Prepared {
            document: Cow::Borrowed(document),
            diagnostics: Vec::new(),
        });
    }
    let mut owners = BTreeMap::new();
    let mut effects = BTreeMap::new();
    let mut items = BTreeMap::new();
    collect_owners(roots, false, &mut owners, &mut effects, &mut items);
    let owner_of = |entry: &AnimationGraphEntry| match &entry.target {
        PropertyTarget::LayerProperty(property) => owners.get(&property.layer_id()).copied(),
        PropertyTarget::EffectProperty(property) => effects.get(&property.effect_id()).copied(),
        PropertyTarget::FxItemProperty(property) => items.get(&property.item_id()).copied(),
    };
    let scripts: Vec<_> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            entry.animator.is_js_script() && (!selected || owner_of(entry).is_some())
        })
        .collect();
    let count = scripts.len();
    if scripts.is_empty() {
        return Ok(Prepared {
            document: Cow::Borrowed(document),
            diagnostics: Vec::new(),
        });
    }
    let script_progress = progress.phase("bake AE scripts", "tracks", count);
    let mut used_ids = entries
        .iter()
        .filter_map(|entry| entry.animator.keyframe_track())
        .flat_map(|track| track.keyframes())
        .map(|key| key.id().as_str().to_owned())
        .collect();
    let mut budget = Budget::default();
    let mut raw = document
        .to_json_value()
        .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))?;
    let mut diagnostics = Vec::new();
    let mut baked = 0;
    // Boa parses on its caller's native stack, so every script runs on one
    // thread sized only from the selected entries it will process. An unrelated
    // unselected script must not increase this scope's stack reservation.
    let longest = scripts
        .iter()
        .filter_map(|(_, entry)| match entry.animator.data() {
            AnimatorData::JsScript {
                layer_time_js_code: Some(code),
                ..
            } => Some(code.len()),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let stack = stack_bytes(longest)?;
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(stack)
            .spawn_scoped(scope, || {
                for (processed, (index, entry)) in scripts.into_iter().enumerate() {
                    let owner = owner_of(entry);
                    match bake_entry(entry, owner, &mut used_ids, &mut budget) {
                        Ok(animator) => {
                            raw["composition"]["dynamics"]["entries"][index]["animator"] = animator.known_value();
                            baked += 1;
                        }
                        Err(error @ BakeError::Budget(_)) => {
                            return Err(AepWriteError::InvalidDocument(format!(
                                "{}: {error}; completed {baked}/{count} tracks, {} keys, {} evaluations, {} fitter probes",
                                entry.target, budget.keys, budget.calls, budget.probes,
                            )));
                        }
                        Err(error) => diagnostics.push(ExportDiagnostic {
                            layer_id: owner.map(|owner| owner.id),
                            message: format!("JS animator {} was not baked: {error}; original animator retained for diagnosed best-effort lowering.", entry.target),
                        }),
                    }
                    script_progress.update(processed + 1);
                }
                Ok(())
            })
            .map_err(|error| {
                AepWriteError::InvalidDocument(format!(
                    "script evaluation could not start a thread with a {} MiB stack: {error}",
                    stack >> 20
                ))
            })?
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })?;
    progress.stage("validate baked AE document");
    if baked == 0 {
        return Ok(Prepared {
            document: Cow::Borrowed(document),
            diagnostics,
        });
    }
    let prepared = EditableFxCompositionDocument::from_json_value(raw)
        .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))?;
    diagnostics.push(ExportDiagnostic {
        layer_id: None,
        message: format!("JS animator baking approximated {baked}/{count} scalar/Path tracks with {} editable keys; millisecond fit validation is not Adobe render-fidelity proof.", budget.keys),
    });
    Ok(Prepared {
        document: Cow::Owned(prepared),
        diagnostics,
    })
}

/// This adapter evaluates JS and stores keys at the owner's local 0..duration.
fn unsupported_playback_clock(layer: &Layer) -> bool {
    let media_clock = |playback: &fx_schema::LayerPlayback| {
        matches!(playback.mapping(), fx_schema::LayerPlaybackMapping::Linear { input, .. }
            if i128::from(playback.input_range().start.as_millis())
                + i128::from(playback.input_offset_ms())
                == i128::from(input.start.as_millis()))
    };
    match layer.data() {
        LayerData::Video(video) => !media_clock(&video.playback),
        LayerData::Audio(audio) => !media_clock(&audio.playback),
        LayerData::Group(group) => match group.playback.mapping() {
            fx_schema::LayerPlaybackMapping::Linear { input, output } => {
                input.duration != output.duration
                    || i128::from(output.start.as_millis())
                        + i128::from(group.playback.input_range().start.as_millis())
                        + i128::from(group.playback.input_offset_ms())
                        - i128::from(input.start.as_millis())
                        != 0
            }
            fx_schema::LayerPlaybackMapping::TimeRemap { .. } => true,
        },
        _ => layer
            .wire_value()
            .get("playback")
            .is_some_and(|value| !value.is_null()),
    }
}

fn collect_owners(
    layers: &[Layer],
    inherited_unsupported_clock: bool,
    owners: &mut BTreeMap<LayerId, Owner>,
    effects: &mut BTreeMap<fx_schema::EffectId, Owner>,
    items: &mut BTreeMap<FxItemId, Owner>,
) {
    for layer in layers {
        // The supported clock is the layer's OWN active-start-relative clock,
        // not its nearest group or source-media clock. The same clock drives
        // both layer-time JS and keyframes. Legacy/nonlinear remaps need their
        // runtime migration and domain rules, and are not guessed here.
        let unsupported_clock = inherited_unsupported_clock || unsupported_playback_clock(layer);
        let owner = Owner {
            id: layer.id(),
            duration_ms: layer.active_range().duration.as_millis(),
            unsupported_clock,
        };
        owners.insert(layer.id(), owner);
        for effect in layer.effects() {
            if let EffectData::Identified { id, .. } = effect.data() {
                effects.insert(*id, owner);
            }
        }
        let masks: &[_] = match layer.data() {
            LayerData::Media(v) => &v.masks,
            LayerData::Video(v) => &v.masks,
            LayerData::Image(v) => &v.masks,
            LayerData::Text(v) => &v.masks,
            LayerData::Rect(v) => &v.masks,
            LayerData::Shape(v) => &v.masks,
            LayerData::Group(v) => &v.masks,
            LayerData::BooleanOperation(v) => &v.masks,
            LayerData::Adjustment(v) => &v.masks,
            _ => &[],
        };
        for mask in masks {
            items.insert(mask.id, owner);
        }
        if let LayerData::Text(text) = layer.data() {
            for animator in &text.animators {
                items.insert(animator.id, owner);
                for selector in &animator.selectors {
                    items.insert(selector.id, owner);
                }
                for selector in &animator.wiggly_selectors {
                    items.insert(selector.id, owner);
                }
            }
            if let Some(options) = &text.path_options {
                items.insert(options.id, owner);
            }
            if let Some(options) = &text.anchor_options {
                items.insert(options.id, owner);
            }
            if let Some(axes) = &text.source_text.font_variations {
                items.insert(axes.id(), owner);
            }
        }
        if let Some(children) = layer.child_layers() {
            collect_owners(children, unsupported_clock, owners, effects, items);
        }
    }
}

fn bake_entry(
    entry: &AnimationGraphEntry,
    owner: Option<Owner>,
    used_ids: &mut BTreeSet<String>,
    budget: &mut Budget,
) -> Result<PropertyAnimator, BakeError> {
    let AnimatorData::JsScript {
        code: None,
        layer_time_js_code: Some(code),
    } = entry.animator.data()
    else {
        return Err(BakeError::Unsupported(
            "legacy/mixed script clocks require runtime migration",
        ));
    };
    if !entry.dependencies.is_empty() || !entry.layer_refs.is_empty() {
        return Err(BakeError::Unsupported(
            "dependency or layer-reference evaluation is not available in this bake adapter",
        ));
    }
    if matches!(entry.target, PropertyTarget::FxItemProperty(_)) {
        return Err(BakeError::Unsupported(
            "FX-item scripts (including mask-item properties) are not supported by this adapter",
        ));
    }
    let owner = owner.ok_or(BakeError::Unsupported("target owner is not available"))?;
    if owner.unsupported_clock {
        return Err(BakeError::Unsupported(
            "owner or ancestor playback remapping is not supported by this bake adapter",
        ));
    }
    if owner.duration_ms > MAX_EXACT_SCRIPT_MILLIS {
        return Err(BakeError::Budget("exact JavaScript millisecond time"));
    }
    if matches!(&entry.target, PropertyTarget::LayerProperty(property)
        if property.property_type() == PropType::ShapePath)
    {
        return path::bake(entry, code, owner, used_ids, budget);
    }
    // Validate the scalar target before paying for evaluation. Public schema
    // checks use the same shape rules as the private runtime model.
    PropertyKeyframeTrack::new(vec![PropertyKeyframe::new(
        fx_schema::KeyframeId::new("bake-type-probe"),
        TimeOffset::from_millis(0),
        PropertyValue::Float(0.0),
        PropertyKeyframeEasing::Hold,
    )])?
    .validate_for_target(&entry.target)?;

    let seed = seed::prefix(entry.random_seed_target.as_ref().unwrap_or(&entry.target));
    let mut runtime = ScriptRuntime::new()?;
    let mut samples = BTreeMap::new();
    let fitted = fit_scalar_curve(owner.duration_ms, tolerance_cap(&entry.target), |time_ms| {
        cached_evaluate(&mut runtime, code, seed, time_ms, &mut samples, budget)
    })?;
    // Fresh ascending playback rejects obvious ambient randomness/global-state
    // dependence rather than silently turning sample order into authored motion.
    validate_fresh(code, seed, owner.duration_ms, &fitted, budget)?;
    let identity = conversion_identity_seed(&serde_json::to_vec(&entry.target)?, code.as_bytes());
    let mut keys = Vec::with_capacity(fitted.keys.len());
    for key in fitted.keys {
        // The exact JavaScript time check also fits the signed persisted domain.
        let time_ms = i64::try_from(key.offset_ms).map_err(|_| BakeError::Budget("key time"))?;
        let key = PropertyKeyframe::new(
            fx_schema::KeyframeId::new(converted_keyframe_id(identity, time_ms, used_ids)),
            TimeOffset::from_millis(time_ms),
            PropertyValue::Float(key.value),
            match key.easing {
                FittedEasing::Hold => PropertyKeyframeEasing::Hold,
                FittedEasing::Linear => PropertyKeyframeEasing::Linear,
                FittedEasing::Cubic { y1, y2 } => PropertyKeyframeEasing::CubicBezier {
                    x1: 1.0 / 3.0,
                    y1,
                    x2: 2.0 / 3.0,
                    y2,
                },
            },
        );
        keys.push(key);
    }
    let track = PropertyKeyframeTrack::new(keys)?;
    track.validate_for_target(&entry.target)?;
    budget.keys = budget
        .keys
        .checked_add(track.keyframes().len())
        .ok_or(BakeError::Budget("key counter overflow"))?;
    Ok(PropertyAnimator::keyframes(track))
}

fn cached_evaluate(
    runtime: &mut ScriptRuntime,
    code: &str,
    seed: u64,
    time_ms: u64,
    samples: &mut BTreeMap<u64, f64>,
    budget: &mut Budget,
) -> Result<f64, BakeError> {
    budget.probe()?;
    if let Some(value) = samples.get(&time_ms) {
        return Ok(*value);
    }
    let value = evaluate(runtime, code, seed, time_ms, budget)?;
    samples.insert(time_ms, value);
    Ok(value)
}

fn evaluate(
    runtime: &mut ScriptRuntime,
    code: &str,
    seed: u64,
    time_ms: u64,
    budget: &mut Budget,
) -> Result<f64, BakeError> {
    evaluate_value(runtime, code, seed, time_ms, budget)?
        .as_number()
        .filter(|value| value.is_finite())
        .ok_or(BakeError::NonScalar(time_ms))
}

fn evaluate_value(
    runtime: &mut ScriptRuntime,
    code: &str,
    seed: u64,
    time_ms: u64,
    budget: &mut Budget,
) -> Result<JsValue, BakeError> {
    budget.sample()?;
    let input = json!({
        "time": {"seconds": time_ms as f64 / 1000.0, "milliseconds": time_ms},
        "randomSeed": random_seed(seed, time_ms), "deps": [],
    });
    let input = JsValue::from_json(&input, runtime.context_mut()).map_err(ScriptError::from)?;
    let refs = JsValue::from_json(&json!({}), runtime.context_mut()).map_err(ScriptError::from)?;
    let metadata =
        JsValue::from_json(&json!({}), runtime.context_mut()).map_err(ScriptError::from)?;
    install_reference_tables(&input, refs, metadata, runtime.context_mut())?;
    Ok(runtime.call(code, input)?)
}

fn validate_fresh(
    code: &str,
    seed: u64,
    duration_ms: u64,
    fitted: &FittedCurve,
    budget: &mut Budget,
) -> Result<(), BakeError> {
    if !fitted.tolerance.is_finite() {
        return Err(BakeError::Unsupported(
            "curve range overflowed the fitting tolerance",
        ));
    }
    let mut runtime = ScriptRuntime::new()?;
    let mut left = 0;
    for time in 0..=duration_ms {
        while left + 1 < fitted.keys.len() && fitted.keys[left + 1].offset_ms <= time {
            left += 1;
        }
        let key = &fitted.keys[left];
        let expected = if let Some(next) = fitted.keys.get(left + 1) {
            let progress = (time - key.offset_ms) as f64 / (next.offset_ms - key.offset_ms) as f64;
            key.value + (next.value - key.value) * next.easing.progress(progress)
        } else {
            key.value
        };
        let actual = evaluate(&mut runtime, code, seed, time, budget)?;
        let error = (actual - expected).abs();
        if !expected.is_finite() || !error.is_finite() || error > fitted.tolerance {
            return Err(BakeError::Validation(time));
        }
    }
    Ok(())
}

fn tolerance_cap(target: &PropertyTarget) -> f64 {
    let PropertyTarget::LayerProperty(property) = target else {
        return f64::INFINITY;
    };
    match property.property_type() {
        PropType::PositionX
        | PropType::PositionY
        | PropType::PositionZ
        | PropType::AnchorPointX
        | PropType::AnchorPointY => 0.5,
        PropType::Opacity
        | PropType::ScaleX
        | PropType::ScaleY
        | PropType::Rotation
        | PropType::RotationX
        | PropType::RotationY
        | PropType::OrientationX
        | PropType::OrientationY
        | PropType::OrientationZ
        | PropType::Skew
        | PropType::SkewAxis => 0.1,
        _ => f64::INFINITY,
    }
}
