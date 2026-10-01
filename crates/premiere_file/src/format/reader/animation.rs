//! Narrow, fail-closed reader for occurrence-local intrinsic Motion and Opacity.

use super::effects::{read_track_matte, TrackMatteKey};
use crate::{
    error::{ensure, unsupported, Result},
    format::{Graph, Located},
    schema::{
        native::{Reference, VideoComponentChain, VideoComponentParam, VideoFilterComponent},
        records, PrAnimatedProperty, PrBlendMode, PrKeyframeEasing, PrLinearWipe, PrMask,
        PrPointKeyframe, PrPropertyAnimation, PrScalarKeyframe, PrStaticCrop, PrStaticTransform,
        CROP_PARAMS, CROP_PARAMS_26_5, MOTION_PARAMS, MOTION_PARAMS_26_5, OPACITY_PARAMS,
        OPACITY_PARAMS_26_5, TICKS, TRACK_MATTE_KEY,
    },
    Omission,
};
use std::collections::BTreeSet;

fn key_fields<'a>(wire: &'a str, expected: usize, context: &str) -> Result<Vec<&'a str>> {
    let fields: Vec<_> = wire.split(',').collect();
    ensure!(
        fields.len() == expected,
        "{context}: unexpected Premiere keyframe shape"
    );
    Ok(fields)
}

/// The value field of a static `StartKeyframe` of `expected` fields.
pub(super) fn start_field<'a>(wire: &'a str, expected: usize, context: &str) -> Result<&'a str> {
    let fields = key_fields(wire, expected, context)?;
    ensure!(
        fields[0] == records::STATIC_KEYFRAME_TIME,
        "{context}: unexpected initial keyframe time"
    );
    Ok(fields[1])
}

pub(super) fn scalar_start(wire: &str, context: &str) -> Result<f64> {
    let value = start_field(wire, 8, context)?
        .parse::<f64>()
        .map_err(|_| unsupported(format!("{context}: invalid initial value")))?;
    ensure!(value.is_finite(), "{context}: nonfinite initial value");
    Ok(value)
}

fn default_param(spec: &crate::schema::MotionParamSpec, wire: &str, context: &str) -> Result<()> {
    let fields = key_fields(wire, if spec.is_point() { 14 } else { 8 }, context)?;
    ensure!(
        fields[0] == records::STATIC_KEYFRAME_TIME,
        "{context}: unexpected initial keyframe time"
    );
    ensure!(
        spec.accepts_default(fields[1]),
        "{context}: nondefault or unknown Motion parameter {:?}",
        spec.name
    );
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct NativeScalarKeyframe {
    source_ticks: i64,
    value: f64,
    outgoing_mode: u8,
    incoming_speed: f64,
    incoming_influence: f64,
    outgoing_speed: f64,
    outgoing_influence: f64,
    /// Whether a Bezier segment into this key ignores the stored in-handle and
    /// arrives with a zero-length one. Premiere 26.5.1 reads a scalar key that
    /// starts a Hold this way (Adobe readbacks of a graphic clip Opacity and a
    /// clip Motion Scale); a Point key into a Hold is unprobed and keeps it.
    incoming_handle_ignored: bool,
}

fn cubic_easing(
    start: NativeScalarKeyframe,
    end: NativeScalarKeyframe,
    context: &str,
) -> Result<PrKeyframeEasing> {
    ensure!(
        (0.0..=1.0).contains(&start.outgoing_influence)
            && (0.0..=1.0).contains(&end.incoming_influence),
        "{context}: Bezier influence must be between zero and one"
    );
    let end = if end.incoming_handle_ignored {
        NativeScalarKeyframe {
            incoming_speed: 0.0,
            incoming_influence: 0.0,
            ..end
        }
    } else {
        end
    };
    if start.value == end.value {
        let outgoing_rise_is_zero = start.outgoing_speed == 0.0 || start.outgoing_influence == 0.0;
        let incoming_rise_is_zero = end.incoming_speed == 0.0 || end.incoming_influence == 0.0;
        if outgoing_rise_is_zero && incoming_rise_is_zero {
            return Ok(PrKeyframeEasing::Linear);
        }
        return Err(unsupported(format!(
            "{context}: Bezier between equal values cannot preserve Premiere velocity"
        )));
    }
    let duration_secs =
        (i128::from(end.source_ticks) - i128::from(start.source_ticks)) as f64 / TICKS as f64;
    let velocity_scale = duration_secs / (end.value - start.value);
    let x1 = start.outgoing_influence;
    let y1 = x1 * start.outgoing_speed * velocity_scale;
    let x2 = 1.0 - end.incoming_influence;
    let y2 = 1.0 - end.incoming_influence * end.incoming_speed * velocity_scale;
    ensure!(
        [x1, y1, x2, y2].into_iter().all(f64::is_finite),
        "{context}: nonfinite normalized Bezier handle"
    );
    Ok(PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 })
}

pub(super) fn point_start(wire: &str, context: &str) -> Result<[f64; 2]> {
    let (x, y) = start_field(wire, 14, context)?
        .split_once(':')
        .ok_or_else(|| unsupported(format!("{context}: invalid point StartKeyframe")))?;
    let point = [
        x.parse::<f64>()
            .map_err(|_| unsupported(format!("{context}: invalid point StartKeyframe")))?,
        y.parse::<f64>()
            .map_err(|_| unsupported(format!("{context}: invalid point StartKeyframe")))?,
    ];
    ensure!(
        point.iter().all(|value| value.is_finite()),
        "{context}: invalid point StartKeyframe"
    );
    Ok(point)
}

pub(super) fn bool_start(wire: &str, context: &str) -> Result<bool> {
    match start_field(wire, 8, context)? {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(unsupported(format!(
            "{context}: invalid boolean StartKeyframe"
        ))),
    }
}

/// The easing of the interval that ends at `native[index]`, from the outgoing
/// mode of the key before it. Point keys read every interval this way; a
/// reading that was probed on scalar keys only belongs in [`scalar_easing_at`].
fn easing_at(
    native: &[NativeScalarKeyframe],
    index: usize,
    context: &str,
) -> Result<PrKeyframeEasing> {
    if index == 0 {
        return Ok(PrKeyframeEasing::Linear);
    }
    let start = native[index - 1];
    match start.outgoing_mode {
        0 => Ok(PrKeyframeEasing::Linear),
        4 => Ok(PrKeyframeEasing::Hold),
        5 => cubic_easing(start, native[index], context),
        mode => Err(unsupported(format!(
            "{context}: unsupported interpolation mode {mode}"
        ))),
    }
}

