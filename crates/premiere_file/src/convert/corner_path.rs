//! A master clip's Corner Pin whose one keyed corner moves on a curved spatial
//! path, as straight Linear keys within a certified error bound.
//!
//! The traversal hypothesis is that Premiere moves a point with Linear keys at
//! a constant speed along its spatial Bézier path, measuring length in
//! normalized frame coordinates. The pinned
//! `premiere_isolated_source_effects_26_5` save supports it and no more: it
//! stores that speed with each key (the interval's path length over its
//! duration), and its Upper Left read back at source 1 s (115.5, 48.6 px) is
//! that traversal's point (115.48, 48.56), not the cubic parameter's (103.7,
//! 43.7). No Adobe render measures the traversal: the bound below holds
//! relative to it.
//!
//! FX keys each coordinate on its own, so the path converts as paired Linear
//! keys on points of it, at the times the traversal reaches them, keeping the
//! saved keys; export writes them as spatially linear native keys. Between two
//! keys a part of the path of length `ℓ` departs from its chord at the same
//! fraction `w` of both by at most `√(w(1 − w)(ℓ² − d²)) ≤ √(ℓ² − d²)/2`
//! for chord length `d` (Stewart: with `P` the point at fraction `w` and `C`
//! the chord's, `|P − C|² = (1 − w)|P − A|² + w|P − B|² − w(1 − w)d²`,
//! where `|P − A| ≤ wℓ` and `|P − B| ≤ (1 − w)ℓ`). A curve is no longer than
//! its control polygon, and no shorter than the sum of its finer chords, which
//! bound each part's length and the times at which the traversal reaches each
//! key. A frame's greatest side `M` turns normalized lengths into source
//! pixels. Key times round to whole milliseconds and FX evaluates at whole
//! milliseconds of its clock, so the error also takes the path's greatest
//! speed times those offsets, and the rounding of the corners that FX draws.
//!
//! The bound is certified over a numerical domain that [`straighten`] checks
//! before it builds anything ([`check_domain`]): frame sides of 1 to
//! [`MAX_FRAME_SIDE`] pixels, every corner, key and spatial control point
//! within [`MAX_COORDINATE`] of the frame's origin, and every key within
//! [`MAX_CLOCK_TICKS`] of the placement's source In. Over it each rounding of
//! the f64 arithmetic has an allowance: [`ROUNDING_STEPS`] and
//! [`BOUND_ROUNDING`] for lengths, fractions, roots and speeds,
//! [`KEY_TIME_ROUNDING_MILLIS`] for key times, [`TURN_MARGIN`] for the path's
//! quad and [`DRAWN_ROUNDING`] for the f32 pixel corners that FX draws, whose
//! quad the straight keys must keep convex ([`QuadCheck::holds_as_drawn`]).

use super::keyframes;
use crate::schema::{
    spatial::is_curved, turn_corners, turns, PrCornerPin, PrEffect, PrEffectParamAnimation,
    PrEffectParamKeys, PrEffectParams, PrKeyframeEasing, PrPointKeyframe, CORNER_PIN, TICKS,
    TICKS_PER_MILLISECOND,
};
use std::cmp::Ordering;

/// Keys of the approximated corner, the saved ones included.
const MAX_KEYS: usize = 64;

/// Bound on the corner's position error at every time, in source pixels.
const MAX_ERROR_PX: f64 = 0.5;

/// The part of [`MAX_ERROR_PX`] for the chords' departure from the path.
const SHAPE_BUDGET_PX: f64 = 0.2;

/// The part of [`MAX_ERROR_PX`] for key times and FX's evaluation clock.
const CLOCK_BUDGET_PX: f64 = 0.25;

/// The part of [`MAX_ERROR_PX`] for the rounding of the corners that FX
/// draws ([`DRAWN_ROUNDING`]); over the numerical domain that rounding is
/// under 0.012 px.
const VALUE_BUDGET_PX: f64 = 0.05;

/// Greatest side of a frame of the numerical domain, in pixels. A side up to
/// 2^13 is an exact f32, and it scales the drawn rounding
/// ([`DRAWN_ROUNDING`]).
const MAX_FRAME_SIDE: u32 = 8192;

/// Greatest magnitude, in frame units, of each coordinate of a corner, key or
/// spatial control point of the numerical domain: four frame widths or
/// heights from the origin. The paths' points that the certificate computes
/// lie within their control points, so every coordinate of it does, which
/// bounds the allowances below.
const MAX_COORDINATE: f64 = 4.0;

/// Greatest distance, in ticks, of a key from the placement's source In in
/// the numerical domain: 2^53, about 9.85 hours. A key's time from the source
/// In then converts to f64 exactly, and a span between two keys within a
/// tick ([`KEY_TIME_ROUNDING_MILLIS`]).
const MAX_CLOCK_TICKS: i128 = 1 << 53;

