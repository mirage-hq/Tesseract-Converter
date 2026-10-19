//! Venetian Blinds through the existing editable shader primitive.
//! Soft edges approximate native feather with a periodic box filter.

use fx_schema::{EffectParam, LayerEffect};

use super::native::{Declarations, DecodedEffect};

pub(crate) const MATCH_NAME: &str = "ADBE Venetian Blinds";
pub(crate) const APPROXIMATION: &str = "Venetian Blinds retained as an editable CustomShader approximation with native Completion (percent), Direction (degrees), Width and Feather (pixels), including numeric keys; absent controls use native defaults 0/0/20/0; projected periodic alpha bands use box-filtered edges with twice Feather as the filter width, not a proven Adobe feather/raster kernel; source-plane placement, alpha and native fidelity are unverified; FX→AEP export omits custom shaders";

// Native name, declaration kind, default and editable shader parameter.
pub(crate) const CONTROLS: [(&str, u32, f64, &str); 4] = [
    ("ADBE Venetian Blinds-0001", 2, 0.0, "completion"),
    ("ADBE Venetian Blinds-0002", 3, 0.0, "direction"),
    ("ADBE Venetian Blinds-0003", 2, 20.0, "width"),
    ("ADBE Venetian Blinds-0004", 2, 0.0, "feather"),
];

fn valid_control(index: usize, value: f64) -> bool {
    value.is_finite()
        && match index {
            0 => (0.0..=100.0).contains(&value),
            2 => (1.0..=32000.0).contains(&value),
            3 => (0.0..=32000.0).contains(&value),
            _ => true,
        }
}

/// Decline the entire effect on ambiguous or unsupported controls, not its owner.
pub(crate) fn lower(source: &DecodedEffect, size: [u16; 2]) -> Result<LayerEffect, String> {
    if size.contains(&0) || source.declarations == Declarations::Unreadable {
        return Err("unknown effect plane or unreadable declarations".into());
    }
    if source.parameters.iter().any(|parameter| {
        !CONTROLS
            .iter()
            .any(|(name, _, _, _)| *name == parameter.match_name)
    }) {
        return Err("unknown Venetian Blinds control".into());
    }
    let mut values = [0.0; 4];
    for (index, &(name, kind, default, _)) in CONTROLS.iter().enumerate() {
        let mut parameters = source.parameters.iter().filter(|p| p.match_name == name);
        let Some(parameter) = parameters.next() else {
            values[index] = default;
            continue;
        };
        if parameters.next().is_some() {
            return Err(format!("{name}: duplicate control"));
        }
        match &parameter.declared_kind {
            Err(error) => return Err(format!("{name}: {error}")),
            Ok(Some(actual)) if *actual != kind => {
                return Err(format!("{name}: incompatible declaration type {actual}"));
            }
            Ok(_) => {}
        }
        let numeric = parameter
            .numeric
            .as_ref()
            .map_err(|error| format!("{name}: {error}"))?;
        if numeric.expression_enabled
            || numeric.dimensions_separated
            || (numeric.animated && numeric.keyframes.is_empty())
        {
            return Err(format!(
                "{name}: unsupported expression, separation or missing keys"
            ));
        }
        let initial = numeric
            .keyframes
            .first()
            .map_or(numeric.values.as_slice(), |key| key.values.as_slice());
        for values in std::iter::once(initial)
            .chain(numeric.keyframes.iter().map(|key| key.values.as_slice()))
        {
            let [value] = values else {
                return Err(format!("{name}: scalar required"));
            };
            if !valid_control(index, *value) {
                return Err(format!("{name}: nonfinite or out-of-range scalar"));
            }
        }
        values[index] = initial[0];
    }
    let [completion, direction, width, feather] = values;
    Ok(LayerEffect::CustomShader {
        name: format!("Venetian Blinds (AE effect {})", source.index),
        description: APPROXIMATION.into(),
        wgsl: include_str!("venetian_blinds.wgsl").into(),
        params: vec![
            param(
                "completion",
                "Transition Completion, percent removed",
                0.0,
                100.0,
                completion,
            ),
            param(
                "direction",
                "Direction in degrees (0 gives vertical bands)",
                -360000.0,
                360000.0,
                direction,
            ),
            param(
                "width",
                "Band period in native plane pixels",
                1.0,
                32000.0,
                width,
            ),
            param(
                "planeWidth",
                "Effect plane width in native pixels",
                1.0,
                65535.0,
                f64::from(size[0]),
            ),
            param(
                "planeHeight",
                "Effect plane height in native pixels",
                1.0,
                65535.0,
                f64::from(size[1]),
            ),
            // AE evaluates bands at integer pixel centers, not GPU half-integer centers.
            param(
                "pixelCenterOffset",
                "Native pixel-center offset in plane pixels",
                -1.0,
                1.0,
                -0.5,
            ),
            param(
                "feather",
                "Soft edge extent in native plane pixels",
                0.0,
                32000.0,
                feather,
            ),
        ],
        texture_inputs: Vec::new(),
    })
}

fn param(name: &str, description: &str, min: f64, max: f64, default: f64) -> EffectParam {
    EffectParam {
        name: name.into(),
        description: description.into(),
        min,
        max,
        default,
    }
}
