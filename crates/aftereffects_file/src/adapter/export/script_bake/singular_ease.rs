//! Finite native temporal-ease approximation without adding animation keys.
use super::*;
use crate::export_document::effects::{bezier, invert_bezier};

// Independently observed AE26.5 KeyframeEase minimum: 0.1 percent.
const MIN_INFLUENCE: f64 = 0.001;
const FIT_INTERVALS: u32 = 16_384;
// Bound the expensive minimax scans per preparation, including repeated curves.
const MAX_FITS: usize = 32;

fn charge_fit(fits: &mut usize) -> Result<(), AepWriteError> {
    if *fits >= MAX_FITS {
        return Err(AepWriteError::InvalidDocument(
            BakeError::Budget("singular temporal-ease fits (maximum 32)").to_string(),
        ));
    }
    *fits += 1;
    Ok(())
}

fn singular(easing: PropertyKeyframeEasing) -> Option<[f64; 4]> {
    let PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = easing else {
        return None;
    };
    ([x1, y1, x2, y2].into_iter().all(f64::is_finite)
        && (0.0..=1.0).contains(&x1)
        && (0.0..=1.0).contains(&x2)
        && ((x1 == 0.0 && y1 != 0.0) || (x2 == 1.0 && y2 != 1.0)))
        .then_some([x1, y1, x2, y2])
}

/// Minimax endpoint-speed gain at fixed minimum native influences. Ordinary
/// handles remain unchanged. Two singular ends share the gain; this deliberately
/// does not claim a global optimum over every possible native cubic.
fn fit([x1, y1, x2, y2]: [f64; 4]) -> Option<(PropertyKeyframeEasing, f64)> {
    let outgoing = x1 == 0.0 && y1 != 0.0;
    let incoming = x2 == 1.0 && y2 != 1.0;
    let native_x1 = if outgoing { MIN_INFLUENCE } else { x1 };
    let native_x2 = if incoming { 1.0 - MIN_INFLUENCE } else { x2 };
    let base_y1 = if outgoing { 0.0 } else { y1 };
    let base_y2 = if incoming { 1.0 } else { y2 };
    let gain_y1 = if outgoing { y1 } else { 0.0 };
    let gain_y2 = if incoming { y2 - 1.0 } else { 0.0 };
    let samples: Vec<_> = (1..FIT_INTERVALS)
        .map(|index| {
            // A uniform native parameter grid also resolves the endpoint region
            // where a uniform time grid misses the singular curve's largest error.
            let u = f64::from(index) / f64::from(FIT_INTERVALS);
            let x = bezier(u, native_x1, native_x2);
            let source_u = invert_bezier(x, x1, x2);
            let error = bezier(u, base_y1, base_y2) - bezier(source_u, y1, y2);
            let coefficient = 3.0 * (1.0 - u) * u * ((1.0 - u) * gain_y1 + u * gain_y2);
            (error, coefficient)
        })
        .collect();
    if samples
        .iter()
        .any(|(a, b)| !a.is_finite() || !b.is_finite())
    {
        return None;
    }
    // max |a+b*gain| is convex. Its minimum is bracketed by the smallest and
    // largest individual zero crossings; no arbitrary speed/gain clamp is used.
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for &(a, b) in &samples {
        if b != 0.0 {
            let root = -a / b;
            if !root.is_finite() {
                return None;
            }
            lo = lo.min(root);
            hi = hi.max(root);
        }
    }
    if !lo.is_finite() || !hi.is_finite() || !(hi - lo).is_finite() {
        return None;
    }
    let maximum = |gain: f64| {
        samples
            .iter()
            .fold(0.0_f64, |error, &(a, b)| error.max((a + b * gain).abs()))
    };
    // Ternary narrowing of a convex objective, deterministic and independent of
    // key duration/value units. This fits one cubic, not a sampled key track.
    for _ in 0..80 {
        let left = lo + (hi - lo) / 3.0;
        let right = hi - (hi - lo) / 3.0;
        if maximum(left) <= maximum(right) {
            hi = right;
        } else {
            lo = left;
        }
    }
    let gain = lo + (hi - lo) * 0.5;
    let native_y1 = base_y1 + gain_y1 * gain;
    let native_y2 = base_y2 + gain_y2 * gain;
    let error = maximum(gain);
    [native_y1, native_y2, error]
        .into_iter()
        .all(f64::is_finite)
        .then_some((
            PropertyKeyframeEasing::CubicBezier {
                x1: native_x1,
                y1: native_y1,
                x2: native_x2,
                y2: native_y2,
            },
            error,
        ))
}