/// FX evaluates keys at whole milliseconds (`fx_schema::Time`): a frame's
/// time rounds to the nearest one, and so does the start of the placement's
/// layer (`timing::time_from_ticks`), so the layer clock is within 1 ms of
/// the placement's; the certificate allows that.
const RUNTIME_CLOCK_ALLOWANCE_SECONDS: f64 = 0.001;

/// Each curved interval's path is cut into 2^`FINE_DEPTH` pieces of equal
/// parameter, whose chords and control polygons bound its length; parts
/// halve down to one piece, within the approved depth limit of 20.
const FINE_DEPTH: u32 = 12;

/// Work limit: the pieces of every curved interval together, counted before
/// any is built.
const MAX_PIECES: usize = 16_384;

/// Relative outward rounding of each computed bound: lengths, fractions of a
/// length, shape and clock terms, speeds, drawn rounding and their total. Each
/// is a sum of at most 2^`FINE_DEPTH` correctly rounded nonnegative terms,
/// which errs by under 2^12·2^-53 < 5·10⁻¹³ relative, or a few correctly
/// rounded products, quotients, differences and roots of floats that are
/// already bounds, each of which errs by at most 2^-53 relative: 10⁻⁹ covers
/// either over a thousand times.
const BOUND_ROUNDING: f64 = 1e-9;

/// Steps of 2^-52·`X`, for a path's greatest control coordinate `X` (at
/// least 1), that bound the absolute f64 error of one piece's computed chord
/// or control polygon, or of a computed node, against the path of the control
/// points as computed (each a key plus its tangent, rounded once). A de
/// Casteljau level errs by at most 2.5 steps beyond its inputs (a difference
/// and a product of magnitude up to `2X`, a sum up to `X`), so a node's
/// coordinate by 7.5, and a control point of one of the 2^`FINE_DEPTH`
/// pieces by 8, as its Hermite step adds half a step. A point then errs by
/// 11.4 and an edge between two by 22.7 (a piece's edges are too short for
/// their own roundings to matter), so a piece's control polygon of three
/// edges by 68 steps and the chord between two nodes by 21.3: 128 are
/// allowed per piece, and twice that for a chord between nodes. The rounding
/// of the product that scales them falls within [`BOUND_ROUNDING`].
const ROUNDING_STEPS: f64 = 128.0;

/// Least magnitude of a quad turn at a control point that certifies its sign.
/// Over the numerical domain an edge's coordinate is under 8, and a computed
/// control point's coordinate is within 14·2^-52·4 of exact (a part's
/// Hermite step adds up to 6.5 steps to a node's 7.5), so a computed turn
/// errs by under 10⁻¹²: the margin is a thousand times that.
const TURN_MARGIN: f64 = 1e-9;

/// Allowance for the f64 error of a key time in milliseconds
/// ([`traversal_millis`]). Over the numerical domain a key's time from the
/// source In converts to f64 exactly and a span between two keys within a
/// tick; the bounds of a fraction of the span err by two roundings, and the
/// product and the sum by one each: at most 7 ticks more. The division into
/// milliseconds adds under 4·10⁻⁹ ms, so a key time errs by under
/// 4·10⁻⁸ ms, which this allows 25 times over.
const KEY_TIME_ROUNDING_MILLIS: f64 = 1e-6;

/// Rounding of a corner that FX draws, per pixel of frame side and unit of
/// [`MAX_COORDINATE`]: 2^-22. `fx_composition` draws a normalized corner
/// value `v` as `(v as f32) * (side as f32)` in f32, two roundings that err
/// by under 2^-23·side·|v| (a side up to [`MAX_FRAME_SIDE`] is an exact
/// f32). `v` is the FX document's value after its JSON round trip, which
/// serde_json's best-effort parse (the workspace has no `float_roundtrip`)
/// leaves within 4·2^-53·|v| of the written one; between two keys FX's f64
/// Linear interpolation adds under 7·2^-53·`X`; and a generated key errs from
/// its point of the path by 8 steps of [`ROUNDING_STEPS`]. Together they stay
/// under 2^-22·side·`X` for coordinates up to `X`, with room for over 2^27
/// more JSON round trips.
const DRAWN_ROUNDING: f64 = 1.0 / 4_194_304.0;

/// A Corner Pin whose keyed corner's curved path became straight keys, and
/// what its report states.
#[derive(Debug)]
pub(super) struct StraightenedPin {
    pub(super) effect: PrEffect,
    /// The keyed corner's Effect Controls name.
    pub(super) corner: &'static str,
    pub(super) saved_keys: usize,
    pub(super) keys: usize,
    /// The certified bound on the position error at every time, in source
    /// pixels.
    pub(super) bound_px: f64,
}

