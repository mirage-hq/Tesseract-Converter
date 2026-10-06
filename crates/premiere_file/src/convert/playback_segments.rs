//! Editable native speed/hold segments on one selected-window clock.

use super::timing::ticks_from_time;
use crate::{
    error::{ensure, unsupported, Result},
    schema::{FrameRate, PrTimeRemap, PrVideoOccurrence, PrVideoStream, TICKS_PER_MILLISECOND},
};
use fx_schema::{
    animator::PropertyKeyframeEasing, TimeRemapExtrapolation, TimeRemapProperty, VideoLayer,
};

/// One native clip on the sequence grid, referring to the original physical media.
#[derive(Debug, Clone, Copy)]
pub(super) struct Segment {
    start: i64,
    end: i64,
    source_in: i64,
    source_out: i64,
    rate: f64,
    held: bool,
}

impl Segment {
    fn hold(start: i64, end: i64, source: i128, intrinsic_ticks: i64) -> Result<Self> {
        ensure!(end > start && intrinsic_ticks > 0, "empty held segment");
        let source_in = source.clamp(0, i128::from(intrinsic_ticks - 1)) as i64;
        let source_out = source_in
            .checked_add(end - start)
            .ok_or_else(|| unsupported("held segment exceeds the native tick range"))?;
        Ok(Self {
            start,
            end,
            source_in,
            source_out,
            rate: 1.0,
            held: true,
        })
    }

    pub(super) fn apply(self, template: &PrVideoOccurrence) -> PrVideoOccurrence {
        PrVideoOccurrence {
            id: None,
            start_ticks: self.start,
            end_ticks: self.end,
            in_ticks: self.source_in,
            out_ticks: self.source_out,
            playback_rate: self.rate,
            time_remap: self
                .held
                .then(|| PrTimeRemap::frame_hold(self.source_in, self.end - self.start)),
            ..template.clone()
        }
    }
}

/// One authored affine source clock, in rational milliseconds per sequence frame.
/// Derived pieces share this clock instead of fitting lines through rounded ticks.
#[derive(Clone, Copy)]
struct SourceClock {
    origin: i128,
    increment: i128,
    denominator: i128,
}

impl SourceClock {
    fn ticks(self, frame: i64, ceil: bool) -> i128 {
        let numerator = self.origin + self.increment * i128::from(frame);
        let millis = i128::from(TICKS_PER_MILLISECOND);
        let fraction = numerator.rem_euclid(self.denominator) * millis;
        numerator.div_euclid(self.denominator) * millis
            + fraction / self.denominator
            + i128::from(ceil && fraction % self.denominator != 0)
    }

    fn rate(self, sequence_step: i64) -> Result<f64> {
        let overflow = || unsupported("source-clock rate exceeds the native range");
        let numerator = self
            .increment
            .checked_mul(i128::from(TICKS_PER_MILLISECOND))
            .ok_or_else(overflow)?;
        let denominator = self
            .denominator
            .checked_mul(i128::from(sequence_step))
            .ok_or_else(overflow)?;
        // Reduce before floating conversion, so large common tick factors do
        // not introduce a second rounding into an otherwise small exact ratio.
        let (mut divisor, mut remainder) = (numerator.abs(), denominator);
        while remainder != 0 {
            (divisor, remainder) = (remainder, divisor % remainder);
        }
        Ok((numerator / divisor) as f64 / (denominator / divisor) as f64)
    }
}

/// Predict the preceding source-grid frame in the plan's ideal boundary model.
/// `source / denominator` is the reflected source time in ticks, not sequence frames.
/// Native sampling and Out clamping can differ; this is not an Adobe decoder contract.
pub(super) fn native_reverse_frame(
    source: i128,
    denominator: i128,
    lower_ticks: i128,
    frame_ticks: i64,
) -> Result<i64> {
    ensure!(
        denominator > 0 && frame_ticks > 0,
        "invalid reverse source clock"
    );
    let previous = source.div_euclid(denominator * i128::from(frame_ticks)) - 1;
    i64::try_from(
        previous
            .max(lower_ticks.div_euclid(i128::from(frame_ticks)))
            .max(0),
    )
    .map_err(|_| unsupported("reverse source frame exceeds the native range"))
}

