//! Bounded same-layer Scale-plus-Slider controls, without an expression runtime.

use super::{Reference, expression, finished, properties, quoted, resolve, token, unique_run};
use crate::{
    properties::{NumericProperty, PropertyError},
    structure::Layer,
};

fn slider<'a>(text: &mut &'a str) -> Option<&'a str> {
    token(text, "effect")?;
    token(text, "(")?;
    let name = quoted(text)?;
    token(text, ")")?;
    token(text, "(")?;
    if token(text, "1").is_none() && quoted(text)? != "Slider" {
        return None;
    }
    token(text, ")")?;
    Some(name)
}

fn repeated(text: &mut &str, name: &str) -> Option<()> {
    token(text, "[")?;
    token(text, name)?;
    token(text, ",")?;
    token(text, name)?;
    token(text, "]")?;
    finished(text).then_some(())
}

fn parse(mut text: &str) -> Option<&str> {
    token(&mut text, "s")?;
    token(&mut text, "=")?;
    if token(&mut text, "value").is_some() {
        token(&mut text, "[")?;
        token(&mut text, "0")?;
        token(&mut text, "]")?;
        token(&mut text, "+")?;
        let name = slider(&mut text)?;
        token(&mut text, ";")?;
        repeated(&mut text, "s")?;
        Some(name)
    } else {
        let name = slider(&mut text)?;
        for part in [";", "x", "=", "value", "[", "0", "]", "+", "s", ";"] {
            token(&mut text, part)?;
        }
        repeated(&mut text, "x")?;
        Some(name)
    }
}

pub(super) fn lower(
    layer: &Layer,
    base: &NumericProperty,
) -> Result<NumericProperty, PropertyError> {
    let root = properties::root_runs(&layer.content)?;
    let transform = unique_run(&root, "ADBE Transform Group")?;
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp")?)?;
    let scale = properties::unique_list(unique_run(&leaves, "ADBE Scale")?, *b"tdbs")?;
    let name =
        parse(expression(scale)?).ok_or(PropertyError::Layout("not a bounded Scale offset"))?;
    let parade = unique_run(&root, "ADBE Effect Parade")?;
    let effects = properties::runs(properties::unique_list(parade, *b"tdgp")?)?;
    let offset = resolve(
        &effects,
        Reference {
            effect: name,
            parameter: "ADBE Slider Control-0001",
        },
    )?;
    add_constant(base, &offset)
}

fn static_value(value: &NumericProperty) -> Option<f64> {
    (!value.animated && value.keyframes.is_empty())
        .then(|| value.values.first().copied())
        .flatten()
        .filter(|v| v.is_finite())
}

fn add_constant(
    base: &NumericProperty,
    offset: &NumericProperty,
) -> Result<NumericProperty, PropertyError> {
    if offset.expression_enabled
        || offset.values.len() > 1
        || offset.keyframes.iter().any(|key| key.values.len() != 1)
    {
        return Err(PropertyError::Layout(
            "Scale offset requires a resolved scalar Slider",
        ));
    }
    let (mut result, factor, addition) = if let Some(offset) = static_value(offset) {
        (base.clone(), 1.0, offset * 0.01)
    } else if let Some(base) = static_value(base) {
        (offset.clone(), 0.01, base)
    } else {
        return add_animated(base, offset);
    };
    let vector = |values: &mut Vec<f64>, factor: f64, addition: f64| -> Result<(), PropertyError> {
        let value = values
            .first()
            .copied()
            .ok_or(PropertyError::Layout("Scale offset component missing"))?
            * factor
            + addition;
        if !value.is_finite() {
            return Err(PropertyError::Layout("non-finite Scale offset"));
        }
        *values = vec![value, value];
        Ok(())
    };
    if !result.values.is_empty() {
        vector(&mut result.values, factor, addition)?;
    }
    for key in &mut result.keyframes {
        vector(&mut key.values, factor, addition)?;
        for speeds in [&mut key.in_speed, &mut key.out_speed] {
            if !speeds.is_empty() {
                vector(speeds, factor, 0.0)?;
            }
        }
        for influences in [&mut key.in_influence, &mut key.out_influence] {
            if !influences.is_empty() {
                vector(influences, 1.0, 0.0)?;
            }
        }
        if !key.spatial_in.is_empty() || !key.spatial_out.is_empty() {
            return Err(PropertyError::Layout("spatial Scale offset is unsupported"));
        }
    }
    if result.values.is_empty() && result.keyframes.is_empty() {
        return Err(PropertyError::Layout("Scale offset has no values"));
    }
    result.expression_enabled = false;
    result.expression_present = false;
    result.dimensions_separated = false;
    Ok(result)
}