impl StraightenedPin {
    /// The approximation's report, for the effect named by `effect`, with its
    /// bound rounded up to the thousandth of a pixel.
    pub(super) fn report(&self, effect: &str) -> String {
        let thousandths = (self.bound_px * 1000.0 * (1.0 + BOUND_ROUNDING)).ceil() as u64;
        format!(
            "{effect}: {}'s curved spatial path through {} saved keys converts as {} straight Linear keys that keep them, within {}.{:03} source pixels at every time of the traversal hypothesized for Premiere: a constant speed along the saved path in normalized frame coordinates, which Premiere's saved key speeds and one readback support and no Adobe render measures; Premiere's spatial tangents, which it recomputes when a key moves, are not kept, and the bound holds for the imported keys only, not once they are edited",
            self.corner,
            self.saved_keys,
            self.keys,
            thousandths / 1000,
            thousandths % 1000
        )
    }
}

/// `effect` with the curved path of its one keyed corner as straight Linear
/// keys on the clock of a placement from `source_in`, or why it has none that
/// stay within [`MAX_ERROR_PX`] of the path in a `frame` of that many pixels.
/// `Ok(None)` when `effect` is no Corner Pin or no corner moves on a curve.
///
/// Converts only the saved form: one keyed corner, every key Linear, the other
/// corners static; the reader admits a curved path only from the key flags
/// that Premiere saved (`format::reader::animation::ensure_saved_curve_form`).
/// The pieces that the path needs are counted before any is built, and the
/// numerical domain of the bound is checked ([`check_domain`]). The path's
/// parts halve until each chord stays within [`SHAPE_BUDGET_PX`] of the path
/// and the quad, the other corners fixed, turns one way at every control
/// point of the part, which certifies it convex along the whole part, as a
/// turn is affine in the moving corner. Too many keys, pieces or halvings, a
/// value or time outside the domain, colliding key times or a quad that fails
/// either check reject the conversion. The straight keys must then keep the
/// quad convex as FX draws it, through the rounding of its f32 pixel corners
/// ([`QuadCheck::holds_as_drawn`]), and pass the existing quad check.
pub(super) fn straighten(
    effect: &PrEffect,
    frame: [u32; 2],
    source_in: i64,
) -> Result<Option<StraightenedPin>, String> {
    let PrEffectParams::CornerPin(pin) = effect.params else {
        return Ok(None);
    };
    let curved = effect.animations.iter().any(|animation| {
        animation
            .keys
            .point()
            .is_some_and(|keys| keys.windows(2).any(|pair| is_curved(&pair[0], &pair[1])))
    });
    if !curved {
        return Ok(None);
    }
    let [animation] = effect.animations.as_slice() else {
        return Err(format!(
            "{} corners are keyed and one moves on a curved spatial path; only the curved path of one keyed corner converts, as straight keys",
            effect.animations.len()
        ));
    };
    let (Some(saved), Some(corner)) = (
        animation.keys.point(),
        CORNER_PIN
            .params
            .iter()
            .position(|param| param.id == animation.param.id),
    ) else {
        return Err("the curved path is not a Corner Pin corner's".to_owned());
    };
    let label = animation.param.label;
    if let Some(key) = saved
        .iter()
        .skip(1)
        .find(|key| key.easing != PrKeyframeEasing::Linear)
    {
        return Err(format!(
            "{label} reaches its key at source time {:.3} s on Hold or Bezier temporal easing; only Linear keys along a curved spatial path convert, as straight keys",
            key.source_ticks as f64 / TICKS as f64
        ));
    }
    if saved.len() > MAX_KEYS {
        return Err(format!(
            "{label} has {} saved keys, more than the {MAX_KEYS} straight keys that its curved spatial path may convert as",
            saved.len()
        ));
    }
    // The work limit, before any piece is built.
    let curved_intervals = saved
        .windows(2)
        .filter(|pair| is_curved(&pair[0], &pair[1]))
        .count();
    let pieces = curved_intervals.saturating_mul(Curve::PIECES);
    if pieces > MAX_PIECES {
        return Err(format!(
            "{label}'s path has {curved_intervals} curved intervals, which would need {pieces} pieces to bound, more than the {MAX_PIECES} that its conversion may use"
        ));
    }
    check_domain(pin, saved, label, frame, source_in)?;
    let side = f64::from(frame[0].max(frame[1]));
    let curves: Vec<Option<Curve>> = saved
        .windows(2)
        .map(|pair| is_curved(&pair[0], &pair[1]).then(|| Cubic::between(&pair[0], &pair[1])))
        .map(|cubic| cubic.map(Curve::new).transpose())
        .collect::<Result<_, _>>()?;
    let quad = QuadCheck::new(pin, corner)?;
    // Each curved interval's parts: the end index and shape bound of each.
    let mut keys = saved.len();
    let mut splits = Vec::with_capacity(curves.len());
    for (pair, curve) in saved.windows(2).zip(&curves) {
        let mut parts = Vec::new();
        if let Some(curve) = curve {
            let context = Context {
                curve,
                quad: &quad,
                side,
                label,
                times: [pair[0].source_ticks, pair[1].source_ticks],
            };
            context.split([0, Curve::PIECES], &mut parts, &mut keys)?;
        }
        splits.push(parts);
    }
    let mut straight = Vec::with_capacity(keys);
    // Per key, its offset from the traversal's time, in seconds.
    let mut offsets = Vec::with_capacity(keys);
    for (index, key) in saved.iter().enumerate() {
        let millis = keyframes::layer_millis(key.source_ticks, source_in)
            .map_err(|error| error.to_string())?;
        offsets.push(seconds(
            ticks_from_millis(millis) - (i128::from(key.source_ticks) - i128::from(source_in)),
        ));
        straight.push(PrPointKeyframe {
            spatial_in_tangent: None,
            spatial_out_tangent: None,
            ..key.clone()
        });
        let (Some(Some(curve)), Some(parts)) = (curves.get(index), splits.get(index)) else {
            continue;
        };
        let times = [key.source_ticks, saved[index + 1].source_ticks];
        for &(end, _) in parts.iter().filter(|(end, _)| *end < Curve::PIECES) {
            let (millis, offset) = traversal_millis(curve.fraction(end), times, source_in)?;
            let source_ticks = i64::try_from(i128::from(source_in) + ticks_from_millis(millis))
                .map_err(|_| format!("{label} key time exceeds Premiere's tick range"))?;
            straight.push(PrPointKeyframe {
                source_ticks,
                value: curve.node(end),
                easing: PrKeyframeEasing::Linear,
                spatial_in_tangent: None,
                spatial_out_tangent: None,
            });
            offsets.push(offset);
        }
    }
    let millis: Vec<i64> = straight
        .iter()
        .map(|key| keyframes::layer_millis(key.source_ticks, source_in))
        .collect::<crate::error::Result<_>>()
        .map_err(|error| error.to_string())?;
    if let Some(pair) = millis.windows(2).find(|pair| pair[0] >= pair[1]) {
        return Err(format!(
            "two keys of {label}'s straightened path fall on one millisecond ({} ms on the clip clock); keys are never merged",
            pair[1]
        ));
    }
    let shape_px = splits
        .iter()
        .flatten()
        .map(|&(_, shape)| shape)
        .fold(0.0, f64::max);
    let speed = greatest_speed(saved, &curves, &straight, &millis, frame, side);
    let offset = offsets.iter().copied().map(f64::abs).fold(0.0, f64::max);
    let clock_px = speed * (offset + RUNTIME_CLOCK_ALLOWANCE_SECONDS) * (1.0 + BOUND_ROUNDING);
    if clock_px.is_nan() || clock_px > CLOCK_BUDGET_PX {
        return Err(format!(
            "{label}'s straightened path moves up to {speed:.1} px/s, so its key times, {:.3} ms off the traversal at most, and FX's millisecond clock allow {clock_px:.3} px, over the {CLOCK_BUDGET_PX} px part of the {MAX_ERROR_PX} px bound",
            offset * 1000.0
        ));
    }
    let value_px = drawn_error_px(frame);
    if value_px.is_nan() || value_px > VALUE_BUDGET_PX {
        return Err(format!(
            "the corners that FX draws on the clip frame round by up to {value_px:.3} px, over the {VALUE_BUDGET_PX} px part of the {MAX_ERROR_PX} px bound"
        ));
    }
    let bound_px = (shape_px + clock_px + value_px) * (1.0 + BOUND_ROUNDING);
    if bound_px.is_nan() || bound_px > MAX_ERROR_PX {
        return Err(format!(
            "{label}'s straightened path stays only within {bound_px:.3} source pixels of the path, over the {MAX_ERROR_PX} px bound"
        ));
    }
    for (pair, times) in straight.windows(2).zip(millis.windows(2)) {
        if let Err(turn) = quad.holds_as_drawn([pair[0].value, pair[1].value], frame) {
            return Err(format!(
                "{label}'s straight keys at {} ms and {} ms of the clip clock bring the quad's turn at {} within the rounding of the f32 pixel corners that FX draws, so FX may draw it flat or turned the other way",
                times[0],
                times[1],
                CORNER_PIN.params[turn_corners(turn)[1]].label
            ));
        }
    }
    let animations = vec![PrEffectParamAnimation {
        param: animation.param,
        keys: PrEffectParamKeys::Point(straight),
    }];
    pin.ensure_convex(&animations)
        .map_err(|reason| format!("{label}'s straightened path fails the quad check: {reason}"))?;
    Ok(Some(StraightenedPin {
        effect: PrEffect {
            mask: None,
            enabled: effect.enabled,
            params: PrEffectParams::CornerPin(pin),
            animations,
        },
        corner: label,
        saved_keys: saved.len(),
        keys,
        bound_px,
    }))
}

