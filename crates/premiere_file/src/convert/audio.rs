//! Sound placements of editable audio layers and of audible video layers, and
//! the clip Volume keys and edge fades of both.
//!
//! Sound uses the document's millisecond clock; it does not snap to the
//! sequence frame grid. Unsupported layer properties are reported, as for video.

use super::{
    keyframes,
    timing::{ticks_from_time, time_from_ticks},
};
use crate::{
    audio_media::SourceSound,
    error::{ensure, unsupported, BuildError, Result},
    export_loss::{omit_field, ExportField, OmissionSink},
    format::MediaId,
    schema::{
        PrAudioFade, PrAudioOccurrence, PrAudioStream, PrFadeCurve, PrKeyframeEasing,
        PrScalarKeyframe, PrVolumeKeys, TICKS_PER_MILLISECOND,
    },
    {approximate, omit, OmissionScope},
};
use fx_schema::{
    animator::{PropertyKeyframe, PropertyKeyframeEasing, PropertyKeyframeTrack},
    AnimationGraph, AudioLayer, LayerId, LinearGain, PropType, PropertyValue, Time,
    TimeRangeProperty, VideoLayer,
};
use std::{collections::BTreeMap, f64::consts::FRAC_PI_2};

/// Retimed sound uses an explicit source clock, also for its gain animator.
/// Both independent saved endpoints round once to the editable millisecond grid.
/// No native pitch flag is inferred by this clock projection.
pub(super) fn playback(
    clip: &PrAudioOccurrence,
    active: TimeRangeProperty,
    source: TimeRangeProperty,
    layer_id: LayerId,
) -> Result<fx_schema::LayerPlayback> {
    if clip.uses_layer_clock() {
        return fx_schema::LayerPlayback::linear(active, active, source, 0).map_err(unsupported);
    }
    let (start, end) = if clip.playback_rate < 0.0 {
        (source.end(), source.start)
    } else {
        (source.start, source.end())
    };
    let property = fx_schema::TimeRemapProperty::new(
        vec![
            fx_schema::TimeRemapKeyframe {
                id: super::premiere_to_tesseract::keyframe_id(layer_id, "audio-clock", 0),
                time: active.start,
                value: start,
                easing: PropertyKeyframeEasing::Linear,
            },
            fx_schema::TimeRemapKeyframe {
                id: super::premiere_to_tesseract::keyframe_id(layer_id, "audio-clock", 1),
                time: active.end(),
                value: end,
                easing: PropertyKeyframeEasing::Linear,
            },
        ],
        fx_schema::TimeRemapExtrapolation::Inactive,
        fx_schema::TimeRemapExtrapolation::Inactive,
    )
    .map_err(|error| unsupported(format!("audio clock: {error}")))?;
    fx_schema::LayerPlayback::remapped(active, property, 0).map_err(unsupported)
}

/// No native retimed Level/fade/handle clock proof exists. Keep representable
/// automation editable on the saved source clock, with one occurrence report.
pub(super) const RETIMED_GAIN_CLOCK_WARNING: &str = "native retimed/reversed Level, fade and handle clocks are unverified; nearest editable approximation uses the saved source clock";
pub(super) const REVERSE_SOURCE_START_CLIPPING_WARNING: &str =
    "reverse sound's native media-end rounding allowance clips its physical source start to zero";

/// Exponent of Premiere's fader curve, measured on AME renders: a Linear clip
/// Volume segment moves linearly in fader position u, with u = g^p for gains
/// up to 0 dB and u = 2 - g^-p above.
const FADER_EXPONENT: f64 = 0.4475;
/// Curve comparisons ignore reference gains below -60 dB: Premiere's gain when
/// import fits an easing, the FX gain when export checks a piece.
const REFERENCE_FLOOR: f64 = 1e-3;

/// Edge-fade precision follows its held level down, never up. Ordinary Volume
/// callers use unity, retaining the absolute policy; silence has no relative band.
fn reference_floor(reference_level: f64) -> f64 {
    REFERENCE_FLOOR
        * if reference_level > 0.0 {
            reference_level.min(1.0)
        } else {
            1.0
        }
}

const FIT_SAMPLES: usize = 128;
const FIT_ITERATIONS: usize = 40;
/// Largest dB difference above the floor that export accepts between one
/// written Linear piece and the FX curve: the fit's bound for keys within
/// -60..+3 dB.
const EXPORT_TOLERANCE_DB: f64 = 0.25;
/// Evenly spaced curve parameters that the export check probes per piece.
const EXPORT_PROBES: usize = 64;
/// Bisection steps that locate a curve parameter or a -60 dB crossing.
const PARAMETER_ITERATIONS: usize = 64;
/// Largest difference of y1 or y2 at which export still recognizes the fit of
/// a Linear segment. The fit is deterministic; the Levels that export writes
/// may round differently from those that were read.
const FIT_MATCH_TOLERANCE: f64 = 1e-9;

fn fader_position(gain: f64) -> f64 {
    if gain <= 0.0 {
        0.0
    } else if gain <= 1.0 {
        gain.powf(FADER_EXPONENT)
    } else {
        2.0 - gain.powf(-FADER_EXPONENT)
    }
}

fn fader_gain(position: f64) -> f64 {
    if position <= 0.0 {
        0.0
    } else if position <= 1.0 {
        position.powf(FADER_EXPONENT.recip())
    } else {
        (2.0 - position).powf(-FADER_EXPONENT.recip())
    }
}

/// Reflect source-clock gains with their incoming easings. A reversed Hold
/// changes on the other side of its key; the nearest editable representation
/// uses a one-source-millisecond step, rather than holding the wrong interval.
pub(super) fn reverse_volume_keys(
    keys: Vec<VolumeKey>,
    origin: i64,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<Vec<VolumeKey>> {
    let reverse_easing = |easing| match easing {
        PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
            PropertyKeyframeEasing::CubicBezier {
                x1: 1.0 - x2,
                y1: 1.0 - y2,
                x2: 1.0 - x1,
                y2: 1.0 - y1,
            }
        }
        other => other,
    };
    let mut output: Vec<VolumeKey> = Vec::with_capacity(keys.len());
    let mut holds = false;
    for index in (0..keys.len()).rev() {
        let key = &keys[index];
        let millis = origin
            .checked_sub(key.millis)
            .ok_or_else(|| unsupported("reversed audio key clock overflows"))?;
        let easing = if let Some(next) = keys.get(index + 1) {
            if next.easing == PropertyKeyframeEasing::Hold && next.gain != key.gain {
                holds = true;
                let step = output
                    .last()
                    .expect("next reverse key already written")
                    .millis
                    .checked_add(1)
                    .ok_or_else(|| unsupported("reversed Hold step overflows"))?;
                ensure!(
                    step <= millis,
                    "reversed audio Hold keys collapse on the millisecond grid"
                );
                output.push(VolumeKey {
                    millis: step,
                    gain: key.gain,
                    easing: PropertyKeyframeEasing::Hold,
                });
                if step == millis {
                    continue;
                }
                PropertyKeyframeEasing::Linear
            } else {
                reverse_easing(next.easing)
            }
        } else {
            PropertyKeyframeEasing::Linear
        };
        output.push(VolumeKey {
            millis,
            gain: key.gain,
            easing,
        });
    }
    if holds {
        approximate(omissions, record, "reversed audio Hold gain steps approximated within one source millisecond; slow playback stretches that timing error");
    }
    Ok(output)
}

/// The FX easing of a Linear clip Volume segment between the Level gains
/// `from` and `to` (1.0 = 0 dB), before other stages multiply them.
///
/// Premiere moves such a segment linearly in fader position, not in gain or
/// dB.
pub(super) fn fitted_level_easing(from: f64, to: f64) -> PropertyKeyframeEasing {
    fitted_level_easing_at_reference(from, to, 1.0)
}

/// The reference is in the same, pre-static-stage units as the Level values.
/// Exported edge fades use ordinary Level records, so their reimport needs this
/// explicit precision input too; it is not persisted or imported-origin state.
pub(super) fn fitted_level_easing_at_reference(
    from: f64,
    to: f64,
    reference_level: f64,
) -> PropertyKeyframeEasing {
    let (start, end) = (fader_position(from), fader_position(to));
    fitted_easing(from, to, reference_level, |t| {
        fader_gain(start + (end - start) * t)
    })
}

/// The FX easing of one key segment from the gain `from` to `to` that follows
/// Premiere's gain `target` at each progress through the segment.
///
/// A cubic Bézier with x1 = 1/3 and x2 = 2/3 makes the progress a cubic
/// polynomial in time. Its y1 and y2 stay in [0, 1], so the gain never leaves
/// the keys' range, and minimize the largest dB error above the reference
/// floor: a nested golden-section search, which the quasi-convex error admits.
fn fitted_easing(
    from: f64,
    to: f64,
    reference_level: f64,
    target: impl Fn(f64) -> f64,
) -> PropertyKeyframeEasing {
    let floor = reference_floor(reference_level);
    let samples: Vec<_> = (0..FIT_SAMPLES)
        .filter_map(|index| {
            let t = (index as f64 + 0.5) / FIT_SAMPLES as f64;
            let gain = target(t);
            (gain >= floor).then(|| {
                let bezier = [
                    3.0 * (1.0 - t).powi(2) * t,
                    3.0 * (1.0 - t) * t * t,
                    t.powi(3),
                ];
                (bezier, gain)
            })
        })
        .collect();
    if from == to || samples.is_empty() {
        return PropertyKeyframeEasing::Linear;
    }
    // The largest ratio between the eased gain and Premiere's, at least 1.
    let error = |y1: f64, y2: f64| {
        samples.iter().fold(1.0_f64, |worst, ([a, b, c], gain)| {
            let ratio = (from + (to - from) * (a * y1 + b * y2 + c)) / gain;
            worst.max(ratio).max(ratio.recip())
        })
    };
    let y1 = golden_section(|y1| error(y1, golden_section(|y2| error(y1, y2))));
    let y2 = golden_section(|y2| error(y1, y2));
    PropertyKeyframeEasing::CubicBezier {
        x1: 1.0 / 3.0,
        y1,
        x2: 2.0 / 3.0,
        y2,
    }
}

/// The argument in [0, 1] that minimizes a unimodal `f`.
fn golden_section(f: impl Fn(f64) -> f64) -> f64 {
    let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
    let (mut low, mut high) = (0.0, 1.0);
    let (mut below, mut above) = (1.0 - ratio, ratio);
    let (mut f_below, mut f_above) = (f(below), f(above));
    for _ in 0..FIT_ITERATIONS {
        if f_below <= f_above {
            (high, above, f_above) = (above, below, f_below);
            below = high - ratio * (high - low);
            f_below = f(below);
        } else {
            (low, below, f_below) = (below, above, f_above);
            above = low + ratio * (high - low);
            f_above = f(above);
        }
    }
    (low + high) / 2.0
}

/// Premiere's fade-in gain at `progress` through an audio transition, from
/// AME renders (run A5) for ordinary curves, and four direct PCM incoming
/// Custom controls. Six outgoing controls with fixed half Clip Gain independently
/// establish the Custom time mirror, not an amplitude complement.
fn fade_gain(curve: PrFadeCurve, progress: f64) -> f64 {
    match curve {
        PrFadeCurve::ConstantGain => progress,
        // Squared-sine fit, not equal power: the ledger's audio transition rows.
        PrFadeCurve::ConstantPower => (FRAC_PI_2 * progress.powf(0.6457)).sin().powi(2),
        PrFadeCurve::ExponentialFade => (3.5 * progress).exp_m1() / 3.5_f64.exp_m1(),
        // Saved/reopened PCM controls (-23, -6, 0, 29) establish this power
        // family with FadeShapeType absent. Keep Constant Power's older fit.
        PrFadeCurve::Custom(shape) => {
            let power = 10.0_f64.powf(-f64::from(shape.value()) / 100.0);
            (FRAC_PI_2 * progress.powf(power)).sin().powi(2)
        }
    }
}

