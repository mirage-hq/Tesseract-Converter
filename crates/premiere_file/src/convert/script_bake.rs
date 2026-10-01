//! Export-owned baking of FX `JsScript` animation into editable native keys.
//!
//! [`bake_scripts`] runs once per export, before media inspection, so that
//! inspection and the writer read the same baked document. A script is baked
//! only when export writes native keys for its target on its owner
//! ([`owners`]); every other script keeps its animator and is reported with
//! its reason, without being evaluated. The writer records the keys it
//! places ([`WrittenAnimation`]), and [`BakedDocument::report_discarded`]
//! reports each baked track that it did not write.
//!
//! Evaluation and fitting reuse the shared `fx_keyframe_bake` runtime,
//! fitter and key identities ([`sample`], [`fit`]); no Adobe runtime is
//! used. The source document, its authored keys and a document without
//! scripts are never changed or copied.

mod fit;
mod owners;
mod sample;
#[cfg(test)]
mod tests;

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
    time::Duration,
};

use fx_keyframe_bake::{
    curve_fit::FittedEasing,
    identity::{conversion_identity_seed, converted_keyframe_id},
};
use fx_schema::{
    animator::{
        AnimationGraphEntry, AnimatorData, PropertyKeyframe, PropertyKeyframeError,
        PropertyKeyframeTrack,
    },
    EditableFxCompositionDocument, KeyframeId, PropertyAnimator, PropertyKeyframeEasing,
    PropertyTarget, PropertyValue, TimeOffset,
};
use serde_json::json;

use self::{
    fit::{Axis, FitError, Key},
    owners::{Binding, Owners, Pair, Rules},
    sample::{Deadline, SampleError},
};
use super::{effects::invert_output_black, tesseract_to_premiere::WrittenAnimation};
#[cfg(test)]
use crate::Omission;
use crate::{
    error::{unsupported, BuildError, Result},
    export_loss::OmissionSink,
    omit, OmissionScope,
};

/// Bounds on one export's script work, all fixed before any script runs.
#[derive(Debug, Clone, Copy)]
struct Limits {
    /// JavaScript calls, counted before any call: each baked track's
    /// millisecond samples and order probes ([`sample::evaluations`]).
    evaluations: u64,
    /// Generated keys, which bounds their memory.
    keys: usize,
    /// Checked between script calls. Boa has no interrupt: a single call is
    /// bounded only by its loop, recursion and VM stack limits, not by time,
    /// heap or native stack ([`sample::stack_bytes`]).
    elapsed: Duration,
}

/// Measured bounds. A track makes one call per millisecond of its owner's
/// window and up to 64 probe calls, so the call bound depends on duration:
/// N scripts on W-millisecond windows fit when N × (W + 65) ≤ 2^25, such as
/// 15,000 scripts on windows of up to 2.1 s, or 1,000 on windows of up to
/// 33 s. The 15,200-script export of the scale test makes 16.2 million
/// calls and generates 344,563 keys; release builds of the earlier
/// implementation spent about a minute of CPU time on it, at a 2.5 to 3.0 GB
/// peak footprint, most of it the baked document. The key bound is three
/// times those keys, and the elapsed-time bound only stops scripts far
/// slower than those.
const LIMITS: Limits = Limits {
    evaluations: 1 << 25,
    keys: 1 << 20,
    elapsed: Duration::from_secs(20 * 60),
};
/// Threads that evaluate independent tracks, each with two realms and one
/// track's samples at a time.
const MAX_WORKERS: usize = 8;
/// Targets that one grouped diagnostic names.
const LISTED_TARGETS: usize = 8;

/// The document that export writes, whose baked tracks replace their scripts.
#[derive(Debug)]
pub(crate) struct BakedDocument<'a> {
    document: Cow<'a, EditableFxCompositionDocument>,
    baked: Vec<BakedTrack>,
}

/// One script whose keys the baked document carries.
#[derive(Debug)]
struct BakedTrack {
    target: PropertyTarget,
    /// The owner, as omissions name it.
    owner: String,
}