/// Approximate FX's reverse source boundary with editable native playback.
/// Endpoint Frame Holds retain the plan's held samples and avoid negative native In
/// at the media end. Source-frame equality is not guaranteed by this ideal model.
/// This is at most three editable clips, not one clip/key per rendered frame.
pub(super) fn reverse(
    clip: &PrVideoOccurrence,
    source: &PrVideoStream,
    sequence_rate: FrameRate,
) -> Result<Vec<Segment>> {
    ensure!(
        clip.playback_rate < 0.0 && clip.time_remap.is_none(),
        "expected constant reverse playback"
    );
    ensure!(
        (0..source.intrinsic_ticks).contains(&clip.in_ticks)
            && clip.out_ticks <= source.intrinsic_ticks,
        "invalid timeline/source ranges"
    );
    let step = sequence_rate.ticks_per_frame();
    let duration = clip
        .end_ticks
        .checked_sub(clip.start_ticks)
        .ok_or_else(|| unsupported("reverse placement exceeds the native tick range"))?;
    ensure!(
        duration > 0 && duration % step == 0,
        "reverse placement must use whole sequence frames"
    );
    let frames = duration / step;
    let span = clip
        .out_ticks
        .checked_sub(clip.in_ticks)
        .ok_or_else(|| unsupported("reverse source window exceeds the native tick range"))?;
    ensure!(
        span > 0 && source.intrinsic_ticks > 0,
        "reverse source window must be nonempty"
    );
    let clock = SourceClock {
        origin: (i128::from(source.intrinsic_ticks) - i128::from(clip.in_ticks))
            * i128::from(frames),
        increment: -i128::from(span),
        denominator: i128::from(frames) * i128::from(TICKS_PER_MILLISECOND),
    };
    reverse_clock(clip, source, sequence_rate, clock, 0, frames)
}

fn reverse_clock(
    clip: &PrVideoOccurrence,
    source: &PrVideoStream,
    sequence_rate: FrameRate,
    clock: SourceClock,
    begin: i64,
    end: i64,
) -> Result<Vec<Segment>> {
    let step = sequence_rate.ticks_per_frame();
    let source_step = source.frame_rate.ticks_per_frame();
    let last_media_frame = (source.intrinsic_ticks - 1) / source_step;
    let selected = |frame| {
        // ceil(source/source_frame) - 1 is the FX left-limit. Ceil to the
        // tick lattice before subtracting its exclusive final tick.
        ((clock.ticks(frame, true) - 1).div_euclid(i128::from(source_step)))
            .clamp(0, i128::from(last_media_frame)) as i64
    };
    let first = selected(begin);
    let last = selected(end - 1);
    let hold = |begin: i64, end: i64, frame: i64| -> Result<Segment> {
        let start = clip.start_ticks + begin * step;
        let end = clip.start_ticks + end * step;
        Segment::hold(
            start,
            end,
            i128::from(frame) * i128::from(source_step),
            source.intrinsic_ticks,
        )
    };
    if first == last {
        return Ok(vec![hold(begin, end, first)?]);
    }
    let first_at_or_below = |target| first_frame(begin, end, |frame| selected(frame) <= target);
    // A full source-frame interval minus its exclusive final tick converts
    // floor(x)-1 to ceil(x)-1 on the integer native tick lattice. This is
    // determined by source FPS, never by the sequence grid or observed error.
    let boundary = source_step - 1;
    let native_at = |frame| -> Result<i64> {
        // Native playback subtracts a truncated elapsed source time. Floor
        // the reflected origin so an exact interior boundary stays on its
        // left side; do not round again from an earlier piece's stored In.
        let ticks =
            i128::from(source.intrinsic_ticks) - clock.ticks(frame, false) - i128::from(boundary);
        i64::try_from(ticks)
            .map_err(|_| unsupported("reverse segment exceeds the native tick range"))
    };
    let head_end = if native_at(begin)? < 0 {
        first_at_or_below(first - 1)
    } else {
        begin
    };
    let tail_start = if native_reverse_frame(
        clock.ticks(end - 1, true) + i128::from(boundary),
        1,
        clock.ticks(end, false) + i128::from(boundary),
        source_step,
    )? != last
    {
        first_at_or_below(last)
    } else {
        end
    };
    let mut segments = Vec::with_capacity(3);
    if head_end > begin {
        segments.push(hold(begin, head_end, first)?);
    }
    if head_end < tail_start {
        let source_in = native_at(head_end)?;
        let source_out = native_at(tail_start)?;
        ensure!(
            source_in >= 0 && source_out > source_in,
            "invalid corrected reverse source window"
        );
        segments.push(Segment {
            start: clip.start_ticks + head_end * step,
            end: clip.start_ticks + tail_start * step,
            source_in,
            source_out,
            rate: clock.rate(step)?,
            held: false,
        });
    }
    if tail_start < end {
        segments.push(hold(tail_start, end, last)?);
    }
    Ok(segments)
}

