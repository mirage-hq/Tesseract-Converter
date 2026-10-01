//! Shared numerical length for a Premiere spatial cubic segment.

use super::PrPointKeyframe;

pub(crate) fn segment_length(start: &PrPointKeyframe, end: &PrPointKeyframe) -> Option<f64> {
    let outgoing = start.spatial_out_tangent.unwrap_or([0.0, 0.0]);
    let incoming = end.spatial_in_tangent.unwrap_or([0.0, 0.0]);
    let p0 = start.value;
    let p3 = end.value;
    if p0
        .into_iter()
        .chain(p3)
        .chain(outgoing)
        .chain(incoming)
        .any(|component| !component.is_finite())
    {
        return None;
    }

    let chord = (p3[0] - p0[0]).hypot(p3[1] - p0[1]);
    if outgoing == [0.0, 0.0] && incoming == [0.0, 0.0] {
        return chord.is_finite().then_some(chord);
    }

    let p1 = [p0[0] + outgoing[0], p0[1] + outgoing[1]];
    let p2 = [p3[0] + incoming[0], p3[1] + incoming[1]];
    if p1
        .into_iter()
        .chain(p2)
        .any(|component| !component.is_finite())
    {
        return None;
    }

    // Composite Simpson integration of the derivative's magnitude. Unlike a
    // polyline approximation this also handles a path returning to its origin.
    let speed = |t: f64| {
        let u = 1.0 - t;
        let dx = 3.0
            * (u * u * (p1[0] - p0[0]) + 2.0 * u * t * (p2[0] - p1[0]) + t * t * (p3[0] - p2[0]));
        let dy = 3.0
            * (u * u * (p1[1] - p0[1]) + 2.0 * u * t * (p2[1] - p1[1]) + t * t * (p3[1] - p2[1]));
        dx.hypot(dy)
    };
    const STEPS: usize = 256;
    let mut sum = speed(0.0) + speed(1.0);
    for i in 1..STEPS {
        sum += if i % 2 == 0 { 2.0 } else { 4.0 } * speed(i as f64 / STEPS as f64);
        if !sum.is_finite() {
            return None;
        }
    }
    let length = sum / (3.0 * STEPS as f64);
    length.is_finite().then_some(length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::PrKeyframeEasing;

    #[test]
    fn curved_closed_path_has_nonzero_length_and_linear_path_matches_chord() {
        let start = PrPointKeyframe {
            source_ticks: 0,
            value: [0.0, 0.0],
            easing: PrKeyframeEasing::Linear,
            spatial_in_tangent: None,
            spatial_out_tangent: Some([1.0, 0.0]),
        };
        let mut end = PrPointKeyframe {
            source_ticks: 1,
            value: [0.0, 0.0],
            easing: PrKeyframeEasing::Linear,
            spatial_in_tangent: Some([0.0, 1.0]),
            spatial_out_tangent: None,
        };
        assert!(segment_length(&start, &end).unwrap() > 1.0);
        end.value = [2.0, 0.0];
        end.spatial_in_tangent = None;
        let mut straight = start;
        straight.spatial_out_tangent = None;
        assert_eq!(segment_length(&straight, &end), Some(2.0));

        straight.value = [f64::MAX, 0.0];
        end.value = [-f64::MAX, 0.0];
        assert_eq!(segment_length(&straight, &end), None);
    }
}
