//! Static Fast Box Blur approximated through the existing editable Gaussian blur.
//! Variance is source-derived; neither the native kernel nor edge alpha is certified.
use std::collections::HashSet;

use fx_schema::{LayerEffect, NonNegativeProperty};

use super::native::DecodedEffect;
use crate::{properties, structure::Layer};

const MATCH_NAME: &str = "ADBE Box Blur2";

fn controls(layer: &Layer, source: &DecodedEffect) -> Result<(), String> {
    let roots = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    let parade = roots
        .iter()
        .find(|(name, _)| *name == "ADBE Effect Parade")
        .ok_or("missing Effect Parade")?
        .1;
    let effects =
        properties::runs(properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let (name, run) = effects
        .get(
            source
                .index
                .checked_sub(1)
                .ok_or("invalid effect ordinal")?,
        )
        .ok_or("effect ordinal absent")?;
    if *name != MATCH_NAME {
        return Err("effect ordinal/name mismatch".into());
    }
    let descriptor = properties::unique_list(run, *b"sspc").map_err(|e| e.to_string())?;
    let table = properties::unique_list(descriptor, *b"parT").map_err(|e| e.to_string())?;
    if !table.is_empty() {
        let rows = properties::runs(table).map_err(|e| e.to_string())?;
        let profile = [
            ("ADBE Box Blur2-0000", 0_u32, 0_u32),
            ("ADBE Box Blur2-0001", 2, 0),
            ("ADBE Box Blur2-0002", 1, 3),
            ("ADBE Box Blur2-0003", 7, 1),
            ("ADBE Box Blur2-0004", 4, 1),
            ("ADBE Effect Built In Params", 9, 0),
        ];
        if rows.len() != profile.len() {
            return Err(
                "requires complete native Box Blur declarations or a sparse instance".into(),
            );
        }
        let mut seen = HashSet::new();
        for (name, run) in rows {
            if !seen.insert(name) {
                return Err("duplicate Box Blur declaration".into());
            }
            let (_, kind, default) = profile
                .iter()
                .find(|(expected, _, _)| *expected == name)
                .ok_or("unknown Box Blur declaration")?;
            let bytes = properties::data(run, *b"pard").map_err(|e| e.to_string())?;
            if bytes.len() != 148
                || bytes[12..16] != kind.to_be_bytes()
                || bytes[56..60] != default.to_be_bytes()
                || (*kind == 7
                    && (bytes[60..62] != 3_u16.to_be_bytes()
                        || bytes[62..64] != 1_u16.to_be_bytes()))
            {
                return Err(
                    "Box Blur declarations conflict with the native default profile".into(),
                );
            }
        }
    }
    let rows =
        properties::runs(properties::unique_list(descriptor, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    for (name, run) in rows {
        if !seen.insert(name) {
            return Err("duplicate Box Blur control".into());
        }
        match name {
            "ADBE Box Blur2-0000" => {
                let leaf = properties::unique_list(run, *b"tdbs").map_err(|e| e.to_string())?;
                let numeric = properties::read_numeric(leaf).map_err(|e| e.to_string())?;
                if numeric.values != [0.]
                    || numeric.animated
                    || !numeric.keyframes.is_empty()
                    || numeric.expression_present
                    || numeric.expression_enabled
                {
                    return Err("nondefault Box Blur dummy control".into());
                }
            }
            "ADBE Box Blur2-0001"
            | "ADBE Box Blur2-0002"
            | "ADBE Box Blur2-0003"
            | "ADBE Box Blur2-0004"
            | "ADBE Group End" => {}
            "ADBE Effect Built In Params" => {
                let options = properties::runs(
                    properties::unique_list(run, *b"tdgp").map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                if options.iter().any(|(name, _)| *name != "ADBE Group End") {
                    return Err("nonempty Box Blur compositing options".into());
                }
            }
            _ => return Err(format!("unknown Box Blur control {name}")),
        }
    }
    Ok(())
}

fn scalar(source: &DecodedEffect, name: &str, default: f64) -> Result<f64, String> {
    let Some(parameter) = source.parameters.iter().find(|p| p.match_name == name) else {
        return Ok(default);
    };
    let numeric = parameter.numeric.as_ref().map_err(|e| e.to_string())?;
    if numeric.animated
        || !numeric.keyframes.is_empty()
        || numeric.expression_present
        || numeric.expression_enabled
        || numeric.dimensions_separated
    {
        return Err(format!(
            "{name}: only static controls without expressions are supported"
        ));
    }
    let [value] = numeric.values.as_slice() else {
        return Err(format!("{name}: expected one scalar"));
    };
    if !value.is_finite() {
        return Err(format!("{name}: nonfinite value"));
    }
    Ok(*value)
}

pub(crate) fn lower(
    source: &DecodedEffect,
    layer: &Layer,
    size: [u16; 2],
) -> Result<LayerEffect, String> {
    controls(layer, source)?;
    let radius = scalar(source, "ADBE Box Blur2-0001", 0.)?;
    let iterations = scalar(source, "ADBE Box Blur2-0002", 3.)?;
    if radius < 0.
        || radius.fract() != 0.
        || iterations < 0.
        || iterations.fract() != 0.
        || iterations > f64::from(i32::MAX)
    {
        return Err("requires nonnegative integral radius and iteration count".into());
    }
    if scalar(source, "ADBE Box Blur2-0003", 1.)? != 1. {
        return Err("requires Both Blur Dimensions".into());
    }
    let repeat = match scalar(source, "ADBE Box Blur2-0004", 1.)? {
        0. => false,
        1. => true,
        _ => return Err("invalid Repeat Edge Pixels switch".into()),
    };
    // A discrete normalized box [-r,r] has variance r(r+1)/3. Sequential
    // iterations add variances. Existing tgfx-style blurriness uses four sigma;
    // its bounded renderer kernel remains a separate approximation.
    let blur = 4. * (iterations * radius * (radius + 1.) / 3.).sqrt();
    let blurriness = NonNegativeProperty::new(blur).ok_or("derived blur is nonfinite")?;
    if repeat && size.contains(&0) {
        return Err("Repeat Edge Pixels requires source dimensions".into());
    }
    Ok(LayerEffect::GaussianBlur {
        blurriness,
        repeat_edge_pixels: Some(repeat),
        layer_size: repeat.then(|| (f64::from(size[0]), f64::from(size[1]))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::{ItemKind, read_project};

    fn native() -> (Layer, DecodedEffect) {
        let project = read_project(include_bytes!(
            "../../tests/fixtures/effects/shape_owner_gaussian.aep"
        ))
        .unwrap();
        let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
            panic!()
        };
        let mut layer = comp.layers[0].clone();
        let parsed = crate::rifx::Rifx::parse_with(
            include_bytes!("../../tests/fixtures/effects/native-static-box-blur.rifx"),
            |_| false,
        )
        .unwrap();
        layer.content = parsed.chunks()[0].children().unwrap().to_vec();
        layer.name = "Unrelated renamed graphic".into();
        let (effects, _) = super::super::native::read_effects(&layer.content, [1920., 1080.]);
        let effect = effects
            .into_iter()
            .find(|e| e.match_name == MATCH_NAME)
            .unwrap();
        (layer, effect)
    }

    #[test]
    fn box_blur_declines_unsupported_axes_animation_and_invalid_controls() {
        let (layer, source) = native();
        for (suffix, value, animated, expression) in [
            ("0001", -1., false, false),
            ("0001", 1.5, false, false),
            ("0001", f64::NAN, false, false),
            ("0002", 1.5, false, false),
            ("0002", -1., false, false),
            ("0003", 2., false, false),
            ("0003", 3., false, false),
            ("0004", 2., false, false),
            ("0001", 50., true, false),
            ("0001", 50., false, true),
        ] {
            let mut effect = source.clone();
            let parameter = effect
                .parameters
                .iter_mut()
                .find(|p| p.match_name.ends_with(suffix));
            if let Some(parameter) = parameter {
                let numeric = parameter.numeric.as_mut().unwrap();
                numeric.values = vec![value];
                numeric.animated = animated;
                numeric.expression_enabled = expression;
            } else {
                let mut parameter = effect
                    .parameters
                    .iter()
                    .find(|p| p.match_name.ends_with("0001"))
                    .unwrap()
                    .clone();
                parameter.match_name = format!("{MATCH_NAME}-{suffix}");
                let numeric = parameter.numeric.as_mut().unwrap();
                numeric.values = vec![value];
                numeric.animated = animated;
                numeric.expression_enabled = expression;
                effect.parameters.push(parameter);
            }
            assert!(
                lower(&effect, &layer, [1280, 720]).is_err(),
                "{suffix}={value} animated={animated} expression={expression}"
            );
        }
        let LayerEffect::GaussianBlur { layer_size, .. } =
            lower(&source, &layer, [1280, 720]).unwrap()
        else {
            panic!()
        };
        assert_eq!(layer_size, Some((1280., 720.)));
        assert!(lower(&source, &layer, [0, 720]).is_err());
    }
}