/// Prefer the established constant, whole-clip hold and exact native-ramp paths.
/// The fallback retains authored legs as editable clips instead of omitting them.
pub(super) fn segmented_remap(video: &VideoLayer) -> Option<&TimeRemapProperty> {
    use super::tesseract_to_premiere::{constant_playback_source_range, held_source};
    let curve = video.playback.time_remap()?;
    let keys = curve.keyframes();
    let first = keys.first()?;
    let last = keys.last()?;
    if keys.len() < 2
        || held_source(video).is_some()
        || super::time_remap::native_ramp(video).is_some()
        || constant_playback_source_range(
            curve,
            video.playback.input_range(),
            video.source_range,
            video.playback.input_offset_ms(),
        )
        .is_ok()
    {
        return None;
    }
    let start = i128::from(video.playback.input_range().start.as_millis())
        + i128::from(video.playback.input_offset_ms());
    let end = start + i128::from(video.playback.input_range().duration.as_millis());
    let extendable = |mode| {
        matches!(
            mode,
            TimeRemapExtrapolation::Hold | TimeRemapExtrapolation::Continue
        )
    };
    let covered = (start >= i128::from(first.time.as_millis()) || extendable(curve.before()))
        && (end <= i128::from(last.time.as_millis()) || extendable(curve.after()));
    let can_hold = |millis: u64| {
        video.source_range.start.as_millis() <= millis
            && millis < video.source_range.end().as_millis()
            && millis < video.source_intrinsic_duration.as_millis()
    };
    let holds_selected = keys.windows(2).all(|pair| {
        (pair[1].easing != PropertyKeyframeEasing::Hold && pair[0].value != pair[1].value)
            || can_hold(pair[0].value.as_millis())
    }) && (start >= i128::from(first.time.as_millis())
        || curve.before() != TimeRemapExtrapolation::Hold
        || can_hold(first.value.as_millis()))
        && (end <= i128::from(last.time.as_millis())
            || !(curve.after() == TimeRemapExtrapolation::Hold
                || (curve.after() == TimeRemapExtrapolation::Continue
                    && last.easing == PropertyKeyframeEasing::Hold))
            || can_hold(last.value.as_millis()));
    (covered
        && holds_selected
        && end > start
        && keys.iter().all(|key| {
            key.value >= video.source_range.start
                && key.value <= video.source_range.end()
                && key.value.as_millis() <= video.source_intrinsic_duration.as_millis()
                && ticks_from_time(key.time, "remap input").is_ok()
                && ticks_from_time(key.value, "remap source").is_ok()
        }))
    .then_some(curve)
}

