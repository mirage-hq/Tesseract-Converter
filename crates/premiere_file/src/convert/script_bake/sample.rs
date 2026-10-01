//! Script evaluation at every owner-local millisecond.
//!
//! A script runs once per integer millisecond of its owner's window, in
//! ascending order in a fresh realm, as playback reaches it. A second fresh
//! realm then evaluates a bounded set of those times in descending order and
//! must reproduce each value exactly. This catches a script whose value
//! depends on evaluation history (a counter, a remembered previous time,
//! `Math.random`), but it cannot prove any script free of history or
//! nondeterminism: history can matter only at times the probe skips, and
//! scripts that share global state across tracks in one playback realm are
//! evaluated in isolation here.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use boa_engine::JsValue;
use fx_keyframe_bake::{
    identity::{hash_random_seed_part, random_seed},
    script::{install_reference_tables, ScriptError, ScriptRuntime},
};
use fx_schema::{PropType, PropertyTarget};
use serde_json::json;

/// Times of the reverse-order probe per track: the window's ends and evenly
/// spaced times between them.
pub(super) const ORDER_PROBES: u64 = 64;

/// The smallest native stack of an evaluation thread: a realm, the runtime's
/// fixed wrapper and a short script need about 1 MiB.
const MIN_STACK_BYTES: usize = 16 << 20;
/// Native stack per byte of the longest script source. Boa parses nested
/// expressions recursively, with no depth limit, and its VM limits do not
/// bound that recursion. Measured in release builds and in this workspace's
/// dev builds, which optimize the parser alike (the workspace profile of
/// `fx_keyframe_bake`, which compiles it): an unclosed parenthesis, the most
/// expensive of the constructs probed, takes 19.4 KiB per source byte, and
/// compiling and dropping a valid source take no more than parsing it. This
/// is an empirical provisioning margin, not a proof: code that a
/// script builds at run time (`eval`, `Function`, `RegExp`) and native
/// recursion over deep run-time values (`String`, `JSON.stringify`) are not
/// bounded by the source size, and can still overflow any finite stack.
const STACK_BYTES_PER_SOURCE_BYTE: usize = 32 << 10;

/// The native stack of an evaluation thread whose longest script source is
/// `longest_source` bytes, without a source-size quota. Host-address overflow
/// is an operational failure; the caller must not silently omit the script.
pub(super) fn stack_bytes(longest_source: usize) -> Option<usize> {
    longest_source
        .checked_mul(STACK_BYTES_PER_SOURCE_BYTE)
        .map(|bytes| bytes.max(MIN_STACK_BYTES))
}

/// Why a track's evaluation stopped.
#[derive(Debug, thiserror::Error)]
pub(super) enum SampleError {
    #[error("the script failed at layer time {time_ms} ms: {source}")]
    Script {
        time_ms: u64,
        #[source]
        source: ScriptError,
    },
    #[error("the script did not return a finite number at layer time {time_ms} ms")]
    NonScalar { time_ms: u64 },
    #[error(
        "the script depends on evaluation history: a fresh reverse-order evaluation returned {probe} instead of {dense} at layer time {time_ms} ms"
    )]
    OrderDependent {
        time_ms: u64,
        dense: f64,
        probe: f64,
    },
    /// The preparation's elapsed-time bound was reached, by this or another
    /// worker; no track evaluates further.
    #[error("evaluation reached the elapsed-time bound")]
    Stopped,
}

/// The cooperative end of one preparation's evaluation, shared by its
/// workers: checked between script calls, never inside one.
#[derive(Debug)]
pub(super) struct Deadline {
    started: Instant,
    limit: Duration,
    stopped: AtomicBool,
}

impl Deadline {
    pub(super) fn new(limit: Duration) -> Self {
        Self {
            started: Instant::now(),
            limit,
            stopped: AtomicBool::new(false),
        }
    }

    pub(super) fn limit(&self) -> Duration {
        self.limit
    }

    /// Fails once the bound is reached, and from then on in every worker.
    fn check(&self) -> Result<(), SampleError> {
        if self.stopped.load(Ordering::Relaxed) {
            return Err(SampleError::Stopped);
        }
        if self.started.elapsed() >= self.limit {
            self.stopped.store(true, Ordering::Relaxed);
            return Err(SampleError::Stopped);
        }
        Ok(())
    }
}

/// The JavaScript calls that evaluating one track over `window_ms` makes.
pub(super) fn evaluations(window_ms: u64) -> u64 {
    // Extreme input windows must exceed admission limits, not wrap below them.
    let samples = window_ms.saturating_add(1);
    samples.saturating_add(samples.min(ORDER_PROBES))
}