/// Where a fade's inner keys sit, as fractions of its span in fade-in order:
/// bounded eased segments within 0.25 dB of the curve above -60 dB on the
/// tested spans. The calibrated Custom family needs additional inner keys.
const fn fade_fractions(curve: PrFadeCurve) -> &'static [f64] {
    match curve {
        PrFadeCurve::ConstantGain => &[],
        PrFadeCurve::ConstantPower => &[0.013, 0.193],
        PrFadeCurve::ExponentialFade => &[0.441],
        PrFadeCurve::Custom(_) => &[0.013, 0.08, 0.13, 0.193, 0.3, 0.441, 0.65],
    }
}

/// One `AudioVolume` key before it gets its id: the layer time, the gain, and
/// the easing that arrives at it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct VolumeKey {
    pub(super) millis: i64,
    pub(super) gain: f64,
    pub(super) easing: PropertyKeyframeEasing,
}

/// Refine imported Linear Volume segments without moving their native keys.
///
/// One cubic cannot fit a wide fader ramp, particularly across unity or from
/// silence. Subdivide in fader position on the existing millisecond grid until
/// the sampled error is at most 0.01 dB above the effective-gain floor. `None`
/// keeps the ordinary absolute policy; an edge fade's reference lowers it with
/// attenuation, never raises it for boosts. The bool reports a piece that still
/// exceeds that bound at the 1 ms resolution limit.
/// Fade-edge key alignment must run first, while keys still match native keys.
pub(super) fn refine_level_keys(
    keys: Vec<VolumeKey>,
    gain: f64,
    reference_level: Option<f64>,
) -> Result<(Vec<VolumeKey>, bool)> {
    let floor = reference_floor(reference_level.unwrap_or(1.0));
    const TOLERANCE_DB: f64 = 0.01;
    // Bound additional work independently of the number of native keys.
    const MAX_INSERTED_KEYS: usize = 4096;
    let Some(first) = keys.first() else {
        return Ok((keys, false));
    };
    if gain == 0.0 {
        return Ok((keys, false));
    }
    let mut refined = vec![*first];
    let mut inserted = 0;
    let mut limited = false;
    for pair in keys.windows(2) {
        let mut pending = vec![(pair[0], pair[1])];
        while let Some((from, mut to)) = pending.pop() {
            if to.easing == PropertyKeyframeEasing::Hold || from.gain == to.gain {
                refined.push(to);
                continue;
            }
            let (start_gain, end_gain) = (from.gain / gain, to.gain / gain);
            let (start, end) = (fader_position(start_gain), fader_position(end_gain));
            let curve = FxVolumeCurve {
                from: from.gain,
                to: to.gain,
                easing: to.easing,
            };
            let within_bound = (1..FIT_SAMPLES).all(|sample| {
                let t = sample as f64 / FIT_SAMPLES as f64;
                let expected = fader_gain(start + (end - start) * t) * gain;
                expected < floor
                    || (20.0 * (curve.at(t).1 / expected).log10()).abs() <= TOLERANCE_DB
            });
            if within_bound {
                refined.push(to);
                continue;
            }
            let span = to
                .millis
                .checked_sub(from.millis)
                .ok_or_else(|| unsupported("Volume key interval exceeds the millisecond range"))?;
            ensure!(span > 0, "Volume keys must have distinct increasing times");
            if span == 1 {
                limited = true;
                refined.push(to);
                continue;
            }
            inserted += 1;
            ensure!(
                inserted <= MAX_INSERTED_KEYS,
                "Volume curve fit requires more than {MAX_INSERTED_KEYS} added keys"
            );
            let offset = span / 2;
            let middle_gain = fader_gain(start + (end - start) * offset as f64 / span as f64);
            let middle = VolumeKey {
                millis: from.millis + offset,
                gain: middle_gain * gain,
                easing: fitted_level_easing_at_reference(
                    start_gain,
                    middle_gain,
                    reference_level.map_or(1.0, |level| level.min(1.0) / gain),
                ),
            };
            to.easing = fitted_level_easing_at_reference(
                middle_gain,
                end_gain,
                reference_level.map_or(1.0, |level| level.min(1.0) / gain),
            );
            // Stack order keeps the resulting keys in time order.
            pending.push((middle, to));
            pending.push((from, middle));
        }
    }
    Ok((refined, limited))
}

/// The keys of a fade over `start..end` milliseconds of the layer clock, up
/// to the gain `level`: a fade-in or its evidence-backed time-mirrored fade-out.
/// The inner keys sit at the curve's fixed fractions of the span,
/// rounded to the millisecond, on the curve; each segment's easing is fitted
/// to the curve between those keys. Short spans coarsen colliding inner keys
/// without moving their endpoints. `None` for a nonpositive span.
pub(super) fn fade_keys(
    curve: PrFadeCurve,
    fade_in: bool,
    start: i64,
    end: i64,
    level: f64,
) -> Option<Vec<VolumeKey>> {
    let span = end.checked_sub(start)?;
    if span <= 0 {
        return None;
    }
    let fractions = fade_fractions(curve);
    let mut offsets = Vec::with_capacity(fractions.len() + 2);
    offsets.push(0);
    offsets.extend(
        fractions
            .iter()
            .map(|fraction| (fraction * span as f64).round() as i64),
    );
    offsets.push(span);
    offsets.dedup();
    let progress: Vec<_> = offsets
        .iter()
        .map(|offset| *offset as f64 / span as f64)
        .collect();
    let gains: Vec<_> = progress.iter().map(|at| fade_gain(curve, *at)).collect();
    // The easing that arrives at each key after the first, in fade-in order.
    let easings = progress.windows(2).zip(gains.windows(2)).map(|(at, gain)| {
        if curve == PrFadeCurve::ConstantGain {
            PropertyKeyframeEasing::Linear
        } else {
            fitted_easing(gain[0], gain[1], 1.0, |t| {
                fade_gain(curve, at[0] + (at[1] - at[0]) * t)
            })
        }
    });
    let keys: Vec<_> = std::iter::once(PropertyKeyframeEasing::Linear)
        .chain(easings)
        .zip(offsets.iter().zip(&gains))
        .map(|(easing, (offset, gain))| VolumeKey {
            millis: start + offset,
            gain: gain * level,
            easing,
        })
        .collect();
    if fade_in {
        return Some(keys);
    }
    // Played backwards, the key times and gains reverse, and the segment that
    // leaves each fade-in key arrives at its mirror: its easing turns end for
    // end, which keeps x1 = 1/3 and x2 = 2/3.
    let mirror = |easing| match easing {
        PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
            PropertyKeyframeEasing::CubicBezier {
                x1,
                y1: 1.0 - y2,
                x2,
                y2: 1.0 - y1,
            }
        }
        other => other,
    };
    Some(
        keys.iter()
            .enumerate()
            .rev()
            .map(|(index, key)| VolumeKey {
                millis: start + end - key.millis,
                gain: key.gain,
                easing: keys
                    .get(index + 1)
                    .map_or(PropertyKeyframeEasing::Linear, |next| mirror(next.easing)),
            })
            .collect(),
    )
}

/// A recognized fade's gains and easings may differ from [`fade_keys`] by this
/// much, so that a document that another build wrote still matches.
const FADE_TOLERANCE: f64 = 1e-9;

fn key_gain(key: &PropertyKeyframe) -> Option<f64> {
    match *key.value() {
        PropertyValue::Float(value) => Some(value),
        _ => None,
    }
}

fn close_easing(actual: PropertyKeyframeEasing, expected: PropertyKeyframeEasing) -> bool {
    match (actual, expected) {
        (PropertyKeyframeEasing::Linear, PropertyKeyframeEasing::Linear) => true,
        (
            PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 },
            PropertyKeyframeEasing::CubicBezier {
                x1: expected_x1,
                y1: expected_y1,
                x2: expected_x2,
                y2: expected_y2,
            },
        ) => [
            x1 - expected_x1,
            y1 - expected_y1,
            x2 - expected_x2,
            y2 - expected_y2,
        ]
        .iter()
        .all(|difference| difference.abs() <= FADE_TOLERANCE),
        _ => false,
    }
}

/// Whether `run` holds exactly the keys that [`fade_keys`] writes for `curve`
/// over the run's own span and level: key times on the millisecond grid, and
/// gains and the easings after the first key within [`FADE_TOLERANCE`].
fn is_fade(run: &[PropertyKeyframe], curve: PrFadeCurve, fade_in: bool) -> bool {
    let (Some(first), Some(last)) = (run.first(), run.last()) else {
        return false;
    };
    let Some(level) = key_gain(if fade_in { last } else { first }).filter(|level| *level > 0.0)
    else {
        return false;
    };
    let start = first.layer_time().as_millis();
    fade_keys(curve, fade_in, start, last.layer_time().as_millis(), level).is_some_and(|keys| {
        keys.len() == run.len()
            && run
                .iter()
                .zip(&keys)
                .enumerate()
                .all(|(index, (key, expected))| {
                    key.layer_time().as_millis() == expected.millis
                        && key_gain(key).is_some_and(|gain| {
                            (gain - expected.gain).abs() <= FADE_TOLERANCE * level
                        })
                        && (index == 0 || close_easing(key.easing(), expected.easing))
                })
    })
}

/// What export writes for the clip Level of a layer beside its edge fades.
enum ClipLevel {
    /// One gain beside a fade, written as a static Level.
    Static(f64),
    /// Clip Volume keys.
    Keyed(WrittenLevel),
}

/// The fades at a layer's edges and the clip Level written beside them.
struct EdgeFades {
    /// A fade and its span in milliseconds.
    fade_in: Option<(PrFadeCurve, i64)>,
    fade_out: Option<(PrFadeCurve, i64)>,
    level: ClipLevel,
}

fn same_gain(key: &PropertyKeyframe, level: f64) -> bool {
    key_gain(key).is_some_and(|gain| (gain - level).abs() <= FADE_TOLERANCE * level)
}

/// Whether the clip Level keys `keys` hold `level` from `start` to `end`
/// milliseconds: no key inside, and the keys around the span hold or keep one
/// value, Premiere holding the first and last values beyond the keys.
fn holds_level(keys: &[&PropertyKeyframe], start: i64, end: i64, level: f64) -> bool {
    let time = |key: &PropertyKeyframe| key.layer_time().as_millis();
    let next = keys.partition_point(|key| time(key) < start);
    if keys.get(next).is_some_and(|key| time(key) <= end) {
        return false;
    }
    let held = match (next.checked_sub(1).map(|index| keys[index]), keys.get(next)) {
        (Some(previous), Some(next)) => (next.easing() == PropertyKeyframeEasing::Hold
            || key_gain(previous) == key_gain(next))
        .then_some(previous),
        (previous, next) => previous.or(next.copied()),
    };
    held.is_some_and(|key| same_gain(key, level))
}

/// Keeps a fade's level in the clip Level keys `others`: unchanged when they
/// hold it over the fade, else with the fade's full-level key `edge` when the
/// key on the far side of it holds that level too. `false` when neither holds.
fn hold_fade_level<'a>(
    others: &mut Vec<&'a PropertyKeyframe>,
    edge: &'a PropertyKeyframe,
    (start, end): (i64, i64),
    fade_in: bool,
) -> bool {
    let Some(level) = key_gain(edge) else {
        return false;
    };
    // Touching fades share their full-level key.
    if others.iter().any(|key| std::ptr::eq(*key, edge)) || holds_level(others, start, end, level) {
        return true;
    }
    let position = others.partition_point(|key| key.layer_time() < edge.layer_time());
    let far_side = if fade_in {
        position.checked_sub(1).map(|index| others[index])
    } else {
        others.get(position).copied()
    };
    let holds = far_side.is_none_or(|key| {
        same_gain(key, level) || (!fade_in && key.easing() == PropertyKeyframeEasing::Hold)
    });
    if holds {
        others.insert(position, edge);
    }
    holds
}