/// Checks that the numerical domain of the bound holds `pin`, whose corner
/// `label` moves by the `saved` keys, on a `frame` of a placement from
/// `source_in`: both frame sides from 1 to [`MAX_FRAME_SIDE`] pixels, every
/// key within [`MAX_CLOCK_TICKS`] of the source In, and every key, spatial
/// control point of the path and corner within [`MAX_COORDINATE`] of the
/// frame's origin on each axis. A nonfinite value lies outside it.
fn check_domain(
    pin: PrCornerPin,
    saved: &[PrPointKeyframe],
    label: &str,
    frame: [u32; 2],
    source_in: i64,
) -> Result<(), String> {
    const OUTSIDE: &str = "outside the numerical domain that the bound covers";
    if !frame.iter().all(|side| (1..=MAX_FRAME_SIDE).contains(side)) {
        return Err(format!(
            "the clip frame of {} by {} pixels is {OUTSIDE}: sides of 1 to {MAX_FRAME_SIDE} pixels",
            frame[0], frame[1]
        ));
    }
    let within = |point: [f64; 2]| {
        point
            .iter()
            .all(|coordinate| coordinate.abs() <= MAX_COORDINATE)
    };
    let seconds = |key: &PrPointKeyframe| key.source_ticks as f64 / TICKS as f64;
    for key in saved {
        if (i128::from(key.source_ticks) - i128::from(source_in)).abs() > MAX_CLOCK_TICKS {
            return Err(format!(
                "{label}'s key at source time {:.3} s is more than 2^53 ticks (about 9.85 hours) from the placement's source In, {OUTSIDE}",
                seconds(key)
            ));
        }
        if !within(key.value) {
            return Err(format!(
                "{label}'s key at source time {:.3} s lies at ({}, {}) in frame units, {OUTSIDE}: coordinates within \u{b1}{MAX_COORDINATE} of the frame's origin",
                seconds(key),
                key.value[0],
                key.value[1]
            ));
        }
    }
    for pair in saved.windows(2) {
        let [_, after, before, _] = Cubic::between(&pair[0], &pair[1]).0;
        for (point, side, key) in [(after, "after", &pair[0]), (before, "before", &pair[1])] {
            if !within(point) {
                return Err(format!(
                    "{label}'s spatial control point {side} its key at source time {:.3} s lies at ({}, {}) in frame units, {OUTSIDE}: coordinates within \u{b1}{MAX_COORDINATE} of the frame's origin",
                    seconds(key),
                    point[0],
                    point[1]
                ));
            }
        }
    }
    for (corner, param) in pin.corners.iter().zip(CORNER_PIN.params) {
        if !within(*corner) {
            return Err(format!(
                "the Corner Pin's {} lies at ({}, {}) in frame units, {OUTSIDE}: coordinates within \u{b1}{MAX_COORDINATE} of the frame's origin",
                param.label, corner[0], corner[1]
            ));
        }
    }
    Ok(())
}