impl BakedDocument<'_> {
    pub(crate) fn document(&self) -> &EditableFxCompositionDocument {
        &self.document
    }

    /// Reports how many baked tracks the exported project writes, and then
    /// each one that it does not write, under its owner: the count comes
    /// first, so that the omission list's bound cannot drop it.
    pub(crate) fn report_discarded(
        &self,
        written: &WrittenAnimation,
        omissions: &mut dyn OmissionSink,
    ) {
        if self.baked.is_empty() {
            return;
        }
        let mut discarded: Vec<(&str, Vec<String>)> = Vec::new();
        for track in &self.baked {
            if written.contains(&track.target) {
                continue;
            }
            let label = track.target.to_string();
            match discarded
                .iter_mut()
                .find(|(owner, _)| *owner == track.owner)
            {
                Some((_, targets)) => targets.push(label),
                None => discarded.push((&track.owner, vec![label])),
            }
        }
        let count: usize = discarded.iter().map(|(_, targets)| targets.len()).sum();
        omit(
            omissions,
            OmissionScope::Feature,
            "composition",
            format!(
                "{} of {} baked JS animation tracks were written as native keys",
                self.baked.len() - count,
                self.baked.len()
            ),
        );
        for (owner, targets) in discarded {
            omit(
                omissions,
                OmissionScope::Feature,
                owner,
                format!(
                    "baked JS animation of {} was not written: export did not write this animation; see this layer's other diagnostics",
                    targets.join(", ")
                ),
            );
        }
    }
}

/// Why one script or track was not baked.
#[derive(Debug, thiserror::Error)]
enum BakeError {
    #[error(transparent)]
    Sample(#[from] SampleError),
    #[error(transparent)]
    Fit(#[from] FitError),
    #[error("the script returns {value} at layer time {time_ms} ms, outside the {range} that export writes for this target")]
    Range {
        time_ms: u64,
        value: f64,
        range: &'static str,
    },
    #[error(
        "Premiere Scale is uniform, but Scale Y leaves the Scale X curve at layer time {0} ms"
    )]
    NonUniform(u64),
    #[error(transparent)]
    Keyframe(#[from] PropertyKeyframeError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl BakeError {
    /// The reason that one grouped diagnostic shares.
    fn category(&self) -> &'static str {
        match self {
            Self::Sample(SampleError::Script { .. }) => "the script failed",
            Self::Sample(SampleError::NonScalar { .. }) => {
                "the script did not return a finite number"
            }
            Self::Sample(SampleError::OrderDependent { .. }) => {
                "the script depends on evaluation history"
            }
            Self::Sample(SampleError::Stopped) => "evaluation stopped",
            Self::Fit(_) | Self::Keyframe(_) | Self::Json(_) => "fitting failed",
            Self::Range { .. } => "the script leaves the value range that export writes",
            Self::NonUniform(_) => "Premiere Scale is uniform, but the Scale axes differ",
        }
    }
}

/// One script that export can write as keys.
#[derive(Debug, Clone, Copy)]
struct Script<'d> {
    entry: usize,
    target: &'d PropertyTarget,
    code: &'d str,
    seed: u64,
}

