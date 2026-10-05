//! Independent placement and gain for the sound of a Dynamic Link composition.

#[cfg(test)]
mod tests;

use aftereffects_file::LinkedAudio;
use fx_conv::ConversionDiagnostic;
use fx_schema::{
    animator::{AnimationGraphEntry, AnimatorData, PropertyKeyframe, PropertyKeyframeTrack},
    AnimationGraph, KeyframeId, Layer, LayerData, LayerId, LayerPlayback, LayerPlaybackMapping,
    LinearGain, PropType, PropertyAnimator, PropertyKeyframeEasing, PropertyTarget, PropertyValue,
    TimeOffset, TimeRangeProperty,
};

use super::{
    background::{identity_transform, plain_group},
    effects::EffectIdAllocator,
    premiere_to_tesseract::placement_volume_track,
    timing::{duration_from_ticks, time_from_ticks},
};
use crate::{
    approximate,
    error::{unsupported, Result},
    linked_compositions::LinkedCompositions,
    omit,
    schema::PrAudioOccurrence,
    Omission, OmissionScope,
};

fn tick_range(start: i64, end: i64) -> Result<TimeRangeProperty> {
    Ok(TimeRangeProperty::new(
        time_from_ticks(start)?,
        duration_from_ticks(end - start)?,
    ))
}

/// Parent content milliseconds as a function of the Premiere item's local time.
#[derive(Clone, Copy)]
pub(super) struct Clock {
    pub(super) rate: f64,
    pub(super) offset: f64,
}

impl Clock {
    pub(super) fn child(self, playback: &LayerPlayback) -> Option<Self> {
        let (input_start, output_start, rate) = match playback.mapping() {
            LayerPlaybackMapping::Linear { input, output } => (
                input.start.as_millis() as f64,
                output.start.as_millis() as f64,
                output.duration.as_millis() as f64 / input.duration.as_millis() as f64,
            ),
            LayerPlaybackMapping::TimeRemap { property } => {
                let [first, last] = property.keyframes() else {
                    return None;
                };
                let window = playback.input_range();
                let start = window.start.as_millis() as f64 + playback.input_offset_ms() as f64;
                let end = start + window.duration.as_millis() as f64;
                if last.easing != PropertyKeyframeEasing::Linear
                    || start < first.time.as_millis() as f64
                    || end > last.time.as_millis() as f64
                {
                    return None;
                }
                (
                    first.time.as_millis() as f64,
                    first.value.as_millis() as f64,
                    (last.value.as_millis() as f64 - first.value.as_millis() as f64)
                        / (last.time.as_millis() as f64 - first.time.as_millis() as f64),
                )
            }
        };
        let offset =
            output_start + (self.offset + playback.input_offset_ms() as f64 - input_start) * rate;
        let rate = self.rate * rate;
        (rate.is_finite() && rate > 0.0 && offset.is_finite()).then_some(Self { rate, offset })
    }