/// [`easing_at`] for scalar keys: Premiere eases a Linear key into a Bezier key
/// with both stored handles (Oracle run C6, F23, probed on Motion Scale).
/// Handles on the chord, such as the zero handles that export writes for a
/// Linear segment, keep the interval exactly Linear.
fn scalar_easing_at(
    native: &[NativeScalarKeyframe],
    index: usize,
    context: &str,
) -> Result<PrKeyframeEasing> {
    if index == 0 || native[index - 1].outgoing_mode != 0 || native[index].outgoing_mode != 5 {
        return easing_at(native, index, context);
    }
    match cubic_easing(native[index - 1], native[index], context)? {
        PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 } if x1 == y1 && x2 == y2 => {
            Ok(PrKeyframeEasing::Linear)
        }
        easing => Ok(easing),
    }
}

pub(super) fn scalar_keys(wire: &str, context: &str) -> Result<Vec<PrScalarKeyframe>> {
    ensure!(
        wire.is_empty() || wire.ends_with(';'),
        "{context}: unterminated keyframe list"
    );
    let mut native = Vec::new();
    for item in wire.split_terminator(';') {
        ensure!(!item.is_empty(), "{context}: empty animation key");
        let fields = key_fields(item, 8, context)?;
        let outgoing_mode = fields[2]
            .parse::<u8>()
            .map_err(|_| unsupported(format!("{context}: invalid interpolation mode")))?;
        ensure!(
            matches!(outgoing_mode, 0 | 4 | 5),
            "{context}: unsupported interpolation mode {outgoing_mode}"
        );
        fields[3]
            .parse::<u8>()
            .map_err(|_| unsupported(format!("{context}: invalid keyframe flags")))?;
        let parse_handle = |field: &str| {
            field
                .parse::<f64>()
                .map_err(|_| unsupported(format!("{context}: invalid interpolation handle")))
        };
        let key = NativeScalarKeyframe {
            source_ticks: fields[0]
                .parse::<i64>()
                .map_err(|_| unsupported(format!("{context}: invalid key time")))?,
            value: fields[1]
                .parse::<f64>()
                .map_err(|_| unsupported(format!("{context}: invalid key value")))?,
            outgoing_mode,
            incoming_speed: parse_handle(fields[4])?,
            incoming_influence: parse_handle(fields[5])?,
            outgoing_speed: parse_handle(fields[6])?,
            outgoing_influence: parse_handle(fields[7])?,
            incoming_handle_ignored: outgoing_mode == 4,
        };
        ensure!(
            [
                key.value,
                key.incoming_speed,
                key.incoming_influence,
                key.outgoing_speed,
                key.outgoing_influence,
            ]
            .into_iter()
            .all(f64::is_finite),
            "{context}: nonfinite key value or interpolation handle"
        );
        ensure!(
            native
                .last()
                .is_none_or(|last: &NativeScalarKeyframe| last.source_ticks < key.source_ticks),
            "{context}: animation keys must have strictly increasing source times"
        );
        native.push(key);
    }

    native
        .iter()
        .enumerate()
        .map(|(index, key)| {
            Ok(PrScalarKeyframe {
                source_ticks: key.source_ticks,
                value: key.value,
                easing: scalar_easing_at(&native, index, context)?,
            })
        })
        .collect()
}

/// Read the exact static standalone Crop layout observed in pinned Adobe projects.
fn read_crop_component(
    graph: &Graph<'_>,
    reference: &Reference,
    from: &str,
) -> Result<PrStaticCrop> {
    let crop = graph.follow::<VideoFilterComponent>(reference, from)?;
    let body = crop
        .value
        .component
        .ok_or_else(|| unsupported(format!("{}: missing Crop Component", crop.identity)))?;
    // Premiere 26.3 writes `Bypass` and `Intrinsic` false; Premiere 26.5.1 omits
    // both, and only in that layout does a missing `Bypass` mean active.
    let premiere_26_5 = body.bypass.is_none() && body.intrinsic.is_none();
    ensure!(
        crop.value.match_name.as_deref() == Some("AE.ADBE AECrop")
            && body.display_name.as_deref() == Some("Crop")
            && (premiere_26_5
                || (body.bypass.as_deref() == Some("false")
                    && body.intrinsic.as_deref() == Some("false"))),
        "{}: unsupported Crop component",
        crop.identity
    );
    let layout = if premiere_26_5 {
        &CROP_PARAMS_26_5
    } else {
        &CROP_PARAMS
    };
    let params = body
        .params
        .ok_or_else(|| unsupported(format!("{}: missing Crop Params", crop.identity)))?;
    ensure!(
        params.items.len() == layout.len(),
        "{}: unsupported Crop parameter layout",
        crop.identity
    );
    let mut values = [0.0; 5];
    let mut seen = BTreeSet::new();
    for param in &params.items {
        let record = graph.locate(param, &crop.identity)?;
        ensure!(
            record.tag() == "VideoComponentParam",
            "{}: unsupported Crop parameter type",
            record.identity()
        );
        let input = graph.decode::<VideoComponentParam>(record)?;
        let id = input
            .value
            .parameter_id
            .parse::<usize>()
            .ok()
            .filter(|id| (1..=layout.len()).contains(id))
            .ok_or_else(|| unsupported(format!("{}: unknown Crop parameter", input.identity)))?;
        ensure!(
            seen.insert(id),
            "{}: duplicate Crop ParameterID {id}",
            input.identity
        );
        let spec = &layout[id - 1];
        ensure!(
            input.value.name.as_deref() == Some(spec.name)
                && input.value.class_id.as_deref() == Some(spec.class_id)
                && input.value.parameter_control_type.as_deref() == spec.control
                && input.value.lower_bound.as_deref() == spec.lower
                && input.value.upper_bound.as_deref() == spec.upper
                && input.value.lower_ui_bound.as_deref() == spec.lower_ui
                && input.value.upper_ui_bound.as_deref() == spec.upper_ui,
            "{}: unexpected Crop parameter {} layout",
            input.identity,
            spec.name
        );
        ensure!(
            input.value.is_time_varying.as_deref() == spec.is_time_varying
                && input.value.keyframes.as_deref().is_none_or(str::is_empty),
            "{}: animated or malformed Crop {} is unsupported",
            input.identity,
            spec.name
        );
        if id == 5 {
            let zoom = bool_start(&input.value.start_keyframe, &input.identity)?;
            ensure!(!zoom, "{}: Crop Zoom is unsupported", input.identity);
            continue;
        }
        // Static StartKeyframe is authored state. Native Crop records can retain
        // a stale CurrentValue (Left=99 while the rendered StartKeyframe is 0).
        let value = scalar_start(&input.value.start_keyframe, &input.identity)?;
        let slot = if id == 6 { 4 } else { id - 1 };
        values[slot] = value;
    }
    ensure!(
        seen.len() == layout.len(),
        "{}: missing Crop parameters",
        crop.identity
    );
    let crop = PrStaticCrop {
        left: values[0],
        top: values[1],
        right: values[2],
        bottom: values[3],
        edge_feather: values[4],
    };
    crop.validate()?;
    Ok(crop)
}