/// A cubic Bézier path's control points, in normalized frame coordinates.
#[derive(Debug, Clone, Copy)]
struct Cubic([[f64; 2]; 4]);

impl Cubic {
    /// The spatial path from `start` to `end`: the two keys and, as control
    /// points, each moved by its tangent.
    fn between(start: &PrPointKeyframe, end: &PrPointKeyframe) -> Self {
        let outgoing = start.spatial_out_tangent.unwrap_or([0.0; 2]);
        let incoming = end.spatial_in_tangent.unwrap_or([0.0; 2]);
        Self([
            start.value,
            along(start.value, outgoing, 1.0),
            along(end.value, incoming, 1.0),
            end.value,
        ])
    }

    /// The point at parameter `u` (de Casteljau).
    fn point(&self, u: f64) -> [f64; 2] {
        let lerp = |from: [f64; 2], to: [f64; 2]| along(from, difference(to, from), u);
        let [p0, p1, p2, p3] = self.0;
        let [q0, q1, q2] = [lerp(p0, p1), lerp(p1, p2), lerp(p2, p3)];
        lerp(lerp(q0, q1), lerp(q1, q2))
    }

    /// The derivative at parameter `u`.
    fn derivative(&self, u: f64) -> [f64; 2] {
        let [p0, p1, p2, p3] = self.0;
        let (v, w) = (1.0 - u, u);
        std::array::from_fn(|axis| {
            3.0 * (v * v * (p1[axis] - p0[axis])
                + 2.0 * v * w * (p2[axis] - p1[axis])
                + w * w * (p3[axis] - p2[axis]))
        })
    }