/// The script's value at every owner-local millisecond from 0 through
/// `window_ms`, evaluated in ascending order in a fresh realm, after an
/// independent reverse-order probe reproduced it.
pub(super) fn sample(
    code: &str,
    seed: u64,
    window_ms: u64,
    deadline: &Deadline,
) -> Result<Vec<f64>, SampleError> {
    let script = |error| SampleError::Script {
        time_ms: 0,
        source: error,
    };
    let mut runtime = ScriptRuntime::new().map_err(script)?;
    // The caller's evaluation budget bounds the window, so it fits memory.
    let mut values = Vec::with_capacity(usize::try_from(window_ms + 1).unwrap_or(0));
    for time_ms in 0..=window_ms {
        deadline.check()?;
        values.push(evaluate(&mut runtime, code, seed, time_ms)?);
    }
    let mut runtime = ScriptRuntime::new().map_err(script)?;
    for time_ms in probe_times(window_ms).rev() {
        deadline.check()?;
        let probe = evaluate(&mut runtime, code, seed, time_ms)?;
        let dense = values[usize::try_from(time_ms).expect("probe times index the samples")];
        if probe != dense {
            return Err(SampleError::OrderDependent {
                time_ms,
                dense,
                probe,
            });
        }
    }
    Ok(values)
}

/// Up to [`ORDER_PROBES`] distinct, ascending times from 0 through `window_ms`.
fn probe_times(window_ms: u64) -> impl DoubleEndedIterator<Item = u64> {
    let count = (window_ms + 1).min(ORDER_PROBES);
    (0..count).map(move |index| {
        if count == 1 {
            0
        } else {
            // Exact in u128: `window_ms` is bounded by the evaluation budget.
            u64::try_from(u128::from(index) * u128::from(window_ms) / u128::from(count - 1))
                .expect("a probe time lies inside the window")
        }
    })
}

/// One call with the runtime's layer-time input: the owner-local time, the
/// target's stable seed at that time, no dependencies and empty references.
fn evaluate(
    runtime: &mut ScriptRuntime,
    code: &str,
    seed: u64,
    time_ms: u64,
) -> Result<f64, SampleError> {
    let script = |source| SampleError::Script { time_ms, source };
    // Owner windows are far below 2^53 ms, so the time is exact in JavaScript.
    let input = json!({
        "time": {"seconds": time_ms as f64 / 1000.0, "milliseconds": time_ms},
        "randomSeed": random_seed(seed, time_ms),
        "deps": [],
    });
    let context = runtime.context_mut();
    let input = JsValue::from_json(&input, context).map_err(|error| script(error.into()))?;
    let refs = JsValue::from_json(&json!({}), context).map_err(|error| script(error.into()))?;
    let metadata = JsValue::from_json(&json!({}), context).map_err(|error| script(error.into()))?;
    install_reference_tables(&input, refs, metadata, context).map_err(script)?;
    runtime
        .call(code, input)
        .map_err(script)?
        .as_number()
        .filter(|value| value.is_finite())
        .ok_or(SampleError::NonScalar { time_ms })
}

/// The runtime's random-seed prefix of `target`, for the targets that export
/// binds. The layer-property numbers are the runtime's append-only seed ids
/// (`fx_composition::script`), not Rust discriminants; an effect parameter
/// hashes its tag, effect id and name bytes. `None` for any other target.
pub(super) fn seed_prefix(target: &PropertyTarget) -> Option<u64> {
    const EFFECT_PARAM_SEED_TAG: u64 = 31;
    let mut hash = 0xcbf2_9ce4_8422_2325;
    match target {
        PropertyTarget::LayerProperty(property) => {
            let id = match property.property_type() {
                PropType::PositionX => 0,
                PropType::PositionY => 1,
                PropType::Opacity => 3,
                PropType::Rotation => 4,
                PropType::ScaleX => 5,
                PropType::ScaleY => 6,
                PropType::AudioVolume => 30,
                _ => return None,
            };
            hash_random_seed_part(&mut hash, property.layer_id().value());
            hash_random_seed_part(&mut hash, id);
        }
        PropertyTarget::EffectProperty(property) => {
            hash_random_seed_part(&mut hash, EFFECT_PARAM_SEED_TAG);
            hash_random_seed_part(&mut hash, property.effect_id().value());
            for byte in property.param_name().bytes() {
                hash_random_seed_part(&mut hash, u64::from(byte));
            }
        }
        PropertyTarget::FxItemProperty(_) => return None,
    }
    Some(hash)
}