pub(super) fn point_keys(wire: &str, context: &str) -> Result<Vec<PrPointKeyframe>> {
    ensure!(
        wire.is_empty() || wire.ends_with(';'),
        "{context}: unterminated keyframe list"
    );
    let mut native = Vec::new();
    let mut points = Vec::new();
    for item in wire.split_terminator(';') {
        ensure!(!item.is_empty(), "{context}: empty animation key");
        let fields = key_fields(item, 14, context)?;
        let (x, y) = fields[1]
            .split_once(':')
            .ok_or_else(|| unsupported(format!("{context}: invalid point key value")))?;
        let parse = |field: &str, name: &str| {
            field
                .parse::<f64>()
                .map_err(|_| unsupported(format!("{context}: invalid {name}")))
        };
        let point = [parse(x, "point key value")?, parse(y, "point key value")?];
        let outgoing_mode = fields[2]
            .parse::<u8>()
            .map_err(|_| unsupported(format!("{context}: invalid interpolation mode")))?;
        ensure!(
            matches!(outgoing_mode, 0 | 4 | 5),
            "{context}: unsupported interpolation mode {outgoing_mode}"
        );
        fields[3]
            .parse::<u8>()
            .map_err(|_| unsupported(format!("{context}: invalid keyframe flags")))?;
        let temporal = NativeScalarKeyframe {
            source_ticks: fields[0]
                .parse::<i64>()
                .map_err(|_| unsupported(format!("{context}: invalid key time")))?,
            value: 0.0,
            outgoing_mode,
            incoming_speed: parse(fields[4], "interpolation handle")?,
            incoming_influence: parse(fields[5], "interpolation handle")?,
            outgoing_speed: parse(fields[6], "interpolation handle")?,
            outgoing_influence: parse(fields[7], "interpolation handle")?,
            // A Point Bezier into a Hold key is unprobed: keep its in-handle.
            incoming_handle_ignored: false,
        };
        let spatial_mode = fields[8]
            .parse::<u8>()
            .map_err(|_| unsupported(format!("{context}: invalid spatial interpolation mode")))?;
        let spatial_flags = fields[9]
            .parse::<u8>()
            .map_err(|_| unsupported(format!("{context}: invalid spatial keyframe flags")))?;
        let incoming = [
            parse(fields[10], "spatial incoming tangent")?,
            parse(fields[11], "spatial incoming tangent")?,
        ];
        let outgoing = [
            parse(fields[12], "spatial outgoing tangent")?,
            parse(fields[13], "spatial outgoing tangent")?,
        ];
        let (spatial_in_tangent, spatial_out_tangent) = match (spatial_mode, spatial_flags) {
            (0, 0) => {
                ensure!(
                    incoming == [0.0, 0.0] && outgoing == [0.0, 0.0],
                    "{context}: linear spatial key has nonzero tangents"
                );
                (None, None)
            }
            // Flag 4 is Premiere's automatic spatial mode. Import its resolved
            // handles as explicit editable tangents; automatic recomputation is
            // deliberately not represented by the FX schema.
            (5, 0 | 4) => (Some(incoming), Some(outgoing)),
            _ => {
                return Err(unsupported(format!(
                    "{context}: unsupported spatial interpolation mode {spatial_mode} with flags {spatial_flags}"
                )))
            }
        };
        ensure!(
            point
                .into_iter()
                .chain(incoming)
                .chain(outgoing)
                .chain([
                    temporal.incoming_speed,
                    temporal.incoming_influence,
                    temporal.outgoing_speed,
                    temporal.outgoing_influence,
                ])
                .all(f64::is_finite),
            "{context}: nonfinite point key or interpolation handle"
        );
        ensure!(
            native
                .last()
                .is_none_or(|last: &NativeScalarKeyframe| last.source_ticks < temporal.source_ticks),
            "{context}: animation keys must have strictly increasing source times"
        );
        native.push(temporal);
        points.push((point, spatial_in_tangent, spatial_out_tangent));
    }
    let mut distance = 0.0;
    for index in 1..points.len() {
        let (previous, incoming) = (points[index - 1], points[index]);
        let segment_length = crate::schema::spatial::segment_length(
            &PrPointKeyframe {
                source_ticks: native[index - 1].source_ticks,
                value: previous.0,
                easing: PrKeyframeEasing::Linear,
                spatial_in_tangent: previous.1,
                spatial_out_tangent: previous.2,
            },
            &PrPointKeyframe {
                source_ticks: native[index].source_ticks,
                value: incoming.0,
                easing: PrKeyframeEasing::Linear,
                spatial_in_tangent: incoming.1,
                spatial_out_tangent: incoming.2,
            },
        )
        .ok_or_else(|| unsupported(format!("{context}: nonfinite spatial curve length")))?;
        distance += segment_length;
        ensure!(
            distance.is_finite(),
            "{context}: nonfinite cumulative spatial curve length"
        );
        native[index].value = distance;
    }
    native
        .iter()
        .enumerate()
        .zip(points)
        .map(
            |((index, key), (value, spatial_in_tangent, spatial_out_tangent))| {
                Ok(PrPointKeyframe {
                    source_ticks: key.source_ticks,
                    value,
                    easing: easing_at(&native, index, context)?,
                    spatial_in_tangent,
                    spatial_out_tangent,
                })
            },
        )
        .collect()
}