/// Scripts that become native keys together.
#[derive(Debug)]
enum Shape<'d> {
    /// One scalar track, or a Corner Pin coordinate whose partner is static.
    Single(Script<'d>),
    /// A Position or Corner Pin point whose two axes, X and Y, are both
    /// scripted: one point track needs shared key times and easing.
    Point([Script<'d>; 2]),
    /// A Position with one scripted axis: the other keeps its static `value`
    /// at the same keys.
    PointWithStatic {
        script: Script<'d>,
        partner: PropertyTarget,
        value: f64,
    },
    /// A uniform Scale, X and Y: both axes take the X axis's keys.
    Uniform([Script<'d>; 2]),
    /// The two outputs of a Levels in Invert's form, the only parameters
    /// that animate: when the output black follows the complement of the
    /// output white, it takes the exact complements at the same keys, which
    /// export writes as one Invert Blend With Original track.
    Complement {
        white: Script<'d>,
        black: Script<'d>,
    },
}

impl Shape<'_> {
    fn scripts(&self) -> impl Iterator<Item = &Script<'_>> {
        let (first, second) = match self {
            Self::Single(script) | Self::PointWithStatic { script, .. } => (script, None),
            Self::Point([first, second])
            | Self::Uniform([first, second])
            | Self::Complement {
                white: first,
                black: second,
            } => (first, Some(second)),
        };
        std::iter::once(first).chain(second)
    }
}

/// Tracks that are fitted together, on their owner's window.
#[derive(Debug)]
struct Unit<'d> {
    shape: Shape<'d>,
    rules: Rules,
    /// The native control that keys the unit's two tracks, if it has two.
    pair: Option<Pair>,
    window_ms: u64,
    owner: String,
}

impl Unit<'_> {
    fn evaluations(&self) -> u64 {
        let tracks = u64::try_from(self.shape.scripts().count()).expect("a unit has 1 or 2 tracks");
        tracks.saturating_mul(sample::evaluations(self.window_ms))
    }

    /// Reports each script of the unit as not baked for `error`, which the
    /// script of entry `cause` caused, or all of them when `cause` is `None`;
    /// its partner keeps its script for the pair.
    fn not_baked(&self, cause: Option<usize>, error: &BakeError, report: &mut Report) {
        for script in self.shape.scripts() {
            match (cause, self.pair) {
                (Some(cause), Some(pair)) if cause != script.entry => report.not_baked(
                    format!(
                        "{}, and the other axis's script is not baked",
                        pair.reason()
                    ),
                    script.target,
                    None,
                ),
                _ => report.not_baked(error.category(), script.target, Some(error.to_string())),
            }
        }
    }
}

/// Why a unit was not baked, and the entry of the script that caused it:
/// `None` when the unit's shared keys failed.
#[derive(Debug)]
struct UnitError {
    cause: Option<usize>,
    error: BakeError,
}

impl<E: Into<BakeError>> From<E> for UnitError {
    fn from(error: E) -> Self {
        Self {
            cause: None,
            error: error.into(),
        }
    }
}

/// The fitted keys of one track, with its values in FX units.
#[derive(Debug)]
struct TrackKeys<'d> {
    target: PropertyTarget,
    /// The scripted entry that the track replaces; `None` for a static
    /// Position axis that the point needs.
    entry: Option<usize>,
    /// The source whose identity the key ids share.
    identity: &'d str,
    keys: Vec<(u64, f64, FittedEasing)>,
}

/// Bake every script that export can write as keys into an export-owned
/// document, reporting each script that it does not bake.
///
/// # Errors
/// Fails the export when the scripts need more work than [`LIMITS`] allows,
/// or no evaluation thread starts: no animation is truncated and nothing is
/// published. One script's failure omits only that script.
#[cfg(test)]
pub(crate) fn bake_scripts<'a>(
    document: &'a EditableFxCompositionDocument,
    omissions: &mut dyn OmissionSink,
) -> Result<BakedDocument<'a>> {
    bake_scripts_with_progress(document, omissions, fx_conv::Progress::default())
}

pub(crate) fn bake_scripts_with_progress<'a>(
    document: &'a EditableFxCompositionDocument,
    omissions: &mut dyn OmissionSink,
    progress: fx_conv::Progress<'_>,
) -> Result<BakedDocument<'a>> {
    #[cfg(test)]
    PREPARATION_CALLS.with(|count| count.set(count.get() + 1));
    bake_with_progress(document, omissions, &LIMITS, progress)
}

#[cfg(test)]
thread_local! {
    // Per-test-thread instrumentation of the real entry point, not a mock baker.
    pub(crate) static PREPARATION_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn bake<'a>(
    document: &'a EditableFxCompositionDocument,
    omissions: &mut dyn OmissionSink,
    limits: &Limits,
) -> Result<BakedDocument<'a>> {
    bake_with_progress(document, omissions, limits, fx_conv::Progress::default())
}

