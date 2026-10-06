//! Bounded lowering of a same-property Scale ramp over its observation interval.

use super::{expression, finished, finite_number, identifier, token, unique_run};
use crate::{
    properties::{self, NumericKeyframe, NumericProperty, NumericValueKind, PropertyError},
    structure::{Composition, Layer},
};

// JS reserved/non-writable names and AE expression-context attributes. An
// assignment to any of these is not an unambiguous ordinary local binding.
const NONLOCAL_BINDINGS: &str = "Infinity NaN arguments await break case catch class const continue debugger default delete do else enum eval export extends false finally for function if implements import in instanceof interface let new null package private protected public return static super switch this throw true try typeof undefined var void while with yield value time thisLayer thisComp thisProperty transform index inPoint outPoint effect comp valueAtTime linear clamp Math posterizeTime length normalize add sub mul div framesToTime timeToFrames loopIn loopOut key nearestKey numKeys seedRandom random gaussRandom wiggle noise ease easeIn easeOut degreesToRadians radiansToDegrees rgbToHsl hslToRgb loopInDuration loopOutDuration lookAt dot cross toComp fromComp toWorld fromWorld velocity speed width height name active content mask smooth hasParent parent anchorPoint position scale rotation opacity text String Number parseInt parseFloat isNaN isFinite";

fn ordinary_local_binding(name: &str) -> bool {
    !NONLOCAL_BINDINGS
        .split_ascii_whitespace()
        .any(|candidate| candidate == name)
}

/// A decimal literal whose spelling has no legacy-octal interpretation.
fn decimal_number(text: &mut &str) -> Option<f64> {
    let trimmed = text.trim_start();
    let unsigned = trimmed
        .strip_prefix('+')
        .or_else(|| trimmed.strip_prefix('-'))
        .unwrap_or(trimmed)
        .as_bytes();
    if unsigned.first() == Some(&b'0') && unsigned.get(1).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    finite_number(text)
}

fn parse(mut text: &str) -> Option<f64> {
    let binding = identifier(&mut text)?;
    if !ordinary_local_binding(binding) {
        return None;
    }
    for part in [
        "=", "scale", "[", "0", "]", "+", "(", "time", "-", "inPoint", ")", "*",
    ] {
        token(&mut text, part)?;
    }
    let rate = decimal_number(&mut text)?;
    token(&mut text, ";")?;
    token(&mut text, "[")?;
    token(&mut text, binding)?;
    token(&mut text, ",")?;
    token(&mut text, binding)?;
    token(&mut text, "]")?;
    finished(text).then_some(rate)
}

/// Returns `None` unless the Scale expression is the admitted affine time ramp.
pub(super) fn lower(
    layer: &Layer,
    composition: &Composition,
    base: &NumericProperty,
) -> Option<Result<NumericProperty, PropertyError>> {
    let roots = properties::root_runs(&layer.content).ok()?;
    let transform = unique_run(&roots, "ADBE Transform Group").ok()?;
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp").ok()?).ok()?;
    let scale = properties::unique_list(unique_run(&leaves, "ADBE Scale").ok()?, *b"tdbs").ok()?;
    let rate = parse(expression(scale).ok()?)?;
    Some(lower_numeric(layer, composition, base, rate))
}