pub(super) fn prepare<'a>(
    document: &'a EditableFxCompositionDocument,
    roots: &[Layer],
    selected: bool,
) -> Result<Prepared<'a>, AepWriteError> {
    let mut owners = BTreeMap::new();
    collect_owners(
        roots,
        &[],
        &mut clock::Clocks::default(),
        &mut owners,
        &mut BTreeMap::new(),
        &mut BTreeMap::new(),
    );
    let mut raw = None;
    let mut diagnostics = Vec::new();
    let mut fits = 0;
    for (index, entry) in document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .enumerate()
    {
        let PropertyTarget::LayerProperty(property) = &entry.target else {
            continue;
        };
        // Spatial/vector, Path and effect-property representability guards are
        // deliberately untouched. Opacity has its separate existing preparation.
        if !matches!(
            property.property_type(),
            PropType::ScaleX | PropType::ScaleY | PropType::Rotation
        ) || selected && !owners.contains_key(&property.layer_id())
            || !entry.dependencies.is_empty()
            || entry.random_seed_target.is_some()
            || !entry.layer_refs.is_empty()
        {
            continue;
        }
        let AnimatorData::Keyframes {
            track,
            enabled: true,
            ..
        } = entry.animator.data()
        else {
            continue;
        };
        let mut keys = track.keyframes().to_vec();
        let mut changed = false;
        for (offset, pair) in track.keyframes().windows(2).enumerate() {
            let (left, right) = (&pair[0], &pair[1]);
            let Some(controls) = singular(right.easing()) else {
                continue;
            };
            let (PropertyValue::Float(from), PropertyValue::Float(to)) =
                (left.value(), right.value())
            else {
                continue;
            };
            if right.layer_time() <= left.layer_time()
                || !from.is_finite()
                || !to.is_finite()
                || !(to - from).is_finite()
            {
                continue;
            }
            charge_fit(&mut fits)?;
            let Some((easing, normalized_error)) = fit(controls) else {
                continue;
            };
            let error = normalized_error * (to - from).abs();
            if !error.is_finite() {
                continue;
            }
            keys[offset + 1] = PropertyKeyframe::new(
                right.id().clone(),
                right.layer_time(),
                right.value().clone(),
                easing,
            );
            changed = true;
            let units = if property.property_type() == PropType::Rotation {
                "degrees"
            } else {
                "scale percentage points"
            };
            diagnostics.push(ExportDiagnostic {
                layer_id: Some(property.layer_id()),
                message: format!("{:?} {}..{}ms cubic {:?}: finite temporal-ease approximation using minimum native influence 0.1%, minimax singular-end speed gain (shared if both ends are singular); original key count/times/values retained, no key resampling. Measured maximum error {error:.9} {units} on {FIT_INTERVALS} native-parameter intervals before native tick quantization; native easing {easing:?}. This is not exact cubic preservation or an all-time native bound: native timestamp/evaluation and render/alpha fidelity are separate evidence and can have larger errors.", property.property_type(), left.layer_time().as_millis(), right.layer_time().as_millis(), controls),
            });
        }
        if !changed {
            continue;
        }
        let track = PropertyKeyframeTrack::new(keys)
            .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))?;
        track
            .validate_for_target(&entry.target)
            .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))?;
        let animator = PropertyAnimator::keyframes(track);
        let raw = match &mut raw {
            Some(raw) => raw,
            slot @ None => slot.insert(
                BakedDocument::new(document)
                    .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))?,
            ),
        };
        raw.replace_animator(index, &animator)
            .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))?;
    }
    Ok(Prepared {
        document: match raw {
            Some(raw) => Cow::Owned(
                raw.finish()
                    .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))?,
            ),
            None => Cow::Borrowed(document),
        },
        diagnostics,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn singular_ease_fit_budget_stops_before_extra_work() {
        let mut fits = 0;
        for _ in 0..super::MAX_FITS {
            super::charge_fit(&mut fits).unwrap();
        }
        let error = super::charge_fit(&mut fits).unwrap_err();
        assert!(error.to_string().contains("script bake budget exceeded"));
        assert_eq!(fits, super::MAX_FITS);
    }

    use super::*;

    #[test]
    fn singular_ease_minimax_dense_grid_and_mirrored_endpoint() {
        for controls in [[0.55, 0.0, 1.0, 0.45], [0.0, 0.55, 0.45, 1.0]] {
            let (PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 }, measured) =
                fit(controls).unwrap()
            else {
                panic!("expected cubic");
            };
            let mut maximum = 0.0_f64;
            for index in 1..262_144 {
                let u = f64::from(index) / 262_144.0;
                let x = bezier(u, x1, x2);
                let source_u = invert_bezier(x, controls[0], controls[2]);
                maximum = maximum
                    .max((bezier(u, y1, y2) - bezier(source_u, controls[1], controls[3])).abs());
            }
            assert!(maximum * 100.0 < 0.159, "{maximum}");
            assert!((maximum - measured).abs() < 1e-7);
        }
    }

    #[test]
    fn singular_ease_finite_both_endpoint_family() {
        let (PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 }, error) =
            fit([0.0, 0.3, 1.0, 0.7]).unwrap()
        else {
            panic!("expected cubic");
        };
        assert_eq!((x1, x2), (0.001, 0.999));
        assert!(y1.is_finite() && y2.is_finite() && error.is_finite());
        assert!((y1 + y2 - 1.0).abs() < 1e-12);
    }

    #[test]
    fn singular_ease_keeps_other_cubic_guards() {
        for [x1, y1, x2, y2] in [
            [-0.1, 0.0, 1.0, 0.45],
            [0.0, 0.45, 1.1, 1.0],
            [0.55, f64::NAN, 1.0, 0.45],
            [0.55, 0.0, 1.0, f64::INFINITY],
            [0.0, 0.0, 1.0, 1.0],
            [0.2, 0.0, 0.8, 1.0],
        ] {
            assert!(singular(PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 }).is_none());
        }
    }
}