fn bake_with_progress<'a>(
    document: &'a EditableFxCompositionDocument,
    omissions: &mut dyn OmissionSink,
    limits: &Limits,
    progress: fx_conv::Progress<'_>,
) -> Result<BakedDocument<'a>> {
    let composition = document.composition();
    let entries = composition.dynamics().entries();
    let scripts = entries
        .iter()
        .filter(|entry| entry.animator.is_js_script())
        .count();
    if scripts == 0 {
        return Ok(BakedDocument {
            document: Cow::Borrowed(document),
            baked: Vec::new(),
        });
    }
    let dimensions = document.dimensions();
    let owners = Owners::new(
        composition.layers(),
        composition.dynamics(),
        [dimensions.width, dimensions.height],
    );
    let mut report = Report::default();
    let units = plan(entries, &owners, &mut report);
    let evaluations = units
        .iter()
        .map(Unit::evaluations)
        .fold(0, u64::saturating_add);
    if evaluations > limits.evaluations {
        return Err(unsupported(format!(
            "script bake budget exceeded: {} scripts on native key targets need at least {evaluations} JavaScript evaluations, more than the {} that one export makes; nothing was published",
            units.iter().map(|unit| unit.shape.scripts().count()).sum::<usize>(),
            limits.evaluations
        )));
    }
    let deadline = Deadline::new(limits.elapsed);
    let phase = progress.phase("baking Premiere scripts", "script tracks", units.len());
    let results = evaluate(&units, &deadline, phase)?;
    progress.stage("assemble baked Premiere document");
    let mut used_ids: BTreeSet<String> = entries
        .iter()
        .filter_map(|entry| entry.animator.keyframe_track())
        .flat_map(PropertyKeyframeTrack::keyframes)
        .map(|key| key.id().as_str().to_owned())
        .collect();
    let mut tracks = Vec::new();
    let mut keys = 0;
    for (unit, result) in units.iter().zip(results) {
        let unit_tracks = match result {
            Ok(unit_tracks) => unit_tracks,
            Err(UnitError {
                error: BakeError::Sample(SampleError::Stopped),
                ..
            }) => {
                return Err(unsupported(format!(
                    "script bake budget exceeded: evaluation reached the elapsed-time bound of {} s; nothing was published",
                    deadline.limit().as_secs()
                )))
            }
            Err(UnitError { cause, error }) => {
                unit.not_baked(cause, &error, &mut report);
                continue;
            }
        };
        let converted = unit_tracks
            .into_iter()
            .map(|track| match keyframe_track(&track, &mut used_ids) {
                Ok(keyframes) => Ok((track.target, track.entry, keyframes)),
                Err(error) => Err(UnitError {
                    cause: track.entry,
                    error,
                }),
            })
            .collect::<std::result::Result<Vec<_>, UnitError>>();
        match converted {
            Ok(converted) => {
                keys += converted
                    .iter()
                    .map(|(_, _, track)| track.keyframes().len())
                    .sum::<usize>();
                if keys > limits.keys {
                    return Err(unsupported(format!(
                        "script bake budget exceeded: the scripts need more than the {} keys that one export generates; nothing was published",
                        limits.keys
                    )));
                }
                tracks.extend(
                    converted
                        .into_iter()
                        .map(|(target, entry, track)| (target, entry, track, unit.owner.clone())),
                );
            }
            Err(UnitError { cause, error }) => unit.not_baked(cause, &error, &mut report),
        }
    }
    let baked_scripts = tracks
        .iter()
        .filter(|(_, entry, _, _)| entry.is_some())
        .count();
    report.emit(omissions);
    omit(
        omissions,
        OmissionScope::Feature,
        "composition",
        format!(
            "JS animation baking: {baked_scripts} of {scripts} scripts became editable keys ({keys} keys, from at most {evaluations} JavaScript evaluations); each fit stays within its target's tolerance at every integer millisecond of its owner's window, which is not Adobe render-fidelity proof"
        ),
    );
    if tracks.is_empty() {
        return Ok(BakedDocument {
            document: Cow::Borrowed(document),
            baked: Vec::new(),
        });
    }
    // Replace only the baked animators and add the static Position axes, so
    // unknown fields and every other record keep their wire form.
    let mut wire = document
        .to_json_value()
        .map_err(|error| unsupported(error.to_string()))?;
    let wire_entries = wire["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .ok_or_else(|| unsupported("a document with scripts has animation entries"))?;
    let mut baked = Vec::with_capacity(baked_scripts);
    for (target, entry, track, owner) in tracks {
        let animator = PropertyAnimator::keyframes(track).known_value();
        match entry {
            Some(entry) => {
                wire_entries[entry]["animator"] = animator;
                baked.push(BakedTrack { target, owner });
            }
            None => wire_entries.push(json!({"target": target, "animator": animator})),
        }
    }
    let document = EditableFxCompositionDocument::from_json_value(wire)
        .map_err(|error| unsupported(error.to_string()))?;
    Ok(BakedDocument {
        document: Cow::Owned(document),
        baked,
    })
}

