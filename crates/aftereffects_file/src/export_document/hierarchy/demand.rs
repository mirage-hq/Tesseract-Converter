//! Coupled root-time demand and conservative plane inverse. No sampled keyframe
//! endpoints are treated as bounds on a continuous animation.

use super::Bounds;

#[derive(Clone, Copy, Debug)]
pub(super) struct Segment {
    pub root_start: u64,
    pub(super) root_end: u64,
    pub local_start: f64,
    pub local_rate: f64,
    pub(super) region: Bounds,
    pub(super) failure: Option<&'static str>,
}

/// A finite crop is usable only when *every* reachable root-time segment has
/// passed each ordered support/transform/clock operator.
#[derive(Clone, Debug)]
pub(crate) struct Demand {
    segments: Vec<Segment>,
}

impl Demand {
    pub(super) fn root(viewport: Bounds, end: u64) -> Self {
        Self {
            segments: vec![Segment {
                root_start: 0,
                root_end: end,
                local_start: 0.0,
                local_rate: 1.0,
                region: viewport,
                failure: None,
            }],
        }
    }

    #[cfg(test)]
    pub(super) fn segments(&self) -> &[Segment] {
        &self.segments
    }

    pub(super) fn full(&mut self, reason: &'static str) {
        for segment in &mut self.segments {
            segment.failure.get_or_insert(reason);
        }
    }

    #[cfg(test)]
    pub(super) fn split_at(&mut self, root_time: u64) {
        let mut result = Vec::with_capacity(self.segments.len() + 1);
        for segment in &self.segments {
            if segment.root_start < root_time && root_time < segment.root_end {
                let mut after = *segment;
                after.local_start += (root_time - after.root_start) as f64 * after.local_rate;
                after.root_start = root_time;
                let mut before = *segment;
                before.root_end = root_time;
                result.extend([before, after]);
            } else {
                result.push(*segment);
            }
        }
        self.segments = result;
    }

    pub(super) fn active(&mut self, start: u64, end: u64) {
        let mut result = Vec::with_capacity(self.segments.len() + 2);
        for segment in &self.segments {
            if segment.failure.is_some() {
                result.push(*segment);
                continue;
            }
            if !segment.local_start.is_finite()
                || !segment.local_rate.is_finite()
                || segment.local_rate <= 0.0
            {
                let mut failed = *segment;
                failed.failure = Some("Group active interval has an unsupported source clock");
                result.push(failed);
                continue;
            }
            let from = (start as f64 - segment.local_start) / segment.local_rate
                + segment.root_start as f64;
            let to =
                (end as f64 - segment.local_start) / segment.local_rate + segment.root_start as f64;
            if !from.is_finite() || !to.is_finite() {
                let mut failed = *segment;
                failed.failure = Some("Group active interval cannot be mapped to root time");
                result.push(failed);
                continue;
            }
            // Round outward: a fractional boundary frame must not disappear.
            let first = segment.root_start.max(from.floor().max(0.0) as u64);
            let last = segment.root_end.min(to.ceil().max(0.0) as u64);
            if first < last {
                let mut clipped = *segment;
                clipped.local_start += (first - segment.root_start) as f64 * segment.local_rate;
                clipped.root_start = first;
                clipped.root_end = last;
                result.push(clipped);
            }
        }
        self.segments = result;
    }

    /// Only an exact affine source clock is accepted; a non-linear/remapped
    /// occurrence must be segmented using its own validated native clock.
    pub(super) fn map_clock(&mut self, start: u64, rate: f64) {
        if !rate.is_finite() || rate <= 0.0 {
            self.full("Group source clock has no proven finite positive affine mapping");
            return;
        }
        for segment in &mut self.segments {
            segment.local_start = (segment.local_start - start as f64) * rate;
            segment.local_rate *= rate;
        }
    }

    /// Apply a conservative inverse to each live root-time segment without
    /// exposing the segment storage to geometry analyzers.
    pub(super) fn inverse_regions(
        &mut self,
        mut inverse: impl FnMut(Bounds) -> Result<Bounds, &'static str>,
    ) {
        for segment in &mut self.segments {
            if segment.failure.is_none() {
                match inverse(segment.region) {
                    Ok(region) => segment.region = region,
                    Err(reason) => segment.failure = Some(reason),
                }
            }
        }
    }

    pub(super) fn inverse_segments(&mut self, plane: impl Fn(Segment) -> Homography) {
        for segment in &mut self.segments {
            if segment.failure.is_some() {
                continue;
            }
            match plane(*segment).preimage(segment.region) {
                Ok(region) => segment.region = region,
                Err(reason) => segment.failure = Some(reason),
            }
        }
    }

    #[cfg(test)]
    pub(super) fn intersect(&mut self, bounds: Bounds) {
        for segment in &mut self.segments {
            for axis in 0..2 {
                segment.region.min[axis] = segment.region.min[axis].max(bounds.min[axis]);
                segment.region.max[axis] = segment.region.max[axis].min(bounds.max[axis]);
            }
        }
    }