/// A fade's key run: its curve and the indices of its first and last keys.
type FadeRun = (PrFadeCurve, usize, usize);

/// A fade run's full-level key and its span in milliseconds.
fn fade_edge(
    keys: &[PropertyKeyframe],
    duration: i64,
    (_, first, last): FadeRun,
    fade_in: bool,
) -> (&PropertyKeyframe, (i64, i64)) {
    let time = |index: usize| keys[index].layer_time().as_millis();
    if fade_in {
        (&keys[last], (0, time(last)))
    } else {
        (&keys[first], (time(first), duration))
    }
}

/// The clip Level keys beside the fade runs `head` and `tail`, and the runs
/// whose level they hold ([`hold_fade_level`]); a run that they cannot hold
/// stays with them. The head settles first.
fn settle_fades(
    keys: &[PropertyKeyframe],
    duration: i64,
    head: Option<FadeRun>,
    tail: Option<FadeRun>,
) -> (Option<FadeRun>, Option<FadeRun>, Vec<&PropertyKeyframe>) {
    let in_run = |index: usize, run: Option<FadeRun>| {
        run.is_some_and(|(_, first, last)| (first..=last).contains(&index))
    };
    let mut others: Vec<_> = keys
        .iter()
        .enumerate()
        .filter(|(index, _)| !in_run(*index, head) && !in_run(*index, tail))
        .map(|(_, key)| key)
        .collect();
    let mut settle = |run: FadeRun, fade_in: bool| {
        let (_, first, last) = run;
        let (edge, span) = fade_edge(keys, duration, run, fade_in);
        let held = hold_fade_level(&mut others, edge, span, fade_in);
        if !held {
            others.extend(&keys[first..=last]);
            others.sort_by_key(|key| key.layer_time());
        }
        held
    };
    let head = head.filter(|run| settle(*run, true));
    let tail = tail.filter(|run| settle(*run, false));
    (head, tail, others)
}

/// The gain of clip Level keys that all hold one, which export writes as a
/// static Level beside a fade.
fn one_gain(keys: &[&PropertyKeyframe]) -> Option<f64> {
    let gain = key_gain(keys.first()?)?;
    keys.iter()
        .all(|key| key_gain(key) == Some(gain))
        .then_some(gain)
}

/// Whether Premiere's reimport keeps the fade `run` under the clip Level keys
/// `written` that export writes, subdivision keys included: none lies within
/// the margin of it but its own full-level key
/// ([`PrAudioFade::LEVEL_KEY_MARGIN_MILLIS`]).
fn clear_of_written_keys(
    written: &WrittenLevel,
    keys: &[PropertyKeyframe],
    duration: i64,
    run: FadeRun,
    fade_in: bool,
) -> bool {
    let (edge, (start, end)) = fade_edge(keys, duration, run, fade_in);
    let edge = edge.layer_time().as_millis();
    let margin = PrAudioFade::LEVEL_KEY_MARGIN_MILLIS;
    written
        .keys
        .iter()
        .all(|(millis, _, _)| *millis == edge || !(start - margin..=end + margin).contains(millis))
}

/// Finds the fades that import writes at the edges of a layer `duration`
/// milliseconds long at the static `volume`: a curve's exact key run
/// ([`is_fade`]) from silence at the layer start, or to silence at its end.
/// The other keys become the clip Level, which must hold each fade's level
/// over it ([`hold_fade_level`]) and, where it stays keyed, keep the keys that
/// export writes for it clear of the fade ([`clear_of_written_keys`]); a run
/// that fails either, and any other run, such as an edited fade, stays with
/// the other keys.
fn edge_fades(keys: &[PropertyKeyframe], duration: i64, volume: LinearGain) -> Result<EdgeFades> {
    // A fade exported as ordinary Level keys still needs its attenuated band.
    // Infer it from editable silence at a clip edge, not source provenance.
    let edge_silence = keys
        .first()
        .is_some_and(|key| key.layer_time().as_millis() == 0 && key_gain(key) == Some(0.0))
        || keys.last().is_some_and(|key| {
            key.layer_time().as_millis() == duration && key_gain(key) == Some(0.0)
        });
    let reference_level =
        edge_silence.then(|| keys.iter().filter_map(key_gain).fold(0.0_f64, f64::max));
    let at = |millis: i64| {
        keys.iter()
            .position(|key| key.layer_time().as_millis() == millis)
    };
    let segments = |curve: PrFadeCurve| 1 + fade_fractions(curve).len();
    // Each fade as (curve, index of its first key, index of its last key).
    let head = at(0).and_then(|first| {
        PrFadeCurve::ALL.into_iter().find_map(|curve| {
            let last = first + segments(curve);
            is_fade(keys.get(first..=last)?, curve, true).then_some((curve, first, last))
        })
    });
    let mut tail = at(duration).and_then(|last| {
        PrFadeCurve::ALL.into_iter().find_map(|curve| {
            let first = last.checked_sub(segments(curve))?;
            (head.is_none_or(|(_, _, head_last)| head_last <= first)
                && is_fade(&keys[first..=last], curve, false))
            .then_some((curve, first, last))
        })
    });
    let mut head = head;
    // A fade that the written Level keys would crowd settles again as Level
    // keys, with the other fade; each pass drops at least one fade.
    loop {
        let (settled_head, settled_tail, others) = settle_fades(keys, duration, head, tail);
        let level = match one_gain(&others) {
            Some(gain) if settled_head.is_some() || settled_tail.is_some() => {
                ClipLevel::Static(gain)
            }
            _ => ClipLevel::Keyed(written_level(&others, volume, reference_level)?),
        };
        let clear = |run: Option<FadeRun>, fade_in: bool| {
            run.is_none_or(|run| match &level {
                ClipLevel::Static(_) => true,
                ClipLevel::Keyed(written) => {
                    clear_of_written_keys(written, keys, duration, run, fade_in)
                }
            })
        };
        let (head_clear, tail_clear) = (clear(settled_head, true), clear(settled_tail, false));
        if head_clear && tail_clear {
            let time = |index: usize| keys[index].layer_time().as_millis();
            return Ok(EdgeFades {
                fade_in: settled_head.map(|(curve, _, last)| (curve, time(last))),
                fade_out: settled_tail.map(|(curve, first, _)| (curve, duration - time(first))),
                level,
            });
        }
        head = settled_head.filter(|_| head_clear);
        tail = settled_tail.filter(|_| tail_clear);
    }
}

/// Evaluate editable gain on its owner's input clock, including held endpoints.
pub(super) fn sample_volume(track: &PropertyKeyframeTrack, time: i64) -> Option<f64> {
    let keys = track.keyframes();
    let next = keys.partition_point(|key| key.layer_time().as_millis() <= time);
    if next == 0 {
        return key_gain(keys.first()?);
    }
    if next == keys.len() {
        return key_gain(keys.last()?);
    }
    let (first, last) = (&keys[next - 1], &keys[next]);
    if last.easing() == PropertyKeyframeEasing::Hold {
        return key_gain(first);
    }
    let curve = FxVolumeCurve {
        from: key_gain(first)?,
        to: key_gain(last)?,
        easing: last.easing(),
    };
    let progress = (time as f64 - first.layer_time().as_millis() as f64)
        / (last.layer_time().as_millis() as f64 - first.layer_time().as_millis() as f64);
    Some(curve.at(curve.parameter(progress)).1)
}

/// Premiere's gain along a written Linear clip Volume segment between the
/// effective gains `from` and `to`. Export writes every keyed Level at or
/// below 0 dB, where the fader position is g^p, so Clip Gain times the curve
/// between Levels is the same curve between effective gains.
fn exported_linear_gain(from: f64, to: f64, progress: f64) -> f64 {
    let position =
        (1.0 - progress) * from.powf(FADER_EXPONENT) + progress * to.powf(FADER_EXPONENT);
    position.powf(FADER_EXPONENT.recip())
}

/// One coordinate of a unit cubic Bézier with inner control coordinates
/// `first` and `second`, at parameter `s`.
fn cubic_bezier(first: f64, second: f64, s: f64) -> f64 {
    let inverse = 1.0 - s;
    3.0 * inverse * inverse * s * first + 3.0 * inverse * s * s * second + s * s * s
}

/// The FX gain between two volume keys, along its easing's parameter: Linear
/// progress is its own parameter, and a cubic Bézier's time and progress are
/// both polynomials in it. Evenly spaced parameters therefore also probe a
/// part of the curve that is steep or narrow in time.
#[derive(Debug, Clone, Copy)]
struct FxVolumeCurve {
    from: f64,
    to: f64,
    easing: PropertyKeyframeEasing,
}

impl FxVolumeCurve {
    /// Normalized time and gain at parameter `s`. The mixer never plays a
    /// negative gain.
    fn at(self, s: f64) -> (f64, f64) {
        let (time, progress) = match self.easing {
            PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
                (cubic_bezier(x1, x2, s), cubic_bezier(y1, y2, s))
            }
            // A Hold segment exports as Hold and is never evaluated.
            PropertyKeyframeEasing::Linear | PropertyKeyframeEasing::Hold => (s, s),
        };
        (
            time,
            (self.from + (self.to - self.from) * progress).max(0.0),
        )
    }

    /// The parameter at normalized time `time`. Time never decreases along a
    /// cubic Bézier whose x1 and x2 lie in [0, 1], as FX requires.
    fn parameter(self, time: f64) -> f64 {
        let PropertyKeyframeEasing::CubicBezier { x1, x2, .. } = self.easing else {
            return time;
        };
        let (mut low, mut high) = (0.0, 1.0);
        for _ in 0..PARAMETER_ITERATIONS {
            let middle = (low + high) / 2.0;
            if cubic_bezier(x1, x2, middle) < time {
                low = middle;
            } else {
                high = middle;
            }
        }
        (low + high) / 2.0
    }

    /// Parameters strictly between `low` and `high` where the progress turns:
    /// the roots of the derivative of the cubic Bézier's y.
    fn turns(self, low: f64, high: f64) -> Vec<f64> {
        let PropertyKeyframeEasing::CubicBezier { y1, y2, .. } = self.easing else {
            return Vec::new();
        };
        // y'(s) / 3 = a s^2 + b s + c, solved in the form that stays accurate
        // as a vanishes; a root that does not exist comes out infinite or NaN.
        let (a, b, c) = (3.0 * y1 - 3.0 * y2 + 1.0, 2.0 * (y2 - 2.0 * y1), y1);
        let discriminant = b * b - 4.0 * a * c;
        if discriminant < 0.0 {
            return Vec::new();
        }
        let q = -0.5 * (b + b.signum() * discriminant.sqrt());
        [q / a, c / q]
            .into_iter()
            .filter(|s| *s > low && *s < high)
            .collect()
    }

    /// The largest dB difference, wherever the FX gain is at or above -60 dB,
    /// between the curve and Premiere's Linear piece from `(time, gain)` key
    /// `start` to key `end`. A sampled check, not a proof: it probes evenly
    /// spaced parameters, the turns of the progress, and each -60 dB crossing
    /// between probes, where the difference is largest next to silence.
    fn piece_error(self, start: (f64, f64), end: (f64, f64), reference_level: f64) -> f64 {
        let floor = reference_floor(reference_level);
        let (low, high) = (self.parameter(start.0), self.parameter(end.0));
        let mut parameters: Vec<f64> = (0..=EXPORT_PROBES)
            .map(|index| low + (high - low) * index as f64 / EXPORT_PROBES as f64)
            .chain(self.turns(low, high))
            .collect();
        parameters.sort_by(f64::total_cmp);
        let error = |s: f64| {
            let (time, gain) = self.at(s);
            if gain < floor {
                return 0.0;
            }
            let progress = ((time - start.0) / (end.0 - start.0)).clamp(0.0, 1.0);
            (20.0 * (exported_linear_gain(start.1, end.1, progress) / gain).log10()).abs()
        };
        let quiet = |s: f64| self.at(s).1 < floor;
        let mut worst = error(low);
        for pair in parameters.windows(2) {
            let (mut below, mut above) = (pair[0], pair[1]);
            worst = worst.max(error(above));
            if quiet(below) == quiet(above) {
                continue;
            }
            // The progress is monotonic between probes, so one crossing lies
            // between them; keep `above` on the audible side.
            if quiet(above) {
                (below, above) = (above, below);
            }
            for _ in 0..PARAMETER_ITERATIONS {
                let middle = (below + above) / 2.0;
                if quiet(middle) {
                    below = middle;
                } else {
                    above = middle;
                }
            }
            worst = worst.max(error(above));
        }
        worst
    }
}