    /// The control points of the part from parameter `from` to `to`, in
    /// Hermite form: the ends and the ends moved by a third of the part's
    /// derivative there, exact for a cubic.
    fn part(&self, from: f64, to: f64) -> Self {
        let third = (to - from) / 3.0;
        let [start, end] = [self.point(from), self.point(to)];
        Self([
            start,
            along(start, self.derivative(from), third),
            along(end, self.derivative(to), -third),
            end,
        ])
    }

    fn chord(&self) -> f64 {
        distance(self.0[0], self.0[3])
    }

    fn polygon(&self) -> f64 {
        self.0
            .windows(2)
            .map(|pair| distance(pair[0], pair[1]))
            .sum()
    }
}

/// A curved interval's path and the length bounds of its equal-parameter
/// pieces: each piece is no shorter than its chord and no longer than its
/// control polygon.
#[derive(Debug)]
struct Curve {
    cubic: Cubic,
    chords: Vec<f64>,
    polygons: Vec<f64>,
    /// Bound on the absolute f64 error of each computed piece length and node
    /// coordinate ([`ROUNDING_STEPS`]).
    slack: f64,
}

impl Curve {
    const PIECES: usize = 1 << FINE_DEPTH;

    fn new(cubic: Cubic) -> Result<Self, String> {
        let (chords, polygons): (Vec<f64>, Vec<f64>) = (0..Self::PIECES)
            .map(|index| {
                let piece = cubic.part(Self::parameter(index), Self::parameter(index + 1));
                (piece.chord(), piece.polygon())
            })
            .unzip();
        let magnitude = cubic
            .0
            .iter()
            .flatten()
            .fold(1.0, |greatest: f64, coordinate| {
                greatest.max(coordinate.abs())
            });
        let curve = Self {
            cubic,
            chords,
            polygons,
            slack: ROUNDING_STEPS * f64::EPSILON * magnitude,
        };
        let [shortest, longest] = curve.length(0, Self::PIECES);
        if !(shortest > 0.0 && longest.is_finite()) {
            return Err(format!(
                "a curved spatial path has no finite positive length (between {shortest} and {longest})"
            ));
        }
        Ok(curve)
    }

    /// The parameter at the start of piece `index`, exact: the pieces are a
    /// power of two.
    fn parameter(index: usize) -> f64 {
        index as f64 / Self::PIECES as f64
    }

    /// The path point at the start of piece `index`, as the key there takes
    /// it: the saved key values at the interval's ends.
    fn node(&self, index: usize) -> [f64; 2] {
        match index {
            0 => self.cubic.0[0],
            Self::PIECES => self.cubic.0[3],
            _ => self.cubic.point(Self::parameter(index)),
        }
    }

    /// Bounds of the path length over pieces `start..end`, rounded outward.
    fn length(&self, start: usize, end: usize) -> [f64; 2] {
        let chords: f64 = self.chords[start..end].iter().sum();
        let polygons: f64 = self.polygons[start..end].iter().sum();
        let slack = (end - start) as f64 * self.slack;
        [
            (chords * (1.0 - BOUND_ROUNDING) - slack).max(0.0),
            polygons * (1.0 + BOUND_ROUNDING) + slack,
        ]
    }

    /// Bounds of the fraction of the path's length before piece `index`,
    /// the fraction of the interval's time at which the traversal reaches it:
    /// a prefix over itself and a suffix, lowest with the least prefix and the
    /// greatest suffix. Their two roundings count in
    /// [`KEY_TIME_ROUNDING_MILLIS`].
    fn fraction(&self, index: usize) -> [f64; 2] {
        let [before_low, before_high] = self.length(0, index);
        let [after_low, after_high] = self.length(index, Self::PIECES);
        [
            before_low / (before_low + after_high),
            before_high / (before_high + after_low),
        ]
    }

    /// The departure in source pixels, on a frame of greatest side `side`, of
    /// the chord between the nodes at pieces `start` and `end` from the path
    /// between them, at every fraction of both.
    fn shape_bound(&self, start: usize, end: usize, side: f64) -> Result<f64, String> {
        let [_, longest] = self.length(start, end);
        // The chord between the path's points, which the nodes round.
        let chord = (distance(self.node(start), self.node(end)) * (1.0 - BOUND_ROUNDING)
            - 2.0 * self.slack)
            .max(0.0);
        let excess = (longest - chord) * (longest + chord);
        if excess.is_nan() || excess < 0.0 {
            return Err(format!(
                "a part of a curved spatial path is shorter than its chord ({longest} against {chord}), so its bound is uncertain"
            ));
        }
        Ok(side / 2.0 * excess.sqrt() * (1.0 + BOUND_ROUNDING))
    }
}

