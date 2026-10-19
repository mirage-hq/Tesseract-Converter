//! Static same-source colored anaglyph approximation; not a stereo compositor.

use std::collections::HashSet;

use fx_schema::{ChannelMixer, LayerEffect, color_tools::ChannelMixerRow};

use crate::{properties, structure::Layer};

use super::native::{Declarations, DecodedEffect};

fn scalar(source: &DecodedEffect, suffix: &str, default: Option<f64>) -> Result<f64, String> {
    let name = format!("ADBE 3D Glasses2{suffix}");
    let mut controls = source.parameters.iter().filter(|p| p.match_name == name);
    let Some(parameter) = controls.next() else {
        return default.ok_or_else(|| format!("{name}: missing control"));
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

fn validate_controls(source: &DecodedEffect, layer: &Layer) -> Result<(), String> {
    let roots = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    let parade = roots
        .iter()
        .find(|(name, _)| *name == "ADBE Effect Parade")
        .ok_or("missing Effect Parade")?
        .1;
    let instances =
        properties::runs(properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let (name, run) = instances
        .get(
            source
                .index
                .checked_sub(1)
                .ok_or("invalid effect ordinal")?,
        )
        .ok_or("missing effect ordinal")?;
    if *name != source.match_name {
        return Err("effect ordinal/name mismatch".into());
    }
    let plugin = properties::unique_list(run, *b"sspc").map_err(|e| e.to_string())?;
    let body =
        properties::runs(properties::unique_list(plugin, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    for (name, run) in body {
        if !seen.insert(name) {
            return Err(format!("duplicate 3D Glasses control {name}"));
        }
        if name == "ADBE Effect Built In Params" {
            let options = properties::runs(
                properties::unique_list(run, *b"tdgp").map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            if options.iter().any(|(name, _)| *name != "ADBE Group End") {
                return Err("nonempty 3D Glasses compositing options".into());
            }
        }
    }
    Ok(())
}

pub(crate) fn lower(
    source: &DecodedEffect,
    layer: &Layer,
    plane: [u16; 2],
) -> Result<Vec<LayerEffect>, String> {
    validate_controls(source, layer)?;
    if source.declarations == Declarations::Unreadable || plane.contains(&0) {
        return Err("unreadable declarations or empty destination plane".into());
    }
    for parameter in &source.parameters {
        if !matches!(
            parameter.match_name.as_str(),
            "ADBE 3D Glasses2-0001"
                | "ADBE 3D Glasses2-0002"
                | "ADBE 3D Glasses2-0003"
                | "ADBE 3D Glasses2-0004"
                | "ADBE 3D Glasses2-0005"
                | "ADBE 3D Glasses2-0006"
                | "ADBE 3D Glasses2-0007"
                | "ADBE 3D Glasses2-0008"
                | "ADBE Group End"
        ) {
            return Err(format!(
                "unknown 3D Glasses control {}",
                parameter.match_name
            ));
        }
    }
    if scalar(source, "-0001", Some(0.))? != 0.
        || scalar(source, "-0002", Some(0.))? != 0.
        || scalar(source, "-0005", None)? != 12.
        || scalar(source, "-0006", Some(8.))? != 8.
        || scalar(source, "-0007", Some(0.))? != 0.
        || scalar(source, "-0008", Some(1.))? != 1.
    {
        return Err("requires same-source Balanced Colored Red Blue, default Balance, zero Vertical Alignment and pixel units".into());
    }
    let swap = scalar(source, "-0004", Some(0.))?;
    if swap != 0. && swap != 1. {
        return Err("Swap Left-Right must be a static checkbox".into());
    }
    let convergence = scalar(source, "-0003", Some(0.))? * if swap == 1. { -1. } else { 1. };
    // Two red/blue splits with a green/blue swap give offsets (2d, -d, -d).
    // d=2c/3 preserves the native red/cyan separation 2c, with centroid +c/3.
    let amount = convergence.abs() * (2. / 3.) * 100. / f64::from(plane[0]);
    if amount > 10. {
        return Err("Scene Convergence exceeds the editable color-fringe amount range".into());
    }
    let split = LayerEffect::ChromaticAberration {
        amount: Some(amount),
        direction: Some(if convergence < 0. { 270. } else { 90. }),
    };
    let row = |red, green, blue| ChannelMixerRow {
        red,
        green,
        blue,
        constant: 0.,
    };
    let swap = LayerEffect::ChannelMixer(ChannelMixer {
        semantic_version: 1,
        red: row(1., 0., 0.),
        green: row(0., 0., 1.),
        blue: row(0., 1., 0.),
        monochrome: false,
    });
    Ok(vec![split.clone(), swap.clone(), split, swap])
}