/// Read one bounded cardinal Linear Wipe component.
fn read_linear_wipe(graph: &Graph<'_>, reference: &Reference, from: &str) -> Result<PrLinearWipe> {
    let effect = graph.follow::<VideoFilterComponent>(reference, from)?;
    let body = effect
        .value
        .component
        .ok_or_else(|| unsupported(format!("{}: missing Component", effect.identity)))?;
    // As for the Crop, Premiere 26.5.1 omits `Bypass` and `Intrinsic`; it also
    // names the effect "Linear Wipe (Legacy)". Its parameters are unchanged.
    let premiere_26_5 = body.bypass.is_none() && body.intrinsic.is_none();
    let display_name = if premiere_26_5 {
        "Linear Wipe (Legacy)"
    } else {
        "Linear Wipe"
    };
    ensure!(
        effect.value.match_name.as_deref() == Some("AE.ADBE Linear Wipe")
            && body.display_name.as_deref() == Some(display_name)
            && (premiere_26_5
                || (body.bypass.as_deref() == Some("false")
                    && body.intrinsic.as_deref() == Some("false"))),
        "{}: unsupported Linear Wipe component",
        effect.identity
    );
    let params = body
        .params
        .ok_or_else(|| unsupported(format!("{}: missing Linear Wipe Params", effect.identity)))?;
    ensure!(
        params.items.len() == 3,
        "{}: unsupported Linear Wipe parameter layout",
        effect.identity
    );
    let mut initial_completion = None;
    let mut completion = None;
    let mut angle = None;
    let mut feather = None;
    let mut ids = BTreeSet::new();
    for reference in &params.items {
        let record = graph.locate(reference, &effect.identity)?;
        ensure!(
            record.tag() == "VideoComponentParam",
            "{}: unsupported Linear Wipe parameter type",
            record.identity()
        );
        let input = graph.decode::<VideoComponentParam>(record)?;
        ensure!(
            ids.insert(input.value.parameter_id.clone()),
            "{}: duplicate Linear Wipe ParameterID",
            input.identity
        );
        let initial = scalar_start(&input.value.start_keyframe, &input.identity)?;
        let name = input.value.name.as_deref().unwrap_or_default();
        match (input.value.parameter_id.as_str(), name) {
            ("1", "Transition Completion") => {
                let keys = scalar_keys(
                    input.value.keyframes.as_deref().unwrap_or_default(),
                    &input.identity,
                )?;
                ensure!(
                    !keys.is_empty() && keys.iter().all(|key| (0.0..=100.0).contains(&key.value)),
                    "{}: Transition Completion requires bounded animation keys",
                    input.identity
                );
                ensure!(
                    (0.0..=100.0).contains(&initial),
                    "{}: initial Transition Completion is out of range",
                    input.identity
                );
                initial_completion = Some(initial);
                completion = Some(keys);
            }
            ("2", "Wipe Angle") => {
                ensure!(
                    input.value.keyframes.is_none()
                        && input.value.is_time_varying.as_deref() != Some("true"),
                    "{}: animated Wipe Angle is unsupported",
                    input.identity
                );
                let rounded = initial.round();
                ensure!(
                    rounded == initial && (i16::MIN as f64..=i16::MAX as f64).contains(&rounded),
                    "{}: Wipe Angle must be an integer",
                    input.identity
                );
                let normalized = (rounded as i16).rem_euclid(360);
                ensure!(
                    matches!(normalized, 0 | 90 | 180 | 270),
                    "{}: only cardinal Wipe Angle values are supported",
                    input.identity
                );
                angle = Some(normalized);
            }
            ("3", "Feather") => {
                ensure!(
                    input.value.keyframes.is_none()
                        && input.value.is_time_varying.as_deref() != Some("true")
                        && (0.0..=32_000.0).contains(&initial),
                    "{}: animated or out-of-range Feather is unsupported",
                    input.identity
                );
                feather = Some(initial);
            }
            _ => {
                return Err(unsupported(format!(
                    "{}: unexpected Linear Wipe parameter {name:?}",
                    input.identity
                )));
            }
        }
    }
    ensure!(
        ids.len() == 3,
        "{}: missing Linear Wipe parameters",
        effect.identity
    );
    Ok(PrLinearWipe {
        initial_completion: initial_completion
            .ok_or_else(|| unsupported("missing initial Transition Completion"))?,
        completion: completion.ok_or_else(|| unsupported("missing Transition Completion"))?,
        angle_degrees: angle.ok_or_else(|| unsupported("missing Wipe Angle"))?,
        feather: feather.ok_or_else(|| unsupported("missing Feather"))?,
    })
}

/// The ordered component references of a video component chain.
pub(super) fn chain_components(chain: &Located<VideoComponentChain>) -> Result<&[Reference]> {
    let content = chain
        .value
        .component_chain
        .as_ref()
        .ok_or_else(|| unsupported(format!("{}: missing ComponentChain", chain.identity)))?;
    Ok(content
        .components
        .as_ref()
        .map_or(&[], |components| components.items.as_slice()))
}

/// The intrinsic Motion and the masks of one chain, as
/// [`read_video_animations`] reads them.
pub(super) struct MotionAndMasks {
    pub(super) transform: PrStaticTransform,
    pub(super) animations: Vec<PrPropertyAnimation>,
    pub(super) crop: PrStaticCrop,
    pub(super) linear_wipe: Option<PrLinearWipe>,
    pub(super) track_matte: Option<TrackMatteKey>,
}

