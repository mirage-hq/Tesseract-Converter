//! Split monotone opaque-paint ramps at native opacity saturation.

use crate::writer::{KeyframeEasing, NumericKeyframe, NumericTrack};

pub(super) fn clipped(track: &NumericTrack, factor: f64) -> Option<NumericTrack> {
    let mut keys = Vec::with_capacity(track.keys.len());
    for key in &track.keys {
        let [value] = key.values.as_slice() else {
            return None;
        };
        if !value.is_finite()
            || *value < 0.0
            || !(*value * factor).is_finite()
            || !key.spatial_in.is_empty()
            || !key.spatial_out.is_empty()
        {
            return None;
        }
        let [easing] = key.easing.as_slice() else {
            return None;
        };
        match easing {
            KeyframeEasing::Linear | KeyframeEasing::Hold => {}
            KeyframeEasing::CubicBezier { x1, y1, x2, y2 }
                if [*x1, *y1, *x2, *y2].into_iter().all(f64::is_finite)
                    && (0.0..=1.0).contains(x1)
                    && (*x1..=1.0).contains(x2)
                    && (0.0..=1.0).contains(y1)
                    && (*y1..=1.0).contains(y2)
                    && (*x1 != 0.0 || *y1 == 0.0)
                    && (*x2 != 1.0 || *y2 == 1.0) => {}
            _ => return None,
        }
    }
    let first = track.keys.first()?;
    keys.push(scaled(first, factor));
    for pair in track.keys.windows(2) {
        let (left, right) = (&pair[0], &pair[1]);
        let duration = right.time_millis.checked_sub(left.time_millis)?;
        if duration <= 0 {
            return None;
        }
        let (start, end) = (left.values[0] * factor, right.values[0] * factor);
        let mut next = scaled(right, factor);
        if !matches!(right.easing[0], KeyframeEasing::Hold) {
            if start >= 1.0 && end >= 1.0 {
                next.easing = vec![KeyframeEasing::Hold];
            } else if (start > 1.0 && end < 1.0) || (start < 1.0 && end > 1.0) {
                let (x1, y1, x2, y2) = match right.easing[0] {
                    KeyframeEasing::CubicBezier { x1, y1, x2, y2 } => (x1, y1, x2, y2),
                    _ => (1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0),
                };
                let target = (1.0 - start) / (end - start);
                let mut low = 0.0;
                let mut high = 1.0;
                for _ in 0..60 {
                    let mid = (low + high) * 0.5;
                    if split([0.0, y1, y2, 1.0], mid).0[3] < target {
                        low = mid;
                    } else {
                        high = mid;
                    }
                }
                let parameter = (low + high) * 0.5;
                let (lx, rx) = split([0.0, x1, x2, 1.0], parameter);
                let (ly, ry) = split([0.0, y1, y2, 1.0], parameter);
                // FX/native key DTOs use integer milliseconds; never merge a crossing into an endpoint.
                let offset = (lx[3] * duration as f64).round() as i64;
                if offset <= 0 || offset >= duration {
                    return None;
                }
                let mut crossing = scaled(right, factor);
                crossing.time_millis = left.time_millis.checked_add(offset)?;
                crossing.values[0] = 1.0;
                if start > 1.0 {
                    crossing.easing = vec![KeyframeEasing::Hold];
                    next.easing = vec![normalized(rx, ry)?];
                } else {
                    crossing.easing = vec![normalized(lx, ly)?];
                    next.easing = vec![KeyframeEasing::Hold];
                }
                keys.push(crossing);
            } else if start == 1.0 && end > 1.0 || start > 1.0 && end == 1.0 {
                next.easing = vec![KeyframeEasing::Hold];
            }
        }
        keys.push(next);
    }
    Some(NumericTrack { keys })
}

fn scaled(key: &NumericKeyframe, factor: f64) -> NumericKeyframe {
    let mut key = key.clone();
    key.values[0] = (key.values[0] * factor).min(1.0);
    key
}

fn split(p: [f64; 4], t: f64) -> ([f64; 4], [f64; 4]) {
    let mix = |a, b| a + (b - a) * t;
    let (a, b, c) = (mix(p[0], p[1]), mix(p[1], p[2]), mix(p[2], p[3]));
    let (d, e) = (mix(a, b), mix(b, c));
    let f = mix(d, e);
    ([p[0], a, d, f], [f, e, c, p[3]])
}

#[cfg(test)]
mod tests;

fn normalized(x: [f64; 4], y: [f64; 4]) -> Option<KeyframeEasing> {
    let dx = x[3] - x[0];
    let dy = y[3] - y[0];
    if dx <= 0.0 || dy <= 0.0 {
        return None;
    }
    Some(KeyframeEasing::CubicBezier {
        x1: ((x[1] - x[0]) / dx).clamp(0.0, 1.0),
        y1: ((y[1] - y[0]) / dy).clamp(0.0, 1.0),
        x2: ((x[2] - x[0]) / dx).clamp(0.0, 1.0),
        y2: ((y[2] - y[0]) / dy).clamp(0.0, 1.0),
    })
}
