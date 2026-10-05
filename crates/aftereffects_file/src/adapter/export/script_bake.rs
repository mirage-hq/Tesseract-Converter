//! Non-mutating scalar/Path/Source Text script preparation for the archive export route.
//!
//! Graph/clock support is deliberately explicit. Unsupported scripts stay in the
//! copy for the lowerer's contextual omission policy; a resource-budget failure
//! aborts the entire preparation before media staging or publication.

mod clock;
mod dependencies;
mod document;
mod execution;
mod input;
mod keys;
mod path;
mod sampling;
mod seed;
mod singular_ease;
mod singular_opacity;

use document::BakedDocument;
use sampling::Sampling;
#[cfg(test)]
mod tests;

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
};

use crate::{AfterEffectsExportOptions, export_document::ExportDiagnostic, writer::AepWriteError};
use boa_engine::JsValue;
use fx_conv::Progress;
use fx_keyframe_bake::{
    curve_fit::{FittedCurve, FittedEasing, fit_sampled_scalar_curve},
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

// JavaScript Number must distinguish adjacent millisecond timestamps. This is
// a representation bound, not a duration/work quota. ScriptRuntime keeps only
// recursion/stack guards: it has no loop-iteration cap, deadline or
// cancellation, so a nonterminating script blocks preparation. Project totals
// do not limit preparation either.
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
    #[error("script preparation aborted")]
    Aborted,
    #[error("{0}")]
    Unsupported(&'static str),
    #[error(transparent)]
    Script(#[from] ScriptError),
    #[error("script must return a finite scalar number at layer time {0}ms")]
    NonScalar(u64),
    #[error("Source Text script must return a valid Unicode string at layer time {0}ms")]
    NonText(u64),
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
    keys: usize,
}

impl Budget {
    fn sample(&mut self) -> Result<(), BakeError> {
        if execution::aborted() {
            return Err(BakeError::Aborted);
        }
        self.calls = self
            .calls
            .checked_add(1)
            .ok_or(BakeError::Budget("evaluation counter overflow"))?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Owner {
    id: LayerId,
    duration_ms: u64,
    start_ms: u64,
    unsupported_clock: bool,
    clock_id: usize,
}

impl Owner {
    fn end_ms(self) -> u64 {
        self.start_ms + self.duration_ms
    }
    fn times(self, sampling: Sampling) -> impl Iterator<Item = u64> {
        sampling
            .offsets(self.duration_ms)
            .map(move |offset| self.start_ms + offset)
    }
}

#[cfg(test)]
pub(super) fn prepare(
    document: &EditableFxCompositionDocument,
) -> Result<Prepared<'_>, AepWriteError> {
    prepare_with_progress(
        document,
        &AfterEffectsExportOptions::default(),
        Progress::default(),
    )
}

pub(super) fn prepare_with_progress<'a>(
    document: &'a EditableFxCompositionDocument,
    options: &AfterEffectsExportOptions,
    progress: Progress<'_>,
) -> Result<Prepared<'a>, AepWriteError> {
    prepare_layers_with_progress(
        document,
        document.composition().layers(),
        false,
        options,
        progress,
    )
}

#[cfg(test)]
pub(super) fn prepare_layers<'a>(
    document: &'a EditableFxCompositionDocument,
    roots: &[fx_schema::Layer],
    selected: bool,
) -> Result<Prepared<'a>, AepWriteError> {
    prepare_layers_with_progress(
        document,
        roots,
        selected,
        &AfterEffectsExportOptions::default(),
        Progress::default(),
    )
}

pub(super) fn prepare_layers_with_progress<'a>(
    document: &'a EditableFxCompositionDocument,
    roots: &[fx_schema::Layer],
    selected: bool,
    options: &AfterEffectsExportOptions,
    progress: Progress<'_>,
) -> Result<Prepared<'a>, AepWriteError> {
    let opacity = singular_opacity::prepare(document, roots, selected)?;
    let scripts = prepare_scripts(
        opacity.document.as_ref(),
        roots,
        selected,
        options,
        progress,
    )?;
    let mut diagnostics = opacity.diagnostics;
    diagnostics.extend(scripts.diagnostics);
    let owned = match scripts.document {
        Cow::Owned(document) => Some(document),
        Cow::Borrowed(_) => None,
    };
    let scripts_document = owned.map(Cow::Owned).unwrap_or(opacity.document);
    // Script dependencies must see the authored cubic, not its native substitute.
    let ease = singular_ease::prepare(scripts_document.as_ref(), roots, selected)?;
    diagnostics.extend(ease.diagnostics);
    let document = match ease.document {
        Cow::Owned(document) => Cow::Owned(document),
        Cow::Borrowed(_) => scripts_document,
    };
    Ok(Prepared {
        document,
        diagnostics,
    })
}

