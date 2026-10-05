//! Owner-domain selection and conservative equivalence of the runtime clock chain.
//! JS and keys share this coordinate; playback is deliberately not sampled here.
use super::*;
use fx_schema::{LayerPlayback, LayerPlaybackMapping, TimeRangeProperty};

#[derive(Default)]
pub(super) struct Clocks {
    signatures: BTreeMap<Vec<String>, usize>,
}

impl Clocks {
    pub(super) fn owner(&mut self, layer: &Layer, parent: &[String]) -> (Owner, Vec<String>) {
        let mut stages = parent.to_vec();
        // Effect identity is intentionally retained: unequal Posterize chains
        // must never be declared equivalent merely because durations agree.
        for effect in layer.effects() {
            let wire = effect.wire_value();
            let payload = wire.get("effect").unwrap_or(wire);
            if payload.get("type").and_then(serde_json::Value::as_str) == Some("posterizeTime") {
                stages.push(wire.to_string());
            }
        }
        let inherited_remap = parent.iter().any(|stage| stage.starts_with("remap:"));
        let domain = domain(layer, inherited_remap);
        let (start, end, mut unsupported_clock) = match domain {
            Some((start, end)) => (start, end, false),
            None => (0, layer.active_range().duration.as_millis(), true),
        };
        unsupported_clock |= parent.iter().any(|stage| stage == "unsupported-clock");
        if unsupported_clock {
            stages.push("unsupported-clock".into());
        }
        let playback = match layer.data() {
            LayerData::Group(group) => Some(&group.playback),
            LayerData::Video(video) => Some(&video.playback),
            LayerData::Audio(audio) => Some(&audio.playback),
            _ => None,
        };
        let stage = match playback {
            Some(playback) if playback.time_remap().is_some() => {
                format!(
                    "remap:{}:window:{start}:{end}",
                    serde_json::to_string(playback).expect("validated playback serializes")
                )
            }
            Some(playback)
                if matches!(layer.data(), LayerData::Group(_))
                    && !inherited_remap
                    && positive_group_window(playback).is_none() =>
            {
                format!(
                    "offset:{}:{start}:{end}",
                    layer.active_range().start.as_millis()
                )
            }
            Some(playback) => match playback.mapping() {
                LayerPlaybackMapping::Linear { input, output } => {
                    let media = !matches!(layer.data(), LayerData::Group(_));
                    if media && inherited_remap {
                        format!(
                            "media-input:{}:{}:{start}:{end}",
                            serde_json::to_string(&playback.input_range())
                                .expect("validated range serializes"),
                            i128::from(input.start.as_millis())
                                - i128::from(playback.input_offset_ms())
                        )
                    } else if media {
                        format!(
                            "offset:{}:{}:{}",
                            i128::from(input.start.as_millis())
                                - i128::from(playback.input_offset_ms()),
                            start,
                            end
                        )
                    } else if input.duration == output.duration {
                        let offset = i128::from(input.start.as_millis())
                            - i128::from(playback.input_offset_ms())
                            - i128::from(output.start.as_millis());
                        let window = playback.input_range();
                        if inherited_remap
                            && (offset != 0
                                || window.start.as_millis() != start
                                || window.end().as_millis() != end)
                        {
                            format!(
                                "{}:window:{start}:{end}",
                                serde_json::to_string(playback)
                                    .expect("validated playback serializes")
                            )
                        } else {
                            format!("offset:{offset}:{start}:{end}")
                        }
                    } else {
                        serde_json::to_string(playback).expect("validated playback serializes")
                    }
                }
                LayerPlaybackMapping::TimeRemap { .. } => unreachable!("handled above"),
            },
            None => format!(
                "offset:{}:{start}:{end}",
                layer.active_range().start.as_millis()
            ),
        };
        // Applying the same zero-offset clamp twice is idempotent. This proves
        // identity Group wrappers in the source's dependency triples, without
        // dropping a distinct window, offset, retime or Posterize stage.
        let zero_clamp = stage.starts_with("offset:0:");
        if !zero_clamp || stages.last() != Some(&stage) {
            stages.push(stage);
        }
        let next = self.signatures.len();
        let clock_id = *self.signatures.entry(stages.clone()).or_insert(next);
        (
            Owner {
                id: layer.id(),
                start_ms: start,
                duration_ms: end.saturating_sub(start),
                unsupported_clock,
                clock_id,
            },
            stages,
        )
    }
}

fn domain(layer: &Layer, inherited_remap: bool) -> Option<(u64, u64)> {
    match layer.data() {
        LayerData::Video(video) => media_domain(&video.playback, video.source_range),
        LayerData::Audio(audio) => media_domain(&audio.playback, audio.source_range),
        LayerData::Group(group) if inherited_remap || group.playback.time_remap().is_some() => {
            Some((0, content_end(&group.layers)))
        }
        LayerData::Group(group) => Some(
            positive_group_window(&group.playback)
                // Runtime retains the initially installed own-active clock if the
                // Group cannot compose a positive affine clock. This is an actual
                // runtime fallback, not a default value for a missing dependency.
                .unwrap_or((0, group.playback.input_range().duration.as_millis())),
        ),
        _ if layer
            .wire_value()
            .get("playback")
            .is_some_and(|value| !value.is_null()) =>
        {
            None
        }
        _ => Some((0, layer.active_range().duration.as_millis())),
    }
}
fn positive_group_window(playback: &LayerPlayback) -> Option<(u64, u64)> {
    let LayerPlaybackMapping::Linear { input, output } = playback.mapping() else {
        return None;
    };
    // Mirror LayerPlayback::positive_affine_clock and GroupLayerExt's anchored
    // ClockTransform endpoint truncation. Select a domain, not sample clocks.
    let range = playback.input_range();
    let rate = output.duration.as_millis() as f64 / input.duration.as_millis() as f64;
    let map = |time: u64| {
        let authored = i128::from(time) + i128::from(playback.input_offset_ms());
        output.start.as_millis() as f64 + (authored as f64 - input.start.as_millis() as f64) * rate
    };
    let start = map(range.start.as_millis());
    let end = map(range.end().as_millis());
    let exact = 0.0..=MAX_EXACT_SCRIPT_MILLIS as f64;
    if !rate.is_finite()
        || rate <= 0.0
        || start.fract() != 0.0
        || !exact.contains(&start)
        || !exact.contains(&end)
    {
        return None;
    }
    Some((
        start as u64,
        (start + range.duration.as_millis() as f64 * rate) as u64,
    ))
}

fn content_end(layers: &[Layer]) -> u64 {
    layers
        .iter()
        .map(|layer| match layer.data() {
            // Runtime Layer::effective_active_range differs only for PAG: its
            // authored playback key interval replaces the fallback active range.
            LayerData::Pag(pag) => pag
                .playback
                .as_ref()
                .and_then(|playback| playback.keyframes().last())
                .map_or(pag.active_range.end().as_millis(), |key| {
                    key.time.as_millis()
                }),
            _ => layer.active_range().end().as_millis(),
        })
        .max()
        .unwrap_or(0)
}

fn media_domain(playback: &LayerPlayback, source: TimeRangeProperty) -> Option<(u64, u64)> {
    match playback.mapping() {
        LayerPlaybackMapping::Linear { input, .. } => Some((0, input.duration.as_millis())),
        LayerPlaybackMapping::TimeRemap { .. } => {
            Some((source.start.as_millis(), source.end().as_millis()))
        }
    }
}