/// The units of every script that export can write as keys, reporting the
/// others. The two tracks of a native control bake together, first axis
/// first; a control whose other track is authored, static or an unbaked
/// script bakes alone only where export writes it so ([`Pair`]).
fn plan<'d>(
    entries: &'d [AnimationGraphEntry],
    owners: &'d Owners<'d>,
    report: &mut Report,
) -> Vec<Unit<'d>> {
    let positions: HashMap<&PropertyTarget, usize> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| (&entry.target, index))
        .collect();
    // An Invert exports only when its two outputs are all that animates.
    let mut effect_entries = HashMap::new();
    for effect in entries.iter().filter_map(|entry| entry.target.effect_id()) {
        *effect_entries.entry(effect).or_insert(0_usize) += 1;
    }
    let mut candidates = BTreeMap::new();
    for (index, entry) in entries.iter().enumerate() {
        if !entry.animator.is_js_script() {
            continue;
        }
        match candidate(index, entry, owners) {
            Ok(candidate) => {
                candidates.insert(index, candidate);
            }
            Err(reason) => report.not_baked(reason, &entry.target, None),
        }
    }
    let mut consumed = BTreeSet::new();
    let mut units = Vec::new();
    for (&index, (script, bound)) in &candidates {
        if !consumed.insert(index) {
            continue;
        }
        let (shape, pair) = match &bound.binding {
            Binding::Scalar(_) => (Shape::Single(*script), None),
            Binding::Pair {
                pair,
                partner,
                first,
                ..
            } => {
                let partner_entry = positions.get(partner).map(|index| &entries[*index]);
                let scripted = positions
                    .get(partner)
                    .and_then(|index| candidates.get(index))
                    .map(|(script, _)| *script);
                let axes = |other: Script<'d>| {
                    if *first {
                        [*script, other]
                    } else {
                        [other, *script]
                    }
                };
                let invert_only =
                    partner.effect_id().and_then(|id| effect_entries.get(&id)) == Some(&2);
                let shape = match (pair, scripted, partner_entry) {
                    (Pair::Invert, Some(other), _) if invert_only => {
                        consumed.insert(other.entry);
                        let [white, black] = axes(other);
                        Shape::Complement { white, black }
                    }
                    (Pair::Invert, Some(_), _) => Shape::Single(*script),
                    (Pair::Scale, Some(other), _) => {
                        consumed.insert(other.entry);
                        Shape::Uniform(axes(other))
                    }
                    (Pair::Position { .. } | Pair::Corner, Some(other), _) => {
                        consumed.insert(other.entry);
                        Shape::Point(axes(other))
                    }
                    (Pair::Position { partner_static }, None, None) => Shape::PointWithStatic {
                        script: *script,
                        partner: partner.clone(),
                        value: *partner_static,
                    },
                    // Export keeps a static Corner Pin partner, and a Levels
                    // output beside an authored or static other output.
                    (Pair::Corner, None, None) => Shape::Single(*script),
                    (Pair::Invert, None, partner_entry)
                        if partner_entry.is_none_or(|entry| !entry.animator.is_js_script()) =>
                    {
                        Shape::Single(*script)
                    }
                    (pair, None, partner_entry) => {
                        let why = match partner_entry {
                            Some(entry) if entry.animator.is_js_script() => {
                                "the other axis's script is not baked"
                            }
                            Some(_) => "the other axis has authored animation",
                            None => "the other axis is static",
                        };
                        report.not_baked(
                            format!("{}, and {why}", pair.reason()),
                            script.target,
                            None,
                        );
                        continue;
                    }
                };
                (shape, Some(*pair))
            }
        };
        units.push(Unit {
            shape,
            rules: bound.binding.rules().clone(),
            pair,
            window_ms: bound.owner.window_ms(),
            owner: bound.owner.record(),
        });
    }
    units
}

