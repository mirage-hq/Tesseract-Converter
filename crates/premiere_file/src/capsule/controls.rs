//! Saved instance values are optional overrides, not template admission gates.

use super::*;
use crate::{format::Element, schema::PrColour};

fn encoded_value(param: Element<'_>) -> Result<Option<EncodedValue>, CapsuleError> {
    let values = param
        .children()
        .filter(|field| field.tag() == "StartKeyframeValue")
        .collect::<Vec<_>>();
    if values.len() > 1 {
        return Err(invalid("ambiguous saved parameter value binding"));
    }
    Ok(values.first().map(|value| EncodedValue {
        encoding: value.attribute("Encoding").unwrap_or_default().into(),
        binary_hash: value.attribute("BinaryHash").map(str::to_owned),
        value: value.text().unwrap_or_default().into(),
    }))
}

pub(super) fn saved_layout(
    graph: &Graph<'_>,
    param: Element<'_>,
    kind: u32,
    metadata: &serde_json::Map<String, serde_json::Value>,
    owner: &str,
    diagnostics: &mut Vec<String>,
) -> Result<CapsuleValue, CapsuleError> {
    let field = |name| param.child(name).and_then(Element::text);
    let expected = if kind == 4 { "24" } else { "11" };
    if field("ParameterControlType") != Some(expected)
        || field("StartKeyframePosition") != Some(records::STATIC_KEYFRAME_TIME)
        || field("IsTimeVarying").is_some_and(|value| value != "false")
        || field("Keyframes").is_some_and(|value| !value.is_empty())
    {
        diagnostics.push(format!(
            "{owner}: unsupported layout override type/clock; template retained"
        ));
        return Ok(CapsuleValue::Unsupported { kind });
    }
    let stored = match encoded_value(param)? {
        Some(encoded) => decode_string(graph, &encoded, owner)?,
        None if kind == 4
            && metadata
                .get("capPropDefault")
                .and_then(serde_json::Value::as_str)
                == Some("") =>
        {
            String::new()
        }
        None => {
            diagnostics.push(format!(
                "{owner}: missing layout override; template retained"
            ));
            return Ok(CapsuleValue::Unsupported { kind });
        }
    };
    for field in metadata.keys() {
        if !matches!(
            field.as_str(),
            "capPropDefault" | "capPropGroupExpanded" | "capPropAnimatable" | "capPropUIName"
        ) {
            diagnostics.push(format!("{owner}: unsupported optional layout field {field:?}; effective layout value retained"));
        }
    }
    if kind == 4 {
        return Ok(CapsuleValue::String(stored));
    }
    let children = decode_children(&stored)?;
    let expanded = metadata
        .get("capPropGroupExpanded")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    Ok(CapsuleValue::Group { children, expanded })
}

/// Premiere's native parameter-control code for saved media replacements.
const MEDIA_DEPENDENCY_PARAMETER_CONTROL_TYPE: &str = "34";

pub(super) fn validate_media_dependency(param: Element<'_>) -> Result<(), String> {
    let field = |name| param.child(name).and_then(Element::text);
    if field("ParameterControlType") != Some(MEDIA_DEPENDENCY_PARAMETER_CONTROL_TYPE) {
        return Err("saved media parameter type does not match its dependency".into());
    }
    if field("IsTimeVarying").is_some_and(|value| value != "false")
        || field("Keyframes").is_some_and(|value| !value.is_empty())
    {
        return Err("keyed media replacement is not mapped".into());
    }
    if field("StartKeyframePosition").is_some_and(|value| value != records::STATIC_KEYFRAME_TIME) {
        return Err("unsupported media replacement clock".into());
    }
    Ok(())
}

pub(super) fn saved_numeric(
    param: Element<'_>,
    kind: u32,
    size: CapsulePoint,
) -> Result<SavedGraphicNumeric, String> {
    let field = |name| param.child(name).and_then(Element::text);
    let expected = match kind {
        1 => "8",
        2 => "4",
        3 => "5",
        5 => "3",
        6 => "6",
        kind => return Err(format!("unsupported saved controller type {kind}")),
    };
    if field("ParameterControlType") != Some(expected) {
        return Err("saved parameter type does not match the controller".into());
    }
    if field("IsTimeVarying").is_some_and(|value| value != "false")
        || field("Keyframes").is_some_and(|value| !value.is_empty())
    {
        return Err("keyed override is not mapped".into());
    }
    let wire = field("StartKeyframe").ok_or("missing saved override")?;
    let fields = wire.split(',').collect::<Vec<_>>();
    if fields.len() < 2 || fields[0] != records::STATIC_KEYFRAME_TIME {
        return Err("unsupported override clock/framing".into());
    }
    let scalar = |value: &str| {
        let value: f64 = value.parse().map_err(|_| "invalid numeric override")?;
        if !value.is_finite() {
            return Err("nonfinite numeric override");
        }
        Ok(value)
    };
    match kind {
        1 | 5 => Ok(SavedGraphicNumeric::Scalar(scalar(fields[1])?)),
        2 => match fields[1] {
            "true" => Ok(SavedGraphicNumeric::Scalar(1.0)),
            "false" => Ok(SavedGraphicNumeric::Scalar(0.0)),
            _ => Err("invalid boolean override".into()),
        },
        3 => {
            let native = fields[1]
                .parse()
                .map_err(|_| "invalid packed colour override")?;
            let colour = PrColour::from_native(native)?;
            // The ordinary Premiere colour decoder establishes RGB, not Capsule
            // alpha semantics. Keep the exact template property's alpha.
            Ok(SavedGraphicNumeric::ColourRgb(colour.fx()))
        }
        6 => {
            let (x, y) = fields[1].split_once(':').ok_or("invalid point override")?;
            let point = [scalar(x)? * size.x, scalar(y)? * size.y];
            if point.iter().any(|value| !value.is_finite()) {
                return Err("nonfinite derived point override".into());
            }
            Ok(SavedGraphicNumeric::Point(point))
        }
        _ => Err("unsupported numeric override".into()),
    }
}

