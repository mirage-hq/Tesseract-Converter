//! Converter-only repair of finite vertical-endpoint Opacity easing.
use super::*;
use crate::export_document::effects::{bezier, invert_bezier};
use fx_keyframe_bake::value_curve::fit_sampled_value_curve;

const TOLERANCE_PERCENT: f64 = 0.01;
const MAX_NATIVE_KEYS: usize = 65_535;
// Local preparation-wide admission policy, independent of the native key limit.
// Each reserved integer-ms grid point is evaluated at most twice: fit + check.
const MAX_OPACITY_SAMPLES: u64 = 65_535;

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
    let entries = document.composition().dynamics().entries();
    let mut used_ids = entries
        .iter()
        .filter_map(|entry| entry.animator.keyframe_track())
        .flat_map(|track| track.keyframes())
        .map(|key| key.id().as_str().to_owned())
        .collect();
    let mut raw = None;
    let mut diagnostics = Vec::new();
    let mut remaining_samples = MAX_OPACITY_SAMPLES;
    for (index, entry) in entries.iter().enumerate() {
        let PropertyTarget::LayerProperty(property) = &entry.target else {
            continue;
        };
        if property.property_type() != PropType::Opacity
            || (selected && !owners.contains_key(&property.layer_id()))
        {
            continue;
        }
        let AnimatorData::Keyframes { enabled: true, .. } = entry.animator.data() else {
            continue;
        };
        let Some(track) = entry.animator.keyframe_track() else {
            continue;
        };
        if !track
            .keyframes()
            .iter()
            .skip(1)
            .any(|key| singular(key.easing()).is_some())
        {
            continue;
        }
        // Keep the original track for normal lowering if it cannot be repaired.
        match reserve_samples(track, &mut remaining_samples)
            .and_then(|()| lower(entry, track, &mut used_ids)) {
            Ok((animator, messages)) => {
                let raw = match &mut raw {
                    Some(raw) => raw,
                    slot @ None => slot.insert(BakedDocument::new(document)
                        .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))?),
                };
                raw.replace_animator(index, &animator)
                    .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))?;
                diagnostics.extend(messages.into_iter().map(|message| ExportDiagnostic {
                    layer_id: Some(property.layer_id()), message,
                }));
            }
            Err(reason) => diagnostics.push(ExportDiagnostic {
                layer_id: Some(property.layer_id()),
                message: format!("Opacity singular-easing preparation declined: {reason}; original track retained for native validation."),
            }),
        }
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

// Reserve the entire track before fitting or creating keys. Declined admission
// leaves the allowance intact; an attempted fit never refunds its reservation.
fn reserve_samples(track: &PropertyKeyframeTrack, remaining: &mut u64) -> Result<(), String> {
    let mut required = 0_u64;
    for pair in track.keyframes().windows(2) {
        if singular(pair[1].easing()).is_none() {
            continue;
        }
        let duration = pair[1]
            .layer_time()
            .as_millis()
            .checked_sub(pair[0].layer_time().as_millis())
            .ok_or("time overflow")?;
        let duration = u64::try_from(duration).map_err(|_| "reversed key times")?;
        if duration == 0 {
            return Err("coincident key times".into());
        }
        required = required
            .checked_add(duration.checked_add(1).ok_or("sample count overflow")?)
            .ok_or("sample count overflow")?;
        if required > *remaining {
            return Err(format!(
                "sample limit: track requires at least {required} integer-ms grid points, {} remaining of preparation-wide {MAX_OPACITY_SAMPLES}",
                *remaining
            ));
        }
    }
    *remaining -= required;
    Ok(())
}