/// A script entry that export can write as keys, or why it cannot.
fn candidate<'d>(
    index: usize,
    entry: &'d AnimationGraphEntry,
    owners: &'d Owners<'d>,
) -> std::result::Result<(Script<'d>, owners::Bound<'d>), String> {
    // A legacy `code` wrapper runs on a retired project or group clock until
    // the runtime migrates it; an empty one beside a layer-time body does not
    // (`fx_composition` wire rules).
    let AnimatorData::JsScript {
        code: legacy,
        layer_time_js_code: Some(code),
    } = entry.animator.data()
    else {
        return Err("legacy or mixed script clocks need the runtime's migration".to_owned());
    };
    if legacy.as_deref().is_some_and(|legacy| !legacy.is_empty()) {
        return Err("legacy or mixed script clocks need the runtime's migration".to_owned());
    }
    if entry
        .animator
        .wire_value()
        .as_object()
        .is_some_and(|fields| {
            fields
                .keys()
                .any(|key| !matches!(key.as_str(), "type" | "code" | "layerTimeJsCode"))
        })
    {
        return Err("unknown script fields may change what the script means".to_owned());
    }
    if !entry.dependencies.is_empty() {
        return Err(
            "script dependencies need graph evaluation, which no public crate provides".to_owned(),
        );
    }
    if !entry.layer_refs.is_empty() {
        return Err(
            "script layer references need live resource context, which export does not have"
                .to_owned(),
        );
    }
    let bound = owners.bind(&entry.target)?;
    let seed = sample::seed_prefix(entry.random_seed_target.as_ref().unwrap_or(&entry.target))
        .ok_or_else(|| {
            "randomSeedTarget names a target outside the native key bindings".to_owned()
        })?;
    Ok((
        Script {
            entry: index,
            target: &entry.target,
            code,
            seed,
        },
        bound,
    ))
}