fn prepare_scripts<'a>(
    document: &'a EditableFxCompositionDocument,
    roots: &[Layer],
    selected: bool,
    options: &AfterEffectsExportOptions,
    progress: Progress<'_>,
) -> Result<Prepared<'a>, AepWriteError> {
    prepare_scripts_with_workers(document, roots, selected, options, progress, 2)
}

fn prepare_scripts_with_workers<'a>(
    document: &'a EditableFxCompositionDocument,
    roots: &[Layer],
    selected: bool,
    options: &AfterEffectsExportOptions,
    progress: Progress<'_>,
    workers: usize,
) -> Result<Prepared<'a>, AepWriteError> {
    let sampling = Sampling::new(options)?;
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
    collect_owners(
        roots,
        &[],
        &mut clock::Clocks::default(),
        &mut owners,
        &mut effects,
        &mut items,
    );
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
    let script_progress = progress.phase("bake FX scripts for AEP", "tracks", count);
    let mut used_ids = entries
        .iter()
        .filter_map(|entry| entry.animator.keyframe_track())
        .flat_map(|track| track.keyframes())
        .map(|key| key.id().as_str().to_owned())
        .collect();
    let mut budget = Budget::default();
    let mut raw = None;
    let mut diagnostics = Vec::new();
    let mut baked = 0;
    let mut static_fallbacks = 0;
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
    let mut processed = 0;
    while processed < count {
        let end = (processed + workers.max(1)).min(count);
        // Do not start speculative successors past a known fatal time bound.
        let end = scripts[processed..end]
            .iter()
            .position(|(_, entry)| {
                owner_of(entry).is_some_and(|owner| {
                    owner
                        .start_ms
                        .checked_add(owner.duration_ms)
                        .is_none_or(|end| end > MAX_EXACT_SCRIPT_MILLIS)
                })
            })
            .map_or(end, |offset| processed + offset + 1);
        let batch = &scripts[processed..end];
        std::thread::scope(|scope| {
            let order = std::sync::Arc::new(execution::Order::default());
            // Drop before scope auto-joins on error/panic, releasing console waits.
            let _abort = order.abort_on_drop();
            let mut handles = Vec::with_capacity(batch.len());
            for (position, (_, entry)) in batch.iter().copied().enumerate() {
                let order = std::sync::Arc::clone(&order);
                let owner_of = &owner_of;
                handles.push(std::thread::Builder::new().stack_size(stack)
                    .spawn_scoped(scope, move || execution::with_order(order, position, || {
                        let mut builder = keys::Builder::default();
                        let mut local_budget = Budget::default();
                        let result = fit_entry(entry, owner_of(entry), entries, owner_of, &mut builder, &mut local_budget, sampling);
                        (result, builder, local_budget.calls)
                    }))
                    .map_err(|error| AepWriteError::InvalidDocument(format!(
                        "script evaluation could not start a thread with a {} MiB stack: {error}", stack >> 20
                    )))?);
            }
            // Every predecessor already has a dedicated running thread. Results,
            // IDs and console turns are published only in source order.
            for (position, ((index, entry), handle)) in
                batch.iter().copied().zip(handles).enumerate()
            {
                let (result, builder, calls) = handle
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
                let owner = owner_of(entry);
                let result = match budget.calls.checked_add(calls) {
                    Some(total) => {
                        budget.calls = total;
                        builder.finish(result, &entry.target, &mut used_ids, &mut budget)
                    }
                    None => {
                        budget.calls = usize::MAX;
                        Err(BakeError::Budget("evaluation counter overflow"))
                    }
                };
                match result {
                    Ok(animator) => {
                        if raw.is_none() {
                            raw = Some(BakedDocument::new(document).map_err(|error| {
                                AepWriteError::InvalidDocument(error.to_string())
                            })?);
                        }
                        raw.as_mut()
                            .expect("successful bake prepares the document")
                            .replace_script(index, &animator)
                            .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))?;
                        baked += 1;
                    }
                    Err(error @ BakeError::Budget(_)) => {
                        return Err(AepWriteError::InvalidDocument(format!(
                            "{}: {error}; completed {baked}/{count} tracks, {} keys, {} evaluations",
                            entry.target, budget.keys, budget.calls,
                        )));
                    }
                    Err(error) => {
                        if let Some((animator, root, value)) =
                            parse_invalid_static_transform(entry, owner, roots, &error)
                        {
                            if raw.is_none() {
                                raw = Some(BakedDocument::new(document).map_err(|error| {
                                    AepWriteError::InvalidDocument(error.to_string())
                                })?);
                            }
                            raw.as_mut()
                                .expect("static fallback prepares the document")
                                .replace_script(index, &animator)
                                .map_err(|error| {
                                    AepWriteError::InvalidDocument(error.to_string())
                                })?;
                            static_fallbacks += 1;
                            diagnostics.push(ExportDiagnostic {
                                    layer_id: owner.map(|owner| owner.id),
                                    message: format!("Root {root}, JS animator {} parse failure: {error}; export working copy uses authored static transform value {value}; scripted motion is lost. Source animator is unchanged; native hierarchy enclosure checks still apply.", entry.target),
                                });
                        } else {
                            diagnostics.push(ExportDiagnostic {
                                    layer_id: owner.map(|owner| owner.id),
                                    message: format!("JS animator {} was not baked: {error}; original animator retained for diagnosed best-effort lowering.", entry.target),
                                });
                        }
                    }
                }
                script_progress.update(processed + position + 1);
                order.advance();
            }
            Ok(())
        })?;
        processed = end;
    }
    progress.stage("validate baked AE document");
    if baked == 0 && static_fallbacks == 0 {
        return Ok(Prepared {
            document: Cow::Borrowed(document),
            diagnostics,
        });
    }
    let prepared = raw
        .expect("a successful bake prepares the document")
        .finish()
        .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))?;
    diagnostics.push(ExportDiagnostic {
        layer_id: None,
        message: format!("FX script baking samples at four times the native {fps}fps output rate, rounded to integer milliseconds, plus owner endpoints. Scalar keys use Linear/Hold interpolation. Evaluation and fresh-runtime validation skip unsampled times; subframe pulses, topology changes, state changes and motion-blur fidelity are not guaranteed.", fps = options.fps),
    });
    diagnostics.push(ExportDiagnostic {
        layer_id: None,
        message: format!("JS animator baking approximated {baked}/{count} scalar/Path/Source Text tracks with {} editable keys; sampled-grid fit validation is not Adobe render-fidelity proof. Source Text changes use editable Hold keys at the first observed grid sample; sub-grid change times can shift by up to one sampling interval.", budget.keys),
    });
    Ok(Prepared {
        document: Cow::Owned(prepared),
        diagnostics,
    })
}