/// What splitting one curved interval needs.
struct Context<'a> {
    curve: &'a Curve,
    quad: &'a QuadCheck,
    side: f64,
    label: &'static str,
    /// The source times of the interval's keys.
    times: [i64; 2],
}

impl Context<'_> {
    /// Splits pieces `start..end` into parts whose chord stays within
    /// [`SHAPE_BUDGET_PX`] of the path and whose control points keep the quad
    /// convex, halving until one holds; each part's end and shape bound go to
    /// `parts`, and `keys` counts the keys so far.
    fn split(
        &self,
        [start, end]: [usize; 2],
        parts: &mut Vec<(usize, f64)>,
        keys: &mut usize,
    ) -> Result<(), String> {
        let shape = self.curve.shape_bound(start, end, self.side)?;
        let part = self
            .curve
            .cubic
            .part(Curve::parameter(start), Curve::parameter(end));
        let convex = self.quad.holds(&part.0);
        if shape <= SHAPE_BUDGET_PX && convex {
            if end < Curve::PIECES {
                *keys += 1;
                if *keys > MAX_KEYS {
                    return Err(format!(
                        "{}'s curved spatial path needs more than {MAX_KEYS} straight keys to stay within {MAX_ERROR_PX} source pixels",
                        self.label
                    ));
                }
            }
            parts.push((end, shape));
            return Ok(());
        }
        if end - start == 1 {
            let [from, to] = self.times.map(|ticks| ticks as f64 / TICKS as f64);
            let why = if convex {
                format!("its chord departs from it by up to {shape:.3} px")
            } else {
                "its quad is not certified convex there".to_owned()
            };
            return Err(format!(
                "{}'s curved spatial path between source times {from:.3} s and {to:.3} s has a part that its finest halving (2^{FINE_DEPTH} pieces) does not bound: {why}",
                self.label
            ));
        }
        let middle = (start + end) / 2;
        self.split([start, middle], parts, keys)?;
        self.split([middle, end], parts, keys)
    }
}

/// The quad of a Corner Pin with one corner free, and the way its saved
/// static quad turns.
#[derive(Debug)]
struct QuadCheck {
    corners: [[f64; 2]; 4],
    corner: usize,
    way: Ordering,
}

impl QuadCheck {
    fn new(pin: PrCornerPin, corner: usize) -> Result<Self, String> {
        let [first, rest @ ..] = turns(pin.corners).map(|turn| turn.partial_cmp(&0.0));
        let way = first
            .filter(|way| way.is_ne() && rest.iter().all(|other| *other == Some(*way)))
            .ok_or("the static corners form a degenerate or non-convex quad")?;
        Ok(Self {
            corners: pin.corners,
            corner,
            way,
        })
    }

    /// Whether the quad turns its way by more than [`TURN_MARGIN`] at every
    /// corner with the free corner at each of `points`.
    fn holds(&self, points: &[[f64; 2]]) -> bool {
        points.iter().all(|&point| {
            let mut corners = self.corners;
            corners[self.corner] = point;
            turns(corners)
                .into_iter()
                .all(|turn| turn.abs() > TURN_MARGIN && turn.partial_cmp(&0.0) == Some(self.way))
        })
    }

    /// Whether FX draws the quad strictly convex, turning its way, at every
    /// time of a straight segment of the free corner from `segment[0]` to
    /// `segment[1]` on a `frame`; otherwise the first turn ([`turns`]) that it
    /// may draw flat or the other way.
    ///
    /// FX draws every corner within [`drawn_radius`] of the exact pixels of
    /// its value on each axis, a static corner's, or the free corner's at a
    /// point of the segment, which FX interpolates between its two keys and
    /// holds before the first and after the last. With the other corners
    /// fixed, a turn is affine along the segment, and the change `E` that
    /// those roundings can make to it, a sum of absolute values of affine
    /// functions, is convex along it; so the drawn turn keeps the turn's sign
    /// at every time when the turn exceeds `2E` at both ends. The factor 2
    /// covers the f64 error of both, under 2^-16 of `E` over the numerical
    /// domain.
    fn holds_as_drawn(&self, segment: [[f64; 2]; 2], frame: [u32; 2]) -> Result<(), usize> {
        let scale = frame.map(f64::from);
        let [across, down] = drawn_radius(frame);
        let way = if self.way == Ordering::Greater {
            1.0
        } else {
            -1.0
        };
        for end in segment {
            let mut corners = self.corners;
            corners[self.corner] = end;
            let pixels = corners.map(|corner| [corner[0] * scale[0], corner[1] * scale[1]]);
            for turn in 0..4 {
                let [from, at, to] = turn_corners(turn).map(|index| pixels[index]);
                let (into, out) = (difference(at, from), difference(to, at));
                // Each corner moves by up to `across` and `down`, so each
                // edge's coordinates by twice that.
                let rounding = 2.0
                    * (down * into[0].abs()
                        + across * into[1].abs()
                        + across * out[1].abs()
                        + down * out[0].abs())
                    + 8.0 * across * down;
                let turned = way * (into[0] * out[1] - into[1] * out[0]);
                if turned.partial_cmp(&(2.0 * rounding)) != Some(Ordering::Greater) {
                    return Err(turn);
                }
            }
        }
        Ok(())
    }
}