fn lower_numeric(
    layer: &Layer,
    composition: &Composition,
    base: &NumericProperty,
    rate_percent_per_sec: f64,
) -> Result<NumericProperty, PropertyError> {
    if base.value_kind != NumericValueKind::Continuous
        || base.dimensions_separated
        || base.animated
        || !base.keyframes.is_empty()
        || !(2..=3).contains(&base.values.len())
    {
        return Err(PropertyError::Layout(
            "Scale time ramp requires one static combined Scale base",
        ));
    }
    let base_value = base.values[0];
    let (Some(in_point), Some(out_point), Some(stretch)) = (
        layer.record.in_point(),
        layer.record.out_point(),
        layer.record.stretch(),
    ) else {
        return Err(PropertyError::Layout("Scale time ramp clock is invalid"));
    };
    if !base_value.is_finite()
        || !rate_percent_per_sec.is_finite()
        || !in_point.is_finite()
        || !out_point.is_finite()
        || !stretch.is_finite()
        || stretch <= 0.0
        || out_point <= in_point
    {
        return Err(PropertyError::Layout("Scale time ramp values are invalid"));
    }

    let has_transform_child = composition.layers.iter().any(|candidate| {
        candidate.record.id() != layer.record.id()
            && candidate.record.parent_id() == layer.record.id()
    });
    let bounds = if has_transform_child {
        let Some(start) = layer.record.start_time() else {
            return Err(PropertyError::Layout(
                "Scale time ramp parent source clock is invalid",
            ));
        };
        if !start.is_finite()
            || !composition.duration_secs.is_finite()
            || composition.duration_secs <= 0.0
        {
            return Err(PropertyError::Layout(
                "Scale time ramp parent observation interval is invalid",
            ));
        }
        // Transform-parent wrappers use the receiving composition's identity
        // clock and stay live even outside the native parent's paint lifetime.
        [
            -start / stretch,
            (composition.duration_secs - start) / stretch,
        ]
    } else {
        [in_point, out_point]
    };
    if bounds.iter().any(|value| !value.is_finite()) || bounds[0] >= bounds[1] {
        return Err(PropertyError::Layout(
            "Scale time ramp observation interval is invalid",
        ));
    }

    // Native Scale storage is a fraction while AE expressions use percentage
    // points. Property key times use source time, so composition-time slope also
    // carries the layer stretch ratio.
    let value_at = |source_time: f64| {
        base_value + (source_time - in_point) * stretch * rate_percent_per_sec * 0.01
    };
    let values = bounds.map(value_at);
    if values.iter().any(|value| !value.is_finite()) {
        return Err(PropertyError::NonFinite);
    }
    let mut lowered = base.clone();
    lowered.values.clear();
    lowered.animated = true;
    lowered.expression_enabled = false;
    lowered.expression_present = false;
    lowered.keyframes = vec![
        linear_key(bounds[0], values[0]),
        linear_key(bounds[1], values[1]),
    ];
    Ok(lowered)
}

