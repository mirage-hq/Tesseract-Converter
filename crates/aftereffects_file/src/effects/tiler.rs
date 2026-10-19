//! Static CC Tiler approximation through existing editable Motion Tile controls.

use fx_schema::LayerEffect;

use super::native::{Declarations, DecodedEffect};

fn values(
    source: &DecodedEffect,
    name: &str,
    kind: u32,
    default: Option<&[f64]>,
) -> Result<Vec<f64>, String> {
    let mut parameters = source.parameters.iter().filter(|p| p.match_name == name);
    let Some(parameter) = parameters.next() else {
        return default
            .map(<[f64]>::to_vec)
            .ok_or_else(|| format!("{name}: missing control"));
    };
    if parameters.next().is_some() {
        return Err(format!("{name}: duplicate control"));
    }
    let declared = parameter
        .declared_kind
        .as_ref()
        .map_err(|e| e.to_string())?;
    if declared.is_some_and(|declared| declared != kind) {
        return Err(format!("{name}: incompatible control declaration"));
    }
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
    let dimensions = if kind == 6 { 2 } else { 1 };
    if numeric.values.len() != dimensions || !numeric.values.iter().all(|v| v.is_finite()) {
        return Err(format!(
            "{name}: finite {dimensions}-component control required"
        ));
    }
    Ok(numeric.values.clone())
}

pub(crate) fn lower(source: &DecodedEffect, size: [f64; 2]) -> Result<LayerEffect, String> {
    if source.declarations == Declarations::Unreadable
        || !size.iter().all(|v| v.is_finite() && *v > 0.)
    {
        return Err("unreadable declarations or invalid source plane".into());
    }
    for parameter in &source.parameters {
        if !matches!(
            parameter.match_name.as_str(),
            "CC Tiler-0000"
                | "CC Tiler-0001"
                | "CC Tiler-0002"
                | "CC Tiler-0003"
                | "ADBE Effect Built In Params"
                | "ADBE Group End"
        ) {
            return Err(format!("unknown Tiler control {}", parameter.match_name));
        }
    }
    let scale = values(source, "CC Tiler-0001", 2, None)?[0] * 100.;
    // Motion Tile's current sampler clamps tile sizes below one percent.
    if !(1. ..=100.).contains(&scale) {
        return Err("Scale requires 1–100 percent".into());
    }
    let center = values(
        source,
        "CC Tiler-0002",
        6,
        Some(&[size[0] / 2., size[1] / 2.]),
    )?;
    if values(source, "CC Tiler-0003", 2, Some(&[0.]))?[0] != 0. {
        return Err("Blend w. Original requires zero".into());
    }
    Ok(LayerEffect::MotionTile {
        tile_center_x: center[0] / size[0],
        tile_center_y: center[1] / size[1],
        tile_width: scale,
        tile_height: scale,
        output_width: 100.,
        output_height: 100.,
        mirror_edges: false,
        phase: 0.,
    })
}
