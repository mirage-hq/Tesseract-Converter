//! Analytical point fitting for the exact same-composition toComp origin form.

use super::*;

/// Times are composition milliseconds. Source remapping changes paint, not
/// the native Transform clock used by `toComp`.
pub(in crate::structure_document) fn composition_origin_curve(
    composition: &Composition,
    items: &HashMap<u32, &ProjectItem>,
    owner: &Layer,
    name: &str,
) -> Result<BTreeMap<i64, [f64; 2]>, String> {
    let source = unique_layer_by_name(composition, name)?;
    if source.record.id() == owner.record.id() {
        return Err("point expression requires a separate control layer".into());
    }
    let (local_start, local_end) = owner_interval_ms(owner)?;
    let from = seconds_to_millis(owner_local_to_comp(owner, local_start as f64 / 1000.0)?)?;
    let to = seconds_to_millis(owner_local_to_comp(owner, local_end as f64 / 1000.0)?)?;
    let span = to.checked_sub(from).ok_or("point interval overflow")?;
    if span <= 0 || span > MAX_DURATION_MS {
        return Err("point composition interval must be positive and at most 60 seconds".into());
    }
    let mut rig = TransformRig::new(composition, items);
    rig.add(source.record.id())?;
    let count = usize::try_from(span + 1).map_err(|_| "point sample count overflow")?;
    if count > MAX_EVALUATIONS {
        return Err("point analytical evaluation bound exceeded".into());
    }
    // Cache the bounded millisecond oracle once. Rescanning analytical curves
    // for every split both wastes work and can exhaust the evaluation allowance.
    let samples = (from..=to)
        .map(|time| {
            let point = rig
                .matrix(source.record.id(), time as f64 / 1000.0)?
                .apply([0.0, 0.0]);
            if point.iter().any(|value| !value.is_finite()) {
                return Err("point is non-finite".into());
            }
            Ok(point)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let kept = fit_points(&samples, MAX_EVALUATIONS - count)?;
    kept.into_iter()
        .map(|index| {
            let offset = i64::try_from(index).map_err(|_| "point key index overflow")?;
            Ok((from + offset, samples[index]))
        })
        .collect()
}

fn fit_points(samples: &[[f64; 2]], mut remaining_work: usize) -> Result<BTreeSet<usize>, String> {
    let mut kept = BTreeSet::from([0, samples.len() - 1]);
    let mut pending = vec![(0, samples.len() - 1)];
    while let Some((a, b)) = pending.pop() {
        let mut worst = FIT_TOLERANCE_PIXELS;
        let mut split = None;
        for index in a + 1..b {
            remaining_work = remaining_work
                .checked_sub(1)
                .ok_or("point fitting work allowance exceeded")?;
            let fraction = (index - a) as f64 / (b - a) as f64;
            let error = (0..2)
                .map(|axis| {
                    let expected =
                        samples[a][axis] + (samples[b][axis] - samples[a][axis]) * fraction;
                    (samples[index][axis] - expected).abs()
                })
                .fold(0.0_f64, f64::max);
            if error > worst {
                worst = error;
                split = Some(index);
            }
        }
        if let Some(index) = split {
            kept.insert(index);
            pending.extend([(a, index), (index, b)]);
        }
    }
    Ok(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_fit_checks_every_millisecond_and_bounds_refinement_work() {
        let samples: Vec<_> = (0..=100)
            .map(|time| {
                let time = f64::from(time);
                [time, time * time / 100.0]
            })
            .collect();
        let kept = fit_points(&samples, 10_000).unwrap();
        let indices: Vec<_> = kept.into_iter().collect();
        for pair in indices.windows(2) {
            let [a, b] = [pair[0], pair[1]];
            for index in a..=b {
                let fraction = (index - a) as f64 / (b - a) as f64;
                for ((start, end), actual) in
                    samples[a].into_iter().zip(samples[b]).zip(samples[index])
                {
                    let value = start + (end - start) * fraction;
                    assert!((actual - value).abs() <= FIT_TOLERANCE_PIXELS);
                }
            }
        }
        assert!(fit_points(&samples, 0).is_err());
    }
}