fn linear_key(time_secs: f64, value: f64) -> NumericKeyframe {
    NumericKeyframe {
        time_secs,
        values: vec![value, value],
        in_interpolation: 1,
        out_interpolation: 1,
        in_speed: vec![0.0, 0.0],
        in_influence: vec![0.0, 0.0],
        out_speed: vec![0.0, 0.0],
        out_influence: vec![0.0, 0.0],
        spatial_in: Vec::new(),
        spatial_out: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{data, list, numeric};
    use crate::{
        schema::layer_records::{FreshLayerClockFields, LayerRecord, NativeRational},
        structure::{ItemKind, read_project},
        timing::Duration24,
    };

    fn fixture(expression: &str) -> (crate::structure::Layer, crate::structure::Composition) {
        let project = read_project(include_bytes!(
            "../../../tests/fixtures/properties/property_1D_opacity.aep"
        ))
        .unwrap();
        let mut composition = project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(composition) => Some((**composition).clone()),
                _ => None,
            })
            .unwrap();
        let mut layer = composition.layers[0].clone();
        layer.record = layer
            .record
            .clone()
            .with_source_clock(FreshLayerClockFields {
                stretch: NativeRational {
                    numerator: 2,
                    denominator: 1,
                },
                start_time: NativeRational {
                    numerator: 2,
                    denominator: 1,
                },
                in_point: NativeRational {
                    numerator: 1,
                    denominator: 1,
                },
                out_point: NativeRational {
                    numerator: 3,
                    denominator: 1,
                },
            })
            .unwrap();
        layer.content = vec![list(
            b"tdgp",
            vec![
                data(b"tdmn", b"ADBE Transform Group"),
                list(
                    b"tdgp",
                    vec![
                        data(b"tdmn", b"ADBE Scale"),
                        numeric(&[2.0, 2.0, 1.0], Some(expression)),
                    ],
                ),
            ],
        )];
        composition.layers = vec![layer.clone()];
        (layer, composition)
    }

    #[test]
    fn affine_time_scale_becomes_two_editable_linear_keys() {
        // Synthetic program over a public native layer/clock envelope. The
        // licensed source body is verified only in private exact-source proof.
        let (layer, composition) =
            fixture("zoom = scale[0] + (time - inPoint) * 2.5;\n[zoom, zoom]");
        let (properties, warnings) =
            super::super::read_layer_transform(&layer, &composition).unwrap();
        let scale = properties
            .iter()
            .find(|property| property.match_name == "ADBE Scale")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        assert!(!scale.expression_enabled);
        assert!(!scale.expression_present);
        assert!(scale.animated);
        assert!(scale.values.is_empty());
        assert_eq!(scale.keyframes.len(), 2);
        assert_eq!(scale.keyframes[0].time_secs, 1.0);
        assert_eq!(scale.keyframes[0].values, [2.0, 2.0]);
        assert_eq!(scale.keyframes[1].time_secs, 3.0);
        assert_eq!(scale.keyframes[1].values, [2.1, 2.1]);
        let interior = (scale.keyframes[0].values[0] + scale.keyframes[1].values[0]) / 2.0;
        assert_eq!(interior, 2.05);
        assert!(
            scale
                .keyframes
                .iter()
                .all(|key| { key.in_interpolation == 1 && key.out_interpolation == 1 })
        );
        assert!(
            warnings
                .iter()
                .any(|warning| { warning.contains("same-property affine time ramp lowered") })
        );
        assert!(
            !warnings
                .iter()
                .any(|warning| warning.contains("control link not lowered"))
        );
    }

    #[test]
    fn transform_parent_ramp_covers_the_receiving_composition() {
        let (layer, mut composition) =
            fixture("zoom = scale[0] + (time - inPoint) * 2.5; [zoom, zoom]");
        composition.duration_secs = 10.0;
        let mut child = layer.clone();
        child.record = LayerRecord::solid_ae26(
            4_000_000_000,
            4_000_000_001,
            Duration24::from_frames(240).unwrap(),
        )
        .unwrap()
        .with_export_options(true, false, 2, layer.record.id(), 0, 0)
        .unwrap();
        composition.layers.push(child);

        let (properties, _) = super::super::read_layer_transform(&layer, &composition).unwrap();
        let scale = properties
            .iter()
            .find(|property| property.match_name == "ADBE Scale")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        assert_eq!(scale.keyframes[0].time_secs, -1.0);
        assert_eq!(scale.keyframes[0].values, [1.9, 1.9]);
        assert_eq!(scale.keyframes[1].time_secs, 4.0);
        assert_eq!(scale.keyframes[1].values, [2.15, 2.15]);
        assert_eq!(
            layer.record.start_time().unwrap()
                + scale.keyframes[0].time_secs * layer.record.stretch().unwrap(),
            0.0
        );
        assert_eq!(
            layer.record.start_time().unwrap()
                + scale.keyframes[1].time_secs * layer.record.stretch().unwrap(),
            composition.duration_secs
        );
    }

    #[test]
    fn member_alias_keeps_the_expression_fallback() {
        let (layer, composition) =
            fixture("zoom = scale[0] + (time - inPoint) * 2.5; [zoom, zoom]");
        let (properties, warnings) = super::super::read_layer_transform_inner(
            &layer,
            &composition,
            None,
            &mut Vec::new(),
            Some(super::super::cross_comp::Member::Scale),
        )
        .unwrap();
        let scale = properties
            .iter()
            .find(|property| property.match_name == "ADBE Scale")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        assert!(scale.expression_enabled);
        assert!(scale.expression_present);
        assert!(scale.keyframes.is_empty());
        assert!(
            !warnings
                .iter()
                .any(|warning| warning.contains("affine time ramp lowered"))
        );
    }

    #[test]
    fn unrelated_ambiguous_or_changed_programs_keep_the_expression_fallback() {
        for expression in [
            "value",
            "time=scale[0]+(time-inPoint)*2.5;[time,time]",
            "value=scale[0]+(time-inPoint)*2.5;[value,value]",
            "if=scale[0]+(time-inPoint)*2.5;[if,if]",
            "scale=scale[0]+(time-inPoint)*2.5;[scale,scale]",
            "zoom=scale[0]+(time-inPoint)*02.5;[zoom,zoom]",
            "zoom=scale[1]+(time-inPoint)*2.5;[zoom,zoom]",
            "zoom=scale[0]+(time+inPoint)*2.5;[zoom,zoom]",
            "zoom=scale[0]+(time-inPoint)*2.5;[zoom,other]",
            "zoom=scale[0]+(time-inPoint)*2.5;[zoom,zoom];other()",
        ] {
            let (layer, composition) = fixture(expression);
            let (properties, _) = super::super::read_layer_transform(&layer, &composition).unwrap();
            let scale = properties
                .iter()
                .find(|property| property.match_name == "ADBE Scale")
                .unwrap()
                .numeric
                .as_ref()
                .unwrap();
            assert!(scale.expression_enabled, "{expression}");
            assert_eq!(scale.values, [2.0, 2.0, 1.0]);
            assert!(scale.keyframes.is_empty());
        }
    }

    #[test]
    fn recognized_program_with_unsupported_base_returns_local_error() {
        let (layer, composition) =
            fixture("zoom = scale[0] + (time - inPoint) * 2.5; [zoom, zoom]");
        let (properties, _) = super::super::read_transform(&layer.content).unwrap();
        let mut base = properties
            .iter()
            .find(|property| property.match_name == "ADBE Scale")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap()
            .clone();
        base.values.truncate(1);
        assert!(super::lower(&layer, &composition, &base).unwrap().is_err());
    }
}
