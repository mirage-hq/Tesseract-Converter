//! Sound placements of editable audio layers and of audible video layers, and
//! the clip Volume keys of both.
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
    schema::{PrAudioOccurrence, PrAudioStream, PrKeyframeEasing, PrScalarKeyframe, PrVolumeKeys},
    {approximate, omit, OmissionScope},
};
use fx_schema::{
    animator::{PropertyKeyframeEasing, PropertyKeyframeTrack},
    AnimationGraph, AudioLayer, LayerId, LinearGain, PropType, PropertyValue, Time,
    TimeRangeProperty, VideoLayer,
};
use std::collections::BTreeMap;

/// Exponent of Premiere's fader curve, measured on AME renders: a Linear clip
/// Volume segment moves linearly in fader position u, with u = g^p for gains
/// up to 0 dB and u = 2 - g^-p above.
const FADER_EXPONENT: f64 = 0.4475;
/// Curve comparisons ignore reference gains below -60 dB: Premiere's gain when
/// import fits an easing, the FX gain when export checks a piece.
const REFERENCE_FLOOR: f64 = 1e-3;
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

/// The FX easing of a Linear clip Volume segment between the Level gains
/// `from` and `to` (1.0 = 0 dB), before other stages multiply them.
///
/// Premiere moves such a segment linearly in fader position, not in gain or
/// dB. A cubic Bézier with x1 = 1/3 and x2 = 2/3 makes the progress a cubic
/// polynomial in time. Its y1 and y2 stay in [0, 1], so the gain never leaves
/// the keys' range, and minimize the largest dB error above -60 dB: a nested
/// golden-section search, which the quasi-convex error admits.
pub(super) fn fitted_level_easing(from: f64, to: f64) -> PropertyKeyframeEasing {
    let (start, end) = (fader_position(from), fader_position(to));
    let samples: Vec<_> = (0..FIT_SAMPLES)
        .filter_map(|index| {
            let t = (index as f64 + 0.5) / FIT_SAMPLES as f64;
            let gain = fader_gain(start + (end - start) * t);
            (gain >= REFERENCE_FLOOR).then(|| {
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
    fn piece_error(self, start: (f64, f64), end: (f64, f64)) -> f64 {
        let (low, high) = (self.parameter(start.0), self.parameter(end.0));
        let mut parameters: Vec<f64> = (0..=EXPORT_PROBES)
            .map(|index| low + (high - low) * index as f64 / EXPORT_PROBES as f64)
            .chain(self.turns(low, high))
            .collect();
        parameters.sort_by(f64::total_cmp);
        let error = |s: f64| {
            let (time, gain) = self.at(s);
            if gain < REFERENCE_FLOOR {
                return 0.0;
            }
            let progress = ((time - start.0) / (end.0 - start.0)).clamp(0.0, 1.0);
            (20.0 * (exported_linear_gain(start.1, end.1, progress) / gain).log10()).abs()
        };
        let quiet = |s: f64| self.at(s).1 < REFERENCE_FLOOR;
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
fn is_imported_fit(curve: FxVolumeCurve, clip_gain: f64) -> bool {
    let PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = curve.easing else {
        return false;
    };
    (x1, x2) == (1.0 / 3.0, 2.0 / 3.0)
        && matches!(
            fitted_level_easing(curve.from / clip_gain, curve.to / clip_gain),
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
        let error = |ms: i64| curve.piece_error(start_point, point(ms));
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

/// The current FX `AudioVolume` keys as clip Volume keys on the source clock.
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
/// written Levels keep. A track with a one-millisecond piece that misses the
/// tolerance is reported, once, as approximated.
fn volume_keys(
    track: &PropertyKeyframeTrack,
    source_in: i64,
    volume: LinearGain,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Result<PrVolumeKeys> {
    let fx_keys = track.keyframes();
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
    let key = |millis: i64, value: f64, easing: PrKeyframeEasing| -> Result<PrScalarKeyframe> {
        Ok(PrScalarKeyframe {
            source_ticks: keyframes::source_ticks(source_in, millis)?,
            value,
            easing,
        })
    };
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
                if !is_imported_fit(curve, clip_gain) {
                    let start_ms = fx_keys[previous].layer_time().as_millis();
                    for (inner_ms, gain) in inner_keys(curve, start_ms, millis, &mut misses)? {
                        keys.push(key(inner_ms, gain, PrKeyframeEasing::Linear)?);
                    }
                }
                PrKeyframeEasing::Linear
            }
        };
        keys.push(key(millis, gains[index], easing)?);
    }
    if let Some((first_ms, _)) = misses.first() {
        let worst = misses.iter().map(|(_, error)| *error).fold(0.0, f64::max);
        approximate(
            omissions,
            record,
            format!(
                "volume curve approximated, not removed: Premiere's Linear pieces follow the FX curve within {EXPORT_TOLERANCE_DB} dB above -60 dB (sampled) except in {} one-millisecond piece(s) from layer time {first_ms} ms, which differ by up to {worst:.2} dB; clip Volume keys cannot be closer than 1 ms",
                misses.len()
            ),
        );
    }
    Ok(PrVolumeKeys { keys, gain: 1.0 })
}

/// The clip Volume keys of an optional FX track for a placement whose source
/// In is `source_in`, or `None` after reporting why its volume animation was
/// not exported.
fn exported_volume_keys(
    track: Option<&PropertyKeyframeTrack>,
    source_in: i64,
    volume: LinearGain,
    record: &str,
    omissions: &mut dyn OmissionSink,
) -> Option<PrVolumeKeys> {
    volume_keys(track?, source_in, volume, record, omissions)
        .map_err(|error| {
            omit(
                omissions,
                OmissionScope::Feature,
                record,
                format!("volume animation was not exported: {error}"),
            );
        })
        .ok()
}

/// Maps one audio layer of the list whose parent is `parent`, with its volume
/// keys, and the inspected sound of its source. Returns `None` after reporting
/// a layer that cannot be exported. A volume animation that cannot be exported
/// is reported, and the sound keeps its static volume.
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
    // Native audio clips have no speed. A layer that `playback` retimes is
    // omitted; a retime without `playback` rejects in `occurrence`.
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
            sound.preserve_audio_pitch,
            ExportField::AudioPitchPreservation,
        ),
        (
            sound.source.enhancement.is_some(),
            ExportField::AudioEnhancement,
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
    let asset_id = sound.source.asset_id.as_str();
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
        ensure!(
            authored == intrinsic_millis || authored == file_millis,
            "sourceIntrinsicDuration {authored} ms differs from the packaged audio duration {file_millis} ms"
        );
        if retimed {
            omit(
                omissions,
                OmissionScope::Occurrence,
                &record,
                "retimed audio layer was not exported",
            );
            return Ok(None);
        }
        let mapped = super::timing::linear_source_range(&sound.playback)?;
        ensure!(mapped.start >= sound.source_range.start && mapped.end() <= sound.source_range.end(),
            "audio playback window extends beyond the authored source selection");
        occurrence(
            asset_id,
            &sound.playback.input_range(),
            &mapped,
            sound.volume,
            |source_in| {
                exported_volume_keys(volume_track, source_in, sound.volume, &record, omissions)
            },
            facts,
        )
        .map(|occurrence| Some((occurrence, facts)))
    })()
    .map_err(|source| BuildError::Context {
        context: record,
        source: Box::new(source),
    })
}

/// The asset whose sound [`embedded`] can export for the clip video `video`:
/// its source, when a positive static volume or keys on its volume make it
/// audible and no enabled Eye Contact replaces its footage. Media inspection
/// reads the sound of these clips only. It counts every volume animation,
/// also one that export then reports as not exported.
pub(crate) fn embedded_sound_asset<'v>(
    video: &'v VideoLayer,
    dynamics: &AnimationGraph,
) -> Option<&'v str> {
    let keyed = super::tesseract_to_premiere::layer_animations(dynamics, video.id)
        .any(|(property, _)| property == PropType::AudioVolume);
    let audible = video.volume.is_some_and(|gain| gain.as_f64() > 0.0) || keyed;
    let replaced = video
        .source
        .eye_contact
        .as_ref()
        .is_some_and(|eye_contact| eye_contact.enabled);
    (audible && !replaced).then_some(video.source.asset_id.as_str())
}

/// Whether the clip video `video` plays its sound on export: a positive
/// volume, or the volume keys `volume_track`, which play from a silent base.
pub(super) fn audible(video: &VideoLayer, volume_track: Option<&PropertyKeyframeTrack>) -> bool {
    video.volume.is_some_and(|gain| gain.as_f64() > 0.0) || volume_track.is_some()
}

/// The sound of an [`audible`] video layer whose source has sound
/// ([`embedded_sound_asset`]). A sound that cannot be exported is reported;
/// the picture is still exported. So is a sound whose volume animation cannot
/// be exported and whose static volume is silent.
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
    if video
        .source
        .eye_contact
        .as_ref()
        .is_some_and(|eye_contact| eye_contact.enabled)
    {
        omit(
            omissions,
            OmissionScope::Feature,
            format!("layer {} ({:?})", video.id, video.name),
            "embedded sound of an Eye Contact clip is not exported: the original asset is not packaged and the replacement's audio is unverified",
        );
        return None;
    }
    let asset_id = video.source.asset_id.as_str();
    let sound = audio_facts.get(asset_id)?;
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
        volume,
        |source_in| exported_volume_keys(volume_track, source_in, volume, &record, omissions),
        facts,
    );
    match exported {
        Ok(occurrence) if occurrence.volume_keys.is_none() && volume.as_f64() <= 0.0 => {
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

/// One sound placement. `volume_keys` receives the source In and returns the
/// clip Volume keys, if any; it runs only for a placement that is valid
/// without them, so that no other failure follows its diagnostics.
fn occurrence(
    asset_id: &str,
    active_range: &TimeRangeProperty,
    source_range: &TimeRangeProperty,
    volume: LinearGain,
    volume_keys: impl FnOnce(i64) -> Option<PrVolumeKeys>,
    facts: &PrAudioStream,
) -> Result<PrAudioOccurrence> {
    ensure!(
        source_range.duration == active_range.duration,
        "retimed audio is unsupported; sourceRange.duration {} ms must equal activeRange.duration {} ms",
        source_range.duration.as_millis(),
        active_range.duration.as_millis()
    );
    let start_ticks = ticks_from_time(active_range.start, "activeRange.start")?;
    let in_ticks = ticks_from_time(source_range.start, "sourceRange.start")?;
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
        id: None,
        media: MediaId(asset_id.to_owned()),
        start_ticks,
        end_ticks: end(start_ticks)?,
        in_ticks,
        out_ticks: end(in_ticks)?,
        volume,
        volume_keys: None,
    };
    occurrence.validate(facts)?;
    occurrence.volume_keys = volume_keys(in_ticks);
    occurrence.validate(facts)?;
    Ok(occurrence)
}

#[cfg(test)]
mod tests {
    use super::{fitted_level_easing, volume_keys};
    use crate::{
        schema::{PrKeyframeEasing, TICKS_PER_MILLISECOND},
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
        let written = volume_keys(
            &track,
            source_in,
            LinearGain::UNITY,
            "layer",
            &mut omissions,
        )
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
}