/// This adapter evaluates JS and stores keys in the owner's actual local domain,
/// including an explicit media remap's absolute source-range start/end.
/// Only syntactically invalid, independent transform bodies can use authored
/// static components. Reparse without invoking user code to distinguish syntax
/// errors from the shared runtime's deliberately coarser ScriptError variants.
fn parse_invalid_static_transform(
    entry: &AnimationGraphEntry,
    owner: Option<Owner>,
    roots: &[Layer],
    error: &BakeError,
) -> Option<(PropertyAnimator, LayerId, f64)> {
    if !matches!(error, BakeError::Script(_))
        || !entry.dependencies.is_empty()
        || !entry.layer_refs.is_empty()
        || entry.random_seed_target.is_some()
    {
        return None;
    }
    let owner = owner?;
    if owner.unsupported_clock || owner.duration_ms > MAX_EXACT_SCRIPT_MILLIS {
        return None;
    }
    let PropertyTarget::LayerProperty(property) = &entry.target else {
        return None;
    };
    let (field, component) = match property.property_type() {
        PropType::PositionX => ("position", Some(0)),
        PropType::PositionY => ("position", Some(1)),
        PropType::PositionZ => ("position", Some(2)),
        PropType::AnchorPointX => ("anchorPoint", Some(0)),
        PropType::AnchorPointY => ("anchorPoint", Some(1)),
        PropType::ScaleX => ("scale", Some(0)),
        PropType::ScaleY => ("scale", Some(1)),
        PropType::OrientationX => ("orientation", Some(0)),
        PropType::OrientationY => ("orientation", Some(1)),
        PropType::OrientationZ => ("orientation", Some(2)),
        PropType::Rotation => ("rotation", None),
        PropType::RotationX => ("rotationX", None),
        PropType::RotationY => ("rotationY", None),
        PropType::Skew => ("skew", None),
        PropType::SkewAxis => ("skewAxis", None),
        PropType::Opacity => ("opacity", None),
        _ => return None,
    };
    fn find(layers: &[Layer], id: LayerId) -> Option<&Layer> {
        layers.iter().find_map(|layer| {
            if layer.id() == id {
                Some(layer)
            } else {
                find(layer.child_layers().unwrap_or(&[]), id)
            }
        })
    }
    let (root, layer) = roots.iter().find_map(|root| {
        find(std::slice::from_ref(root), owner.id).map(|layer| (root.id(), layer))
    })?;
    // A failed owner transform leaves its authored base active independently
    // of descendant content. Descendants still pass normal export safety checks;
    // this is not recovery of failed geometry, Text or effect scripts.
    if !plain_static_transform_owner(layer) {
        return None;
    }
    let authored = layer.wire_value().get("transform")?.get(field)?;
    let value = match component {
        Some(index) => authored.get(index)?.as_f64()?,
        None => authored.as_f64()?,
    };
    if !value.is_finite() {
        return None;
    }
    let AnimatorData::JsScript {
        code: None,
        layer_time_js_code: Some(code),
    } = entry.animator.data()
    else {
        return None;
    };
    let mut runtime = ScriptRuntime::new().ok()?;
    let source = format!("(function(input) {{\n\"use strict\";\n{code}\n}})");
    let parse_error = boa_engine::Script::parse(
        boa_engine::Source::from_bytes(source.as_str()),
        None,
        runtime.context_mut(),
    )
    .err()?;
    if !matches!(
        parse_error.as_native()?.kind,
        boa_engine::JsNativeErrorKind::Syntax
    ) {
        return None;
    }
    let animator = PropertyAnimator::constant(PropertyValue::Float(value)).ok()?;
    Some((animator, root, value))
}