/// The greatest distance in pixels, on each axis of a `frame`, between a
/// corner that FX draws and the exact pixels of its value
/// ([`DRAWN_ROUNDING`]): exact products.
fn drawn_radius(frame: [u32; 2]) -> [f64; 2] {
    frame.map(|side| DRAWN_ROUNDING * MAX_COORDINATE * f64::from(side))
}

/// The greatest distance in source pixels between a corner that FX draws on
/// a `frame` and the exact pixels of its value, rounded up.
fn drawn_error_px(frame: [u32; 2]) -> f64 {
    let [across, down] = drawn_radius(frame);
    across.hypot(down) * (1.0 + BOUND_ROUNDING)
}

/// The whole millisecond on a placement's clock from `source_in` that is
/// nearest the time at which the traversal of the interval between source
/// times `times` reaches `fraction` of its length, and its greatest offset in
/// seconds from any time within those bounds.
fn traversal_millis(
    fraction: [f64; 2],
    [start, end]: [i64; 2],
    source_in: i64,
) -> Result<(i64, f64), String> {
    let origin = (i128::from(start) - i128::from(source_in)) as f64;
    let duration = (i128::from(end) - i128::from(start)) as f64;
    let [low, high] =
        fraction.map(|fraction| (origin + duration * fraction) / TICKS_PER_MILLISECOND as f64);
    let nearest = ((low + high) / 2.0).round();
    if !(low.is_finite() && high.is_finite() && nearest.abs() < i64::MAX as f64 / 2.0) {
        return Err("a straightened key time is not a finite millisecond".to_owned());
    }
    // The f64 error of the millisecond bounds (`KEY_TIME_ROUNDING_MILLIS`).
    let offset = (nearest - low).abs().max((high - nearest).abs()) + KEY_TIME_ROUNDING_MILLIS;
    Ok((nearest as i64, offset / 1000.0))
}

/// The greatest speed in source pixels per second of the traversal of the
/// `saved` keys' path, whose curved intervals are `curves`, and of the
/// `straight` keys at `millis` on a clip clock.
fn greatest_speed(
    saved: &[PrPointKeyframe],
    curves: &[Option<Curve>],
    straight: &[PrPointKeyframe],
    millis: &[i64],
    frame: [u32; 2],
    side: f64,
) -> f64 {
    let traversal = saved
        .windows(2)
        .zip(curves)
        .map(|(pair, curve)| {
            let length = curve.as_ref().map_or_else(
                || distance(pair[0].value, pair[1].value),
                |curve| curve.length(0, Curve::PIECES)[1],
            );
            side * length
                / seconds(i128::from(pair[1].source_ticks) - i128::from(pair[0].source_ticks))
        })
        .fold(0.0, f64::max);
    let scale = frame.map(f64::from);
    let keyed = straight
        .windows(2)
        .zip(millis.windows(2))
        .map(|(pair, times)| {
            let moved = difference(pair[1].value, pair[0].value);
            (scale[0] * moved[0]).hypot(scale[1] * moved[1])
                / ((i128::from(times[1]) - i128::from(times[0])) as f64 / 1000.0)
        })
        .fold(0.0, f64::max);
    traversal.max(keyed)
}

fn ticks_from_millis(millis: i64) -> i128 {
    i128::from(millis) * i128::from(TICKS_PER_MILLISECOND)
}

fn seconds(ticks: i128) -> f64 {
    ticks as f64 / TICKS as f64
}

/// `start` moved by `amount` times `direction`.
fn along(start: [f64; 2], direction: [f64; 2], amount: f64) -> [f64; 2] {
    [
        start[0] + direction[0] * amount,
        start[1] + direction[1] * amount,
    ]
}

fn difference(to: [f64; 2], from: [f64; 2]) -> [f64; 2] {
    [to[0] - from[0], to[1] - from[1]]
}

fn distance(from: [f64; 2], to: [f64; 2]) -> f64 {
    (to[0] - from[0]).hypot(to[1] - from[1])
}

#[cfg(test)]
#[path = "tests/corner_path.rs"]
mod tests;