    pub(super) fn finite_union(&self) -> Result<Bounds, &'static str> {
        let mut bounds: Option<Bounds> = None;
        for segment in &self.segments {
            if let Some(reason) = segment.failure {
                return Err(reason);
            }
            if segment
                .region
                .min
                .iter()
                .zip(segment.region.max)
                .any(|(min, max)| min >= &max)
            {
                continue;
            }
            match &mut bounds {
                Some(bounds) => bounds.include(segment.region),
                None => bounds = Some(segment.region),
            }
        }
        bounds.ok_or("Consumer demand has no positive visible region")
    }
}

/// Homogeneous mapping of a source plane into the root viewport. `camera_d`
/// is the original root lens distance, never a cropped composition's width.
#[derive(Clone, Copy)]
pub(super) struct Homography {
    matrix: [[f64; 3]; 3],
    camera_d: f64,
}

impl Homography {
    pub(super) const fn new(matrix: [[f64; 3]; 3], camera_d: f64) -> Self {
        Self { matrix, camera_d }
    }

    pub(super) fn affine(linear: [f64; 4], translation: [f64; 2]) -> Self {
        Self::new(
            [
                [linear[0], linear[1], translation[0]],
                [linear[2], linear[3], translation[1]],
                [0.0, 0.0, 1.0],
            ],
            1.0,
        )
    }

    pub(super) fn preimage(self, region: Bounds) -> Result<Bounds, &'static str> {
        let m = self.matrix;
        if !self.camera_d.is_finite()
            || self.camera_d <= f64::EPSILON
            || m.iter().flatten().any(|x| !x.is_finite())
            || region
                .min
                .iter()
                .chain(region.max.iter())
                .any(|x| !x.is_finite())
        {
            return Err("Projective demand has non-finite coordinates or invalid root camera");
        }
        let cofactor = [
            [
                m[1][1] * m[2][2] - m[1][2] * m[2][1],
                m[0][2] * m[2][1] - m[0][1] * m[2][2],
                m[0][1] * m[1][2] - m[0][2] * m[1][1],
            ],
            [
                m[1][2] * m[2][0] - m[1][0] * m[2][2],
                m[0][0] * m[2][2] - m[0][2] * m[2][0],
                m[0][2] * m[1][0] - m[0][0] * m[1][2],
            ],
            [
                m[1][0] * m[2][1] - m[1][1] * m[2][0],
                m[0][1] * m[2][0] - m[0][0] * m[2][1],
                m[0][0] * m[1][1] - m[0][1] * m[1][0],
            ],
        ];
        let det = m[0][0] * cofactor[0][0] + m[0][1] * cofactor[1][0] + m[0][2] * cofactor[2][0];
        let norm = m.iter().flatten().map(|x| x.abs()).fold(0.0, f64::max);
        if !det.is_finite() || det.abs() <= f64::EPSILON * norm.powi(3) * 64.0 {
            return Err("Projective demand plane has an unproved or singular determinant");
        }
        let inv = cofactor.map(|row| row.map(|value| value / det));
        let corners = [
            region.min,
            [region.max[0], region.min[1]],
            [region.min[0], region.max[1]],
            region.max,
        ];
        let mut output: Option<Bounds> = None;
        let mut inverse_sign = 0;
        for [x, y] in corners {
            let w = inv[2][0] * x + inv[2][1] * y + inv[2][2];
            let cancellation = f64::EPSILON
                * (inv[2][0].abs() * x.abs() + inv[2][1].abs() * y.abs() + inv[2][2].abs())
                * 8.0;
            if !w.is_finite() || w.abs() <= cancellation {
                return Err("Inverse projective denominator approaches the horizon");
            }
            let sign = if w > 0.0 { 1 } else { -1 };
            if inverse_sign != 0 && sign != inverse_sign {
                return Err("Inverse projective denominator crosses zero inside consumer demand");
            }
            inverse_sign = sign;
            let p = [
                (inv[0][0] * x + inv[0][1] * y + inv[0][2]) / w,
                (inv[1][0] * x + inv[1][1] * y + inv[1][2]) / w,
            ];
            if p.iter().any(|value| !value.is_finite()) {
                return Err("Projective preimage is non-finite");
            }
            match &mut output {
                Some(bounds) => bounds.include(Bounds { min: p, max: p }),
                None => output = Some(Bounds { min: p, max: p }),
            }
        }
        let output = output.ok_or("Projective demand has no corners")?;
        let forward = [
            output.min,
            [output.max[0], output.min[1]],
            [output.min[0], output.max[1]],
            output.max,
        ];
        let mut forward_sign = 0;
        for [x, y] in forward {
            let w = m[2][0] * x + m[2][1] * y + m[2][2];
            let cancellation = f64::EPSILON
                * (m[2][0].abs() * x.abs() + m[2][1].abs() * y.abs() + m[2][2].abs())
                * 8.0;
            if !w.is_finite() || w <= f64::EPSILON || w.abs() <= cancellation {
                return Err("Forward projective denominator approaches the near plane");
            }
            let sign = if w > 0.0 { 1 } else { -1 };
            if forward_sign != 0 && sign != forward_sign {
                return Err("Forward projective denominator crosses the near plane");
            }
            forward_sign = sign;
        }
        Ok(output)
    }
}
