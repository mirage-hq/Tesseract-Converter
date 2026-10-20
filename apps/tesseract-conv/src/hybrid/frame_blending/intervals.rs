//! Split an isolated picture's sampling switches without changing its source clock.
use fx_schema::{
    EditableFxCompositionDocument, Layer, LayerData, LayerId, LayerPlaybackMapping,
    PropertyKeyframeEasing,
};
use serde_json::json;
use std::collections::BTreeMap;

pub(super) fn split(
    document: &EditableFxCompositionDocument,
    layer: &Layer,
    owners: &mut BTreeMap<LayerId, LayerId>,
) -> anyhow::Result<Option<Layer>> {
    let LayerData::Video(video) = layer.data() else {
        return Ok(None);
    };
    // A single unanimated picture has no external matte/parent/property references
    // to redirect, nor temporal effects whose samples could cross a split.
    if document.composition().layers().len() != 1
        || !document.composition().dynamics().entries().is_empty()
        || video.parent.is_some()
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
        let (a, b) = (u64::try_from(a)?, u64::try_from(b)?);
        if let Some(last) = spans.last_mut().filter(|last| last.2 == slow) {
            last.1 = b;
        } else {
            spans.push((a, b, slow));
        }
    }
    if !spans.iter().any(|span| span.2) || !spans.iter().any(|span| !span.2) {
        return Ok(None);
    }
    let original = serde_json::to_value(layer)?;
    let id = original["id"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("video ID is not an integer"))?;
    if id.checked_add(u64::try_from(spans.len())?).is_none() {
        return Ok(None);
    }
    let mut children = Vec::with_capacity(spans.len());
    for (index, (start, end, slow)) in spans.into_iter().enumerate() {
        let child_id = id + u64::try_from(index)? + 1;
        let mut child = original.clone();
        child["id"] = json!(child_id);
        child["parent"] = json!(id);
        child["playback"]["inputRange"] = json!({"start":start,"duration":end-start});
        child["frameBlending"] = json!(slow);
        children.push(child);
        owners.insert(LayerId::new(child_id), video.id);
    }
    let result = serde_json::from_value(json!({
        "type":"Group", "id":id, "name":video.name,
        "playback":{"type":"windowed", "inputRange":range, "inputOffsetMs":0,
            "mapping":{"type":"linear", "input":range, "output":range}},
        "transform":{"anchorPoint":[0,0],"position":[0,0],"scale":[100,100],"rotation":0,"opacity":100},
        "layers":children
    }))?;
    Ok(Some(result))
}