fn lower(
    entry: &AnimationGraphEntry,
    track: &PropertyKeyframeTrack,
    used_ids: &mut BTreeSet<String>,
) -> Result<(PropertyAnimator, Vec<String>), String> {
    let original = track.keyframes();
    let identity = conversion_identity_seed(
        &serde_json::to_vec(&entry.target).map_err(|error| error.to_string())?,
        b"singular-opacity-ms-v1",
    );
    let mut keys = vec![original[0].clone()];
    let mut messages = Vec::new();
    for pair in original.windows(2) {
        let (left, right) = (&pair[0], &pair[1]);
        let Some([x1, y1, x2, y2]) = singular(right.easing()) else {
            keys.push(right.clone());
            continue;
        };
        let (PropertyValue::Float(from), PropertyValue::Float(to)) = (left.value(), right.value())
        else {
            return Err("non-scalar value".into());
        };
        let start = left.layer_time().as_millis();
        let end = right.layer_time().as_millis();
        let duration = u64::try_from(end.checked_sub(start).ok_or("time overflow")?)
            .map_err(|_| "reversed key times")?;
        if duration == 0 || !from.is_finite() || !to.is_finite() {
            return Err("nonfinite values or coincident key times".into());
        }
        let evaluate = |offset: u64| {
            if offset == 0 {
                *from
            } else if offset == duration {
                *to
            } else {
                let u = invert_bezier(offset as f64 / duration as f64, x1, x2);
                from + (to - from) * bezier(u, y1, y2)
            }
        };
        let mut fitted = fit_sampled_value_curve(
            0..=duration,
            TOLERANCE_PERCENT / 2.0,
            MAX_NATIVE_KEYS.saturating_sub(keys.len()).saturating_add(1),
            |offset| {
                let value = evaluate(offset);
                if value.is_finite() {
                    Ok(value)
                } else {
                    Err("nonfinite sampled value".to_owned())
                }
            },
            |a, b, actual, t| Some((a + (b - a) * t - actual).abs()),
        )
        .map_err(|error| format!("sampled fit failed: {error:?}"))?;
        // Streaming value reduction may discard a constant endpoint. Preserve it
        // explicitly; continuous fades always use Linear, never artificial Holds.
        if fitted.last().is_none_or(|key| key.offset_ms != duration) {
            fitted.push(fx_keyframe_bake::value_curve::ValueKey {
                offset_ms: duration,
                value: *to,
                linear: true,
            });
        }
        let mut segment_keys = Vec::new();
        for key in fitted.iter().skip(1) {
            let time = start
                .checked_add(i64::try_from(key.offset_ms).map_err(|_| "time overflow")?)
                .ok_or("time overflow")?;
            segment_keys.push(PropertyKeyframe::new(
                if time == end {
                    right.id().clone()
                } else {
                    fx_schema::KeyframeId::new(converted_keyframe_id(identity, time, used_ids))
                },
                TimeOffset::from_millis(time),
                PropertyValue::Float(key.value),
                PropertyKeyframeEasing::Linear,
            ));
        }
        let mut interval = 0;
        let mut maximum_error: f64 = 0.0;
        for offset in 0..=duration {
            while interval + 1 < fitted.len() && offset > fitted[interval + 1].offset_ms {
                interval += 1;
            }
            let a = &fitted[interval];
            let saved = match fitted.get(interval + 1) {
                Some(b) => {
                    a.value
                        + (b.value - a.value) * (offset - a.offset_ms) as f64
                            / (b.offset_ms - a.offset_ms) as f64
                }
                None => a.value,
            };
            maximum_error = maximum_error.max((saved - evaluate(offset)).abs());
        }
        if !maximum_error.is_finite() || maximum_error > TOLERANCE_PERCENT {
            return Err(format!(
                "sampled error {maximum_error} exceeds {TOLERANCE_PERCENT} percentage points"
            ));
        }
        messages.push(format!("Opacity {start}..{end}ms cubic [{x1},{y1},{x2},{y2}] has a vertical endpoint not representable by finite native temporal ease; sampled original curve at integer milliseconds into {} editable Linear keys (including endpoints), pre-native tolerance {TOLERANCE_PERCENT} percentage points, measured maximum sampled error {maximum_error} before native property-clock quantization. Native tick quantization adds error and is not covered by that fit cap. Sub-millisecond behavior and Adobe/native pixel fidelity remain unverified.", fitted.len()));
        keys.extend(segment_keys);
        if keys.len() > MAX_NATIVE_KEYS {
            return Err("native key count exceeded".into());
        }
    }
    // Untouched ordinary segments can exhaust the remaining native slots after
    // an earlier singular segment expands, so validate the complete track too.
    if keys.len() > MAX_NATIVE_KEYS {
        return Err("native key count exceeded".into());
    }
    let track = PropertyKeyframeTrack::new(keys).map_err(|error| error.to_string())?;
    track
        .validate_for_target(&entry.target)
        .map_err(|error| error.to_string())?;
    Ok((PropertyAnimator::keyframes(track), messages))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singular_opacity_admission_boundary_and_checked_time() {
        let make_track = |times: &[i64]| {
            PropertyKeyframeTrack::new(
                times
                    .iter()
                    .enumerate()
                    .map(|(index, time)| {
                        PropertyKeyframe::new(
                            fx_schema::KeyframeId::new(format!("admission-{index}")),
                            TimeOffset::from_millis(*time),
                            PropertyValue::Float(50.0),
                            PropertyKeyframeEasing::CubicBezier {
                                x1: 0.55,
                                y1: 0.0,
                                x2: 1.0,
                                y2: 0.45,
                            },
                        )
                    })
                    .collect(),
            )
            .unwrap()
        };
        let track = make_track(&[0, 2]);
        let mut remaining = 2;
        assert!(reserve_samples(&track, &mut remaining).is_err());
        assert_eq!(remaining, 2);
        remaining = 3;
        reserve_samples(&track, &mut remaining).unwrap();
        assert_eq!(remaining, 0);
        let track = make_track(&[0, 2, 4]);
        remaining = 5;
        assert!(reserve_samples(&track, &mut remaining).is_err());
        assert_eq!(remaining, 5);
        remaining = 6;
        reserve_samples(&track, &mut remaining).unwrap();
        assert_eq!(remaining, 0);
        // Schema-valid long duration; invalid extreme times are rejected upstream.
        let track = make_track(&[0, 1_000_000_000]);
        remaining = MAX_OPACITY_SAMPLES;
        assert!(
            reserve_samples(&track, &mut remaining)
                .unwrap_err()
                .contains("sample limit")
        );
        assert_eq!(remaining, MAX_OPACITY_SAMPLES);
    }

    #[test]
    fn singular_opacity_out_of_range_x_controls_are_not_repaired() {
        for [x1, y1, x2, y2] in [[-0.1, 0.0, 1.0, 0.45], [0.0, 0.45, 1.1, 1.0]] {
            assert!(singular(PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 }).is_none());
        }
    }

    #[test]
    fn singular_opacity_nonfinite_controls_are_not_repaired() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(
                singular(PropertyKeyframeEasing::CubicBezier {
                    x1: 0.55,
                    y1: value,
                    x2: 1.0,
                    y2: 0.45,
                })
                .is_none()
            );
        }
    }
}
