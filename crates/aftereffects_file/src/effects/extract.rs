//! Bounded static Extract approximation using the existing editable Luma Key.
//! Only a single hard luminance cutoff is admitted; native raster equality is unproven.

use fx_schema::LayerEffect;

use super::native::{Declarations, DecodedEffect};

fn scalar(source: &DecodedEffect, name: &str, default: f64) -> Result<f64, String> {
    let mut controls = source.parameters.iter().filter(|p| p.match_name == name);
    let Some(parameter) = controls.next() else {
        return Ok(default);
    };
    if controls.next().is_some() {
        return Err(format!("{name}: duplicate control"));
    }
    parameter
        .declared_kind
        .as_ref()
        .map_err(|e| e.to_string())?;
    let numeric = parameter.numeric.as_ref().map_err(|e| e.to_string())?;
    if numeric.animated
        || !numeric.keyframes.is_empty()
        || numeric.expression_present
        || numeric.expression_enabled
        || numeric.dimensions_separated
    {
        return Err(format!(
            "{name}: requires static controls without expressions"
        ));
    }
    match numeric.values.as_slice() {
        [value] if value.is_finite() => Ok(*value),
        _ => Err(format!("{name}: finite scalar required")),
    }
}

pub(crate) fn lower(source: &DecodedEffect) -> Result<LayerEffect, String> {
    if source.declarations == Declarations::Unreadable {
        return Err("unreadable Extract declarations".into());
    }
    for parameter in &source.parameters {
        if !matches!(
            parameter.match_name.as_str(),
            "ADBE Extract-0001"
                | "ADBE Extract-0002"
                | "ADBE Extract-0003"
                | "ADBE Extract-0004"
                | "ADBE Extract-0005"
                | "ADBE Extract-0006"
                | "ADBE Extract-0007"
                | "ADBE Group End"
        ) {
            return Err(format!("unknown Extract control {}", parameter.match_name));
        }
    }
    if scalar(source, "ADBE Extract-0002", 1.)? != 1.
        || scalar(source, "ADBE Extract-0005", 0.)? != 0.
        || scalar(source, "ADBE Extract-0006", 0.)? != 0.
        || scalar(source, "ADBE Extract-0007", 0.)? != 0.
    {
        return Err("requires Luminance, zero softness and noninverted output".into());
    }
    let black = scalar(source, "ADBE Extract-0003", 0.)?;
    let white = scalar(source, "ADBE Extract-0004", 255.)?;
    if !(0. ..=255.).contains(&black) || !(0. ..=255.).contains(&white) || black >= white {
        return Err("requires finite ordered Black/White Points in 0..255".into());
    }
    let (threshold, invert) = if black == 0. && white < 255. {
        (white / 255., 1.)
    } else if white == 255. && black > 0. {
        (black / 255., 0.)
    } else {
        return Err("requires exactly one active luminance cutoff".into());
    };
    Ok(LayerEffect::LumaKey {
        threshold: Some(threshold),
        softness: Some(0.),
        invert: Some(invert),
    })
}