// The recognized expression adds two independently timed curves. Preserve the
// native curves analytically, then fit their sum to sparse editable keys rather
// than exporting a frame-by-frame animation or silently discarding either input.
fn add_animated(
    base: &NumericProperty,
    offset: &NumericProperty,
) -> Result<NumericProperty, PropertyError> {
    use super::delayed_position::{evaluate_position, fitted_property, fitted_value};
    use fx_keyframe_bake::curve_fit::fit_scalar_curve;
    const TOLERANCE: f64 = 0.0001; // Native Scale fractions: 0.01 percentage points.
    for source in [base, offset] {
        if source.keyframes.len() < 2
            || source.keyframes.len() > 4096
            || source.keyframes.iter().any(|k| {
                !k.time_secs.is_finite()
                    || k.values.is_empty()
                    || k.values.len() != source.keyframes[0].values.len()
                    || k.values.iter().any(|v| !v.is_finite())
                    || !k.spatial_in.is_empty()
                    || !k.spatial_out.is_empty()
            })
            || source
                .keyframes
                .windows(2)
                .any(|pair| pair[0].time_secs >= pair[1].time_secs)
        {
            return Err(PropertyError::Layout("invalid Scale sum source curve"));
        }
    }
    let first = base.keyframes[0]
        .time_secs
        .min(offset.keyframes[0].time_secs);
    let last = base
        .keyframes
        .last()
        .ok_or(PropertyError::Layout("missing Scale keys"))?
        .time_secs
        .max(
            offset
                .keyframes
                .last()
                .ok_or(PropertyError::Layout("missing Slider keys"))?
                .time_secs,
        );
    let duration = ((last - first) * 1000.0).ceil();
    if !duration.is_finite() || !(1.0..=60_000.0).contains(&duration) {
        return Err(PropertyError::Layout(
            "Scale sum duration exceeds fitting budget",
        ));
    }
    // Checked above: at most 60,000 positive whole milliseconds.
    let duration = duration as u64;
    let mut count = 0usize;
    let mut evaluate = |ms: u64| -> Result<Vec<f64>, PropertyError> {
        count += 1;
        if count > 200_000 {
            return Err(PropertyError::Layout(
                "Scale sum evaluation budget exhausted",
            ));
        }
        let time = first + ms as f64 / 1000.0;
        let value = evaluate_position(base, time, 0.0, 1.0)?[0]
            + evaluate_position(offset, time, 0.0, 1.0)?[0] * 0.01;
        if !value.is_finite() {
            return Err(PropertyError::NonFinite);
        }
        Ok(vec![value, value])
    };
    let curve = fit_scalar_curve(duration, TOLERANCE, |ms| {
        Ok::<_, PropertyError>(evaluate(ms)?[0])
    })?;
    if curve.keys.len() > 128 {
        return Err(PropertyError::Layout("Scale sum exceeds sparse key budget"));
    }
    let mut segment = 0;
    for ms in 0..=duration {
        if (fitted_value(&curve, ms, &mut segment) - evaluate(ms)?[0]).abs() > TOLERANCE {
            return Err(PropertyError::Layout("Scale sum exceeds fitting tolerance"));
        }
    }
    fitted_property(&curve, 0, first, 0.0, 1.0, &mut evaluate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::properties::NumericValueKind;

    fn constant(values: Vec<f64>) -> NumericProperty {
        NumericProperty {
            values,
            animated: false,
            expression_enabled: false,
            expression_present: false,
            dimensions_separated: false,
            keyframes: Vec::new(),
            value_kind: NumericValueKind::Continuous,
        }
    }

    #[test]
    fn scale_offset_recognizes_only_complete_authored_forms() {
        assert_eq!(
            parse("s = value[0]+effect(\"Scale-Offset\")(1);\r[s,s]"),
            Some("Scale-Offset")
        );
        assert_eq!(
            parse("s=effect(\"Scale\")(\"Slider\"); x=value[0]+s; [x,x]"),
            Some("Scale")
        );
        assert!(parse("s=value[0]+effect('Scale')(2);[s,s]").is_none());
        assert!(parse("s=value[0]+effect('Scale')(1);[s,s];evil()").is_none());
        assert!(parse("s=value[1]+effect('Scale')(1);[s,s]").is_none());
    }

    #[test]
    fn scale_offset_preserves_authored_keys_and_easing_without_sampling() {
        let mut base = constant(Vec::new());
        base.animated = true;
        base.keyframes.push(crate::properties::NumericKeyframe {
            time_secs: 24.541666666666668,
            values: vec![0.07, 0.2, 1.0],
            in_interpolation: 2,
            out_interpolation: 2,
            in_speed: vec![0.5, 0.9, 0.0],
            out_speed: vec![-0.25, 0.9, 0.0],
            in_influence: vec![33.0, 20.0, 20.0],
            out_influence: vec![16.0, 20.0, 20.0],
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        });
        let result = add_constant(&base, &constant(vec![25.0])).unwrap();
        assert_eq!(result.keyframes.len(), 1);
        let key = &result.keyframes[0];
        assert_eq!(key.time_secs, base.keyframes[0].time_secs);
        assert_eq!(key.values, vec![0.32, 0.32]);
        assert_eq!(key.in_speed, vec![0.5, 0.5]);
        assert_eq!(key.out_speed, vec![-0.25, -0.25]);
        assert_eq!(key.out_influence, vec![16.0, 16.0]);
        assert_eq!(key.in_interpolation, 2);
        let mut slider = base.clone();
        slider.keyframes[0].values = vec![25.0];
        let result = add_constant(&constant(vec![0.07]), &slider).unwrap();
        assert_eq!(result.keyframes[0].values, vec![0.32, 0.32]);
        assert_eq!(result.keyframes[0].in_speed, vec![0.005, 0.005]);
        assert!(add_constant(&base, &slider).is_err());
    }

    #[test]
    #[ignore = "requires local licensed AEP_SCALE_OFFSET_SOURCE, which cannot be redistributed"]
    fn local_external_source_restores_sh09_scale_curves() {
        use sha2::{Digest, Sha256};
        let bytes = std::fs::read(
            std::env::var_os("AEP_SCALE_OFFSET_SOURCE").expect("local licensed source path"),
        )
        .unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            "28bbce1b8c9f9625105d632504a97c598394a4753d6b0d923fb19942e302bb5d"
        );
        let project = crate::structure::read_project(&bytes).unwrap();
        let crate::structure::ItemKind::Composition(comp) = &project.item(724).unwrap().kind else {
            panic!("composition 724")
        };
        for (id, first) in [(729, 0.2), (749, 0.06954350674895556)] {
            let owner = comp.layers.iter().find(|l| l.record.id() == id).unwrap();
            let (properties, warnings) = super::super::read_layer_transform(owner, comp).unwrap();
            let numeric = properties
                .iter()
                .find(|p| p.match_name == "ADBE Scale")
                .unwrap()
                .numeric
                .as_ref()
                .unwrap();
            assert!(!numeric.expression_enabled, "{id}: {warnings:?}");
            assert!(!numeric.keyframes.is_empty());
            assert!(numeric.keyframes.len() <= 128);
            assert!((numeric.keyframes[0].values[0] - first).abs() < 0.0001);
            assert!(
                numeric
                    .keyframes
                    .iter()
                    .all(|k| k.values.len() == 2 && k.values[0] == k.values[1])
            );
            if id == 749 {
                assert!(
                    (numeric.keyframes.last().unwrap().values[0] - 1.7590598535582218).abs()
                        < 0.0001
                );
            }
            eprintln!(
                "native724/{id}: {} editable Scale keys; first={:?}, last={:?}",
                numeric.keyframes.len(),
                numeric.keyframes.first(),
                numeric.keyframes.last()
            );
        }
    }

    #[test]
    fn scale_offset_converts_percent_and_repeats_only_first_axis() {
        let mut base = constant(vec![0.07, 0.2, 1.0]);
        base.expression_enabled = true;
        let result = add_constant(&base, &constant(vec![25.0])).unwrap();
        assert_eq!(result.values, vec![0.32, 0.32]);
        assert!(!result.expression_enabled);
        assert!(add_constant(&base, &constant(vec![f64::INFINITY])).is_err());
    }
}