// Bound lossy recovery to an ordinary Group owner. Its children's drawable
// profiles do not affect the runtime's persisted-base fallback semantics.
fn plain_static_transform_owner(layer: &Layer) -> bool {
    // Deterministic JS clock support does not widen lossy parse-error recovery.
    let plain_clock = match layer.data() {
        LayerData::Group(group) => match group.playback.mapping() {
            fx_schema::LayerPlaybackMapping::Linear { input, output } => {
                input.duration == output.duration
                    && i128::from(output.start.as_millis())
                        + i128::from(group.playback.input_range().start.as_millis())
                        + i128::from(group.playback.input_offset_ms())
                        - i128::from(input.start.as_millis())
                        == 0
            }
            _ => false,
        },
        _ => false,
    };
    if !plain_clock {
        return false;
    }
    match layer.data() {
        LayerData::Group(group) => {
            !group.layers.is_empty()
                && group.fills.is_empty()
                && group.effects.is_empty()
                && group.masks.is_empty()
                && group.track_matte.is_none()
                && !group.motion_blur
                && group.blend_mode == fx_schema::BlendMode::Normal
        }
        _ => false,
    }
}

fn collect_owners(
    layers: &[Layer],
    parent_clock: &[String],
    clocks: &mut clock::Clocks,
    owners: &mut BTreeMap<LayerId, Owner>,
    effects: &mut BTreeMap<fx_schema::EffectId, Owner>,
    items: &mut BTreeMap<FxItemId, Owner>,
) {
    for layer in layers {
        let (owner, child_clock) = clocks.owner(layer, parent_clock);
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
            collect_owners(children, &child_clock, clocks, owners, effects, items);
        }
    }
}