/// Every unit's result, in unit order, from up to [`MAX_WORKERS`] threads
/// whose native stack fits the longest script's parse
/// ([`sample::stack_bytes`]). Each track runs in its own realms, so the
/// results do not depend on the schedule or on how many threads start. The
/// elapsed-time bound stops every thread at its next call.
///
/// # Errors
/// Fails when no evaluation thread starts.
fn evaluate<'d>(
    units: &[Unit<'d>],
    deadline: &Deadline,
    phase: fx_conv::ProgressPhase<'_>,
) -> Result<Vec<std::result::Result<Vec<TrackKeys<'d>>, UnitError>>> {
    if units.is_empty() {
        return Ok(Vec::new());
    }
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .clamp(1, MAX_WORKERS)
        .min(units.len());
    let longest = units
        .iter()
        .flat_map(|unit| unit.shape.scripts())
        .map(|script| script.code.len())
        .max()
        .unwrap_or(0);
    let stack = sample::stack_bytes(longest).ok_or_else(|| {
        unsupported(
            "script evaluation stack size exceeds host address space; nothing was published",
        )
    })?;
    let next = AtomicUsize::new(0);
    let completed = Mutex::new(0usize);
    let mut results = std::thread::scope(|scope| {
        let spawn = || {
            std::thread::Builder::new()
                .stack_size(stack)
                .spawn_scoped(scope, || {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(unit) = units.get(index) else {
                            break;
                        };
                        let result = bake_unit(unit, deadline);
                        let mut completed =
                            completed.lock().unwrap_or_else(|error| error.into_inner());
                        *completed += 1;
                        phase.update(*completed);
                        done.push((index, result));
                    }
                    done
                })
        };
        // Any one thread finishes every unit, so later spawn failures only
        // leave fewer threads.
        let first = spawn().map_err(|error| {
            unsupported(format!(
                "script evaluation could not start a thread with a {} MiB stack: {error}; nothing was published",
                stack >> 20
            ))
        })?;
        let handles: Vec<_> = std::iter::once(first)
            .chain((1..workers).map_while(|_| spawn().ok()))
            .collect();
        Ok::<_, BuildError>(
            handles
                .into_iter()
                .flat_map(|handle| {
                    handle
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                })
                .collect::<Vec<_>>(),
        )
    })?;
    results.sort_unstable_by_key(|(index, _)| *index);
    Ok(results.into_iter().map(|(_, result)| result).collect())
}

/// The keys of one unit's tracks.
fn bake_unit<'d>(
    unit: &Unit<'d>,
    deadline: &Deadline,
) -> std::result::Result<Vec<TrackKeys<'d>>, UnitError> {
    let sampled = |script: &Script<'d>| -> std::result::Result<Vec<f64>, UnitError> {
        let fails = |error: BakeError| UnitError {
            cause: Some(script.entry),
            error,
        };
        let values = sample::sample(script.code, script.seed, unit.window_ms, deadline)
            .map_err(|error| fails(error.into()))?;
        if let Some(range) = unit.rules.range {
            if let Some((time_ms, value)) = (0_u64..)
                .zip(values.iter().copied())
                .find(|(_, value)| !range.contains(*value))
            {
                return Err(fails(BakeError::Range {
                    time_ms,
                    value,
                    range: range.name(),
                }));
            }
        }
        Ok(values)
    };
    let cap = unit.rules.tolerance_cap;
    let track = |script: &Script<'d>, keys: &[Key], values: &[f64]| TrackKeys {
        target: script.target.clone(),
        entry: Some(script.entry),
        identity: script.code,
        keys: keyed(keys, values),
    };
    Ok(match &unit.shape {
        Shape::Single(script) => {
            let values = sampled(script)?;
            let keys = fit::fit(
                &[Axis {
                    values: &values,
                    cap,
                }],
                &unit.rules,
            )?;
            vec![track(script, &keys, &values)]
        }
        Shape::Point([x, y]) => {
            let (a, b) = (sampled(x)?, sampled(y)?);
            let keys = fit::fit(
                &[Axis { values: &a, cap }, Axis { values: &b, cap }],
                &unit.rules,
            )?;
            vec![track(x, &keys, &a), track(y, &keys, &b)]
        }
        Shape::PointWithStatic {
            script,
            partner,
            value,
        } => {
            let values = sampled(script)?;
            let keys = fit::fit(
                &[Axis {
                    values: &values,
                    cap,
                }],
                &unit.rules,
            )?;
            let static_axis = TrackKeys {
                target: partner.clone(),
                entry: None,
                identity: script.code,
                keys: keys
                    .iter()
                    .map(|key| (key.offset_ms, *value, key.easing))
                    .collect(),
            };
            vec![track(script, &keys, &values), static_axis]
        }
        Shape::Uniform([x, y]) => {
            let (a, b) = (sampled(x)?, sampled(y)?);
            let keys = fit::fit(&[Axis { values: &a, cap }], &unit.rules)?;
            if a != b {
                let tolerance = fit::tolerance(Axis { values: &b, cap })?;
                fit::follows(&keys, &a, &b, tolerance).map_err(|time_ms| UnitError {
                    cause: Some(y.entry),
                    error: BakeError::NonUniform(time_ms),
                })?;
            }
            // Both axes take Scale X's values, as a uniform Scale needs.
            vec![track(x, &keys, &a), track(y, &keys, &a)]
        }
        Shape::Complement { white, black } => {
            let (w, b) = (sampled(white)?, sampled(black)?);
            let keys = fit::fit(&[Axis { values: &w, cap }], &unit.rules)?;
            let complement: Vec<f64> = w.iter().copied().map(invert_output_black).collect();
            let tolerance = fit::tolerance(Axis { values: &b, cap })?;
            if fit::follows(&keys, &complement, &b, tolerance).is_ok() {
                vec![track(white, &keys, &w), track(black, &keys, &complement)]
            } else {
                // Not an Invert: two Levels outputs, each on its own keys.
                let black_keys = fit::fit(&[Axis { values: &b, cap }], &unit.rules)?;
                vec![track(white, &keys, &w), track(black, &black_keys, &b)]
            }
        }
    })
}