/// Read Motion, Crop, Linear Wipe and Track Matte Key; occurrence callers keep
/// other standard effects separate.
pub(super) fn read_video_animations(
    graph: &Graph<'_>,
    chain: &Located<VideoComponentChain>,
    components: &[&Reference],
    allow_motion: bool,
) -> Result<MotionAndMasks> {
    ensure!(
        allow_motion || components.is_empty(),
        "{}: sequence-level video components are unsupported",
        chain.identity
    );
    ensure!(
        components.len() <= 5,
        "{}: only Motion, Opacity, Crop, one Linear Wipe and one Track Matte Key component are supported",
        chain.identity
    );
    let mut motion_reference = None;
    let mut crop = None;
    let mut linear_wipe = None;
    let mut track_matte = None;
    for component in components {
        let record = graph.follow::<VideoFilterComponent>(component, &chain.identity)?;
        // Only the Opacity reader converts a mask; one on Motion, Crop or
        // Linear Wipe is unmeasured.
        ensure!(
            record.value.sub_components.is_none()
                || record.value.match_name.as_deref() == Some("AE.ADBE Opacity"),
            "{}: a mask on {} is not converted",
            record.identity,
            record
                .value
                .component
                .as_ref()
                .and_then(|body| body.display_name.as_deref())
                .or(record.value.match_name.as_deref())
                .unwrap_or("this component")
        );
        match record.value.match_name.as_deref() {
            Some("AE.ADBE Motion") => {
                ensure!(
                    motion_reference.replace(component).is_none(),
                    "{}: duplicate intrinsic Motion component",
                    chain.identity
                );
            }
            Some("AE.ADBE AECrop") => {
                ensure!(
                    crop.is_none(),
                    "{}: duplicate Crop component",
                    chain.identity
                );
                crop = Some(read_crop_component(graph, component, &chain.identity)?);
            }
            Some("AE.ADBE Linear Wipe") => {
                ensure!(
                    linear_wipe.is_none(),
                    "{}: duplicate Linear Wipe component",
                    chain.identity
                );
                linear_wipe = Some(read_linear_wipe(graph, component, &chain.identity)?);
            }
            Some(name) if name == TRACK_MATTE_KEY.match_name => {
                ensure!(
                    track_matte.is_none(),
                    "{}: duplicate Track Matte Key component",
                    chain.identity
                );
                track_matte = Some(read_track_matte(graph, component, &chain.identity)?);
            }
            Some("AE.ADBE Opacity") => {}
            _ => {
                return Err(unsupported(format!(
                    "{}: unsupported video component {:?}",
                    record.identity, record.value.match_name
                )))
            }
        }
    }
    let Some(motion_reference) = motion_reference else {
        ensure!(
            chain
                .value
                .default_motion
                .as_deref()
                .is_none_or(|value| value == "true"),
            "{}: nondefault DefaultMotion",
            chain.identity
        );
        return Ok(MotionAndMasks {
            transform: PrStaticTransform::default(),
            animations: Vec::new(),
            crop: crop.unwrap_or_default(),
            linear_wipe,
            track_matte,
        });
    };
    ensure!(
        chain.value.default_motion.as_deref() != Some("true"),
        "{}: explicit Motion conflicts with DefaultMotion",
        chain.identity
    );
    let motion = graph.follow::<VideoFilterComponent>(motion_reference, &chain.identity)?;
    let body = motion
        .value
        .component
        .ok_or_else(|| unsupported(format!("{}: missing Component", motion.identity)))?;
    ensure!(
        motion.value.match_name.as_deref() == Some("AE.ADBE Motion")
            && body.display_name.as_deref() == Some("Motion")
            && matches!(body.bypass.as_deref(), None | Some("false"))
            && body.intrinsic.as_deref() == Some("true"),
        "{}: unsupported video component",
        motion.identity
    );
    // Premiere 26.3 writes `Bypass` false; Premiere 26.5 omits it and adds Motion Crop.
    let premiere_26_5 = body.bypass.is_none();
    let layout = if premiere_26_5 {
        &MOTION_PARAMS_26_5[..]
    } else {
        &MOTION_PARAMS[..]
    };
    let params = body
        .params
        .ok_or_else(|| unsupported(format!("{}: missing Motion Params", motion.identity)))?;
    ensure!(
        params.items.len() == layout.len(),
        "{}: unsupported Motion parameter layout",
        motion.identity
    );
    let mut ids = BTreeSet::new();
    let mut transform = PrStaticTransform::default();
    let mut uniform_scale = true;
    let mut animations = Vec::new();
    for param in &params.items {
        let record = graph.locate(param, &motion.identity)?;
        ensure!(
            matches!(record.tag(), "VideoComponentParam" | "PointComponentParam"),
            "{}: unsupported Motion parameter type",
            record.identity()
        );
        let input = graph.decode::<VideoComponentParam>(record)?;
        let name = super::required(input.value.name.as_deref(), &input.identity, "Name")?;
        let id = input.value.parameter_id.as_str();
        ensure!(
            ids.insert(id.to_owned()),
            "{}: duplicate Motion ParameterID",
            input.identity
        );
        let spec = id
            .parse::<usize>()
            .ok()
            .and_then(|id| id.checked_sub(1))
            .and_then(|index| layout.get(index))
            .filter(|spec| spec.id.to_string() == id)
            .ok_or_else(|| {
                unsupported(format!("{}: unknown Motion parameter {id}", input.identity))
            })?;
        ensure!(
            (name == spec.name || (spec.id == 2 && name == "Scale Height"))
                && record.tag() == spec.record.tag,
            "{}: unexpected Motion parameter {name:?} or record type",
            input.identity
        );
        ensure!(
            input.value.bypass.is_none(),
            "{}: unsupported Motion parameter Bypass",
            input.identity
        );
        // Saves in the 26.3 layout vary these fields (Premiere 9-14 write a Scale
        // UpperUIBound of 100 or none), so only the 26.5 layout checks them.
        if premiere_26_5 {
            let bounds = spec
                .bounds
                .map_or((None, None, None), |(lower, upper, upper_ui)| {
                    (Some(lower), Some(upper), upper_ui)
                });
            ensure!(
                input.value.class_id.as_deref() == Some(spec.record.class_id)
                    && input.value.parameter_control_type.as_deref() == spec.control
                    && (
                        input.value.lower_bound.as_deref(),
                        input.value.upper_bound.as_deref(),
                        input.value.upper_ui_bound.as_deref(),
                    ) == bounds
                    && input.value.lower_ui_bound.is_none(),
                "{}: unexpected Motion parameter layout",
                input.identity
            );
        }
        let initial = input.value.start_keyframe.as_str();
        let wire = input.value.keyframes.as_deref().unwrap_or("");
        let is_time_varying = input.value.is_time_varying.as_deref();
        ensure!(
            matches!(is_time_varying, None | Some("true") | Some("false")),
            "{}: invalid IsTimeVarying",
            input.identity
        );
        if let Some(property) = spec.animation {
            let animation = match property {
                PrAnimatedProperty::Opacity => {
                    return Err(unsupported("Opacity cannot appear in Motion parameters"));
                }
                PrAnimatedProperty::Position => {
                    transform.position = point_start(initial, &input.identity)?;
                    let keys = point_keys(wire, &input.identity)?;
                    (!keys.is_empty()).then_some(PrPropertyAnimation::Position(keys))
                }
                PrAnimatedProperty::AnchorPoint => {
                    transform.anchor_point = point_start(initial, &input.identity)?;
                    let keys = point_keys(wire, &input.identity)?;
                    (!keys.is_empty()).then_some(PrPropertyAnimation::AnchorPoint(keys))
                }
                PrAnimatedProperty::Rotation => {
                    transform.rotation = scalar_start(initial, &input.identity)?;
                    let keys = scalar_keys(wire, &input.identity)?;
                    (!keys.is_empty()).then_some(PrPropertyAnimation::Rotation(keys))
                }
                PrAnimatedProperty::UniformScale => {
                    transform.scale[1] = scalar_start(initial, &input.identity)?;
                    let keys = scalar_keys(wire, &input.identity)?;
                    (!keys.is_empty()).then_some(PrPropertyAnimation::UniformScale(keys))
                }
                PrAnimatedProperty::ScaleWidth => {
                    transform.scale[0] = scalar_start(initial, &input.identity)?;
                    let keys = scalar_keys(wire, &input.identity)?;
                    (!keys.is_empty()).then_some(PrPropertyAnimation::ScaleWidth(keys))
                }
            };
            // Without keys the parameter keeps its StartKeyframe, whatever
            // `IsTimeVarying` says. Premiere 26.5.1 saves Opacity so and
            // renders that value (`read_video_compositing`).
            if let Some(animation) = animation {
                ensure!(
                    is_time_varying != Some("false"),
                    "{}: keyframes conflict with disabled IsTimeVarying",
                    input.identity
                );
                if let Some(reason) = animation.unmeasured_form() {
                    return Err(unsupported(format!("{}: {reason}", input.identity)));
                }
                animations.push(animation);
            }
            continue;
        }
        if spec.is_crop() {
            // Motion Crop has no mapping; only its zero default converts.
            let value = scalar_start(initial, &input.identity)?;
            ensure!(
                value == 0.0 && wire.is_empty() && is_time_varying != Some("true"),
                "{}: nonzero or keyed Motion {name} is unsupported",
                input.identity
            );
            continue;
        }
        ensure!(
            wire.is_empty() && input.value.is_time_varying.as_deref() != Some("true"),
            "{}: animated {name} is unsupported",
            input.identity
        );
        if spec.id == 4 {
            uniform_scale = bool_start(initial, &input.identity)?;
        } else {
            default_param(spec, initial, &input.identity)?;
        }
    }
    ensure!(
        ids.len() == layout.len(),
        "{}: missing Motion parameters",
        motion.identity
    );
    let keyed = |property| {
        animations
            .iter()
            .any(|animation| animation.property() == property)
    };
    if uniform_scale {
        // Premiere scales both axes by Scale and leaves Scale Width unchanged
        // while Uniform Scale is on (AME render of clip S2 of
        // `premiere_isolated_motion_opacity_26_5`). Whether it also ignores
        // Scale Width keys there is unmeasured.
        ensure!(
            !keyed(PrAnimatedProperty::ScaleWidth),
            "{}: animated Scale Width is unsupported under Uniform Scale",
            motion.identity
        );
        transform.scale[0] = transform.scale[1];
    } else {
        // Without Uniform Scale, Scale is the height; Scale Width keys beside
        // a static Scale Height are measured, keyed Scale Height is not.
        ensure!(
            !keyed(PrAnimatedProperty::UniformScale),
            "{}: animated Scale Height without Uniform Scale is unsupported",
            motion.identity
        );
    }
    Ok(MotionAndMasks {
        transform,
        animations,
        crop: crop.unwrap_or_default(),
        linear_wipe,
        track_matte,
    })
}

