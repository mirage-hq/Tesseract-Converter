//! Read the public CLI's pre-windowed media clocks without changing source bytes.
//! The canonical FX schema stays strict; this archive boundary admits only the
//! historical Video/Audio/Group timing shapes with explicit positive ranges.
//! Legacy milliseconds may be unsigned integers or exact integer-valued floats.
use crate::{metadata::invalid, TesseractFileError};
use fx_schema::{
    Duration, EditableFxCompositionDocument, LayerPlayback, Time, TimeRangeProperty,
    TimeRemapExtrapolation, TimeRemapProperty,
};
use serde_json::{Map, Value};

pub(crate) fn read(bytes: &[u8]) -> Result<EditableFxCompositionDocument, TesseractFileError> {
    let original_error = match EditableFxCompositionDocument::from_json_slice(bytes) {
        Ok(document) => return Ok(document),
        Err(error) => error,
    };
    let mut value: Value =
        serde_json::from_slice(bytes).map_err(fx_schema::EditableFxDocumentError::from)?;
    let Some(layers) = value
        .pointer_mut("/composition/layers")
        .and_then(Value::as_array_mut)
    else {
        return Err(original_error.into());
    };
    if !migrate_layers(layers)? {
        return Err(original_error.into());
    }
    Ok(EditableFxCompositionDocument::from_json_value(value)?)
}

fn migrate_layers(layers: &mut [Value]) -> Result<bool, TesseractFileError> {
    let mut changed = false;
    for layer in layers {
        let Some(layer) = layer.as_object_mut() else {
            continue;
        };
        if matches!(
            layer.get("type").and_then(Value::as_str),
            Some("Video" | "Audio" | "Group")
        ) && layer.contains_key("activeRange")
        {
            let playback = migrate_clock(layer)?;
            layer.insert("playback".to_owned(), serde_json::to_value(playback)?);
            layer.remove("activeRange");
            changed = true;
        }
        if matches!(
            layer.get("type").and_then(Value::as_str),
            Some("Group" | "BooleanOperation" | "AiEdit")
        ) {
            if let Some(children) = layer.get_mut("layers").and_then(Value::as_array_mut) {
                changed |= migrate_layers(children)?;
            }
        }
    }
    Ok(changed)
}

fn exact_milliseconds(value: &Value) -> Option<u64> {
    if let Some(integer) = value.as_u64() {
        return Some(integer);
    }
    let float = value.as_f64()?;
    if !(0.0..=((1_u64 << 53) - 1) as f64).contains(&float) || float.fract() != 0.0 {
        return None;
    }
    // The exact-clock bound and integral check make this conversion lossless.
    Some(float as u64)
}

fn range(layer: &Map<String, Value>, field: &str) -> Result<TimeRangeProperty, TesseractFileError> {
    let value = layer
        .get(field)
        .ok_or_else(|| invalid(format!("legacy layer requires {field}")))?;
    if field == "activeRange"
        && value
            .as_object()
            .is_some_and(|range| range.keys().any(|key| key != "start" && key != "duration"))
    {
        return Err(invalid(
            "legacy activeRange contains unsupported clock fields",
        ));
    }
    let start = value.get("start").and_then(exact_milliseconds);
    let duration = value
        .get("duration")
        .and_then(exact_milliseconds)
        .filter(|duration| *duration > 0);
    let (Some(start), Some(duration)) = (start, duration) else {
        return Err(invalid(format!(
            "legacy {field} requires exact non-negative start and positive duration"
        )));
    };
    Ok(TimeRangeProperty::new(
        Time::from_millis(start),
        Duration::from_millis(duration),
    ))
}

fn migrate_clock(layer: &Map<String, Value>) -> Result<LayerPlayback, TesseractFileError> {
    let active = range(layer, "activeRange")?;
    let group = layer.get("type").and_then(Value::as_str) == Some("Group");
    if group && layer.contains_key("sourceRange") {
        return Err(invalid("legacy Group cannot contain sourceRange"));
    }
    let source = if group {
        None
    } else {
        Some(range(layer, "sourceRange")?)
    };
    let playback = layer.get("playback").filter(|value| !value.is_null());
    if let Some(playback) = playback {
        if playback.get("type").is_some() {
            return Err(invalid(
                "windowed playback cannot coexist with legacy activeRange",
            ));
        }
        let property: TimeRemapProperty = serde_json::from_value(playback.clone())
            .map_err(fx_schema::EditableFxDocumentError::from)?;
        let input = if property.before() == TimeRemapExtrapolation::Inactive
            && property.after() == TimeRemapExtrapolation::Inactive
        {
            let keys = property.keyframes();
            // TimeRemapProperty validates at least two increasing exact keys.
            TimeRangeProperty::new(
                keys[0].time,
                Duration::from_millis(
                    keys[keys.len() - 1].time.as_millis() - keys[0].time.as_millis(),
                ),
            )
        } else {
            active
        };
        return LayerPlayback::remapped(input, property, 0).map_err(invalid);
    }
    let output = match source {
        None => TimeRangeProperty::new(Time::ZERO, active.duration),
        Some(source) if layer.get("type").and_then(Value::as_str) == Some("Audio") => {
            TimeRangeProperty::new(source.start, active.duration)
        }
        Some(source) => source,
    };
    LayerPlayback::linear(active, active, output, 0).map_err(invalid)
}
