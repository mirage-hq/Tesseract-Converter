//! Separate native sampling switches while retaining the complete authored clock.
use super::*;
use fx_schema::layer::FrameBlendingData;
use serde_json::json;

pub(super) fn isolated_root_occurrences(
    document: &EditableFxCompositionDocument,
    roots: &[Layer],
) -> Result<Option<Vec<Layer>>, AepWriteError> {
    let [layer] = roots else { return Ok(None) };
    if document.composition().layers().len() != 1
        || !document.composition().dynamics().entries().is_empty()
        || layer.parent_id().is_some()
    {
        return Ok(None);
    }
    let Some(first_extra_id) = layer.id().value().checked_add(1) else {
        return Ok(None);
    };
    video_occurrences(layer, first_extra_id)
}

pub(super) fn nested_occurrences(
    document: &EditableFxCompositionDocument,
    roots: &[Layer],
) -> Result<Option<Vec<Layer>>, AepWriteError> {
    let composition = document.composition();
    let facts = source_variants::DocumentReferenceFacts::analyze(
        composition.layers(),
        composition.segment_layer_ids(),
        composition.dynamics().entries(),
    )
    .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))?;
    let Some(next_id) = facts
        .occupied_layer_ids()
        .last()
        .map_or(Some(1), |id| id.value().checked_add(1))
    else {
        return Ok(None);
    };
    let mut planner = NestedIntervals {
        document,
        facts,
        next_id,
        changed: false,
    };
    let layers = roots
        .iter()
        .map(|layer| planner.layer(layer))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(planner.changed.then_some(layers))
}

struct NestedIntervals<'a> {
    document: &'a EditableFxCompositionDocument,
    facts: source_variants::DocumentReferenceFacts,
    next_id: u64,
    changed: bool,
}

impl NestedIntervals<'_> {
    fn layer(&mut self, layer: &Layer) -> Result<Layer, AepWriteError> {
        let LayerData::Group(group) = layer.data() else {
            return Ok(layer.clone());
        };
        if !matches!(group.playback.mapping(), LayerPlaybackMapping::Linear { input, output }
            if input.duration == output.duration)
            || group.motion_blur
        {
            return Ok(layer.clone());
        }
        let mut children = None;
        if let [video] = group.layers.as_slice()
            && video.parent_id().is_none_or(|parent| parent == group.id)
        {
            let mut eligibility = self.facts.eligibility(video.id());
            // Containment is retained; no other consumer may observe the copies.
            eligibility.nested_or_parented = false;
            let animated = self
                .document
                .composition()
                .dynamics()
                .entries()
                .iter()
                .any(|entry| entry.target.layer_id() == Some(video.id()));
            if !animated
                && !eligibility.has_external_references()
                && !eligibility.has_unsupported_graph_edges()
                && let Some(occurrences) = video_occurrences(video, self.next_id)?
            {
                self.next_id += u64::try_from(occurrences.len() - 1).map_err(|_| {
                    AepWriteError::Invalid("blending occurrence count exceeds identity range")
                })?;
                children = Some(occurrences);
                self.changed = true;
            }
        }
        let children = match children {
            Some(children) => children,
            None => group
                .layers
                .iter()
                .map(|child| self.layer(child))
                .collect::<Result<Vec<_>, _>>()?,
        };
        let mut value = layer.wire_value().clone();
        value["layers"] = serde_json::to_value(children)
            .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))?;
        serde_json::from_value(value)
            .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))
    }
}

fn video_occurrences(
    layer: &Layer,
    first_extra_id: u64,
) -> Result<Option<Vec<Layer>>, AepWriteError> {
    let LayerData::Video(video) = layer.data() else {
        return Ok(None);
    };
    // Splitting must not change temporal effects or shared ownership.
    if !matches!(video.frame_blending, Some(FrameBlendingData::Boolean(true)))
        || video.track_matte.is_some()
        || !video.masks.is_empty()
        || !video.effects.is_empty()
        || video.motion_blur
        || video.transform.position.is_3d()
        || video.transform.rotation_x != 0.0
        || video.transform.rotation_y != 0.0
        || video.transform.orientation != [0.0; 3]
        || video.placement.is_some()
        || video.source.time_remap.is_some()
        || video.blend_mode != fx_schema::BlendMode::Normal
    {
        return Ok(None);
    }
    let LayerPlaybackMapping::TimeRemap { property } = video.playback.mapping() else {
        return Ok(None);
    };
    let range = video.playback.input_range();
    let offset = i128::from(video.playback.input_offset_ms());
    let start = i128::from(range.start.as_millis());
    let end = i128::from(range.end().as_millis());
    let keys = property.keyframes();
    if keys.len() < 2
        || start + offset < i128::from(keys[0].time.as_millis())
        || end + offset > i128::from(keys[keys.len() - 1].time.as_millis())
        || keys.iter().skip(1).any(|key| {
            !matches!(
                key.easing,
                PropertyKeyframeEasing::Linear | PropertyKeyframeEasing::Hold
            )
        })
    {
        return Ok(None);
    }
    let mut spans: Vec<(u64, u64, bool)> = Vec::new();
    for pair in keys.windows(2) {
        let a = (i128::from(pair[0].time.as_millis()) - offset).max(start);
        let b = (i128::from(pair[1].time.as_millis()) - offset).min(end);
        if a >= b {
            continue;
        }
        let distance = pair[1]
            .value
            .as_millis()
            .abs_diff(pair[0].value.as_millis());
        let elapsed = pair[1].time.as_millis() - pair[0].time.as_millis();
        let slow = matches!(pair[1].easing, PropertyKeyframeEasing::Linear)
            && distance > 0
            && distance < elapsed;
        // These intersections lie inside the unsigned active range.
        let a = u64::try_from(a).map_err(|_| AepWriteError::Invalid("invalid blending span"))?;
        let b = u64::try_from(b).map_err(|_| AepWriteError::Invalid("invalid blending span"))?;
        if let Some(last) = spans.last_mut().filter(|last| last.2 == slow) {
            last.1 = b;
        } else {
            spans.push((a, b, slow));
        }
    }
    if !spans.iter().any(|span| span.2) || !spans.iter().any(|span| !span.2) {
        return Ok(None);
    }
    let extra_ids = u64::try_from(spans.len() - 1)
        .map_err(|_| AepWriteError::Invalid("blending occurrence count exceeds identity range"))?;
    if first_extra_id.checked_add(extra_ids).is_none() {
        return Ok(None);
    }
    let original = serde_json::to_value(layer)
        .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))?;
    let occurrences = spans
        .into_iter()
        .enumerate()
        .map(|(index, (start, end, slow))| {
            let mut occurrence = original.clone();
            let index = u64::try_from(index).map_err(|_| {
                AepWriteError::Invalid("blending occurrence index exceeds identity range")
            })?;
            occurrence["id"] = json!(if index == 0 {
                video.id.value()
            } else {
                first_extra_id + index - 1
            });
            occurrence["playback"]["inputRange"] = json!({"start":start,"duration":end-start});
            occurrence["frameBlending"] = json!(slow);
            serde_json::from_value(occurrence)
                .map_err(|error| AepWriteError::InvalidDocument(error.to_string()))
        })
        .collect::<Result<Vec<Layer>, _>>()?;
    Ok(Some(occurrences))
}