    fn property(self, playback: &LayerPlayback) -> Option<Self> {
        let LayerPlaybackMapping::Linear { input, .. } = playback.mapping() else {
            return None;
        };
        Some(Self {
            rate: self.rate,
            // Ordinary audio animators follow the authored input clock, not
            // the decoding source offset or playback rate of the media itself.
            offset: self.offset + playback.input_offset_ms() as f64
                - input.start.as_millis() as f64,
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn layer(
    clip: &PrAudioOccurrence,
    parent: Option<LayerId>,
    next_index: &mut usize,
    effect_ids: &mut EffectIdAllocator,
    linked: &mut LinkedCompositions<'_>,
    dynamics: &mut AnimationGraph,
    intrinsic_ticks: i64,
    omissions: &mut Vec<Omission>,
) -> Result<Option<Layer>> {
    if !linked.contains(&clip.media) {
        omit(
            omissions,
            OmissionScope::Occurrence,
            clip.record(),
            "linked After Effects composition has no resolved project; its audio item was omitted",
        );
        return Ok(None);
    }
    let first_id = (*next_index as u64)
        .checked_add(1)
        .ok_or_else(|| unsupported("linked audio identities exceed the identity range"))?
        .max(effect_ids.next());
    let wrapper_id = LayerId::new(first_id);
    let content_id = first_id
        .checked_add(1)
        .ok_or_else(|| unsupported("linked audio identities exceed the identity range"))?;
    let source_range = if clip.playback_rate < 0.0 {
        let source_in = intrinsic_ticks - clip.out_ticks;
        if source_in < 0 {
            approximate(
                omissions,
                clip.record(),
                super::audio::REVERSE_SOURCE_START_CLIPPING_WARNING,
            );
        }
        tick_range(source_in.max(0), intrinsic_ticks - clip.in_ticks)?
    } else {
        tick_range(clip.in_ticks, clip.out_ticks)?
    };
    let source_end = if clip.playback_rate < 0.0 {
        intrinsic_ticks - clip.in_ticks
    } else {
        clip.out_ticks
    };
    let mut audio = match linked.audio(
        &clip.media,
        wrapper_id,
        content_id,
        time_from_ticks(source_end)?,
    )? {
        Ok(audio) => audio,
        Err(reason) => {
            omit(omissions, OmissionScope::Occurrence, clip.record(), reason);
            return Ok(None);
        }
    };
    let volume_track = match placement_volume_track(clip, wrapper_id, intrinsic_ticks, omissions) {
        Ok(track) => track,
        Err(error) => {
            omit(
                omissions,
                OmissionScope::Occurrence,
                clip.record(),
                format!("linked audio gain could not be imported: {error}"),
            );
            return Ok(None);
        }
    };
    if let Err(error) = apply_gain(&mut audio, clip, volume_track.as_ref(), omissions) {
        omit(
            omissions,
            OmissionScope::Occurrence,
            clip.record(),
            format!("linked audio gain could not be represented: {error}"),
        );
        return Ok(None);
    }
    let active = tick_range(clip.start_ticks, clip.end_ticks)?;
    let mut wrapper = plain_group(
        wrapper_id,
        "Premiere linked AE audio".into(),
        active,
        identity_transform(),
        vec![audio.root],
    )?;
    wrapper.parent = parent;
    wrapper.playback = super::audio::playback(clip, active, source_range, wrapper_id)?;
    if clip.playback_rate != 1.0 {
        approximate(omissions, clip.record(), "linked audio clock retains independently rounded source/placement endpoints; sound-only group export still diagnoses clock approximation");
    }
    if clip.preserve_audio_pitch {
        approximate(omissions, clip.record(), "linked outer pitch-ON is projected onto editable descendant Audio pitch flags; independently staged inner/outer pitch decisions are not represented");
        if !crate::schema::RUNTIME_PITCH_RATES.contains(&clip.playback_rate) {
            approximate(omissions, clip.record(), "linked pitch-ON clock lies outside the existing forward 0.25x–4x preservation range; ordinary mapped resampling is used");
        }
    }
    *next_index = usize::try_from(audio.next_id - 1)
        .map_err(|_| unsupported("linked audio identities exceed the index range"))?;
    effect_ids.skip_to(audio.next_id);
    let mut entries = dynamics.entries().to_vec();
    entries.extend(audio.animations);
    *dynamics = AnimationGraph::from_entries(entries)
        .map_err(super::premiere_to_tesseract::map_animation_graph_error)?;
    for diagnostic in audio.diagnostics {
        let reason = format!("linked After Effects sound: {diagnostic}");
        if diagnostic.diagnostic().kind == fx_conv::DiagnosticKind::Approximation {
            approximate(omissions, clip.record(), reason);
        } else {
            omit(omissions, OmissionScope::Feature, clip.record(), reason);
        }
    }
    Ok(Some(Layer::from_data(&LayerData::Group(wrapper))?))
}

fn scaled(value: &PropertyValue, gain: f64) -> Result<PropertyValue> {
    let PropertyValue::Float(value) = value else {
        return Err(unsupported("linked AudioVolume is not a scalar gain"));
    };
    let value = LinearGain::new(value * gain).map_err(|error| unsupported(error.to_string()))?;
    Ok(PropertyValue::Float(value.as_f64()))
}

fn scaled_animator(animator: &PropertyAnimator, gain: f64) -> Result<PropertyAnimator> {
    let data = match animator.data() {
        AnimatorData::Constant { value } => AnimatorData::Constant {
            value: scaled(value, gain)?,
        },
        AnimatorData::Keyframes {
            track,
            enabled,
            disabled_value,
        } => AnimatorData::Keyframes {
            track: PropertyKeyframeTrack::new(
                track
                    .keyframes()
                    .iter()
                    .map(|key| {
                        Ok(PropertyKeyframe::new(
                            key.id().clone(),
                            key.layer_time(),
                            scaled(key.value(), gain)?,
                            key.easing(),
                        ))
                    })
                    .collect::<Result<_>>()?,
            )
            .map_err(|error| unsupported(error.to_string()))?,
            enabled: *enabled,
            disabled_value: disabled_value
                .as_ref()
                .map(|value| scaled(value, gain))
                .transpose()?,
        },
        AnimatorData::JsScript { .. } => {
            return Err(unsupported("linked audio gain cannot contain a script"))
        }
    };
    Ok(PropertyAnimator::from_data(&data)?)
}

fn placed_track(
    track: &PropertyKeyframeTrack,
    id: LayerId,
    clock: Clock,
    gain: f64,
) -> Result<PropertyAnimator> {
    let keys = track
        .keyframes()
        .iter()
        .enumerate()
        .map(|(index, key)| {
            let millis = clock.offset + clock.rate * key.layer_time().as_millis() as f64;
            if !millis.is_finite() || millis.abs() > ((1_u64 << 53) - 1) as f64 {
                return Err(unsupported(
                    "linked audio gain key exceeds the exact millisecond range",
                ));
            }
            Ok(PropertyKeyframe::new(
                KeyframeId::new(format!("linked-audio-{id}-{index}")),
                TimeOffset::from_millis(millis.round() as i64),
                scaled(key.value(), gain)?,
                key.easing(),
            ))
        })
        .collect::<Result<_>>()?;
    Ok(PropertyAnimator::keyframes(
        PropertyKeyframeTrack::new(keys).map_err(|error| unsupported(error.to_string()))?,
    ))
}

/// Keep both editable envelopes. Original knots and Hold jumps survive;
/// quarter-segment samples approximate curvature without per-frame baking.
fn product_track(
    inner: &PropertyKeyframeTrack,
    outer: &PropertyKeyframeTrack,
    id: LayerId,
) -> Result<PropertyAnimator> {
    use std::collections::BTreeSet;
    let mut times = BTreeSet::new();
    let mut holds = BTreeSet::new();
    for key in inner.keyframes().iter().chain(outer.keyframes()) {
        let time = key.layer_time().as_millis();
        times.insert(time);
        if key.easing() == PropertyKeyframeEasing::Hold {
            holds.insert(time);
            times.insert(time.saturating_sub(1));
        }
    }
    let knots: Vec<_> = times.iter().copied().collect();
    let mut inserted = 0;
    for pair in knots.windows(2) {
        for fraction in 1..4 {
            if inserted == 4096 {
                break;
            }
            let time =
                i128::from(pair[0]) + (i128::from(pair[1]) - i128::from(pair[0])) * fraction / 4;
            inserted += usize::from(times.insert(
                i64::try_from(time).map_err(|_| unsupported("gain knot exceeds its time range"))?,
            ));
        }
        if inserted == 4096 {
            break;
        }
    }
    let keys = times
        .into_iter()
        .enumerate()
        .map(|(index, time)| {
            let left = super::audio::sample_volume(inner, time)
                .ok_or_else(|| unsupported("invalid AE gain curve"))?;
            let right = super::audio::sample_volume(outer, time)
                .ok_or_else(|| unsupported("invalid Premiere gain curve"))?;
            Ok(PropertyKeyframe::new(
                KeyframeId::new(format!("linked-product-{id}-{index}")),
                TimeOffset::from_millis(time),
                scaled(&PropertyValue::Float(left), right)?,
                if holds.contains(&time) {
                    PropertyKeyframeEasing::Hold
                } else {
                    PropertyKeyframeEasing::Linear
                },
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(PropertyAnimator::keyframes(
        PropertyKeyframeTrack::new(keys).map_err(|error| unsupported(error.to_string()))?,
    ))
}

fn apply_gain(
    audio: &mut LinkedAudio,
    clip: &PrAudioOccurrence,
    track: Option<&PropertyKeyframeTrack>,
    omissions: &mut Vec<Omission>,
) -> Result<()> {
    fn visit(
        layer: &mut Layer,
        clock: Option<Clock>,
        entries: &mut Vec<AnimationGraphEntry>,
        clip: &PrAudioOccurrence,
        track: Option<&PropertyKeyframeTrack>,
        omissions: &mut Vec<Omission>,
    ) -> Result<()> {
        let mut data = layer.data().clone();
        match &mut data {
            LayerData::Group(group) => {
                let clock = clock.and_then(|clock| clock.child(&group.playback));
                for child in &mut group.layers {
                    visit(child, clock, entries, clip, track, omissions)?;
                }
            }
            LayerData::Audio(sound) => {
                if clip.preserve_audio_pitch {
                    sound.preserve_audio_pitch = true;
                }
                let target = PropertyTarget::layer(sound.id, PropType::AudioVolume);
                let existing = entries.iter().position(|entry| entry.target == target);
                let static_gain = match existing.map(|index| entries[index].animator.data()) {
                    None => Some(sound.volume.as_f64()),
                    Some(AnimatorData::Constant {
                        value: PropertyValue::Float(value),
                    }) => Some(*value),
                    Some(AnimatorData::Keyframes {
                        enabled: false,
                        disabled_value,
                        ..
                    }) => Some(match disabled_value {
                        Some(PropertyValue::Float(value)) => *value,
                        _ => sound.volume.as_f64(),
                    }),
                    _ => None,
                };
                let placed = match (
                    track,
                    static_gain,
                    clock.and_then(|clock| clock.property(&sound.playback)),
                ) {
                    (Some(track), Some(gain), Some(clock)) => {
                        Some(placed_track(track, sound.id, clock, gain)?)
                    }
                    (Some(track), None, Some(clock)) => {
                        let Some(AnimatorData::Keyframes { track: inner, .. }) =
                            existing.map(|index| entries[index].animator.data())
                        else {
                            return Err(unsupported("linked audio gain cannot contain a script"));
                        };
                        let mapped = placed_track(track, sound.id, clock, 1.0)?;
                        let AnimatorData::Keyframes { track: outer, .. } = mapped.data() else {
                            return Err(unsupported("linked gain mapping did not produce keys"));
                        };
                        let animator = product_track(inner, outer, sound.id)?;
                        approximate(omissions, clip.record(), format!("linked sound layer {} combines AE and Premiere gain with editable Linear/Hold keys at source knots and up to 4096 quarter-segment samples; curvature between samples is approximated", sound.id));
                        Some(animator)
                    }
                    (Some(_), _, None) => {
                        approximate(omissions, clip.record(), format!("linked sound layer {} retains AE gain with the Premiere item's static gain; gain timing through a non-affine owner clock is approximated", sound.id));
                        None
                    }
                    _ => None,
                };
                sound.volume = LinearGain::new(sound.volume.as_f64() * clip.volume.as_f64())
                    .map_err(|error| unsupported(error.to_string()))?;
                if let Some(animator) = placed {
                    if let Some(index) = existing {
                        entries[index].animator = animator;
                    } else {
                        entries.push(AnimationGraphEntry {
                            target,
                            animator,
                            dependencies: Vec::new(),
                            random_seed_target: None,
                            layer_refs: Default::default(),
                        });
                    }
                } else if let Some(index) = existing {
                    entries[index].animator =
                        scaled_animator(&entries[index].animator, clip.volume.as_f64())?;
                }
            }
            _ => return Ok(()),
        }
        *layer = Layer::from_data(&data)?;
        Ok(())
    }
    let active = tick_range(clip.start_ticks, clip.end_ticks)?;
    let source = tick_range(clip.in_ticks, clip.out_ticks)?;
    let clock = if clip.uses_layer_clock() {
        Clock {
            rate: source.duration.as_millis() as f64 / active.duration.as_millis() as f64,
            offset: source.start.as_millis() as f64,
        }
    } else {
        // Retimed placement gain already uses the composition/source clock.
        Clock {
            rate: 1.0,
            offset: 0.0,
        }
    };
    visit(
        &mut audio.root,
        Some(clock),
        &mut audio.animations,
        clip,
        track,
        omissions,
    )
}