/// `keys` with their values from `values`.
fn keyed(keys: &[Key], values: &[f64]) -> Vec<(u64, f64, FittedEasing)> {
    keys.iter()
        .map(|key| {
            let value =
                values[usize::try_from(key.offset_ms).expect("key offsets index the samples")];
            (key.offset_ms, value, key.easing)
        })
        .collect()
}

/// The FX keyframe track of `track`, with deterministic ids that no authored
/// or earlier key uses.
fn keyframe_track(
    track: &TrackKeys<'_>,
    used_ids: &mut BTreeSet<String>,
) -> std::result::Result<PropertyKeyframeTrack, BakeError> {
    let identity = conversion_identity_seed(
        &serde_json::to_vec(&track.target)?,
        track.identity.as_bytes(),
    );
    let mut keys = Vec::with_capacity(track.keys.len());
    for &(offset_ms, value, easing) in &track.keys {
        // The evaluation budget keeps windows far inside i64 milliseconds.
        let time_ms = i64::try_from(offset_ms).expect("owner windows fit i64 milliseconds");
        let key = PropertyKeyframe::new(
            KeyframeId::new(converted_keyframe_id(identity, time_ms, used_ids)),
            TimeOffset::from_millis(time_ms),
            PropertyValue::Float(value),
            match easing {
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
    let keyframes = PropertyKeyframeTrack::new(keys)?;
    keyframes.validate_for_target(&track.target)?;
    Ok(keyframes)
}

/// Scripts that were not baked, grouped by reason in first-report order,
/// with each reason's count and first [`LISTED_TARGETS`] targets.
#[derive(Debug, Default)]
struct Report {
    reasons: Vec<(String, usize, Vec<String>)>,
    index: HashMap<String, usize>,
}

impl Report {
    fn not_baked(
        &mut self,
        reason: impl Into<String>,
        target: &PropertyTarget,
        detail: Option<String>,
    ) {
        let reason = reason.into();
        // A thrown error's stack trace follows its first line.
        let label = match detail.as_deref().and_then(|detail| detail.lines().next()) {
            Some(detail) => format!("{target} ({detail})"),
            None => target.to_string(),
        };
        let slot = *self.index.entry(reason.clone()).or_insert_with(|| {
            self.reasons.push((reason, 0, Vec::new()));
            self.reasons.len() - 1
        });
        let (_, count, targets) = &mut self.reasons[slot];
        *count += 1;
        if targets.len() < LISTED_TARGETS {
            targets.push(label);
        }
    }

    fn emit(self, omissions: &mut dyn OmissionSink) {
        for (reason, count, targets) in self.reasons {
            let more = count - targets.len();
            let tail = if more == 0 {
                String::new()
            } else {
                format!(", and {more} more")
            };
            omit(
                omissions,
                OmissionScope::Feature,
                "composition",
                format!(
                    "{count} JS animation script(s) kept their animator, which export does not convert: {reason}: {}{tail}",
                    targets.join("; ")
                ),
            );
        }
    }
}