#[cfg(test)]
fn bake_entry(
    entry: &AnimationGraphEntry,
    owner: Option<Owner>,
    entries: &[AnimationGraphEntry],
    owner_of: &impl Fn(&AnimationGraphEntry) -> Option<Owner>,
    used_ids: &mut BTreeSet<String>,
    budget: &mut Budget,
    sampling: Sampling,
) -> Result<PropertyAnimator, BakeError> {
    let mut builder = keys::Builder::default();
    let result = fit_entry(
        entry,
        owner,
        entries,
        owner_of,
        &mut builder,
        budget,
        sampling,
    );
    builder.finish(result, &entry.target, used_ids, budget)
}

fn fit_entry(
    entry: &AnimationGraphEntry,
    owner: Option<Owner>,
    entries: &[AnimationGraphEntry],
    owner_of: &impl Fn(&AnimationGraphEntry) -> Option<Owner>,
    builder: &mut keys::Builder,
    budget: &mut Budget,
    sampling: Sampling,
) -> Result<Vec<keys::Key>, BakeError> {
    let AnimatorData::JsScript {
        code: None,
        layer_time_js_code: Some(code),
    } = entry.animator.data()
    else {
        return Err(BakeError::Unsupported(
            "legacy/mixed script clocks require runtime migration",
        ));
    };
    if !entry.layer_refs.is_empty() {
        return Err(BakeError::Unsupported(
            "layer-reference evaluation is not available in this bake adapter",
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
    if owner.end_ms() > MAX_EXACT_SCRIPT_MILLIS {
        return Err(BakeError::Budget("exact JavaScript millisecond time"));
    }
    if let PropertyTarget::LayerProperty(property) = &entry.target {
        if !entry.dependencies.is_empty()
            && matches!(
                property.property_type(),
                PropType::ShapePath | PropType::TextContent
            )
        {
            return Err(BakeError::Unsupported(
                "dependent Path/Text scripts are not supported by this scalar adapter",
            ));
        }
        match property.property_type() {
            PropType::ShapePath => {
                return path::bake(entry, code, owner, builder, budget, sampling);
            }
            PropType::TextContent => {
                return bake_text(entry, code, owner, builder, budget, sampling);
            }
            _ => {}
        }
    }
    validate_scalar_target(&entry.target)?;

    let seed = seed::prefix(entry.random_seed_target.as_ref().unwrap_or(&entry.target));
    let mut runtime = execution::runtime()?;
    let dependencies = dependencies::Program::new(entry, entries, owner, owner_of)?;
    // The fitter evaluates each offset once and owns its samples.
    let fitted = fit_sampled_scalar_curve(
        owner.times(sampling),
        tolerance_cap(&entry.target),
        |time_ms| dependencies.evaluate(&mut runtime, code, seed, time_ms, budget),
    )?;
    // Fresh ascending playback rejects obvious ambient randomness/global-state
    // dependence rather than silently turning sample order into authored motion.
    validate_fresh(code, seed, owner, &fitted, budget, sampling, &dependencies)?;
    builder.identify(entry, code)?;
    let mut keys = Vec::with_capacity(fitted.keys.len());
    for key in fitted.keys {
        // The exact JavaScript time check also fits the signed persisted domain.
        let time_ms = i64::try_from(key.offset_ms).map_err(|_| BakeError::Budget("key time"))?;
        let key = builder.record(
            time_ms,
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
    Ok(keys)
}

fn validate_scalar_target(target: &PropertyTarget) -> Result<(), BakeError> {
    // Match runtime-shaped conversion before supplying a Float to a consumer;
    // numeric JS on Text/Bool/vector/color/path targets is not a scalar value.
    PropertyKeyframeTrack::new(vec![PropertyKeyframe::new(
        fx_schema::KeyframeId::new("bake-type-probe"),
        TimeOffset::from_millis(0),
        PropertyValue::Float(0.0),
        PropertyKeyframeEasing::Hold,
    )])?
    .validate_for_target(target)?;
    Ok(())
}

fn bake_text(
    entry: &AnimationGraphEntry,
    code: &str,
    owner: Owner,
    builder: &mut keys::Builder,
    budget: &mut Budget,
    sampling: Sampling,
) -> Result<Vec<keys::Key>, BakeError> {
    let seed = seed::prefix(entry.random_seed_target.as_ref().unwrap_or(&entry.target));
    builder.identify(entry, code)?;
    let mut runtime = execution::runtime()?;
    let mut keys = Vec::new();
    for time_ms in owner.times(sampling) {
        let text = evaluate_text(&mut runtime, code, seed, time_ms, budget)?;
        if keys.last().is_some_and(|key: &keys::Key| {
            matches!(&key.value, PropertyValue::String(previous) if previous == &text)
        }) {
            continue;
        }
        let time = i64::try_from(time_ms).map_err(|_| BakeError::Budget("key time"))?;
        keys.push(builder.record(
            time,
            PropertyValue::String(text),
            PropertyKeyframeEasing::Hold,
        ));
    }
    // Out-of-order probes on a fresh VM reject counters and stateful scripts
    // that could otherwise match a second ascending sampling pass.
    let mut fresh = execution::runtime()?;
    for time_ms in [owner.end_ms(), owner.start_ms]
        .into_iter()
        .chain(owner.times(sampling))
    {
        let value = evaluate_text(&mut fresh, code, seed, time_ms, budget)?;
        let position = keys.partition_point(|key| key.time_ms <= time_ms as i64);
        if !matches!(&keys[position.saturating_sub(1)].value, PropertyValue::String(expected) if expected == &value)
        {
            return Err(BakeError::Validation(time_ms));
        }
    }
    Ok(keys)
}

fn evaluate_text(
    runtime: &mut ScriptRuntime,
    code: &str,
    seed: u64,
    time_ms: u64,
    budget: &mut Budget,
) -> Result<String, BakeError> {
    evaluate_value(runtime, code, seed, time_ms, budget)?
        .as_string()
        .and_then(|text| text.to_std_string().ok())
        .ok_or(BakeError::NonText(time_ms))
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
    let input = input::build(runtime.context_mut(), time_ms, random_seed(seed, time_ms));
    let refs = input::empty_object(runtime.context_mut());
    let metadata = input::empty_object(runtime.context_mut());
    install_reference_tables(&input, refs, metadata, runtime.context_mut())?;
    Ok(runtime.call(code, input)?)
}

fn validate_fresh(
    code: &str,
    seed: u64,
    owner: Owner,
    fitted: &FittedCurve,
    budget: &mut Budget,
    sampling: Sampling,
    dependencies: &dependencies::Program<'_>,
) -> Result<(), BakeError> {
    if !fitted.tolerance.is_finite() {
        return Err(BakeError::Unsupported(
            "curve range overflowed the fitting tolerance",
        ));
    }
    let mut runtime = execution::runtime()?;
    let mut left = 0;
    // Probe out of order so a counter cannot pass as a time-driven animation.
    for time in [owner.end_ms(), owner.start_ms]
        .into_iter()
        .chain(owner.times(sampling))
    {
        if fitted.keys[left].offset_ms > time {
            left = 0;
        }
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
        let actual = dependencies.evaluate(&mut runtime, code, seed, time, budget)?;
        let error = (actual - expected).abs();
        if !expected.is_finite() || !error.is_finite() || error > fitted.tolerance {
            return Err(BakeError::Validation(time));
        }
    }
    Ok(())
}

fn tolerance_cap(target: &PropertyTarget) -> f64 {
    // Corner Pin coordinates use the logical source plane. A giant invisible
    // point must not relax the fit of later visible points: doing so can create
    // a native output extent that the authored animation never requested.
    // Preserve every sampled coordinate exactly, including the giant points;
    // this changes neither their values nor the existing sampled-time domain.
    if let PropertyTarget::EffectProperty(property) = target
        && matches!(
            property.param_name(),
            "upperLeftX"
                | "upperLeftY"
                | "upperRightX"
                | "upperRightY"
                | "lowerLeftX"
                | "lowerLeftY"
                | "lowerRightX"
                | "lowerRightY"
        )
    {
        return 0.0;
    }
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
