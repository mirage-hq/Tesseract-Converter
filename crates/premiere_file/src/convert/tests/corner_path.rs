use super::{greatest_speed, straighten, Cubic, Curve, MAX_ERROR_PX, MAX_KEYS};
use crate::schema::{
    PrCornerPin, PrEffect, PrEffectParamAnimation, PrEffectParamKeys, PrEffectParams,
    PrKeyframeEasing, PrPointKeyframe, CORNER_PIN, TICKS, TICKS_PER_MILLISECOND,
};

/// The frame of the pinned save's source, `feature_timecoded_source.mp4`.
const FRAME: [u32; 2] = [1920, 1080];

/// A Linear point key at `seconds` with spatial tangents `incoming` and
/// `outgoing`.
fn key(seconds: f64, value: [f64; 2], incoming: [f64; 2], outgoing: [f64; 2]) -> PrPointKeyframe {
    PrPointKeyframe {
        source_ticks: (seconds * TICKS as f64) as i64,
        value,
        easing: PrKeyframeEasing::Linear,
        spatial_in_tangent: Some(incoming),
        spatial_out_tangent: Some(outgoing),
    }
}

/// The Upper Left keys of the pinned `premiere_isolated_source_effects_26_5`
/// save as its wire reads: source 0, 3 and 6 s, Linear, with Premiere's
/// resolved automatic spatial tangents.
fn saved_upper_left() -> Vec<PrPointKeyframe> {
    vec![
        key(
            0.0,
            [0.0, 0.0],
            [0.0, 0.0],
            [0.027777778605620067, 0.02469135820865631],
        ),
        key(
            3.0,
            [0.1666666716337204, 0.14814814925193787],
            [-0.006944444651405015, -0.04012345770994822],
            [0.006944444651405015, 0.04012345770994822],
        ),
        key(
            6.0,
            [0.0416666679084301, 0.24074074625968933],
            [0.020833333954215053, -0.015432099501291912],
            [0.0, 0.0],
        ),
    ]
}