/// Read the intrinsic Opacity: its value, blend mode, Opacity keys and its
/// one static mask (`SubComponents`; `super::mask`), whose bypassed form is
/// reported in `omissions` and converts no mask.
pub(super) fn read_video_compositing(
    graph: &Graph<'_>,
    chain: &Located<VideoComponentChain>,
    components: &[&Reference],
    omissions: &mut Vec<Omission>,
) -> Result<(
    f64,
    PrBlendMode,
    Option<PrPropertyAnimation>,
    Option<PrMask>,
)> {
    let mut opacity_components = Vec::new();
    for reference in components {
        let component = graph.follow::<VideoFilterComponent>(reference, &chain.identity)?;
        match component.value.match_name.as_deref() {
            Some("AE.ADBE Opacity") => opacity_components.push(component),
            Some("AE.ADBE Motion" | "AE.ADBE AECrop" | "AE.ADBE Linear Wipe") => {}
            Some(name) if name == TRACK_MATTE_KEY.match_name => {}
            _ => {
                return Err(unsupported(format!(
                    "{}: unsupported video component",
                    component.identity
                )))
            }
        }
    }
    ensure!(
        opacity_components.len() <= 1,
        "{}: duplicate intrinsic Opacity",
        chain.identity
    );
    let Some(component) = opacity_components.into_iter().next() else {
        ensure!(
            chain
                .value
                .default_opacity
                .as_deref()
                .is_none_or(|value| value == "true"),
            "{}: nondefault opacity",
            chain.identity
        );
        return Ok((100.0, PrBlendMode::Normal, None, None));
    };
    ensure!(
        chain.value.default_opacity.as_deref() != Some("true"),
        "{}: explicit Opacity conflicts with DefaultOpacity",
        chain.identity
    );
    let mask = match &component.value.sub_components {
        Some(sub_components) => {
            super::mask::read_opacity_mask(graph, &component.identity, sub_components, omissions)?
        }
        None => None,
    };
    let body = component
        .value
        .component
        .ok_or_else(|| unsupported(format!("{}: missing Component", component.identity)))?;
    ensure!(
        component.value.match_name.as_deref() == Some("AE.ADBE Opacity")
            && body.display_name.as_deref() == Some("Opacity")
            && matches!(body.bypass.as_deref(), None | Some("false"))
            && body.intrinsic.as_deref() == Some("true"),
        "{}: unsupported Opacity component",
        component.identity
    );
    // Premiere 26.3 writes `Bypass` false; Premiere 26.5 omits it and raises the
    // primary Blend Mode's upper bound. Both write the same blend pairs.
    let layout = if body.bypass.is_none() {
        &OPACITY_PARAMS_26_5
    } else {
        &OPACITY_PARAMS
    };
    let params = body
        .params
        .ok_or_else(|| unsupported(format!("{}: missing Opacity Params", component.identity)))?;
    ensure!(
        params.items.len() == layout.len(),
        "{}: unsupported Opacity parameter layout",
        component.identity
    );
    let mut parameter_ids = BTreeSet::new();
    let mut opacity = None;
    let mut opacity_animation = None;
    let mut blend = [None, None];
    for reference in &params.items {
        let record = graph.locate(reference, &component.identity)?;
        ensure!(
            record.tag() == "VideoComponentParam",
            "{}: unsupported Opacity parameter type",
            record.identity()
        );
        let input = graph.decode::<VideoComponentParam>(record)?;
        let id =
            input.value.parameter_id.parse::<usize>().map_err(|_| {
                unsupported(format!("{}: invalid Opacity ParameterID", input.identity))
            })?;
        ensure!(
            parameter_ids.insert(id),
            "{}: duplicate Opacity ParameterID {id}",
            input.identity
        );
        let spec = id
            .checked_sub(1)
            .and_then(|index| layout.get(index))
            .filter(|spec| spec.id == id)
            .ok_or_else(|| {
                unsupported(format!(
                    "{}: unknown Opacity parameter {id}",
                    input.identity
                ))
            })?;
        ensure!(
            super::required(input.value.name.as_deref(), &input.identity, "Name")? == spec.name
                && input.value.class_id.as_deref() == Some(spec.class_id)
                && input.value.parameter_control_type.as_deref() == spec.control
                && input.value.lower_bound.as_deref() == Some(spec.lower_bound)
                && spec.accepts_upper_bound(input.value.upper_bound.as_deref()),
            "{}: unexpected Opacity parameter layout",
            input.identity
        );
        ensure!(
            input.value.bypass.is_none(),
            "{}: unsupported Opacity parameter Bypass",
            input.identity
        );
        let is_time_varying = input.value.is_time_varying.as_deref();
        ensure!(
            matches!(is_time_varying, None | Some("true") | Some("false")),
            "{}: invalid IsTimeVarying",
            input.identity
        );
        let value = scalar_start(&input.value.start_keyframe, &input.identity)?;
        if spec.animation == Some(PrAnimatedProperty::Opacity) {
            ensure!(
                (0.0..=100.0).contains(&value),
                "{}: opacity out of range",
                input.identity
            );
            opacity = Some(value);
            let keys = scalar_keys(
                input.value.keyframes.as_deref().unwrap_or(""),
                &input.identity,
            )?;
            // Premiere 26.5.1 saves the explicit Opacity of a masked clip with
            // `IsTimeVarying` true and no `Keyframes` (fixture
            // `feature_opacity_masks_26_5_strict`, clips A to D, saved from an
            // XML-authored `false`; `oracle/17/facts.md`,
            // `fragments/A-saved.xml`); the native render shows the
            // `StartKeyframe` value (A at alpha 0.500 = Mask Opacity 50 times
            // Opacity 100, B and C opaque inside the mask), so that state reads
            // as the static value, as it does for Motion's animated parameters.
            if !keys.is_empty() {
                ensure!(
                    is_time_varying != Some("false"),
                    "{}: Opacity keyframes conflict with disabled IsTimeVarying",
                    input.identity
                );
                opacity_animation = Some(PrPropertyAnimation::Opacity(keys));
            }
        } else {
            ensure!(
                input.value.keyframes.as_deref().is_none_or(str::is_empty)
                    && is_time_varying != Some("true"),
                "{}: animated blend mode unsupported",
                input.identity
            );
            ensure!(
                value.fract() == 0.0 && (0.0..=f64::from(u8::MAX)).contains(&value),
                "{}: invalid blend mode",
                input.identity
            );
            blend[id - 2] = Some(value as u8);
        }
    }
    ensure!(
        parameter_ids.len() == layout.len(),
        "{}: missing Opacity parameters",
        component.identity
    );
    let primary = blend[0].ok_or_else(|| unsupported("missing primary Blend Mode"))?;
    let legacy = blend[1].ok_or_else(|| unsupported("missing legacy Blend Mode"))?;
    Ok((
        opacity.ok_or_else(|| unsupported("missing Opacity"))?,
        PrBlendMode::from_native_values(primary, legacy),
        opacity_animation,
        mask,
    ))
}

