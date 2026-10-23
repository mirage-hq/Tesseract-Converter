//! Load-only migration from font animators to static text selections.

use serde_json::Value;

/// Fold retired font animators before graph validation in either composition reader.
/// Invalid or unrepresentable retired values retain the authored static selection;
/// migration itself never rejects a document. Other animators and their dependencies
/// are retained: font dependencies are now ambient model values.
pub fn migrate_static_fonts(document: &mut Value) {
    let entries = document
        .get_mut("dynamics")
        .and_then(|graph| graph.get_mut("entries"))
        .and_then(Value::as_array_mut)
        .map(std::mem::take);
    if let Some(entries) = entries {
        let mut retained = Vec::with_capacity(entries.len());
        for entry in entries {
            let target = entry.get("target").or_else(|| entry.get("property"));
            let Some(target) = target else {
                retained.push(entry);
                continue;
            };
            let field = match target.get("propertyType").and_then(Value::as_str) {
                Some("fontFamily") => "fontFamily",
                Some("fontStyle") => "fontStyle",
                _ => {
                    retained.push(entry);
                    continue;
                }
            };
            let layer_id = target.get("layerId").and_then(Value::as_u64);
            let animator = &entry["animator"];
            if animator.get("enabled").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            let value = match animator.get("type").and_then(Value::as_str) {
                Some("constant") => string_value(&animator["value"]),
                Some("keyframes") => {
                    let keys = animator.get("keyframes").and_then(Value::as_array);
                    let first = keys.and_then(|keys| keys.first()).map(|key| &key["value"]);
                    if let (Some(keys), Some(first)) = (keys, first) {
                        if keys.iter().any(|key| &key["value"] != first) {
                            log::warn!("migrating time-varying {field} on text layer {layer_id:?} to its first keyframe; font selection is static");
                        }
                    }
                    first.and_then(string_value)
                }
                _ => None,
            };
            if let (Some(layer_id), Some(value)) = (layer_id, value) {
                if let Some(text) = find_text(document.get_mut("layers"), layer_id) {
                    text[field] = Value::String(value.to_owned());
                }
            } else {
                log::warn!("dropping retired {field} animator on layer {layer_id:?}; retaining sourceText because no static string value is available");
            }
        }
        document["dynamics"]["entries"] = Value::Array(retained);
    }
    if let Some(layers) = document.get_mut("layers").and_then(Value::as_array_mut) {
        for layer in layers {
            migrate_static_fonts(layer);
        }
    }
}

fn string_value(value: &Value) -> Option<&str> {
    (value.get("type")?.as_str()? == "string")
        .then(|| value.get("value")?.as_str())
        .flatten()
}

fn find_text(layers: Option<&mut Value>, id: u64) -> Option<&mut Value> {
    for layer in layers?.as_array_mut()? {
        if layer.get("id").and_then(Value::as_u64) == Some(id)
            && layer.get("type").and_then(Value::as_str) == Some("Text")
        {
            return layer.get_mut("sourceText").filter(|text| text.is_object());
        }
        if let Some(text) = find_text(layer.get_mut("layers"), id) {
            return Some(text);
        }
    }
    None
}