// The predicate must change only from false to true over the frame interval.
fn first_frame(mut begin: i64, mut end: i64, predicate: impl Fn(i64) -> bool) -> i64 {
    while begin < end {
        let middle = begin + (end - begin) / 2;
        if predicate(middle) {
            end = middle;
        } else {
            begin = middle + 1;
        }
    }
    begin
}

/// Split authored legs on the signed input clock, leaving ineligible samples empty.
/// Cuts move to the first sequence sample at or after a key. Cubic legs use
/// their endpoint line; endpoint holds retain valid samples at media-edge cuts.
pub(super) fn segmented(
    video: &VideoLayer,
    clip: &PrVideoOccurrence,
    source: &PrVideoStream,
    sequence_rate: FrameRate,
) -> Result<Option<Vec<Segment>>> {
    let Some(curve) = segmented_remap(video) else {
        return Ok(None);
    };
    let step = sequence_rate.ticks_per_frame();
    let duration = clip.end_ticks - clip.start_ticks;
    ensure!(
        duration > 0 && duration % step == 0 && source.intrinsic_ticks > 0,
        "invalid segmented placement"
    );
    let frames = duration / step;
    let input_start = (i128::from(video.playback.input_range().start.as_millis())
        + i128::from(video.playback.input_offset_ms()))
        * i128::from(TICKS_PER_MILLISECOND);
    let input_duration = i128::from(video.playback.input_range().duration.as_millis())
        * i128::from(TICKS_PER_MILLISECOND);
    let frame_at = |input: i64| {
        let numerator = (i128::from(input) - input_start) * i128::from(frames);
        (-(-numerator).div_euclid(input_duration)).clamp(0, i128::from(frames)) as i64
    };
    let timeline_at = |frame| clip.start_ticks + frame * step;
    let source_lower =
        i128::from(video.source_range.start.as_millis()) * i128::from(TICKS_PER_MILLISECOND);
    let source_upper = (i128::from(video.source_range.end().as_millis())
        * i128::from(TICKS_PER_MILLISECOND))
    .min(i128::from(source.intrinsic_ticks));
    let can_hold = |point| source_lower <= point && point < source_upper;
    let keys = curve.keyframes();
    let mut segments = Vec::new();
    let first = &keys[0];
    let before_end = frame_at(ticks_from_time(first.time, "first remap key")?);
    if before_end > 0
        && curve.before() == TimeRemapExtrapolation::Hold
        && can_hold(i128::from(ticks_from_time(
            first.value,
            "held remap source",
        )?))
    {
        segments.push(Segment::hold(
            clip.start_ticks,
            timeline_at(before_end),
            i128::from(ticks_from_time(first.value, "held remap source")?),
            source.intrinsic_ticks,
        )?);
    }
    for (index, pair) in keys.windows(2).enumerate() {
        let from_time = ticks_from_time(pair[0].time, "remap key")?;
        let to_time = ticks_from_time(pair[1].time, "remap key")?;
        let begin = if index == 0 && curve.before() == TimeRemapExtrapolation::Continue {
            0
        } else {
            frame_at(from_time)
        };
        let end = if index == keys.len() - 2
            && curve.after() == TimeRemapExtrapolation::Continue
            && pair[1].easing != PropertyKeyframeEasing::Hold
        {
            frames
        } else {
            frame_at(to_time)
        };
        if begin == end {
            continue;
        }
        let from = i128::from(ticks_from_time(pair[0].value, "remap source")?);
        let to = i128::from(ticks_from_time(pair[1].value, "remap source")?);
        let start_ticks = timeline_at(begin);
        let end_ticks = timeline_at(end);
        if pair[1].easing == PropertyKeyframeEasing::Hold || from == to {
            if can_hold(from) {
                segments.push(Segment::hold(
                    start_ticks,
                    end_ticks,
                    from,
                    source.intrinsic_ticks,
                )?);
            }
            continue;
        }
        // Keep the line rational in milliseconds until selecting a native tick.
        // Early input-tick rounding can move a sample across a selection edge.
        let millis = i128::from(TICKS_PER_MILLISECOND);
        let from_ms = from / millis;
        let delta_ms = (to - from) / millis;
        let overflow = || unsupported("segmented source clock exceeds the native range");
        let denominator = i128::from((to_time - from_time) / TICKS_PER_MILLISECOND)
            .checked_mul(i128::from(frames))
            .ok_or_else(overflow)?;
        let origin = (input_start / millis - i128::from(from_time) / millis)
            .checked_mul(i128::from(frames))
            .and_then(|offset| delta_ms.checked_mul(offset))
            .and_then(|offset| from_ms.checked_mul(denominator)?.checked_add(offset))
            .ok_or_else(overflow)?;
        let increment = delta_ms
            .checked_mul(input_duration / millis)
            .ok_or_else(overflow)?;
        increment
            .checked_mul(i128::from(frames))
            .and_then(|span| origin.checked_add(span))
            .ok_or_else(overflow)?;
        let clock = SourceClock {
            origin,
            increment,
            denominator,
        };
        let backwards = to < from;
        // Eligibility needs floor for right limits and ceil for left limits.
        let source_at = |frame| clock.ticks(frame, backwards);
        let (begin, end) = if backwards {
            (
                first_frame(begin, end, |frame| source_at(frame) <= source_upper),
                first_frame(begin, end, |frame| source_at(frame) <= source_lower),
            )
        } else {
            (
                first_frame(begin, end, |frame| source_at(frame) >= source_lower),
                first_frame(begin, end, |frame| source_at(frame) >= source_upper),
            )
        };
        if begin >= end {
            continue;
        }
        let hold = |begin, end| {
            let period = i128::from(source.frame_rate.ticks_per_frame());
            let selected = (source_at(begin) - i128::from(backwards)).div_euclid(period) * period;
            Segment::hold(
                timeline_at(begin),
                timeline_at(end),
                selected,
                source.intrinsic_ticks,
            )
        };
        if end - begin == 1 {
            segments.push(hold(begin, end)?);
            continue;
        }
        // Only the exclusive endpoint may lie past the physical media. Keep
        // the preceding line unchanged and hold its final eligible sample.
        let run_end =
            if (0..=i128::from(source.intrinsic_ticks)).contains(&clock.ticks(end, !backwards)) {
                end
            } else {
                end - 1
            };
        if backwards {
            segments.extend(reverse_clock(
                clip,
                source,
                sequence_rate,
                clock,
                begin,
                run_end,
            )?);
        } else {
            // Native playback truncates elapsed ticks before adding In. Ceil
            // the forward origin to retain exact interior frame boundaries.
            let start_source = i64::try_from(clock.ticks(begin, true)).map_err(|_| overflow())?;
            let end_source = i64::try_from(clock.ticks(run_end, true)).map_err(|_| overflow())?;
            if start_source == end_source {
                segments.push(hold(begin, run_end)?);
            } else {
                segments.push(Segment {
                    start: timeline_at(begin),
                    end: timeline_at(run_end),
                    source_in: start_source,
                    source_out: end_source,
                    rate: clock.rate(step)?,
                    held: false,
                });
            }
        }
        if run_end < end {
            segments.push(hold(run_end, end)?);
        }
    }
    let last = &keys[keys.len() - 1];
    let after_start = frame_at(ticks_from_time(last.time, "last remap key")?);
    if after_start < frames
        && (curve.after() == TimeRemapExtrapolation::Hold
            || (curve.after() == TimeRemapExtrapolation::Continue
                && last.easing == PropertyKeyframeEasing::Hold))
        && can_hold(i128::from(ticks_from_time(
            last.value,
            "held remap source",
        )?))
    {
        segments.push(Segment::hold(
            timeline_at(after_start),
            clip.end_ticks,
            i128::from(ticks_from_time(last.value, "held remap source")?),
            source.intrinsic_ticks,
        )?);
    }
    Ok(Some(segments))
}
