//! Exact finite-interval lowering of an affine Rotation offset loop.

use super::{expression, finished, quoted, token, unique_run};
use crate::{
    properties::{self, NumericProperty, NumericValueKind, PropertyError},
    structure::{Composition, Layer},
};

fn recognized(mut text: &str) -> bool {
    token(&mut text, "loopOut").is_some()
        && token(&mut text, "(").is_some()
        && quoted(&mut text) == Some("offset")
        && token(&mut text, ")").is_some()
        && finished(text)
}

pub(super) fn lower(
    layer: &Layer,
    composition: &Composition,
    base: &NumericProperty,
) -> Option<Result<NumericProperty, PropertyError>> {
    let roots = properties::root_runs(&layer.content).ok()?;
    let transform = unique_run(&roots, "ADBE Transform Group").ok()?;
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp").ok()?).ok()?;
    let leaf =
        properties::unique_list(unique_run(&leaves, "ADBE Rotate Z").ok()?, *b"tdbs").ok()?;
    if !recognized(expression(leaf).ok()?) {
        return None;
    }
    Some((|| {
        let (Some(start), Some(stretch)) = (layer.record.start_time(), layer.record.stretch())
        else {
            return Err(PropertyError::Layout("invalid Rotation offset-loop clock"));
        };
        if !start.is_finite()
            || !stretch.is_finite()
            || stretch == 0.0
            || !composition.duration_secs.is_finite()
            || composition.duration_secs <= 0.0
        {
            return Err(PropertyError::Layout(
                "invalid Rotation offset-loop interval",
            ));
        }
        // Parent transforms stay live outside their own layer lifetime. Bound the
        // preparation by the receiving composition, not this layer's in/out points.
        let a = -start / stretch;
        let b = (composition.duration_secs - start) / stretch;
        extend(base, [a.min(b), a.max(b)])
    })())
}

fn extend(base: &NumericProperty, bounds: [f64; 2]) -> Result<NumericProperty, PropertyError> {
    let [first, last] = base.keyframes.as_slice() else {
        return Err(PropertyError::Layout(
            "Rotation offset loop requires exactly two keys",
        ));
    };
    if base.value_kind != NumericValueKind::Continuous
        || base.dimensions_separated
        || bounds.iter().any(|value| !value.is_finite())
        || bounds[0] >= bounds[1]
        || base.keyframes.iter().any(|key| {
            !key.time_secs.is_finite()
                || key.values.len() != 1
                || !key.values[0].is_finite()
                || key.in_interpolation != 1
                || key.out_interpolation != 1
                || !key.spatial_in.is_empty()
                || !key.spatial_out.is_empty()
        })
        || first.time_secs >= last.time_secs
    {
        return Err(PropertyError::Layout(
            "Rotation offset loop requires finite Linear scalar keys",
        ));
    }
    let slope = (last.values[0] - first.values[0]) / (last.time_secs - first.time_secs);
    if !slope.is_finite() {
        return Err(PropertyError::NonFinite);
    }
    let mut lowered = base.clone();
    // Before the first native key, loopOut retains the ordinary constant value.
    if bounds[0] < first.time_secs {
        let mut prefix = first.clone();
        prefix.time_secs = bounds[0];
        lowered.keyframes.insert(0, prefix);
    }
    if bounds[1] > last.time_secs {
        let mut endpoint = last.clone();
        endpoint.time_secs = bounds[1];
        endpoint.values[0] = last.values[0] + slope * (bounds[1] - last.time_secs);
        if !endpoint.values[0].is_finite() {
            return Err(PropertyError::NonFinite);
        }
        lowered.keyframes.push(endpoint);
    }
    lowered.animated = true;
    lowered.expression_enabled = false;
    lowered.expression_present = false;
    Ok(lowered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::properties::NumericKeyframe;

    fn native() -> NumericProperty {
        let key = |time_secs, value| NumericKeyframe {
            time_secs,
            values: vec![value],
            in_interpolation: 1,
            out_interpolation: 1,
            in_speed: vec![0.0],
            in_influence: vec![100.0 / 6.0],
            out_speed: vec![0.0],
            out_influence: vec![100.0 / 6.0],
            spatial_in: vec![],
            spatial_out: vec![],
        };
        NumericProperty {
            values: vec![0.0],
            animated: true,
            expression_enabled: true,
            expression_present: true,
            dimensions_separated: false,
            keyframes: vec![key(0.0, 0.0), key(1.8, 20.0)],
            value_kind: NumericValueKind::Continuous,
        }
    }

    #[test]
    fn rotation_offset_loop_exact_grammar_and_linear_extension() {
        for source in ["loopOut(\"offset\")", " loopOut ( 'offset' ) ; "] {
            assert!(recognized(source));
        }
        for source in [
            "loopOut(\"cycle\")",
            "loopOut(\"offset\", 1)",
            "loopOut(\"offset\") + 1",
            "value; loopOut(\"offset\")",
        ] {
            assert!(!recognized(source));
        }
        let lowered = extend(&native(), [-2.0, 9.0]).unwrap();
        assert_eq!(lowered.keyframes.len(), 4);
        assert_eq!(lowered.keyframes[0].values, [0.0]);
        assert_eq!(lowered.keyframes[3].values, [100.0]);
        assert!(!lowered.expression_enabled);
        assert!(!lowered.expression_present);
        assert_eq!(extend(&native(), [0.0, 1.0]).unwrap().keyframes.len(), 2);
    }

    #[test]
    fn rotation_offset_loop_declines_non_affine_or_invalid_keys() {
        for edit in 0..8 {
            let mut value = native();
            match edit {
                0 => value.keyframes[1].in_interpolation = 2,
                1 => value.keyframes[0].out_interpolation = 3,
                2 => value.keyframes[1].time_secs = 0.0,
                3 => value.keyframes[1].values[0] = f64::NAN,
                4 => value.keyframes[0].spatial_in.push(0.0),
                5 => value.keyframes.push(value.keyframes[1].clone()),
                6 => value.dimensions_separated = true,
                _ => value.value_kind = NumericValueKind::Integer,
            }
            assert!(extend(&value, [0.0, 10.0]).is_err(), "guard {edit}");
        }
        assert!(extend(&native(), [0.0, f64::INFINITY]).is_err());
    }
}