#[cfg(test)]
mod cubic_easing_tests {
    use super::*;

    fn equal_value_keys() -> (NativeScalarKeyframe, NativeScalarKeyframe) {
        let start = NativeScalarKeyframe {
            source_ticks: 0,
            value: 42.0,
            outgoing_mode: 5,
            incoming_speed: 0.0,
            incoming_influence: 0.25,
            outgoing_speed: 0.0,
            outgoing_influence: 0.25,
            incoming_handle_ignored: false,
        };
        let end = NativeScalarKeyframe {
            source_ticks: TICKS,
            outgoing_mode: 0,
            ..start
        };
        (start, end)
    }

    #[test]
    fn equal_value_zero_effective_rises_are_linear() {
        let (start, end) = equal_value_keys();
        for (start, end) in [
            (start, end),
            (
                NativeScalarKeyframe {
                    outgoing_speed: 1.0,
                    outgoing_influence: 0.0,
                    ..start
                },
                end,
            ),
            (
                start,
                NativeScalarKeyframe {
                    incoming_speed: 1.0,
                    incoming_influence: 0.0,
                    ..end
                },
            ),
        ] {
            assert!(matches!(
                cubic_easing(start, end, "test").unwrap(),
                PrKeyframeEasing::Linear
            ));
        }
    }

    #[test]
    fn equal_value_nonzero_effective_rise_is_rejected() {
        let (start, end) = equal_value_keys();
        assert!(cubic_easing(
            NativeScalarKeyframe {
                outgoing_speed: 1.0,
                ..start
            },
            end,
            "test"
        )
        .is_err());
        assert!(cubic_easing(
            start,
            NativeScalarKeyframe {
                incoming_speed: 1.0,
                ..end
            },
            "test"
        )
        .is_err());
    }

    #[test]
    fn a_key_whose_in_handle_is_ignored_arrives_with_a_zero_length_handle() {
        let (start, end) = equal_value_keys();
        // A key that starts a Hold, with a stored in-rise that would bend the
        // segment: between equal values it stays flat instead of failing...
        let end = NativeScalarKeyframe {
            outgoing_mode: 4,
            incoming_speed: 5.0,
            incoming_handle_ignored: true,
            ..end
        };
        assert_eq!(
            cubic_easing(start, end, "test").unwrap(),
            PrKeyframeEasing::Linear
        );
        // ...and between different values it arrives at (1, 1).
        let end = NativeScalarKeyframe { value: 84.0, ..end };
        assert_eq!(
            cubic_easing(start, end, "test").unwrap(),
            PrKeyframeEasing::CubicBezier {
                x1: 0.25,
                y1: 0.0,
                x2: 1.0,
                y2: 1.0,
            }
        );
        // The stored in-influence is still checked.
        let end = NativeScalarKeyframe {
            incoming_influence: 1.5,
            ..end
        };
        assert!(cubic_easing(start, end, "test").is_err());
    }