pub(super) fn saved_text(
    graph: &Graph<'_>,
    param: Element<'_>,
    metadata: &serde_json::Map<String, serde_json::Value>,
    owner: &str,
    diagnostics: &mut Vec<String>,
) -> Result<CapsuleValue, CapsuleError> {
    let field = |name| param.child(name).and_then(Element::text);
    if field("ParameterControlType") != Some("23")
        || field("StartKeyframePosition") != Some(records::STATIC_KEYFRAME_TIME)
        || field("IsTimeVarying").is_some_and(|value| value != "false")
        || field("Keyframes").is_some_and(|value| !value.is_empty())
    {
        diagnostics.push(format!(
            "{owner}: unsupported Text override type/clock; template Text and animation retained"
        ));
        return Ok(CapsuleValue::Unsupported { kind: 0 });
    }
    let Some(encoded) = encoded_value(param)? else {
        diagnostics.push(format!(
            "{owner}: missing Text override; template Text retained"
        ));
        return Ok(CapsuleValue::Unsupported { kind: 0 });
    };
    let stored = decode_string(graph, &encoded, owner)?;
    let older = metadata
        .get("capPropFontEditInfo")
        .and_then(serde_json::Value::as_object);
    let current;
    let (text, style) = if let Some(style) = older {
        (stored, style)
    } else {
        current = match serde_json::from_str::<serde_json::Value>(&stored) {
            Ok(serde_json::Value::Object(value)) => value,
            _ => {
                diagnostics.push(format!(
                    "{owner}: unsupported Text value profile; template Text retained"
                ));
                return Ok(CapsuleValue::Unsupported { kind: 0 });
            }
        };
        let Some(text) = current
            .get("textEditValue")
            .and_then(serde_json::Value::as_str)
        else {
            diagnostics.push(format!(
                "{owner}: missing current textEditValue; template Text retained"
            ));
            return Ok(CapsuleValue::Unsupported { kind: 0 });
        };
        (text.to_owned(), &current)
    };
    let uniform = |field: &str| {
        let value = style.get(field)?;
        if let Some(values) = value.as_array() {
            let first = values.first()?;
            values.iter().all(|value| value == first).then_some(first)
        } else {
            Some(value)
        }
    };
    let font = uniform("fontEditValue")
        .and_then(serde_json::Value::as_str)
        .filter(|font| !font.is_empty())
        .map(str::to_owned);
    let size = uniform("fontSizeEditValue")
        .and_then(serde_json::Value::as_f64)
        .filter(|size| size.is_finite() && *size > 0.0);
    let all_caps = uniform("fontFSAllCapsValue").and_then(serde_json::Value::as_bool);
    if font.is_none() {
        diagnostics.push(format!(
            "{owner}: unsupported/missing uniform font override; template font retained"
        ));
    }
    if size.is_none() {
        diagnostics.push(format!(
            "{owner}: unsupported/missing uniform font size override; template size retained"
        ));
    }
    if all_caps.is_none() {
        diagnostics.push(format!(
            "{owner}: unsupported/missing uniform All Caps override; template value retained"
        ));
    }
    let flag = |field| {
        uniform(field)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    };
    let value = CapsuleTextValue {
        text,
        font,
        size,
        all_caps,
        faux_bold: flag("fontFSBoldValue"),
        faux_italic: flag("fontFSItalicValue"),
        small_caps: flag("fontFSSmallCapsValue"),
    };
    if value.faux_bold || value.faux_italic || value.small_caps {
        diagnostics.push(format!("{owner}: faux Bold/Italic/Small Caps overrides are not mapped; supported text/font/size/All Caps retained"));
    }
    for field in style.keys() {
        if !matches!(
            field.as_str(),
            "capPropFontEdit"
                | "capPropFontFauxStyleEdit"
                | "capPropFontSizeEdit"
                | "capPropTextRunCount"
                | "fontEditValue"
                | "fontSizeEditValue"
                | "fontTextRunLength"
                | "fontFSAllCapsValue"
                | "fontFSBoldValue"
                | "fontFSItalicValue"
                | "fontFSSmallCapsValue"
                | "textEditValue"
        ) {
            diagnostics.push(format!("{owner}: unsupported optional Text field {field:?}; supported override fields retained"));
        }
    }
    Ok(CapsuleValue::Text(value))
}