/// Whether a cubic easing is the fit that import gives Premiere's Linear
/// segment between these gains' Levels, which Clip Gain `clip_gain` scales.
fn is_imported_fit(curve: FxVolumeCurve, clip_gain: f64, reference_level: Option<f64>) -> bool {
    let PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = curve.easing else {
        return false;
    };
    (x1, x2) == (1.0 / 3.0, 2.0 / 3.0)
        && matches!(
            fitted_level_easing_at_reference(curve.from / clip_gain, curve.to / clip_gain, reference_level.map_or(1.0, |level| level.min(1.0) / clip_gain)),
            PropertyKeyframeEasing::CubicBezier { y1: fitted_y1, y2: fitted_y2, .. }
                if (fitted_y1 - y1).abs() <= FIT_MATCH_TOLERANCE
                    && (fitted_y2 - y2).abs() <= FIT_MATCH_TOLERANCE
        )
}

/// Keys inside one FX segment from `start_ms` to `end_ms` on the layer clock,
/// as (time, gain), so that Premiere's Linear pieces follow the curve within
/// `EXPORT_TOLERANCE_DB`. Each key lies on the curve and on the millisecond
/// grid, which reimport keeps, at the farthest millisecond found whose piece
/// still meets the tolerance. A one-millisecond piece that misses it cannot be
/// divided; each is added to `misses` with its start time and sampled error.
fn inner_keys(
    curve: FxVolumeCurve,
    start_ms: i64,
    end_ms: i64,
    reference_level: f64,
    misses: &mut Vec<(i64, f64)>,
) -> Result<Vec<(i64, f64)>> {
    let span = (end_ms - start_ms) as f64;
    let point = |ms: i64| {
        let time = (ms - start_ms) as f64 / span;
        let gain = if ms == end_ms {
            curve.to
        } else {
            curve.at(curve.parameter(time)).1
        };
        (time, gain)
    };
    let mut keys = Vec::new();
    let (mut start, mut start_point) = (start_ms, (0.0, curve.from));
    loop {
        let error = |ms: i64| curve.piece_error(start_point, point(ms), reference_level);
        let whole = error(end_ms);
        if whole <= EXPORT_TOLERANCE_DB {
            break;
        }
        if end_ms - start == 1 {
            misses.push((start, whole));
            break;
        }
        // Double the piece while it meets the tolerance, then bisect.
        let (mut reach, mut reach_error) = (start + 1, error(start + 1));
        let mut step = 2;
        while start + step < end_ms {
            let candidate = error(start + step);
            if candidate > EXPORT_TOLERANCE_DB {
                break;
            }
            (reach, reach_error) = (start + step, candidate);
            step *= 2;
        }
        let (mut low, mut high) = (reach + 1, (start + step).min(end_ms) - 1);
        while low <= high {
            let middle = low + (high - low) / 2;
            let candidate = error(middle);
            if candidate <= EXPORT_TOLERANCE_DB {
                (reach, reach_error) = (middle, candidate);
                low = middle + 1;
            } else {
                high = middle - 1;
            }
        }
        if reach_error > EXPORT_TOLERANCE_DB {
            misses.push((start, reach_error));
        }
        start_point = point(reach);
        keys.push((reach, start_point.1));
        start = reach;
    }
    Ok(keys)
}

/// The clip Volume keys that export writes for FX `AudioVolume` keys, in
/// layer milliseconds, and the one-millisecond pieces that miss the tolerance
/// ([`inner_keys`]).
struct WrittenLevel {
    /// Each key's layer time, gain and the easing that arrives at it.
    keys: Vec<(i64, f64, PrKeyframeEasing)>,
    /// The start time and sampled error of each piece that misses the tolerance.
    misses: Vec<(i64, f64)>,
}

/// The [`WrittenLevel`] of the FX keys `fx_keys` of a placement at the static
/// `volume`.
///
/// The keys keep effective FX gains; `clip_volume_records` later divides mono
/// gains by 1/√2 and moves a peak above 0 dB into Clip Gain, so every keyed
/// Level is at or below 0 dB, where the fader curve scales with the gain.
/// Hold stays Hold. A segment whose easing is the fit that import gives the
/// Linear segment between its Levels exports as that segment, so a recognized
/// imported curve keeps its native keys. Recognition predicts the Levels in
/// effective gain, without the mono factor: a peak above 0 dB, among the keys
/// and the static `volume`, becomes Clip Gain. A mono prediction can thus be
/// up to 3.01 dB below the written Levels. That scale does not change a fit
/// whose gains stay above -60 dB, but a mono fade from or to silence may not
/// be recognized. Any other segment exports as Linear pieces that meet
/// `EXPORT_TOLERANCE_DB` in effective gain (see [`inner_keys`]), which the
/// written Levels keep. `reference_level` is an edge-silence envelope's peak
/// effective gain, independent of the overridden static Volume; `None` keeps
/// ordinary absolute precision. Only comparison precision changes, not values.
fn written_level(
    fx_keys: &[&PropertyKeyframe],
    volume: LinearGain,
    reference_level: Option<f64>,
) -> Result<WrittenLevel> {
    let gains = fx_keys
        .iter()
        .map(|key| match *key.value() {
            PropertyValue::Float(value) if value.is_finite() && value >= 0.0 => Ok(value),
            _ => Err(unsupported(
                "volume keyframes must be finite, nonnegative gains",
            )),
        })
        .collect::<Result<Vec<_>>>()?;
    let clip_gain = gains
        .iter()
        .copied()
        .fold(volume.as_f64(), f64::max)
        .max(1.0);
    let mut keys = Vec::with_capacity(fx_keys.len());
    let mut misses = Vec::new();
    for (index, fx_key) in fx_keys.iter().enumerate() {
        let millis = fx_key.layer_time().as_millis();
        let easing = match (index.checked_sub(1), fx_key.easing()) {
            (None, _) => PrKeyframeEasing::Linear,
            (Some(_), PropertyKeyframeEasing::Hold) => PrKeyframeEasing::Hold,
            (Some(previous), easing) => {
                let curve = FxVolumeCurve {
                    from: gains[previous],
                    to: gains[index],
                    easing,
                };
                if !is_imported_fit(curve, clip_gain, reference_level) {
                    let start_ms = fx_keys[previous].layer_time().as_millis();
                    for (inner_ms, gain) in inner_keys(
                        curve,
                        start_ms,
                        millis,
                        reference_level.unwrap_or(1.0),
                        &mut misses,
                    )? {
                        keys.push((inner_ms, gain, PrKeyframeEasing::Linear));
                    }
                }
                PrKeyframeEasing::Linear
            }
        };
        keys.push((millis, gains[index], easing));
    }
    Ok(WrittenLevel { keys, misses })
}