/// A Corner Pin whose Upper Left moves by `keys`, the other corners at the
/// frame's, as in the save.
fn pin(keys: Vec<PrPointKeyframe>) -> PrEffect {
    PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::CornerPin(PrCornerPin {
            corners: [keys[0].value, [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
        }),
        animations: vec![PrEffectParamAnimation {
            param: &CORNER_PIN.params[0],
            keys: PrEffectParamKeys::Point(keys),
        }],
    }
}

/// A Linear point key at `seconds` on a straight spatial path: no tangents.
fn straight_key(seconds: f64, value: [f64; 2]) -> PrPointKeyframe {
    PrPointKeyframe {
        spatial_in_tangent: None,
        spatial_out_tangent: None,
        ..key(seconds, value, [0.0; 2], [0.0; 2])
    }
}

/// A Corner Pin whose Upper Left moves by `keys` and whose Upper Right, Lower
/// Left and Lower Right are `others`.
fn pin_among(keys: Vec<PrPointKeyframe>, others: [[f64; 2]; 3]) -> PrEffect {
    let mut effect = pin(keys);
    if let PrEffectParams::CornerPin(pin) = &mut effect.params {
        pin.corners[1..].copy_from_slice(&others);
    }
    effect
}

/// Upper Left's closed loop over source 0 to 3 s: from `point` out along
/// `offset`, back through `point` to the other side and back, its control
/// points `point`, `point + offset`, `point - offset` and `point`.
fn closed_loop(point: [f64; 2], offset: [f64; 2]) -> Vec<PrPointKeyframe> {
    let [x, y] = offset;
    vec![
        key(0.0, point, [0.0, 0.0], [x, y]),
        key(3.0, point, [-x, -y], [0.0, 0.0]),
    ]
}

/// `count` keys a second apart whose every interval takes the shape of the
/// saved path's first one, to and fro.
fn zigzag(count: usize) -> Vec<PrPointKeyframe> {
    (0..count)
        .map(|index| {
            let odd = index % 2 == 1;
            key(
                index as f64,
                if odd {
                    [0.1666666716337204, 0.14814814925193787]
                } else {
                    [0.0, 0.0]
                },
                if odd {
                    [-0.006944444651405015, -0.04012345770994822]
                } else {
                    [0.006944444651405015, 0.04012345770994822]
                },
                if odd {
                    [-0.02, -0.03]
                } else {
                    [0.027777778605620067, 0.02469135820865631]
                },
            )
        })
        .collect()
}

/// The straightened keys of `effect`.
fn straight_keys(effect: &PrEffect) -> &[PrPointKeyframe] {
    let [animation] = effect.animations.as_slice() else {
        panic!("{effect:?}")
    };
    animation.keys.point().unwrap()
}

/// A cubic Bézier point, computed apart from the module's evaluator.
fn bernstein(controls: [[f64; 2]; 4], u: f64) -> [f64; 2] {
    let v = 1.0 - u;
    let weights = [v * v * v, 3.0 * v * v * u, 3.0 * v * u * u, u * u * u];
    std::array::from_fn(|axis| {
        weights
            .iter()
            .zip(controls)
            .map(|(weight, control)| weight * control[axis])
            .sum()
    })
}

/// The constant-speed traversal of `keys`' path, computed independently of
/// the certificate: each interval's length tabulated over 200 000 chords and
/// inverted by bisection.
struct DenseTraversal {
    keys: Vec<PrPointKeyframe>,
    /// Per interval: its control points and the length before each chord end.
    tables: Vec<([[f64; 2]; 4], Vec<f64>)>,
}

impl DenseTraversal {
    const CHORDS: usize = 200_000;

    fn new(keys: &[PrPointKeyframe]) -> Self {
        let tables = keys
            .windows(2)
            .map(|pair| {
                let [start, end] = [&pair[0], &pair[1]];
                let outgoing = start.spatial_out_tangent.unwrap_or_default();
                let incoming = end.spatial_in_tangent.unwrap_or_default();
                let controls = [
                    start.value,
                    [start.value[0] + outgoing[0], start.value[1] + outgoing[1]],
                    [end.value[0] + incoming[0], end.value[1] + incoming[1]],
                    end.value,
                ];
                let mut lengths = vec![0.0];
                let mut previous = controls[0];
                for index in 1..=Self::CHORDS {
                    let point = bernstein(controls, index as f64 / Self::CHORDS as f64);
                    let step = (point[0] - previous[0]).hypot(point[1] - previous[1]);
                    lengths.push(lengths[index - 1] + step);
                    previous = point;
                }
                (controls, lengths)
            })
            .collect();
        Self {
            keys: keys.to_vec(),
            tables,
        }
    }

    /// The point at source time `ticks`, held before the first key and after
    /// the last.
    fn at(&self, ticks: f64) -> [f64; 2] {
        let last = self.keys.len() - 1;
        if ticks <= self.keys[0].source_ticks as f64 {
            return self.keys[0].value;
        }
        if ticks >= self.keys[last].source_ticks as f64 {
            return self.keys[last].value;
        }
        let interval = self
            .keys
            .windows(2)
            .position(|pair| ticks < pair[1].source_ticks as f64)
            .unwrap();
        let [start, end] =
            [interval, interval + 1].map(|index| self.keys[index].source_ticks as f64);
        let (controls, lengths) = &self.tables[interval];
        let target = lengths[Self::CHORDS] * (ticks - start) / (end - start);
        let chord = lengths
            .partition_point(|length| *length < target)
            .clamp(1, Self::CHORDS);
        let span = lengths[chord] - lengths[chord - 1];
        let within = if span > 0.0 {
            (target - lengths[chord - 1]) / span
        } else {
            0.0
        };
        bernstein(
            *controls,
            (chord as f64 - 1.0 + within) / Self::CHORDS as f64,
        )
    }
}

/// The time of `key` on FX's layer clock of a placement from `source_in`:
/// its source time from there rounded to the nearest whole millisecond, ties
/// away from zero, as import writes every key, a saved one too.
fn layer_millis(key: &PrPointKeyframe, source_in: i64) -> f64 {
    ((key.source_ticks - source_in) as f64 / TICKS_PER_MILLISECOND as f64).round()
}

/// The pixels that FX draws for the normalized corner `value` on a `frame`:
/// each coordinate in f32 times the frame side in f32 (`fx_composition`'s
/// Corner Pin lowering).
fn drawn(value: [f64; 2], frame: [u32; 2]) -> [f64; 2] {
    std::array::from_fn(|axis| f64::from(value[axis] as f32 * frame[axis] as f32))
}

/// FX's drawn value of straight Linear `keys` at the whole millisecond
/// `millis` of the layer clock of a placement from `source_in`: each key at
/// its rounded time ([`layer_millis`]), Linear in f64 between two keys as FX
/// interpolates and held outside them, then [`drawn`].
fn fx_at(keys: &[PrPointKeyframe], source_in: i64, millis: f64, frame: [u32; 2]) -> [f64; 2] {
    let next = keys.partition_point(|key| layer_millis(key, source_in) <= millis);
    let value = match (
        next.checked_sub(1).map(|index| &keys[index]),
        keys.get(next),
    ) {
        (Some(before), Some(after)) => {
            let [from, to] = [before, after].map(|key| layer_millis(key, source_in));
            let progress = (millis - from) / (to - from);
            std::array::from_fn(|axis| {
                before.value[axis] + (after.value[axis] - before.value[axis]) * progress
            })
        }
        (Some(only), None) | (None, Some(only)) => only.value,
        (None, None) => panic!("no keys"),
    };
    drawn(value, frame)
}

/// The greatest distance in source pixels between `traversal` at every
/// 0.25 ms of `span` seconds of a clip clock from `source_in` and FX's drawn
/// value of `keys` ([`fx_at`]) at each whole millisecond within 1 ms of it:
/// the renderer's frame time and the layer's start each round to the nearest
/// millisecond.
fn dense_error(
    traversal: &DenseTraversal,
    keys: &[PrPointKeyframe],
    source_in: i64,
    span: [f64; 2],
    frame: [u32; 2],
) -> f64 {
    let steps = ((span[1] - span[0]) * 4000.0) as usize;
    (0..=steps)
        .map(|step| {
            let millis = span[0] * 1000.0 + step as f64 / 4.0;
            let native = traversal.at(source_in as f64 + millis * TICKS_PER_MILLISECOND as f64);
            let exact = [0, 1].map(|axis| f64::from(frame[axis]) * native[axis]);
            ((millis - 1.0).ceil() as i64..=(millis + 1.0).floor() as i64)
                .map(|runtime| {
                    let imported = fx_at(keys, source_in, runtime as f64, frame);
                    (imported[0] - exact[0]).hypot(imported[1] - exact[1])
                })
                .fold(0.0, f64::max)
        })
        .fold(0.0, f64::max)
}

#[test]
fn the_saved_upper_left_path_straightens_within_the_approved_bounds() {
    // P1's clock: source In 1 s.
    let source_in = TICKS;
    let saved = saved_upper_left();
    let straightened = straighten(&pin(saved.clone()), FRAME, source_in)
        .unwrap()
        .expect("a curved path");
    let keys = straight_keys(&straightened.effect);
    assert_eq!(straightened.keys, keys.len());
    assert!(keys.len() <= MAX_KEYS, "{}", keys.len());
    assert!(
        straightened.bound_px <= MAX_ERROR_PX,
        "{}",
        straightened.bound_px
    );
    assert_eq!(
        (straightened.corner, straightened.saved_keys),
        ("Upper Left", 3)
    );
    // Straight Linear keys on whole milliseconds of the clip clock, in
    // strict time order, with the saved keys among them unchanged.
    assert!(keys.iter().all(|key| key.easing == PrKeyframeEasing::Linear
        && key.spatial_in_tangent.is_none()
        && key.spatial_out_tangent.is_none()
        && (key.source_ticks - source_in) % TICKS_PER_MILLISECOND == 0));
    assert!(keys
        .windows(2)
        .all(|pair| pair[0].source_ticks < pair[1].source_ticks));
    for saved in &saved {
        assert!(
            keys.iter()
                .any(|key| key.source_ticks == saved.source_ticks && key.value == saved.value),
            "{saved:?}"
        );
    }
    // Every straight key is a point of the path that the traversal reaches
    // within half a millisecond of the key's rounded time: it passes within
    // 10⁻³ px of the key, sampled every microsecond of that millisecond.
    let traversal = DenseTraversal::new(&saved);
    for key in keys {
        let nearest = (-500..=500)
            .map(|micros| {
                let ticks = key.source_ticks as f64
                    + f64::from(micros) * TICKS_PER_MILLISECOND as f64 / 1000.0;
                let native = traversal.at(ticks);
                (1920.0 * (native[0] - key.value[0])).hypot(1080.0 * (native[1] - key.value[1]))
            })
            .fold(f64::INFINITY, f64::min);
        assert!(nearest < 1e-3, "{key:?}: {nearest}");
    }
    // The independent traversal stays within the certified bound at every
    // 0.25 ms of the clip clock, off-trim times included.
    let error = dense_error(&traversal, keys, source_in, [-1.5, 5.5], FRAME);
    assert!(
        error <= straightened.bound_px,
        "{error} > {}",
        straightened.bound_px
    );
}

#[test]
fn the_path_lengths_agree_with_the_key_speeds_premiere_saved() {
    // The saved keys' temporal speeds are each interval's path length, in
    // normalized frame coordinates, over its 3 s: the traversal that the
    // straightened keys follow. Premiere's own float noise leaves them 10⁻⁷
    // from the exact length that the bounds enclose.
    let saved = saved_upper_left();
    for (pair, speed) in saved
        .windows(2)
        .zip([0.07511325146344426, 0.05404495105825096])
    {
        let curve = Curve::new(Cubic::between(&pair[0], &pair[1])).unwrap();
        let [shortest, longest] = curve.length(0, Curve::PIECES);
        assert!(longest - shortest < 1e-8, "{shortest} to {longest}");
        let saved_length = speed * 3.0;
        assert!(
            ((shortest + longest) / 2.0 - saved_length).abs() / saved_length < 1e-6,
            "{shortest} to {longest} against {saved_length}"
        );
    }
}

#[test]
fn each_placement_straightens_its_own_copy_on_its_own_clock() {
    // P1 from source 1 s and P2 from 2 s take the same source times, whole
    // milliseconds of both clocks. A source In between milliseconds moves the
    // generated keys to that clock's whole milliseconds; the saved keys stay.
    let effect = pin(saved_upper_left());
    let keys = |source_in: i64| {
        straight_keys(
            &straighten(&effect, FRAME, source_in)
                .unwrap()
                .unwrap()
                .effect,
        )
        .to_vec()
    };
    assert_eq!(keys(TICKS), keys(2 * TICKS));
    let offset = TICKS + TICKS_PER_MILLISECOND / 3;
    let shifted = keys(offset);
    assert!(shifted.iter().all(|key| saved_upper_left()
        .iter()
        .any(|saved| saved.source_ticks == key.source_ticks)
        || (key.source_ticks - offset) % TICKS_PER_MILLISECOND == 0));
    // On that clock the saved keys, a third of a millisecond off it, take
    // their rounded times as the generated ones do, and FX's drawn corner on
    // its runtime clock stays within the copy's bound.
    let straightened = straighten(&effect, FRAME, offset).unwrap().unwrap();
    let error = dense_error(
        &DenseTraversal::new(&saved_upper_left()),
        straight_keys(&straightened.effect),
        offset,
        [-1.5, 5.5],
        FRAME,
    );
    assert!(
        error <= straightened.bound_px,
        "{error} > {}",
        straightened.bound_px
    );
}

#[test]
fn straight_paths_and_other_effects_keep_their_exact_mappings() {
    let mut straight = saved_upper_left();
    for key in &mut straight {
        (key.spatial_in_tangent, key.spatial_out_tangent) = (None, None);
    }
    assert!(straighten(&pin(straight), FRAME, 0).unwrap().is_none());
    let blur = PrEffect {
        mask: None,
        enabled: true,
        params: PrEffectParams::GaussianBlur(crate::schema::PrGaussianBlur {
            blurriness: 20.0,
            repeat_edge_pixels: false,
        }),
        animations: Vec::new(),
    };
    assert!(straighten(&blur, FRAME, 0).unwrap().is_none());
}

#[test]
fn paths_beyond_the_approved_bounds_are_rejected_with_their_reason() {
    let saved = saved_upper_left();
    // The saved path over `seconds` per interval.
    let timed = |seconds: f64| -> Vec<PrPointKeyframe> {
        saved
            .iter()
            .enumerate()
            .map(|(index, saved)| PrPointKeyframe {
                source_ticks: (index as f64 * seconds * TICKS as f64) as i64,
                ..saved.clone()
            })
            .collect()
    };
    // A bulge through the other corners' diagonal: both keys keep the quad
    // convex, the path between them does not.
    let bulge = vec![
        key(0.0, [0.0, 0.0], [0.0, 0.0], [0.6, 0.9]),
        key(3.0, [0.2, 0.0], [0.4, 0.9], [0.0, 0.0]),
    ];
    // 65 saved keys: the saved path, then 62 straight steps.
    let mut long = saved.clone();
    for index in 1..=62 {
        let mut step = key(
            6.0 + f64::from(index),
            [0.0416666679084301, 0.24074074625968933],
            [0.0, 0.0],
            [0.0, 0.0],
        );
        step.value[1] -= 0.001 * f64::from(index);
        (step.spatial_in_tangent, step.spatial_out_tangent) = (None, None);
        long.push(step);
    }
    let mut held = saved.clone();
    held[1].easing = PrKeyframeEasing::Hold;
    let mut nonfinite = saved.clone();
    nonfinite[0].spatial_out_tangent = Some([f64::NAN, 0.0]);
    let mut two_corners = pin(saved.clone());
    two_corners.animations.push(PrEffectParamAnimation {
        param: &CORNER_PIN.params[1],
        keys: PrEffectParamKeys::Point(vec![
            key(0.0, [1.0, 0.0], [0.0, 0.0], [0.0, 0.0]),
            key(1.0, [0.9, 0.0], [0.0, 0.0], [0.0, 0.0]),
        ]),
    });
    for (case, effect, frame, reason) in [
        (
            "an 8K frame",
            pin(saved.clone()),
            [7680, 4320],
            "Upper Left's curved spatial path needs more than 64 straight keys to stay within 0.5 source pixels",
        ),
        (
            "a curved interval of 1 ms",
            pin(timed(0.001)),
            FRAME,
            "two keys of Upper Left's straightened path fall on one millisecond",
        ),
        (
            "the saved path ten times as fast",
            pin(timed(0.3)),
            FRAME,
            "Upper Left's straightened path moves up to 1442.",
        ),
        (
            "a path crossing the quad's diagonal",
            pin(bulge),
            FRAME,
            "has a part that its finest halving (2^12 pieces) does not bound: its quad is not certified convex there",
        ),
        (
            "65 saved keys",
            pin(long),
            FRAME,
            "Upper Left has 65 saved keys, more than the 64 straight keys that its curved spatial path may convert as",
        ),
        (
            "a Hold into a curved key",
            pin(held),
            FRAME,
            "Upper Left reaches its key at source time 3.000 s on Hold or Bezier temporal easing",
        ),
        (
            "a nonfinite tangent",
            pin(nonfinite),
            FRAME,
            "Upper Left's spatial control point after its key at source time 0.000 s lies at (NaN, ",
        ),
        (
            "two keyed corners",
            two_corners,
            FRAME,
            "2 corners are keyed and one moves on a curved spatial path",
        ),
        (
            "a frame without size",
            pin(saved.clone()),
            [0, 0],
            "the clip frame of 0 by 0 pixels is outside the numerical domain that the bound covers",
        ),
    ] {
        let error = straighten(&effect, frame, 0).unwrap_err();
        assert!(error.contains(reason), "{case}: {error}");
    }
}

#[test]
fn a_path_far_outside_the_frame_is_refused_before_its_rounding_exceeds_the_bound() {
    // A tiny closed loop 65536 frame widths to the right of the frame, the
    // other corners a frame from it: a convex quad whose loop needs no
    // generated key, well within the shape and clock budgets. Its coordinate
    // 65536.0078125 is an f32, but FX's f32 pixel product is a whole pixel
    // off, over the whole bound: f32(x · 1920) = 125829136, not 125829135.
    let x = 65536.0078125;
    assert_eq!(f64::from(x as f32), x);
    assert_eq!(drawn([x, 0.25], FRAME)[0] - x * 1920.0, 1.0);
    let far = pin_among(
        closed_loop([x, 0.25], [0.00002, 0.0]),
        [[x + 1.0, 0.0], [x, 1.0], [x + 1.0, 1.0]],
    );
    let error = straighten(&far, FRAME, 0).unwrap_err();
    assert!(
        error.contains("Upper Left's key at source time 0.000 s lies at (65536.0078125, 0.25) in frame units, outside the numerical domain that the bound covers"),
        "{error}"
    );
    // The same loop about the frame converts.
    let near = pin_among(
        closed_loop([0.5, 0.25], [0.00002, 0.0]),
        [[1.5, 0.0], [0.5, 1.0], [1.5, 1.0]],
    );
    let straightened = straighten(&near, FRAME, 0).unwrap().unwrap();
    assert!(
        straightened.bound_px <= MAX_ERROR_PX,
        "{}",
        straightened.bound_px
    );
}

#[test]
fn a_frame_converts_only_with_both_sides_within_the_domain() {
    for frame in [[1920, 0], [0, 1080], [8193, 1080], [1920, 8193]] {
        let error = straighten(&pin(saved_upper_left()), frame, 0).unwrap_err();
        assert!(
            error.contains(&format!(
                "the clip frame of {} by {} pixels is outside the numerical domain that the bound covers",
                frame[0], frame[1]
            )),
            "{frame:?}: {error}"
        );
    }
    assert!(straighten(&pin(saved_upper_left()), [8192, 1], 0)
        .is_err_and(|error| !error.contains("numerical domain")));
}

#[test]
fn a_loop_whose_drawn_quad_rounds_flat_is_refused() {
    // Upper Left 10^-8 inside the other corners' diagonal, on a tiny loop
    // along it: every control quad turns by 10^-8, above the f64 margin, and
    // the loop fits the bound without a generated key. But both keys draw in
    // f32 at (960, 540) px, on the diagonal: a flat quad.
    let point = [0.5, 0.5 - 1e-8];
    assert_eq!(drawn(point, FRAME), [960.0, 540.0]);
    let error = straighten(&pin(closed_loop(point, [1e-4, -1e-4])), FRAME, 0).unwrap_err();
    assert!(
        error.contains("Upper Left's straight keys at 0 ms and 3000 ms of the clip clock bring the quad's turn at Upper Left within the rounding of the f32 pixel corners that FX draws"),
        "{error}"
    );
    // The same loop a tenth of the frame inside the diagonal converts.
    let clear = straighten(&pin(closed_loop([0.4, 0.4], [1e-4, -1e-4])), FRAME, 0)
        .unwrap()
        .unwrap();
    assert!(clear.bound_px <= MAX_ERROR_PX, "{}", clear.bound_px);
}

/// The turn at Upper Left, in px² times 2^32, of the quad that FX draws with
/// Upper Left at `upper_left` and the other corners at the frame's
/// ([`drawn`]): exact, since the drawn pixels of these tests lie on a 2^-16
/// px grid.
fn drawn_turn_at_upper_left(upper_left: [f64; 2], frame: [u32; 2]) -> i128 {
    let grid = |corner: [f64; 2]| -> [i128; 2] {
        drawn(corner, frame).map(|pixels| {
            let scaled = pixels * 65536.0;
            assert_eq!(scaled.fract(), 0.0, "{pixels}");
            scaled as i128
        })
    };
    let [upper_left, upper_right, lower_left] = [upper_left, [1.0, 0.0], [0.0, 1.0]].map(grid);
    (upper_left[0] - lower_left[0]) * (upper_right[1] - upper_left[1])
        - (upper_left[1] - lower_left[1]) * (upper_right[0] - upper_left[0])
}

#[test]
fn a_straight_segment_whose_drawn_interior_is_flat_is_refused() {
    // Upper Left's keys at 0 and 30 s draw exactly at f32 pixels (512 - 2^-14,
    // 768) and (1536 - 2^-13, 256 + 2^-15) of a 2048 x 1024 frame, on a line
    // 1/16 px² inside the diagonal of the other corners, parallel to it; a
    // curved interval then moves it into the frame. The quad is strictly
    // convex at both keys, in f64 and as drawn, which the f64 quad check
    // confirms along the segment; the rounding of FX's values between them
    // is not.
    let frame = [2048, 1024];
    let near = [0.25 - 2f64.powi(-25), 0.75];
    let far = [0.75 - 2f64.powi(-24), 0.25 + 2f64.powi(-25)];
    let keys = |shift: f64| {
        let at = |point: [f64; 2]| [point[0] - shift, point[1] - shift];
        vec![
            straight_key(0.0, at(near)),
            PrPointKeyframe {
                spatial_out_tangent: Some([-0.1, 0.0]),
                ..straight_key(30.0, at(far))
            },
            PrPointKeyframe {
                spatial_in_tangent: Some([0.05, 0.05]),
                ..straight_key(60.0, at([0.5, 0.2]))
            },
        ]
    };
    // FX draws the quad at each whole millisecond between the keys from its
    // f64 Linear value in f32 pixels: strictly convex at both keys, flat at
    // thousands of milliseconds between them.
    let turn = |millis: i64| {
        let progress = millis as f64 / 30000.0;
        drawn_turn_at_upper_left(
            std::array::from_fn(|axis| near[axis] + (far[axis] - near[axis]) * progress),
            frame,
        )
    };
    assert_eq!([turn(0), turn(30_000)], [1 << 28; 2]);
    let flat = (1..30_000).filter(|&millis| turn(millis) <= 0).count();
    assert!(flat > 1000, "{flat}");
    let error = straighten(&pin(keys(0.0)), frame, 0).unwrap_err();
    assert!(
        error.contains("Upper Left's straight keys at 0 ms and 30000 ms of the clip clock bring the quad's turn at Upper Left within the rounding of the f32 pixel corners that FX draws"),
        "{error}"
    );
    // A tenth of the frame farther inside, the same path converts.
    let straightened = straighten(&pin(keys(0.05)), frame, 0).unwrap().unwrap();
    assert!(
        straightened.bound_px <= MAX_ERROR_PX,
        "{}",
        straightened.bound_px
    );
}

#[test]
fn key_times_a_whole_tick_range_apart_are_refused_without_overflow() {
    // A loop about the frame whose two keys lie at the ends of Premiere's
    // tick range: their span, 2^64 - 1 ticks, exceeds i64.
    let mut keys = closed_loop([0.5, 0.25], [0.00002, 0.0]);
    (keys[0].source_ticks, keys[1].source_ticks) = (i64::MIN, i64::MAX);
    let extreme = pin_among(keys, [[1.5, 0.0], [0.5, 1.0], [1.5, 1.0]]);
    let error = straighten(&extreme, FRAME, 0).unwrap_err();
    assert!(
        error.contains("is more than 2^53 ticks (about 9.85 hours) from the placement's source In, outside the numerical domain that the bound covers"),
        "{error}"
    );
    // The speed of keys that far apart is finite: their span widens before
    // it is taken.
    let straight = [(i64::MIN, [0.0, 0.0]), (i64::MAX, [1.0, 1.0])].map(|(source_ticks, value)| {
        PrPointKeyframe {
            source_ticks,
            ..straight_key(0.0, value)
        }
    });
    let millis = straight
        .each_ref()
        .map(|key| super::super::keyframes::layer_millis(key.source_ticks, 0).unwrap());
    let speed = greatest_speed(&straight, &[None], &straight, &millis, FRAME, 1920.0);
    assert!(speed.is_finite() && speed > 0.0, "{speed}");
}

#[test]
fn curved_intervals_over_the_piece_budget_are_refused_before_any_is_built() {
    // Five curved intervals would need 5 × 4096 pieces. The first cannot be
    // measured, with a NaN tangent: building it would report that instead,
    // so the budget refuses the path before building any interval.
    let mut five = zigzag(6);
    five[0].spatial_out_tangent = Some([f64::NAN, 0.0]);
    let error = straighten(&pin(five), FRAME, 0).unwrap_err();
    assert!(
        error.contains("Upper Left's path has 5 curved intervals, which would need 20480 pieces to bound, more than the 16384 that its conversion may use"),
        "{error}"
    );
    // Four curved intervals, 16384 pieces, fit the budget: a gentle wave
    // across the frame, smooth at its keys, each interval bulging the other
    // way.
    let bend = |interval: i32| 0.002 * f64::from(if interval % 2 == 0 { 1 } else { -1 });
    let wave: Vec<_> = (0..5)
        .map(|index| {
            key(
                3.0 * f64::from(index),
                [0.1 + 0.05 * f64::from(index), 0.1],
                [-0.05 / 3.0, bend(index - 1)],
                [0.05 / 3.0, bend(index)],
            )
        })
        .collect();
    let straightened = straighten(&pin(wave), FRAME, 0).unwrap().unwrap();
    assert!(
        straightened.keys <= MAX_KEYS && straightened.bound_px <= MAX_ERROR_PX,
        "{} keys, {}",
        straightened.keys,
        straightened.bound_px
    );
}

#[test]
fn the_report_rounds_its_bound_up() {
    let straightened = straighten(&pin(saved_upper_left()), FRAME, TICKS)
        .unwrap()
        .unwrap();
    let report = straightened.report("Corner Pin effect");
    let shown: f64 = report
        .split("within ")
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .and_then(|bound| bound.parse().ok())
        .unwrap_or_else(|| panic!("{report}"));
    assert!(
        shown >= straightened.bound_px && shown - straightened.bound_px < 0.001,
        "{shown} against {}",
        straightened.bound_px
    );
}
