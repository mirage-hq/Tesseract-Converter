//! Diagnosed source omission for unsupported, fully wet static Cutout reception.
use crate::{
    effects::native::{Declarations, DecodedEffect},
    properties,
    structure::Layer,
};

pub(super) fn cutout(layer: &Layer, effect: &DecodedEffect) -> bool {
    if effect.match_name != "CC Light Sweep"
        || !effect.enabled
        || !layer.record.flags().effects_active
        || effect.declarations == Declarations::Unreadable
    {
        return false;
    }
    let controls: Vec<_> = effect
        .parameters
        .iter()
        .filter(|p| p.match_name == "CC Light Sweep-0009")
        .collect();
    let [control] = controls.as_slice() else {
        return false;
    };
    let Ok(value) = &control.numeric else {
        return false;
    };
    // PF_Param_POPUP (7) when locally declared. Native occurrences may use the
    // shared plugin declaration instead; the canonical match name still binds
    // this explicit scalar to Light Reception.
    if !matches!(control.declared_kind, Ok(None | Some(7)))
        || value.animated
        || !value.keyframes.is_empty()
        || value.expression_enabled
        || value.expression_present
        || value.dimensions_separated
        || value.values.as_slice() != [3.0]
    {
        return false;
    }
    // A wet/dry mix or effect mask can preserve source pixels. Do not hide them.
    compositing_is_default(layer, effect).unwrap_or(false)
}

fn compositing_is_default(
    layer: &Layer,
    effect: &DecodedEffect,
) -> Result<bool, crate::properties::PropertyError> {
    let roots = properties::root_runs(&layer.content)?;
    let parade = super::super::control_links::unique_run(&roots, "ADBE Effect Parade")?;
    let effects = properties::runs(properties::unique_list(parade, *b"tdgp")?)?;
    let Some((name, run)) = effect.index.checked_sub(1).and_then(|i| effects.get(i)) else {
        return Ok(false);
    };
    if *name != "CC Light Sweep" {
        return Ok(false);
    }
    let descriptor = properties::unique_list(run, *b"sspc")?;
    let body = properties::unique_list(descriptor, *b"tdgp")?;
    let controls = properties::runs(body)?;
    let options: Vec<_> = controls
        .iter()
        .filter(|(name, _)| *name == "ADBE Effect Built In Params")
        .collect();
    match options.as_slice() {
        [] => Ok(true),
        [(_, run)] => Ok(properties::runs(properties::unique_list(run, *b"tdgp")?)?
            .iter()
            .all(|(name, _)| *name == "ADBE Group End")),
        _ => Ok(false),
    }
}