impl WrittenLevel {
    /// The keys on the source clock of a placement whose source In is
    /// `source_in`. A piece that misses the tolerance is reported, once, as
    /// approximated.
    fn volume_keys(
        self,
        source_in: i64,
        record: &str,
        omissions: &mut dyn OmissionSink,
    ) -> Result<PrVolumeKeys> {
        let keys = self
            .keys
            .into_iter()
            .map(|(millis, value, easing)| {
                Ok(PrScalarKeyframe {
                    source_ticks: keyframes::source_ticks(source_in, millis)?,
                    value,
                    easing,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if let Some((first_ms, _)) = self.misses.first() {
            let worst = self
                .misses
                .iter()
                .map(|(_, error)| *error)
                .fold(0.0, f64::max);
            approximate(
                omissions,
                record,
                format!(
                    "volume curve approximated, not removed: Premiere's Linear pieces follow the FX curve within {EXPORT_TOLERANCE_DB} dB above -60 dB (sampled) except in {} one-millisecond piece(s) from layer time {first_ms} ms, which differ by up to {worst:.2} dB; clip Volume keys cannot be closer than 1 ms",
                    self.misses.len()
                ),
            );
        }
        Ok(PrVolumeKeys { keys, gain: 1.0 })
    }
}

/// The clip Volume of one exported placement: its static level, its Level
/// keys and the fades at its edges.
struct ExportedVolume {
    volume: LinearGain,
    keys: Option<PrVolumeKeys>,
    fade_in: Option<PrAudioFade>,
    fade_out: Option<PrAudioFade>,
}

/// The clip Volume of a placement at the static `volume`, with the source In
/// `source_in`, whose layer plays over `active_range` with the optional FX
/// keys `track`: the static volume alone without keys, or zero gain after
/// reporting why its volume animation was not exported.
fn exported_volume(
    track: Option<&PropertyKeyframeTrack>,
    active_range: &TimeRangeProperty,
    source_in: i64,
    volume: LinearGain,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> ExportedVolume {
    let unkeyed = ExportedVolume {
        volume,
        keys: None,
        fade_in: None,
        fade_out: None,
    };
    let Some(track) = track else {
        return unkeyed;
    };
    keyed_volume(track, active_range, source_in, volume, record, omissions).unwrap_or_else(
        |error| {
            omit(
                omissions,
                OmissionScope::Feature,
                record,
                format!("volume animation was not exported; sound kept at zero gain: {error}"),
            );
            // A static fallback can play through the animation's silent regions.
            ExportedVolume {
                volume: LinearGain::ZERO,
                ..unkeyed
            }
        },
    )
}

/// [`exported_volume`] of FX keys: the fades at the layer's edges become
/// one-sided transitions ([`edge_fades`]), and the keys that remain the clip
/// Level keys, or a static level when they hold one gain beside a fade.
fn keyed_volume(
    track: &PropertyKeyframeTrack,
    active_range: &TimeRangeProperty,
    source_in: i64,
    volume: LinearGain,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<ExportedVolume> {
    let duration = i64::try_from(active_range.duration.as_millis())
        .map_err(|_| unsupported("audio range exceeds Premiere's tick range"))?;
    let edges = edge_fades(track.keyframes(), duration, volume)?;
    let fade = |fade: Option<(PrFadeCurve, i64)>| -> Result<Option<PrAudioFade>> {
        fade.map(|(curve, millis)| {
            Ok(PrAudioFade {
                id: None,
                curve,
                duration_ticks: millis
                    .checked_mul(TICKS_PER_MILLISECOND)
                    .ok_or_else(|| unsupported("audio fade exceeds Premiere's tick range"))?,
            })
        })
        .transpose()
    };
    let (fade_in, fade_out) = (fade(edges.fade_in)?, fade(edges.fade_out)?);
    let (volume, keys) = match edges.level {
        ClipLevel::Static(gain) => (
            LinearGain::new(gain)
                .map_err(|_| unsupported("volume keyframes must be finite, nonnegative gains"))?,
            None,
        ),
        ClipLevel::Keyed(written) => (
            volume,
            Some(written.volume_keys(source_in, record, omissions)?),
        ),
    };
    Ok(ExportedVolume {
        volume,
        keys,
        fade_in,
        fade_out,
    })
}

/// The constant source window currently authored by a sound. Fractional endpoint
/// requests round once to the editable source millisecond, independently of placement.
struct ConstantAudioClock {
    source_range: TimeRangeProperty,
    backwards: bool,
    rounded: bool,
}

fn constant_source_window(playback: &fx_schema::LayerPlayback) -> Result<ConstantAudioClock> {
    use fx_schema::LayerPlaybackMapping;
    let active = playback.input_range();
    let map = |time: Time| -> Result<(i128, bool)> {
        let shifted = i128::from(time.as_millis()) + i128::from(playback.input_offset_ms());
        let (input_start, input_span, source_start, source_span) = match playback.mapping() {
            LayerPlaybackMapping::Linear { input, output } => (
                i128::from(input.start.as_millis()),
                i128::from(input.duration.as_millis()),
                i128::from(output.start.as_millis()),
                i128::from(output.duration.as_millis()),
            ),
            LayerPlaybackMapping::TimeRemap { property } => {
                let keys = property.keyframes();
                ensure!(keys.len() >= 2
                    && property.before() == fx_schema::TimeRemapExtrapolation::Inactive
                    && property.after() == fx_schema::TimeRemapExtrapolation::Inactive,
                    "audio export requires bounded linear constant TimeRemap; variable curves, loops and holds are not converted");
                let first = &keys[0];
                let last = &keys[keys.len() - 1];
                let input_start = i128::from(first.time.as_millis());
                let input_span = i128::from(last.time.as_millis()) - input_start;
                let source_start = i128::from(first.value.as_millis());
                let source_span = i128::from(last.value.as_millis()) - source_start;
                ensure!(keys.iter().skip(1).all(|key| key.easing == PropertyKeyframeEasing::Linear
                    && (i128::from(key.value.as_millis())-source_start)*input_span
                    == (i128::from(key.time.as_millis())-input_start)*source_span),
                    "audio export requires linear constant TimeRemap; variable curves and holds are not converted");
                ensure!(
                    (input_start..=input_start + input_span).contains(&shifted),
                    "audio TimeRemap does not cover its active window"
                );
                (input_start, input_span, source_start, source_span)
            }
        };
        let numerator = (shifted - input_start) * source_span;
        let delta = super::timing::nearest(numerator, input_span)
            .ok_or_else(|| unsupported("audio source clock overflows"))?;
        Ok((source_start + delta, numerator % input_span != 0))
    };
    let (start, rounded_start) = map(active.start)?;
    let (end, rounded_end) = map(active.end())?;
    ensure!(
        start != end,
        "audio constant clock collapses on the source millisecond grid"
    );
    let low = u64::try_from(start.min(end))
        .map_err(|_| unsupported("audio source clock is negative or overflows"))?;
    let span = u64::try_from((end - start).abs())
        .map_err(|_| unsupported("audio source span overflows"))?;
    Ok(ConstantAudioClock {
        source_range: TimeRangeProperty::new(
            Time::from_millis(low),
            fx_schema::Duration::from_millis(span),
        ),
        backwards: end < start,
        rounded: rounded_start || rounded_end,
    })
}

/// Source-clock gain keys export as clip Level, not native transition handles.
/// Their curve remains editable even when an imported fade has been edited.
fn exported_source_volume(
    sound: &AudioLayer,
    track: Option<&PropertyKeyframeTrack>,
    source_in: i64,
    facts: &PrAudioStream,
    playback_rate: f64,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> ExportedVolume {
    let unkeyed = ExportedVolume {
        volume: sound.volume,
        keys: None,
        fade_in: None,
        fade_out: None,
    };
    let Some(track) = track else {
        return unkeyed;
    };
    let build = |omissions: &mut dyn OmissionSink| -> Result<PrVolumeKeys> {
        let mut keys = track
            .keyframes()
            .iter()
            .map(|key| {
                let local = i128::from(key.layer_time().as_millis());
                let source = match sound.playback.mapping() {
                    fx_schema::LayerPlaybackMapping::Linear { input, output } => {
                        i128::from(output.start.as_millis())
                            + super::timing::nearest(
                                local * i128::from(output.duration.as_millis()),
                                i128::from(input.duration.as_millis()),
                            )
                            .ok_or_else(|| unsupported("audio gain source clock overflows"))?
                    }
                    fx_schema::LayerPlaybackMapping::TimeRemap { .. } => local,
                };
                let PropertyValue::Float(gain) = key.value() else {
                    return Err(unsupported("audio gain keys must be scalar"));
                };
                Ok(VolumeKey {
                    millis: i64::try_from(source)
                        .map_err(|_| unsupported("audio gain time overflows"))?,
                    gain: *gain,
                    easing: key.easing(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if constant_source_window(&sound.playback)?.backwards {
            keys = reverse_volume_keys(
                keys,
                i64::try_from(time_from_ticks(facts.intrinsic_ticks)?.as_millis())
                    .map_err(|_| unsupported("audio duration overflows"))?,
                record,
                omissions,
            )?;
        }
        let source_in_ms = keyframes::layer_millis(source_in, 0)?;
        let keys = keys
            .into_iter()
            .enumerate()
            .map(|(index, key)| {
                Ok(PropertyKeyframe::new(
                    super::premiere_to_tesseract::keyframe_id(sound.id, "audio-export-gain", index),
                    fx_schema::TimeOffset::from_millis(
                        key.millis
                            .checked_sub(source_in_ms)
                            .ok_or_else(|| unsupported("audio gain offset overflows"))?,
                    ),
                    PropertyValue::Float(key.gain),
                    key.easing,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let track =
            PropertyKeyframeTrack::new(keys).map_err(|error| unsupported(error.to_string()))?;
        let span_ms = constant_source_window(&sound.playback)?
            .source_range
            .duration
            .as_millis();
        let span_ms =
            i64::try_from(span_ms).map_err(|_| unsupported("audio gain span overflows"))?;
        let reference_level = track
            .keyframes()
            .iter()
            .any(|key| {
                key_gain(key) == Some(0.0) && [0, span_ms].contains(&key.layer_time().as_millis())
            })
            .then(|| {
                track
                    .keyframes()
                    .iter()
                    .filter_map(key_gain)
                    .fold(0.0_f64, f64::max)
            });
        written_level(
            &track.keyframes().iter().collect::<Vec<_>>(),
            sound.volume,
            reference_level,
        )?
        .volume_keys(source_in, record, omissions)
    };
    match build(omissions) {
        Ok(keys) => {
            let reason = if playback_rate == 1.0 {
                "unit-forward source-clock volume exports as editable Level keys, including fades; native transition editing state is normalized to Level automation and source key times round to milliseconds"
            } else {
                "native retimed/reversed Level, fade and handle clocks are unverified; nearest editable approximation exports current source-clock Level keys, including fades; source key times round to milliseconds and native transition editing state is not restored"
            };
            approximate(omissions, record, reason);
            ExportedVolume {
                keys: Some(keys),
                ..unkeyed
            }
        }
        Err(error) => {
            omit(
                omissions,
                OmissionScope::Feature,
                record,
                format!(
                    "source-clock volume animation was not exported; sound kept at zero gain: {error}"
                ),
            );
            ExportedVolume {
                volume: LinearGain::ZERO,
                ..unkeyed
            }
        }
    }
}

/// Only an inspected positive whole-sample clock establishes the alternate
/// floor representation. Adjacent millisecond values are not a tolerance.
fn is_audio_source_floor(authored: u64, file_ticks: i64, sample_rate: u32) -> bool {
    let rate = i64::from(sample_rate);
    (8_000..=192_000).contains(&sample_rate)
        && crate::schema::TICKS % rate == 0
        && file_ticks > 0
        && file_ticks % (crate::schema::TICKS / rate) == 0
        && u64::try_from(file_ticks / TICKS_PER_MILLISECOND) == Ok(authored)
}

/// Maps one audio layer of the list whose parent is `parent`, with its volume
/// keys and edge fades, and the inspected sound of its source. Returns `None` after reporting
/// a layer that cannot be exported. A volume animation that cannot be exported
/// is reported, and the sound keeps its placement and source at zero gain.
pub(super) fn layer<'a>(
    sound: &AudioLayer,
    parent: Option<LayerId>,
    volume_track: Option<&PropertyKeyframeTrack>,
    audio_facts: &'a BTreeMap<String, SourceSound>,
    omissions: &mut dyn OmissionSink,
) -> Result<Option<(PrAudioOccurrence, &'a PrAudioStream)>> {
    let record = format!("layer {} ({:?})", sound.id, sound.name);
    if sound.is_hidden {
        omit(
            omissions,
            OmissionScope::Occurrence,
            &record,
            "hidden audio was not exported (would otherwise become audible)",
        );
        return Ok(None);
    }
    // Explicit TimeRemap also changes the gain animator's clock to source time.
    let retimed = match sound.playback.mapping() {
        fx_schema::LayerPlaybackMapping::Linear { input, output } => {
            input.duration != output.duration
        }
        fx_schema::LayerPlaybackMapping::TimeRemap { .. } => true,
    };
    for (changed, field) in [
        (!sound.description.is_empty(), ExportField::Description),
        (sound.metadata.is_some(), ExportField::Metadata),
        (sound.parent != parent, ExportField::ParentGrouping),
        (
            sound.playback.time_remap().is_some() && !retimed,
            ExportField::AudioPlaybackSettings,
        ),
        (
            sound.caption_presentation.is_some(),
            ExportField::CaptionPresentation,
        ),
        (sound.captions_enabled == Some(true), ExportField::Captions),
    ] {
        if changed {
            omit_field(
                omissions,
                sound.id,
                field,
                &record,
                format!("{field} was not exported"),
            );
        }
    }
    // The sound exports its active asset, as the renderer plays it; the
    // enhancement toggle and the other asset have no native form.
    if let Some(enhancement) = &sound.source.enhancement {
        let output = enhancement.enhanced_asset_id.as_str();
        let reason = if enhancement.enabled {
            format!(
                "audio enhancement exported as its active output {output:?}; the original asset {:?} and the enhancement toggle were not exported",
                sound.source.asset_id.as_str()
            )
        } else {
            format!(
                "inactive audio enhancement output {output:?} and the enhancement toggle were not exported"
            )
        };
        omit_field(
            omissions,
            sound.id,
            ExportField::AudioEnhancement,
            &record,
            reason,
        );
    }
    let asset_id = sound.source.active_asset_id().as_str();
    // The duration check below compares `sourceIntrinsicDuration` with the
    // asset that exports; a mismatch names an enhanced output.
    let active_output = if asset_id == sound.source.asset_id.as_str() {
        String::new()
    } else {
        format!(" of the active audio enhancement output {asset_id:?}")
    };
    let (facts, file_ticks) = match audio_facts.get(asset_id) {
        Some(SourceSound::Supported(facts)) => (facts, facts.intrinsic_ticks),
        Some(SourceSound::PaddedToPicture { stream, file_ticks }) => (stream, *file_ticks),
        other => {
            let reason = match other {
                Some(SourceSound::Unsupported(reason)) => reason,
                _ => "audio source has no sound to export",
            };
            omit(omissions, OmissionScope::Occurrence, &record, reason);
            return Ok(None);
        }
    };
    (|| {
        // A sound padded to its picture may carry either the picture duration
        // that Premiere records or the AAC length; export writes the former.
        let intrinsic_millis = time_from_ticks(facts.intrinsic_ticks)?.as_millis();
        let file_millis = time_from_ticks(file_ticks)?.as_millis();
        let authored = sound.source_intrinsic_duration.as_millis();
        let source_floor = authored != intrinsic_millis
            && authored != file_millis
            && is_audio_source_floor(authored, file_ticks, facts.sample_rate);
        ensure!(
            authored == intrinsic_millis || authored == file_millis || source_floor,
            "sourceIntrinsicDuration {authored} ms differs from the packaged audio duration {file_millis} ms{active_output}"
        );
        if source_floor {
            approximate(omissions, &record, format!(
                "sourceIntrinsicDuration {authored} ms is the floor of the packaged audio duration{active_output} (nearest {file_millis} ms); native audio retains its exact sample clock"
            ));
        }
        let clock = match constant_source_window(&sound.playback) {
            Ok(clock) => clock,
            Err(error) if sound.playback.time_remap().is_some() => {
                omit(omissions, OmissionScope::Occurrence, &record, error.to_string());
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        if clock.rounded {
            approximate(omissions, &record, "constant audio source endpoints rounded independently to the nearest editable millisecond; exported rate follows those endpoints");
        }
        let (mapped, backwards) = (clock.source_range, clock.backwards);
        let playback_rate = mapped.duration.as_millis() as f64 / sound.playback.input_range().duration.as_millis() as f64 * if backwards { -1.0 } else { 1.0 };
        ensure!(mapped.start >= sound.source_range.start && mapped.end() <= sound.source_range.end(),
            "audio playback window extends beyond the authored source selection");
        occurrence(
            asset_id,
            &sound.playback.input_range(),
            &mapped,
            playback_rate,
            |active_range, source_in| {
                if retimed {
                    return exported_source_volume(sound, volume_track, source_in, facts, playback_rate, &record, omissions);
                }
                exported_volume(
                    volume_track,
                    active_range,
                    source_in,
                    sound.volume,
                    &record,
                    omissions,
                )
            },
            facts,
        )
        .map(|mut occurrence| {
            occurrence.preserve_audio_pitch = sound.preserve_audio_pitch;
            if sound.preserve_audio_pitch && playback_rate.abs() == 1.0 {
                // This field report is an acceptance caveat, not a dropped flag.
                omit_field(omissions, sound.id, ExportField::AudioPitchPreservation, &record, "unit-speed audio pitch preservation exported with the general constant-rate canonical pair; native unit-speed flag acceptance and fidelity remain unverified");
            }
            Some((occurrence, facts))
        })
    })()
    .map_err(|source| BuildError::Context {
        context: record,
        source: Box::new(source),
    })
}

/// The sound selected independently of Eye Contact, matching the runtime's
/// `VideoSource::active_audio_asset_id` without requiring runtime model types.
fn selected_sound_asset(video: &VideoLayer) -> &str {
    video
        .source
        .audio_enhancement
        .as_ref()
        .filter(|enhancement| enhancement.enabled)
        .map_or(video.source.asset_id.as_str(), |enhancement| {
            enhancement.enhanced_asset_id.as_str()
        })
}

/// The asset whose sound [`embedded`] can export for an audible video.
/// Inspection counts every volume animation, including unsupported ones.
pub(crate) fn embedded_sound_asset<'v>(
    video: &'v VideoLayer,
    dynamics: &AnimationGraph,
) -> Option<&'v str> {
    let keyed = super::tesseract_to_premiere::layer_animations(dynamics, video.id)
        .any(|(property, _)| property == PropType::AudioVolume);
    let audible = video.volume.is_some_and(|gain| gain.as_f64() > 0.0) || keyed;
    audible.then(|| selected_sound_asset(video))
}

/// Whether the clip video `video` plays its sound on export: a positive
/// volume, or the volume keys `volume_track`, which play from a silent base.
pub(super) fn audible(video: &VideoLayer, volume_track: Option<&PropertyKeyframeTrack>) -> bool {
    video.volume.is_some_and(|gain| gain.as_f64() > 0.0) || volume_track.is_some()
}

/// The sound of an [`audible`] video layer whose source has sound
/// ([`embedded_sound_asset`]). A sound that cannot be exported is reported;
/// the picture is still exported. So is a sound whose volume animation cannot
/// be exported and whose static volume is silent; fades alone play at the
/// level that their keys hold, the placement's static level.
pub(super) fn embedded<'a>(
    video: &VideoLayer,
    volume_track: Option<&PropertyKeyframeTrack>,
    audio_facts: &'a BTreeMap<String, SourceSound>,
    omissions: &mut dyn OmissionSink,
) -> Option<(PrAudioOccurrence, &'a PrAudioStream)> {
    if !audible(video, volume_track) {
        return None;
    }
    let volume = video.volume.unwrap_or(LinearGain::ZERO);
    let asset_id = selected_sound_asset(video);
    let sound = match audio_facts.get(asset_id) {
        Some(sound) => sound,
        None => {
            if video
                .source
                .audio_enhancement
                .as_ref()
                .is_some_and(|enhancement| enhancement.enabled)
            {
                omit(omissions, OmissionScope::Feature,
                    format!("layer {} ({:?})", video.id, video.name),
                    format!("embedded audio was not exported: active audio enhancement output {asset_id:?} has no sound"));
            }
            return None;
        }
    };
    if video.is_hidden {
        omit(
            omissions,
            OmissionScope::Feature,
            format!("layer {} ({:?})", video.id, video.name),
            "hidden video's embedded audio is not exported; whether the FX renderer mutes hidden layers is unverified",
        );
        return None;
    }
    // A retimed picture exports as native speed or reverse; sound is exported at unit speed only.
    if !matches!(video.playback.mapping(),
        fx_schema::LayerPlaybackMapping::Linear { input, output } if input.duration == output.duration
    ) || video.source.time_remap.is_some()
    {
        omit(
            omissions,
            OmissionScope::Feature,
            format!("layer {} ({:?})", video.id, video.name),
            "embedded audio of a retimed video layer was not exported",
        );
        return None;
    }
    let facts = match sound {
        SourceSound::Supported(facts) | SourceSound::PaddedToPicture { stream: facts, .. } => facts,
        SourceSound::Unsupported(reason) => {
            omit(
                omissions,
                OmissionScope::Feature,
                format!("layer {} ({:?})", video.id, video.name),
                format!("embedded audio was not exported: {reason}"),
            );
            return None;
        }
    };
    let record = format!("layer {} ({:?})", video.id, video.name);
    // Shared inspection is per asset, but the authored duration belongs to
    // each use. The selected sound must not borrow the selected picture's
    // duration; a short AAC stream may use only its own inspected picture pad.
    let file_ticks = match sound {
        SourceSound::PaddedToPicture { file_ticks, .. } => *file_ticks,
        _ => facts.intrinsic_ticks,
    };
    let duration_matches = [facts.intrinsic_ticks, file_ticks]
        .into_iter()
        .any(|ticks| {
            time_from_ticks(ticks).is_ok_and(|duration| {
                duration.as_millis() == video.source_intrinsic_duration.as_millis()
            })
        });
    if !duration_matches {
        omit(omissions, OmissionScope::Feature, &record,
            format!("embedded audio was not exported: sourceIntrinsicDuration {} ms differs from selected sound {asset_id:?} duration ({} ticks; file {file_ticks} ticks)",
                video.source_intrinsic_duration.as_millis(), facts.intrinsic_ticks));
        return None;
    }
    let mapped = match super::timing::linear_source_range(&video.playback) {
        Ok(range) => range,
        Err(error) => {
            omit(
                omissions,
                OmissionScope::Occurrence,
                &record,
                error.to_string(),
            );
            return None;
        }
    };
    if mapped.start < video.source_range.start || mapped.end() > video.source_range.end() {
        omit(
            omissions,
            OmissionScope::Occurrence,
            &record,
            "embedded audio playback window extends beyond the authored source selection",
        );
        return None;
    }
    let exported = occurrence(
        asset_id,
        &video.playback.input_range(),
        &mapped,
        1.0,
        |active_range, source_in| {
            exported_volume(
                volume_track,
                active_range,
                source_in,
                volume,
                &record,
                omissions,
            )
        },
        facts,
    );
    match exported {
        // Keep admitted positive-base sound when failed keys force zero gain.
        Ok(occurrence)
            if occurrence.volume_keys.is_none()
                && occurrence.volume.as_f64() <= 0.0
                && volume.as_f64() <= 0.0 =>
        {
            omit(
                omissions,
                OmissionScope::Feature,
                record,
                "embedded audio was not exported: without its volume animation it is silent",
            );
            None
        }
        Ok(occurrence) => Some((occurrence, facts)),
        Err(error) => {
            omit(
                omissions,
                OmissionScope::Feature,
                record,
                format!("embedded audio was not exported: {error}"),
            );
            None
        }
    }
}

/// One sound placement. `clip_volume` receives the active range and the
/// source In and returns the clip Volume; it runs only for a placement that
/// has valid geometry, so no source/range failure follows its key diagnostics.
/// Static gain is already a validated LinearGain; the closure supplies it.
fn occurrence(
    asset_id: &str,
    active_range: &TimeRangeProperty,
    source_range: &TimeRangeProperty,
    playback_rate: f64,
    clip_volume: impl FnOnce(&TimeRangeProperty, i64) -> ExportedVolume,
    facts: &PrAudioStream,
) -> Result<PrAudioOccurrence> {
    ensure!(
        playback_rate != 1.0 || source_range.duration == active_range.duration,
        "retimed audio is unsupported; sourceRange.duration {} ms must equal activeRange.duration {} ms",
        source_range.duration.as_millis(),
        active_range.duration.as_millis()
    );
    let start_ticks = ticks_from_time(active_range.start, "activeRange.start")?;
    let source_start = ticks_from_time(source_range.start, "sourceRange.start")?;
    let source_end = ticks_from_time(source_range.end(), "sourceRange.end")?;
    let (in_ticks, out_ticks) = if playback_rate < 0.0 {
        (
            facts.intrinsic_ticks - source_end,
            facts.intrinsic_ticks - source_start,
        )
    } else {
        (source_start, source_end)
    };
    let duration_ticks = ticks_from_time(
        Time::from_millis(active_range.duration.as_millis()),
        "activeRange.duration",
    )?;
    let end = |start: i64| {
        start
            .checked_add(duration_ticks)
            .ok_or_else(|| unsupported("audio range exceeds Premiere's tick range"))
    };
    let mut occurrence = PrAudioOccurrence {
        source_channel: None,
        preserve_audio_pitch: false,
        playback_rate,
        id: None,
        media: MediaId(asset_id.to_owned()),
        start_ticks,
        end_ticks: end(start_ticks)?,
        in_ticks,
        out_ticks,
        volume: LinearGain::UNITY,
        volume_keys: None,
        fade_in: None,
        fade_out: None,
    };
    occurrence.validate(facts)?;
    let exported = clip_volume(active_range, in_ticks);
    occurrence.volume = exported.volume;
    occurrence.volume_keys = exported.keys;
    occurrence.fade_in = exported.fade_in;
    occurrence.fade_out = exported.fade_out;
    occurrence.validate(facts)?;
    Ok(occurrence)
}

#[cfg(test)]
#[path = "audio/source_duration_tests.rs"]
mod source_duration_tests;

#[cfg(test)]
mod tests {
    use super::{fade_keys, fitted_level_easing, written_level, VolumeKey};
    use crate::{
        schema::{PrFadeCurve, PrKeyframeEasing, TICKS_PER_MILLISECOND},
        tests::support::premiere_linear_gain,
        OmissionKind,
    };
    use fx_schema::{
        animator::{PropertyKeyframe, PropertyKeyframeEasing, PropertyKeyframeTrack},
        KeyframeId, LinearGain, PropertyValue, TimeOffset,
    };

    /// The largest dB difference between the fitted easing and Premiere's
    /// curve wherever Premiere's gain is above -60 dB.
    fn max_error_db(from: f64, to: f64) -> f64 {
        let PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = fitted_level_easing(from, to)
        else {
            panic!("a Linear volume segment gets a fitted cubic easing");
        };
        assert_eq!((x1, x2), (1.0 / 3.0, 2.0 / 3.0));
        assert!((0.0..=1.0).contains(&y1) && (0.0..=1.0).contains(&y2));
        (1..4000)
            .map(|index| f64::from(index) / 4000.0)
            .filter_map(|t| {
                let expected = premiere_linear_gain(from, to, t);
                // x1 = 1/3 and x2 = 2/3 make x(t) = t, so the progress is y(t).
                let progress =
                    3.0 * (1.0 - t).powi(2) * t * y1 + 3.0 * (1.0 - t) * t * t * y2 + t.powi(3);
                (expected >= 1e-3)
                    .then(|| (20.0 * ((from + (to - from) * progress) / expected).log10()).abs())
            })
            .fold(0.0, f64::max)
    }

    #[test]
    fn audio_clock_reverse_cubic_and_hold_reflect_incoming_segments() {
        let keys = vec![
            VolumeKey {
                millis: 0,
                gain: 0.25,
                easing: PropertyKeyframeEasing::Linear,
            },
            VolumeKey {
                millis: 100,
                gain: 1.0,
                easing: PropertyKeyframeEasing::CubicBezier {
                    x1: 0.2,
                    y1: 0.3,
                    x2: 0.7,
                    y2: 0.9,
                },
            },
            VolumeKey {
                millis: 200,
                gain: 0.5,
                easing: PropertyKeyframeEasing::Hold,
            },
        ];
        let mut notes = Vec::new();
        let reflected = super::reverse_volume_keys(keys, 500, "reverse", &mut notes).unwrap();
        assert_eq!(
            reflected
                .iter()
                .map(|key| (key.millis, key.gain))
                .collect::<Vec<_>>(),
            [(300, 0.5), (301, 1.0), (400, 1.0), (500, 0.25)]
        );
        assert_eq!(reflected[1].easing, PropertyKeyframeEasing::Hold);
        let PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = reflected[3].easing else {
            panic!("reflected cubic")
        };
        for (actual, expected) in [(x1, 0.3), (y1, 0.1), (x2, 0.8), (y2, 0.7)] {
            assert!((actual - expected).abs() < 1e-12);
        }
        assert_eq!(notes.len(), 1);
        assert!(notes[0].reason.contains("one source millisecond"));
    }

    #[test]
    fn fitted_volume_easing_follows_premieres_fader_curve() {
        let gain = |db: f64| 10_f64.powf(db / 20.0);
        // The bounds of the conversion-accuracy ledger.
        assert!(max_error_db(1.0, gain(-20.0)) <= 0.01);
        assert!(max_error_db(gain(-60.0), 1.0) <= 0.25);
        assert!(max_error_db(1.0, 0.0) <= 1.5);
        assert!(max_error_db(gain(-60.0), gain(15.0)) <= 4.03);
        // The edges of the 0.25 dB region (both keys within -60..+3 dB) and of
        // the 1.5 dB region (silence and a key at +6 dB or lower), both ways.
        for (from, to) in [(gain(-60.0), gain(3.0)), (gain(3.0), gain(-60.0))] {
            let error = max_error_db(from, to);
            assert!(error <= 0.25, "{from} to {to}: {error} dB");
        }
        for (from, to) in [(0.0, gain(6.0)), (gain(6.0), 0.0)] {
            let error = max_error_db(from, to);
            assert!(error <= 1.5, "{from} to {to}: {error} dB");
        }
        // A flat segment needs no curve.
        assert_eq!(
            fitted_level_easing(0.5, 0.5),
            PropertyKeyframeEasing::Linear
        );
    }

    /// A volume key as (layer milliseconds, gain, easing that arrives at it).
    #[test]
    fn refined_volume_keys_preserve_stages_holds_and_timing_limits() {
        use PropertyKeyframeEasing::{Hold, Linear};
        let peak = 10_f64.powf(6.0 / 20.0);
        let keys = vec![
            VolumeKey {
                millis: -500,
                gain: 0.0,
                easing: Linear,
            },
            VolumeKey {
                millis: 3000,
                gain: peak * 0.5,
                easing: fitted_level_easing(0.0, peak),
            },
            VolumeKey {
                millis: 3500,
                gain: 0.1,
                easing: Hold,
            },
        ];
        let (refined, limited) = super::refine_level_keys(keys.clone(), 0.5, None).unwrap();
        assert!(!limited);
        assert!(refined.len() > keys.len());
        for key in &keys {
            let found = refined
                .iter()
                .find(|found| found.millis == key.millis)
                .unwrap();
            assert_eq!(found.gain, key.gain);
        }
        assert_eq!(refined.last(), keys.last());
        let played: Vec<_> = refined
            .iter()
            .map(|key| (key.millis, key.gain, key.easing))
            .collect();
        let expected = super::fader_gain(super::fader_position(peak) * 0.5) * 0.5;
        assert!((20.0 * (fx_gain(&played, 1250.0) / expected).log10()).abs() <= 0.01);
        assert_eq!(fx_gain(&played, 3250.0), peak * 0.5);

        let short = vec![
            keys[0],
            VolumeKey {
                millis: -499,
                ..keys[1]
            },
        ];
        let (refined, limited) = super::refine_level_keys(short.clone(), 0.5, None).unwrap();
        assert!(limited);
        assert_eq!(refined, short);
    }

    type Key = (i64, f64, PropertyKeyframeEasing);

    /// The FX gain at `ms`. Unlike export, which probes the curve parameter,
    /// this bisects a cubic Bézier's x for the time.
    fn fx_gain(keys: &[Key], ms: f64) -> f64 {
        let next = keys.iter().position(|key| key.0 as f64 >= ms).unwrap();
        let (end, to, easing) = keys[next];
        if next == 0 || end as f64 == ms {
            return to;
        }
        let (start, from, _) = keys[next - 1];
        let t = (ms - start as f64) / (end - start) as f64;
        let bezier = |p1: f64, p2: f64, s: f64| {
            3.0 * (1.0 - s).powi(2) * s * p1 + 3.0 * (1.0 - s) * s * s * p2 + s.powi(3)
        };
        let progress = match easing {
            PropertyKeyframeEasing::Hold => 0.0,
            PropertyKeyframeEasing::Linear => t,
            PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
                let (mut low, mut high) = (0.0, 1.0);
                for _ in 0..100 {
                    let middle = (low + high) / 2.0;
                    if bezier(x1, x2, middle) < t {
                        low = middle;
                    } else {
                        high = middle;
                    }
                }
                bezier(y1, y2, (low + high) / 2.0)
            }
        };
        (from + (to - from) * progress).max(0.0)
    }

    /// Exports FX keys with a 250 ms source In and unity static volume, as
    /// written keys (layer milliseconds, gain, easing) and reported reasons.
    fn export(keys: &[Key]) -> (Vec<(i64, f64, PrKeyframeEasing)>, Vec<String>) {
        let track = PropertyKeyframeTrack::new(
            keys.iter()
                .enumerate()
                .map(|(index, (ms, gain, easing))| {
                    PropertyKeyframe::new(
                        KeyframeId::new(format!("key-{index}")),
                        TimeOffset::from_millis(*ms),
                        PropertyValue::Float(*gain),
                        *easing,
                    )
                })
                .collect(),
        )
        .unwrap();
        let source_in = 250 * TICKS_PER_MILLISECOND;
        let mut omissions = Vec::new();
        let fx_keys: Vec<_> = track.keyframes().iter().collect();
        let written = written_level(&fx_keys, LinearGain::UNITY, None)
            .unwrap()
            .volume_keys(source_in, "layer", &mut omissions)
            .unwrap();
        assert_eq!(written.gain, 1.0);
        let keys = written
            .keys
            .iter()
            .map(|key| {
                let offset = key.source_ticks - source_in;
                assert_eq!(
                    offset % TICKS_PER_MILLISECOND,
                    0,
                    "off the millisecond grid"
                );
                (offset / TICKS_PER_MILLISECOND, key.value, key.easing)
            })
            .collect();
        (
            keys,
            omissions
                .into_iter()
                .map(|item| {
                    assert_eq!(item.kind, OmissionKind::Approximated);
                    item.reason
                })
                .collect(),
        )
    }

    /// Pieces between written keys, as (start ms, end ms, largest dB error).
    type PieceErrors = Vec<(i64, i64, f64)>;

    /// The largest dB difference between `played` and the FX curve `fx` on
    /// each piece between `times`, wherever the FX gain is at or above -60 dB,
    /// from 400 times per piece.
    fn piece_errors(times: &[i64], played: impl Fn(f64) -> f64, fx: &[Key]) -> PieceErrors {
        times
            .windows(2)
            .map(|pair| {
                let error = (1..400)
                    .map(|index| pair[0] as f64 + (pair[1] - pair[0]) as f64 * index as f64 / 400.0)
                    .filter(|ms| fx_gain(fx, *ms) >= 1e-3)
                    .map(|ms| (20.0 * (played(ms) / fx_gain(fx, ms)).log10()).abs())
                    .fold(0.0, f64::max);
                (pair[0], pair[1], error)
            })
            .collect()
    }

    #[test]
    fn exported_volume_keys_follow_the_fx_curve() {
        use PropertyKeyframeEasing::{Hold, Linear};
        let cubic = |x1, y1, x2, y2| PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 };
        // Imported curves that reach +6 dB, as in the pinned keys case. Once
        // the peak moves into Clip Gain, their Levels are no longer the ones
        // that were fitted.
        let (level, peak) = (0.100_000_002_096_970_82, 1.995_262_376_351_628_4);
        // FX keys, the keys export adds, and whether a one-millisecond piece
        // still misses the tolerance.
        let cases: [(&str, Vec<Key>, usize, bool); 10] = [
            (
                "1 s fade in",
                vec![(0, 0.0, Linear), (1000, 1.0, Linear)],
                12,
                true,
            ),
            (
                "2 s fade out",
                vec![(0, 1.0, Linear), (2000, 0.0, Linear)],
                12,
                false,
            ),
            (
                "100 ms fade in",
                vec![(0, 0.0, Linear), (100, 1.0, Linear)],
                8,
                true,
            ),
            (
                "0 to -20 dB",
                vec![(0, 1.0, Linear), (1000, 0.1, Linear)],
                3,
                false,
            ),
            // Clip Gain takes the +12 dB peak.
            (
                "-6 to +12 dB",
                vec![(0, 0.5, Linear), (1000, 4.0, Linear)],
                3,
                false,
            ),
            // Vertical in time at 500 ms.
            (
                "steep narrow cubic",
                vec![(0, 0.1, Linear), (1000, 1.0, cubic(1.0, 0.0, 0.0, 1.0))],
                12,
                true,
            ),
            // Below silence, where the mixer plays none, and above 0 dB.
            (
                "overshooting cubic",
                vec![(0, 0.1, Linear), (1000, 1.0, cubic(0.3, -0.4, 0.7, 1.4))],
                9,
                false,
            ),
            (
                "hold, then a 1 ms rise",
                vec![(0, 1.0, Linear), (500, 0.25, Hold), (501, 1.0, Linear)],
                0,
                true,
            ),
            (
                "imported -20 to +6 dB",
                vec![
                    (0, level, Linear),
                    (80, peak, fitted_level_easing(level, peak)),
                ],
                3,
                false,
            ),
            (
                "imported silence to +6 dB",
                vec![
                    (0, 0.0, Linear),
                    (3500, peak, fitted_level_easing(0.0, peak)),
                ],
                3,
                false,
            ),
        ];
        for (name, fx, added, grid_limited) in cases {
            let (written, reasons) = export(&fx);
            let (native, reimport) = written_errors(&fx, &written);
            assert_eq!(written.len() - fx.len(), added, "{name}");
            for key in &fx {
                assert!(
                    written
                        .iter()
                        .any(|written| (written.0, written.1) == (key.0, key.1)),
                    "{name}: key {key:?} was not kept"
                );
            }
            // Pieces the grid can divide meet the tolerance on a denser,
            // independent grid. Reimport fits each piece again, which adds at
            // most the fit's bound: 0.25 dB, or 1.5 dB from or to silence.
            let quiet = |ms: i64| written.iter().any(|key| key.0 == ms && key.1 < 1e-3);
            for ((start, end, native), (_, _, reimport)) in native.iter().zip(&reimport) {
                let fit_bound = if quiet(*start) || quiet(*end) {
                    1.5
                } else {
                    0.25
                };
                let reimport_bound = 0.25 + fit_bound;
                assert!(
                    (*native <= 0.25 && *reimport <= reimport_bound) || end - start == 1,
                    "{name}: {start}..{end} ms: native {native} dB, reimport {reimport} dB"
                );
            }
            let misses = native.iter().filter(|piece| piece.2 > 0.25).count();
            assert_eq!(misses > 0, grid_limited, "{name}: {native:?}");
            assert_eq!(
                reasons.len(),
                usize::from(grid_limited),
                "{name}: {reasons:?}"
            );
            assert!(reasons.iter().all(|reason| reason.starts_with(
                "volume curve approximated, not removed: Premiere's Linear pieces follow the FX curve within 0.25 dB"
            ) && reason.contains(&format!(" {misses} one-millisecond piece(s) "))));
        }

        // The reproduced defect: this fade's midpoint came back at 0.192.
        let fade = [(0, 0.0, Linear), (1000, 1.0, Linear)];
        let reimported = reimported_keys(&export(&fade).0);
        let midpoint = fx_gain(&reimported, 500.0);
        assert!(
            (20.0 * (midpoint / 0.5).log10()).abs() <= 0.25,
            "{midpoint}"
        );

        // An imported curve keeps its one native Linear segment, although it
        // follows Premiere's curve only within the fit's 1.5 dB near silence.
        let imported = [(0, 0.0, Linear), (1000, 1.0, fitted_level_easing(0.0, 1.0))];
        let (written, reasons) = export(&imported);
        assert_eq!(written.len(), 2);
        assert!(reasons.is_empty(), "{reasons:?}");
        assert_eq!(reimported_keys(&written), imported);
        let (native, _) = written_errors(&imported, &written);
        assert!(native[0].2 > 0.25 && native[0].2 <= 1.5, "{native:?}");
    }

    /// The Clip Gain that the writer splits off written keys: their peak
    /// above 0 dB.
    fn written_clip_gain(written: &[(i64, f64, PrKeyframeEasing)]) -> f64 {
        written.iter().map(|key| key.1).fold(1.0, f64::max)
    }

    /// The FX keys that reimport builds from written keys: each Linear
    /// segment gets the fit of its Levels again.
    fn reimported_keys(written: &[(i64, f64, PrKeyframeEasing)]) -> Vec<Key> {
        let clip_gain = written_clip_gain(written);
        written
            .iter()
            .enumerate()
            .map(|(index, (ms, gain, easing))| {
                let easing = match (index.checked_sub(1), easing) {
                    (Some(_), PrKeyframeEasing::Hold) => PropertyKeyframeEasing::Hold,
                    (Some(previous), _) => {
                        fitted_level_easing(written[previous].1 / clip_gain, gain / clip_gain)
                    }
                    (None, _) => PropertyKeyframeEasing::Linear,
                };
                (*ms, *gain, easing)
            })
            .collect()
    }

    /// Per written piece, the largest dB difference from the FX curve of
    /// Premiere's playback (Levels under the written Clip Gain, on the full
    /// fader curve) and of the curve that reimport fits.
    fn written_errors(
        fx: &[Key],
        written: &[(i64, f64, PrKeyframeEasing)],
    ) -> (PieceErrors, PieceErrors) {
        let clip_gain = written_clip_gain(written);
        let played = |ms: f64| {
            let next = written.iter().position(|key| key.0 as f64 >= ms).unwrap();
            let ((start, from, _), (end, to, easing)) = (written[next - 1], written[next]);
            let t = (ms - start as f64) / (end - start) as f64;
            match easing {
                PrKeyframeEasing::Hold => from,
                _ => clip_gain * premiere_linear_gain(from / clip_gain, to / clip_gain, t),
            }
        };
        let reimported = reimported_keys(written);
        let times: Vec<_> = written.iter().map(|key| key.0).collect();
        (
            piece_errors(&times, played, fx),
            piece_errors(&times, |ms| fx_gain(&reimported, ms), fx),
        )
    }

    /// Premiere's fade-in gain, from the curves that run A5 fitted to AME
    /// renders, written out here rather than taken from the conversion.
    fn premiere_fade_gain(curve: PrFadeCurve, progress: f64) -> f64 {
        match curve {
            PrFadeCurve::ConstantGain => progress,
            PrFadeCurve::ConstantPower => (std::f64::consts::PI / 2.0 * progress.powf(0.6457))
                .sin()
                .powi(2),
            PrFadeCurve::ExponentialFade => (3.5 * progress).exp_m1() / 3.5_f64.exp_m1(),
            PrFadeCurve::Custom(shape) => {
                let power = 10.0_f64.powf(-f64::from(shape.value()) / 100.0);
                (std::f64::consts::FRAC_PI_2 * progress.powf(power))
                    .sin()
                    .powi(2)
            }
        }
    }

    /// The gain of FX keys at `millis`, for their Linear and x1 = 1/3,
    /// x2 = 2/3 cubic easing.
    fn eased_gain(keys: &[VolumeKey], millis: f64) -> f64 {
        let next = keys
            .iter()
            .position(|key| key.millis as f64 >= millis)
            .unwrap()
            .max(1);
        let (from, to) = (keys[next - 1], keys[next]);
        let t = (millis - from.millis as f64) / (to.millis - from.millis) as f64;
        let progress = match to.easing {
            PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
                assert_eq!((x1, x2), (1.0 / 3.0, 2.0 / 3.0));
                3.0 * (1.0 - t).powi(2) * t * y1 + 3.0 * (1.0 - t) * t * t * y2 + t.powi(3)
            }
            PropertyKeyframeEasing::Linear => t,
            PropertyKeyframeEasing::Hold => panic!("fades have no Hold"),
        };
        from.gain + (to.gain - from.gain) * progress
    }

    /// The largest dB difference between a fade's keys over `0..span` ms and
    /// Premiere's curve, wherever that is above -60 dB of the fade's level.
    fn fade_error_db(curve: PrFadeCurve, fade_in: bool, span: i64) -> f64 {
        let level = 0.5;
        let keys = fade_keys(curve, fade_in, 0, span, level).unwrap();
        assert_eq!(keys.len(), 2 + super::fade_fractions(curve).len());
        let edges = if fade_in { (0.0, level) } else { (level, 0.0) };
        assert_eq!((keys[0].gain, keys[keys.len() - 1].gain), edges);
        (1..4000)
            .map(|index| f64::from(index) / 4000.0)
            .filter_map(|tau| {
                let progress = if fade_in { tau } else { 1.0 - tau };
                let expected = level * premiere_fade_gain(curve, progress);
                let actual = eased_gain(&keys, tau * span as f64);
                (expected >= level * 1e-3).then(|| (20.0 * (actual / expected).log10()).abs())
            })
            .fold(0.0, f64::max)
    }

    #[test]
    fn fade_keys_follow_premieres_curves_within_the_bound() {
        // Constant Gain is linear gain: one Linear segment is exact.
        for span in [1, 267, 2000] {
            for fade_in in [true, false] {
                let error = fade_error_db(PrFadeCurve::ConstantGain, fade_in, span);
                assert!(error < 1e-9, "{span} ms: {error} dB");
            }
        }
        // The shortest spans (39 ms is the worst of a sweep of every span to
        // 3 s), the fixtures' fades, and a long one.
        for (curve, spans) in [
            (
                PrFadeCurve::ConstantPower,
                [39, 40, 100, 115, 267, 749, 2000],
            ),
            (
                PrFadeCurve::ExponentialFade,
                [2, 3, 100, 115, 267, 749, 2000],
            ),
        ] {
            for span in spans {
                for fade_in in [true, false] {
                    let error = fade_error_db(curve, fade_in, span);
                    assert!(error <= 0.25, "{curve:?} {span} ms: {error} dB");
                }
            }
        }
        // A fade-out plays the fade-in backwards.
        let fade_in = fade_keys(PrFadeCurve::ConstantPower, true, 0, 749, 1.0).unwrap();
        let fade_out = fade_keys(PrFadeCurve::ConstantPower, false, 0, 749, 1.0).unwrap();
        for millis in (1..749).map(f64::from) {
            let (a, b) = (
                eased_gain(&fade_in, millis),
                eased_gain(&fade_out, 749.0 - millis),
            );
            assert!((a - b).abs() <= 1e-9 * a.max(b), "{millis} ms: {a} {b}");
        }
    }

    #[test]
    fn short_fades_coarsen_inner_keys_without_moving_endpoints() {
        let custom = PrFadeCurve::Custom(crate::schema::CustomFadeShape::new(0).unwrap());
        for curve in PrFadeCurve::ALL.into_iter().chain([custom]) {
            for span in [1, 2, 16, 38, 39] {
                for fade_in in [true, false] {
                    let keys = fade_keys(curve, fade_in, 10, 10 + span, 0.5).unwrap();
                    assert_eq!(
                        (keys[0].millis, keys.last().unwrap().millis),
                        (10, 10 + span)
                    );
                    assert!(keys.windows(2).all(|pair| pair[0].millis < pair[1].millis));
                    let expected = if fade_in { (0.0, 0.5) } else { (0.5, 0.0) };
                    assert_eq!((keys[0].gain, keys.last().unwrap().gain), expected);
                }
            }
            assert!(fade_keys(curve, true, 10, 10, 1.0).is_none());
        }
    }

    #[test]
    fn edge_fade_reference_only_tightens_attenuated_precision() {
        assert_eq!(super::reference_floor(1.0), 1e-3);
        assert_eq!(super::reference_floor(0.5), 5e-4);
        assert_eq!(super::reference_floor(2.0), 1e-3);
        assert_eq!(super::reference_floor(0.0), 1e-3);
        let curve = super::FxVolumeCurve {
            from: 0.0,
            to: 8e-4,
            easing: PropertyKeyframeEasing::Linear,
        };
        let span = ((0.0, 0.0), (1.0, 8e-4));
        assert_eq!(curve.piece_error(span.0, span.1, 1.0), 0.0);
        assert!(curve.piece_error(span.0, span.1, 0.5) > 0.27);
        assert_eq!(
            fitted_level_easing(0.0, 8e-4),
            PropertyKeyframeEasing::Linear
        );
        assert!(matches!(
            super::fitted_level_easing_at_reference(0.0, 8e-4, 0.5),
            PropertyKeyframeEasing::CubicBezier { .. }
        ));
    }

    #[test]
    fn nonfade_volume_keeps_absolute_precision() {
        let keys: Vec<_> = [(0, 4e-4), (1000, 8e-4)]
            .into_iter()
            .enumerate()
            .map(|(index, (time, gain))| {
                PropertyKeyframe::new(
                    KeyframeId::new(format!("ordinary-{index}")),
                    TimeOffset::from_millis(time),
                    PropertyValue::Float(gain),
                    PropertyKeyframeEasing::Linear,
                )
            })
            .collect();
        let fades = super::edge_fades(&keys, 1000, LinearGain::UNITY).unwrap();
        let super::ClipLevel::Keyed(written) = fades.level else {
            panic!("ordinary Volume remains keyed")
        };
        assert_eq!(written.keys.len(), 2);
        assert!(written.misses.is_empty());
        assert_eq!((written.keys[0].1, written.keys[1].1), (4e-4, 8e-4));
    }

    #[test]
    fn custom_fade_keys_follow_the_calibrated_power_family() {
        for shape in [-23, -19, -6, 0, 11, 29] {
            let curve = PrFadeCurve::Custom(crate::schema::CustomFadeShape::new(shape).unwrap());
            let error = fade_error_db(curve, false, 67);
            assert!(error <= 0.25, "outgoing shape {shape}, 67 ms: {error} dB");
        }
        for shape in [-23, -6, 0, 29] {
            let curve = PrFadeCurve::Custom(crate::schema::CustomFadeShape::new(shape).unwrap());
            for span in [39, 2000] {
                let error = fade_error_db(curve, true, span);
                assert!(
                    error <= 0.25,
                    "incoming shape {shape}, {span} ms: {error} dB"
                );
            }
        }
    }
}