    #[test]
    fn equal_value_invalid_influence_is_rejected() {
        let (start, end) = equal_value_keys();
        assert!(cubic_easing(
            NativeScalarKeyframe {
                outgoing_influence: 1.1,
                ..start
            },
            end,
            "test"
        )
        .is_err());
        assert!(cubic_easing(
            start,
            NativeScalarKeyframe {
                incoming_influence: -0.1,
                ..end
            },
            "test"
        )
        .is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    use std::io::Read;

    // Adobe-authored explore_bezier_keyframes project, pinned from long-term Asset
    // WJ313gJR2j08vrgbBlsL_txt at SHA-256
    // 6d336d67efad9f857b7307e3f9dc5eb4860b5a2bf270ae6d48e3eacd7da8506f.
    const ADOBE_POSITION_PATH: &[u8] =
        include_bytes!("../../../tests/fixtures/feature_motion_position_path_adobe.prproj");

    /// Oracle run C6's F23 probe (Premiere 26.5.1, `f23_bezier_end_probe.prproj`):
    /// Motion Scale keys K1 at 1 s (100, Linear), K2 at 2 s (150, Bezier), K3 at
    /// 4 s (100, Hold) and K4 at 5 s (150, Bezier), verbatim as saved in P1
    /// (Premiere's handles) and P3 (K2 and K4 in 12.5/s at 0.6, K1 out 20/s at
    /// 0.4). Premiere read K1 to K2 back within 7.9e-6 of these readings
    /// (control/evidence/6a-JRB-2077-r8/f23/).
    const F23_P1: &str = "254016000000,100.,0,0,0,0.16666666666666666,50,0.16666666666666666;508032000000,150.,5,0,50,0.16666666666666666,0,0.16666666666666666;1016064000000,100.,4,0,-25,0.16666666666666666,0,0.33333333333333331;1270080000000,150.,5,0,50,0.16666666666666666,0,0.16666666666666666;";
    const F23_P3: &str = "254016000000,100.,0,0,0,0.16666666666666666,20,0.40000000000000002;508032000000,150.,5,0,12.5,0.59999999999999998,0,0.16666666666666666;1016064000000,100.,4,0,-25,0.16666666666666666,0,0.33333333333333331;1270080000000,150.,5,0,12.5,0.59999999999999998,0,0.16666666666666666;";

    #[test]
    fn linear_key_eases_into_a_bezier_key_unless_its_handles_lie_on_the_chord() {
        // P3's stored handles ease K1 into K2.
        let keys = scalar_keys(F23_P3, "P3").unwrap();
        assert!(
            matches!(keys[1].easing, PrKeyframeEasing::CubicBezier { x1, x2, .. }
                if x1 == 0.4 && x2 == 1.0 - 0.6),
            "{:?}",
            keys[1].easing
        );
        // P1's handles at the average speed, and the zero handles that export
        // writes around a Linear segment, lie on the chord.
        let exported = format!(
            "0,0.,0,0,0,0,0,0;{TICKS},90.,5,0,0,0,3,0.4;{},0.,0,0,2,0.2,0,0;",
            2 * TICKS
        );
        for (save, wire) in [("P1", F23_P1), ("export", exported.as_str())] {
            let keys = scalar_keys(wire, save).unwrap();
            assert_eq!(keys[1].easing, PrKeyframeEasing::Linear, "{save}");
        }
    }

    #[test]
    fn each_scalar_start_and_end_mode_pair_reads_its_easing() {
        use PrKeyframeEasing::{CubicBezier, Hold, Linear};
        // 0 to 64 over one second: the start key's out-handle (128/s at 0.25)
        // and the end key's in-handle (32/s at 0.5) normalize exactly.
        let both = CubicBezier {
            x1: 0.25,
            y1: 0.5,
            x2: 0.5,
            y2: 0.75,
        };
        for (start, end, expected) in [
            (0, 0, Linear),
            (0, 4, Linear),
            // F23: a Linear key eases into a Bezier key with both handles.
            (0, 5, both),
            (4, 0, Hold),
            (4, 4, Hold),
            (4, 5, Hold),
            (5, 0, both),
            // F26: a key that starts a Hold arrives with a zero-length handle.
            (
                5,
                4,
                CubicBezier {
                    x1: 0.25,
                    y1: 0.5,
                    x2: 1.0,
                    y2: 1.0,
                },
            ),
            (5, 5, both),
        ] {
            let wire = format!(
                "{TICKS},0,{start},0,0,0,128,0.25;{},64,{end},0,32,0.5,0,0;",
                2 * TICKS
            );
            let keys = scalar_keys(&wire, "test").unwrap();
            assert_eq!(
                keys[1].easing, expected,
                "start mode {start}, end mode {end}"
            );
        }
    }

    #[test]
    fn linear_point_key_stays_linear_before_a_bezier_key() {
        // F23 was probed on scalar keys only, so a Linear Position key before a
        // Bezier key keeps the reading from before F23: Linear, with its stored
        // handles neither used nor checked. K1 at 1 s is Linear with an
        // outgoing handle of 0.2/s; K2 at 2 s is Bezier with an incoming
        // handle of 0.125/s at 0.6; the spatial path is linear.
        let wire = |k1_x: f64, k2_x: f64, k1_out_influence: f64| {
            format!(
                "{TICKS},{k1_x}:0.5,0,0,0,0.16666666666666666,0.2,{k1_out_influence},0,0,0,0,0,0;\
                 {},{k2_x}:0.5,5,0,0.125,0.6,0,0.16666666666666666,0,0,0,0,0,0;",
                2 * TICKS
            )
        };
        let readings: Vec<_> = [
            // As a cubic Bézier these handles would ease the interval.
            ("off the chord", wire(0.0, 0.5, 0.4)),
            // The Bezier formula rejects a rise between equal path distances.
            ("at one point", wire(0.25, 0.25, 0.4)),
            // The Bezier formula rejects an influence above one.
            ("influence 1.5", wire(0.0, 0.5, 1.5)),
        ]
        .iter()
        .map(|(case, wire)| {
            let easing = point_keys(wire, case).map(|keys| keys[1].easing);
            (*case, easing.map_err(|error| error.to_string()))
        })
        .collect();
        assert_eq!(
            readings,
            ["off the chord", "at one point", "influence 1.5"]
                .map(|case| (case, Ok(PrKeyframeEasing::Linear)))
        );
        // Read as scalar keys (0 to 0.5, the path distance), the same handles
        // ease into the Bezier key (F23).
        let scalar = format!(
            "{TICKS},0,0,0,0,0.16666666666666666,0.2,0.4;{},0.5,5,0,0.125,0.6,0,0.16666666666666666;",
            2 * TICKS
        );
        assert!(matches!(
            scalar_keys(&scalar, "scalar").unwrap()[1].easing,
            PrKeyframeEasing::CubicBezier { .. }
        ));
    }

    #[test]
    fn closed_spatial_curve_retains_nonzero_temporal_distance() {
        let wire = "0,0:0,5,0,0,0,1,0.4,5,0,0,0,1,0;254016000000,0:0,0,0,1,0.2,0,0,5,0,0,1,0,0;";
        let keys = point_keys(wire, "closed curve").unwrap();
        assert_eq!(keys[0].value, keys[1].value);
        assert!(matches!(
            keys[1].easing,
            PrKeyframeEasing::CubicBezier { .. }
        ));
    }

    #[test]
    fn point_reader_rejects_nonfinite_spatial_curve_length() {
        let wire = "0,1.7976931348623157e308:0,5,0,0,0,1,0.4,0,0,0,0,0,0;254016000000,-1.7976931348623157e308:0,0,0,1,0.2,0,0,0,0,0,0,0,0;";
        let error = point_keys(wire, "overflowing curve").unwrap_err();
        assert!(
            error.to_string().contains("nonfinite spatial curve length"),
            "{error}"
        );
    }

    #[test]
    fn adobe_authored_automatic_position_path_imports_resolved_tangents() {
        let mut xml = String::new();
        GzDecoder::new(ADOBE_POSITION_PATH)
            .read_to_string(&mut xml)
            .unwrap();
        let graph = Graph::parse(&xml).unwrap();
        let reference = Reference {
            id: Some("113".to_owned()),
            uid: None,
            index: None,
        };
        let record = graph.locate(&reference, "Adobe Position fixture").unwrap();
        let input = graph.decode::<VideoComponentParam>(record).unwrap();
        assert_eq!(input.value.name.as_deref(), Some("Position"));
        let keys = point_keys(input.value.keyframes.as_deref().unwrap(), &input.identity).unwrap();
        assert_eq!(
            keys.iter().map(|key| key.source_ticks).collect::<Vec<_>>(),
            [914_449_132_800_000, 915_380_524_800_000]
        );
        assert_eq!(
            keys.iter().map(|key| key.value).collect::<Vec<_>>(),
            [
                [-0.028645824640989304, 0.7314815521240234],
                [0.36979167461395257, 0.7314815097384987],
            ]
        );
        assert_eq!(keys[0].easing, PrKeyframeEasing::Linear);
        assert_eq!(keys[1].easing, PrKeyframeEasing::Linear);
        assert_eq!(keys[0].spatial_in_tangent, Some([0.0, 0.0]));
        assert_eq!(
            keys[0].spatial_out_tangent,
            Some([0.06640624987582365, -7.064254126110115e-9])
        );
        assert_eq!(
            keys[1].spatial_in_tangent,
            Some([-0.06640624987582365, 7.064254126110115e-9])
        );
        assert_eq!(keys[1].spatial_out_tangent, Some([0.0, 0.0]));
    }
}
