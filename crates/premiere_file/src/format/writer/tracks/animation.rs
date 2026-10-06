//! Clip-local intrinsic Motion records for editable keyframe export.

use super::{point_start_keyframe, scalar_start_keyframe, video_track_id};
use crate::format::{
    invalid,
    writer::graph::{CropIds, LinearWipeIds, MotionIds, OpacityIds, TrackMatteIds},
    Result,
};
use crate::schema::{
    native::{
        MotionBody, MotionParams, MotionPrivateData, PointComponentParam, Record, SubComponents,
        VideoComponentParam, VideoFilterComponent,
    },
    records, PrBlendMode, PrKeyframeEasing, PrLinearWipe, PrPointKeyframe, PrPropertyAnimation,
    PrScalarKeyframe, PrStaticCrop, PrStaticTransform, PrTrackMatte, CROP_PARAMS, MOTION_PARAMS,
    OPACITY_PARAMS, TRACK_MATTE_KEY,
};

fn native_handles(
    start: &crate::schema::PrScalarKeyframe,
    end: &crate::schema::PrScalarKeyframe,
) -> Result<(f64, f64, f64, f64)> {
    let PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = end.easing else {
        return Ok((0.0, 0.0, 0.0, 0.0));
    };
    let duration_secs = (i128::from(end.source_ticks) - i128::from(start.source_ticks)) as f64
        / crate::schema::TICKS as f64;
    let average_velocity = (end.value - start.value) / duration_secs;
    let outgoing_speed = if x1 == 0.0 {
        0.0
    } else {
        average_velocity * y1 / x1
    };
    let incoming_influence = 1.0 - x2;
    let incoming_speed = if incoming_influence == 0.0 {
        0.0
    } else {
        average_velocity * (1.0 - y2) / incoming_influence
    };
    ensure_valid!(
        [average_velocity, outgoing_speed, incoming_speed]
            .into_iter()
            .all(f64::is_finite),
        "nonfinite native Bezier velocity"
    );
    Ok((incoming_speed, incoming_influence, outgoing_speed, x1))
}

// The serializer uses interpolation and outgoing handles on the start key, while
// FX stores the complete normalized timing curve on the key at the interval end.
pub(super) fn scalar_keyframes(keys: &[PrScalarKeyframe]) -> Result<String> {
    let mut wire = String::new();
    for (index, key) in keys.iter().enumerate() {
        let incoming = if let Some(previous) = index.checked_sub(1) {
            let (speed, influence, _, _) = native_handles(&keys[previous], key)?;
            (speed, influence)
        } else {
            (0.0, 0.0)
        };
        let outgoing = if let Some(next) = keys.get(index + 1) {
            let (_, _, speed, influence) = native_handles(key, next)?;
            (speed, influence)
        } else {
            (0.0, 0.0)
        };
        let mode = keys
            .get(index + 1)
            .map_or(PrKeyframeEasing::Linear, |next| next.easing)
            .native_outgoing_mode();
        wire.push_str(&format!(
            "{},{},{mode},0,{},{},{},{};",
            key.source_ticks, key.value, incoming.0, incoming.1, outgoing.0, outgoing.1
        ));
    }
    Ok(wire)
}

fn point_handles(start: &PrPointKeyframe, end: &PrPointKeyframe) -> Result<(f64, f64, f64, f64)> {
    let PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = end.easing else {
        return Ok((0.0, 0.0, 0.0, 0.0));
    };
    let duration_secs = (i128::from(end.source_ticks) - i128::from(start.source_ticks)) as f64
        / crate::schema::TICKS as f64;
    let distance = crate::schema::spatial::segment_length(start, end)
        .ok_or_else(|| invalid("nonfinite spatial curve length"))?;
    ensure_valid!(
        distance != 0.0,
        "zero-length cubic spatial segment cannot preserve Premiere velocity"
    );
    let average_velocity = distance / duration_secs;
    let outgoing_speed = if x1 == 0.0 {
        0.0
    } else {
        average_velocity * y1 / x1
    };
    let incoming_influence = 1.0 - x2;
    let incoming_speed = if incoming_influence == 0.0 {
        0.0
    } else {
        average_velocity * (1.0 - y2) / incoming_influence
    };
    ensure_valid!(
        [average_velocity, outgoing_speed, incoming_speed]
            .into_iter()
            .all(f64::is_finite),
        "nonfinite native Bezier velocity"
    );
    Ok((incoming_speed, incoming_influence, outgoing_speed, x1))
}

pub(super) fn point_keyframes(keys: &[PrPointKeyframe]) -> Result<String> {
    let mut wire = String::new();
    for (index, key) in keys.iter().enumerate() {
        let incoming = if let Some(previous) = index.checked_sub(1) {
            let (speed, influence, _, _) = point_handles(&keys[previous], key)?;
            (speed, influence)
        } else {
            (0.0, 0.0)
        };
        let outgoing = if let Some(next) = keys.get(index + 1) {
            let (_, _, speed, influence) = point_handles(key, next)?;
            (speed, influence)
        } else {
            (0.0, 0.0)
        };
        let temporal_mode = keys
            .get(index + 1)
            .map_or(PrKeyframeEasing::Linear, |next| next.easing)
            .native_outgoing_mode();
        let spatial_mode =
            u8::from(key.spatial_in_tangent.is_some() || key.spatial_out_tangent.is_some()) * 5;
        let spatial_in = key.spatial_in_tangent.unwrap_or([0.0, 0.0]);
        let spatial_out = key.spatial_out_tangent.unwrap_or([0.0, 0.0]);
        wire.push_str(&format!(
            "{},{}:{},{temporal_mode},0,{},{},{},{},{spatial_mode},0,{},{},{},{};",
            key.source_ticks,
            key.value[0],
            key.value[1],
            incoming.0,
            incoming.1,
            outgoing.0,
            outgoing.1,
            spatial_in[0],
            spatial_in[1],
            spatial_out[0],
            spatial_out[1]
        ));
    }
    Ok(wire)
}

pub(super) fn records(
    transform: PrStaticTransform,
    animations: &[PrPropertyAnimation],
    ids: &MotionIds,
) -> Result<Vec<Record>> {
    // Uniform Scale is on when both axes have one Scale at every time: equal
    // static axes that no Scale Width key moves apart.
    let uniform = transform.scale[0] == transform.scale[1]
        && !animations
            .iter()
            .any(|animation| matches!(animation, PrPropertyAnimation::ScaleWidth(_)));
    ensure_valid!(
        uniform
            || !animations
                .iter()
                .any(|animation| { matches!(animation, PrPropertyAnimation::UniformScale(_)) }),
        "animated nonuniform Scale cannot be written as a single native Scale track"
    );
    let filter = Record::VideoFilterComponent(VideoFilterComponent {
        object_id: ids.component,
        class_id: Some(records::VIDEO_FILTER_COMPONENT.class_id.to_owned()),
        version: Some(records::VIDEO_FILTER_COMPONENT.version.to_owned()),
        component: Some(MotionBody {
            version: Some(records::VIDEO_FILTER_COMPONENT_BODY_VERSION.to_owned()),
            params: Some(MotionParams::from_ids(ids.params)),
            id: Some("1".to_owned()),
            display_name: Some("Motion".to_owned()),
            instance_name: None,
            bypass: Some("false".to_owned()),
            intrinsic: Some("true".to_owned()),
        }),
        premiere_filter_private_data: Some(
            MotionPrivateData {
                encoding: "base64",
                binary_hash: "3805854c-243a-c4be-bd40-a8a40000000e".to_owned(),
                value: "AQA=".to_owned(),
            }
            .into(),
        ),
        sub_components: None,
        match_name: Some("AE.ADBE Motion".to_owned()),
        video_filter_type: Some("2".to_owned()),
    });
    let mut output = vec![filter];
    for (&object_id, spec) in ids.params.iter().zip(&MOTION_PARAMS) {
        let static_value = match spec.id {
            1 => format!("{}:{}", transform.position[0], transform.position[1]),
            2 => transform.scale[1].to_string(),
            3 => transform.scale[0].to_string(),
            4 => uniform.to_string(),
            5 => transform.rotation.to_string(),
            6 => format!(
                "{}:{}",
                transform.anchor_point[0], transform.anchor_point[1]
            ),
            _ => spec.initial.to_owned(),
        };
        let record = if spec.is_point() {
            let animation = spec.animation.and_then(|property| {
                animations
                    .iter()
                    .find(|animation| animation.property() == property)
                    .and_then(PrPropertyAnimation::point_keys)
            });
            Record::PointComponentParam(PointComponentParam {
                object_id,
                class_id: spec.record.class_id,
                version: spec.record.version,
                name: spec.name,
                is_time_varying: animation.is_none().then_some("false"),
                parameter_control_type: spec.control,
                start_keyframe: point_start_keyframe(&static_value),
                keyframes: animation.map(point_keyframes).transpose()?,
                parameter_id: spec.id,
            })
        } else {
            let (lower_bound, upper_bound, upper_ui_bound) =
                spec.bounds.map_or((None, None, None), |(lo, hi, ui)| {
                    (
                        Some(lo.to_owned()),
                        Some(hi.to_owned()),
                        ui.map(str::to_owned),
                    )
                });
            let animation = spec.animation.and_then(|property| {
                animations
                    .iter()
                    .find(|animation| animation.property() == property)
            });
            Record::VideoComponentParam(VideoComponentParam {
                object_id,
                class_id: Some(spec.record.class_id.to_owned()),
                version: Some(spec.record.version.to_owned()),
                name: Some(if spec.id == 2 && !uniform {
                    "Scale Height".to_owned()
                } else {
                    spec.name.to_owned()
                }),
                is_time_varying: animation.is_none().then_some("false".to_owned()),
                discontinuous_interpolate: None,
                parameter_control_type: spec.control.map(str::to_owned),
                start_keyframe: scalar_start_keyframe(&static_value),
                current_value: None,
                keyframes: animation
                    .and_then(PrPropertyAnimation::scalar_keys)
                    .map(scalar_keyframes)
                    .transpose()?,
                lower_bound,
                upper_bound,
                parameter_id: spec.id.to_string(),
                lower_ui_bound: None,
                upper_ui_bound,
                bypass: None,
            })
        };
        output.push(record);
    }
    Ok(output)
}

/// The intrinsic Opacity component, for a video clip and for a graphic clip
/// alike: its value, blend pair and any Opacity keys among `animations`, and
/// the `SubComponents` reference to its mask when `ids` allocate one.
pub(super) fn opacity_records(
    opacity: f64,
    blend_mode: PrBlendMode,
    animations: &[PrPropertyAnimation],
    ids: &OpacityIds,
) -> Result<Vec<Record>> {
    let filter = Record::VideoFilterComponent(VideoFilterComponent {
        object_id: ids.component,
        class_id: Some(records::VIDEO_FILTER_COMPONENT.class_id.to_owned()),
        version: Some(records::VIDEO_FILTER_COMPONENT.version.to_owned()),
        component: Some(MotionBody {
            version: Some("5".to_owned()),
            params: Some(MotionParams::from_ids(ids.params)),
            id: Some("2".to_owned()),
            display_name: Some("Opacity".to_owned()),
            instance_name: None,
            bypass: Some("false".to_owned()),
            intrinsic: Some("true".to_owned()),
        }),
        premiere_filter_private_data: None,
        sub_components: ids
            .mask
            .as_ref()
            .map(|mask| SubComponents::from_ids([mask.component])),
        match_name: Some("AE.ADBE Opacity".to_owned()),
        video_filter_type: Some("2".to_owned()),
    });
    let (primary, legacy) = blend_mode.native_values();
    let values = [opacity.to_string(), primary.to_string(), legacy.to_string()];
    let mut output = vec![filter];
    for ((&object_id, spec), value) in ids.params.iter().zip(&OPACITY_PARAMS).zip(values) {
        let animation = spec.animation.and_then(|property| {
            animations
                .iter()
                .find(|animation| animation.property() == property)
        });
        output.push(Record::VideoComponentParam(VideoComponentParam {
            object_id,
            class_id: Some(spec.class_id.to_owned()),
            version: Some("9".to_owned()),
            name: Some(spec.name.to_owned()),
            is_time_varying: animation.is_none().then_some("false".to_owned()),
            discontinuous_interpolate: spec.animation.is_none().then_some("true".to_owned()),
            parameter_control_type: spec.control.map(str::to_owned),
            start_keyframe: scalar_start_keyframe(&value),
            keyframes: animation
                .and_then(PrPropertyAnimation::scalar_keys)
                .map(scalar_keyframes)
                .transpose()?,
            current_value: None,
            lower_bound: Some(spec.lower_bound.to_owned()),
            upper_bound: Some(spec.upper_bound.to_owned()),
            parameter_id: spec.id.to_string(),
            lower_ui_bound: None,
            upper_ui_bound: None,
            bypass: None,
        }));
    }
    Ok(output)
}

pub(super) fn crop_records(crop: PrStaticCrop, ids: &CropIds) -> Vec<Record> {
    let filter = Record::VideoFilterComponent(VideoFilterComponent {
        object_id: ids.component,
        class_id: Some(records::VIDEO_FILTER_COMPONENT.class_id.to_owned()),
        version: Some(records::VIDEO_FILTER_COMPONENT.version.to_owned()),
        component: Some(MotionBody {
            version: Some("5".to_owned()),
            params: Some(MotionParams::from_ids(ids.params)),
            id: Some("3".to_owned()),
            display_name: Some("Crop".to_owned()),
            instance_name: None,
            bypass: Some("false".to_owned()),
            intrinsic: Some("false".to_owned()),
        }),
        premiere_filter_private_data: None,
        sub_components: None,
        match_name: Some("AE.ADBE AECrop".to_owned()),
        video_filter_type: Some("2".to_owned()),
    });
    let values = [
        crop.left,
        crop.top,
        crop.right,
        crop.bottom,
        0.0,
        crop.edge_feather,
    ];
    let mut output = vec![filter];
    for ((&object_id, spec), value) in ids.params.iter().zip(&CROP_PARAMS).zip(values) {
        let (wire, current_value) = if spec.id == 5 {
            ("false".to_owned(), None)
        } else {
            let wire = value.to_string();
            let current = (value != 0.0).then(|| wire.clone());
            (wire, current)
        };
        output.push(Record::VideoComponentParam(VideoComponentParam {
            object_id,
            class_id: Some(spec.class_id.to_owned()),
            version: Some(records::VIDEO_COMPONENT_PARAM.version.to_owned()),
            name: Some(spec.name.to_owned()),
            is_time_varying: Some("false".to_owned()),
            discontinuous_interpolate: None,
            parameter_control_type: spec.control.map(str::to_owned),
            start_keyframe: scalar_start_keyframe(&wire),
            current_value,
            keyframes: None,
            lower_bound: spec.lower.map(str::to_owned),
            upper_bound: spec.upper.map(str::to_owned),
            parameter_id: spec.id.to_string(),
            lower_ui_bound: spec.lower_ui.map(str::to_owned),
            upper_ui_bound: spec.upper_ui.map(str::to_owned),
            bypass: None,
        }));
    }
    output
}

pub(super) fn linear_wipe_records(wipe: &PrLinearWipe, ids: &LinearWipeIds) -> Result<Vec<Record>> {
    let initial = wipe.initial_completion;
    Ok(vec![
        Record::VideoFilterComponent(VideoFilterComponent {
            object_id: ids.component,
            class_id: Some(records::VIDEO_FILTER_COMPONENT.class_id.to_owned()),
            version: Some(records::VIDEO_FILTER_COMPONENT.version.to_owned()),
            component: Some(MotionBody {
                version: Some("5".to_owned()),
                params: Some(MotionParams::from_ids(ids.params)),
                id: Some("3".to_owned()),
                display_name: Some("Linear Wipe".to_owned()),
                instance_name: None,
                bypass: Some("false".to_owned()),
                intrinsic: Some("false".to_owned()),
            }),
            premiere_filter_private_data: None,
            sub_components: None,
            match_name: Some("AE.ADBE Linear Wipe".to_owned()),
            video_filter_type: Some("2".to_owned()),
        }),
        Record::VideoComponentParam(VideoComponentParam {
            object_id: ids.params[0],
            class_id: Some(records::VIDEO_COMPONENT_PARAM.class_id.to_owned()),
            version: Some(records::VIDEO_COMPONENT_PARAM.version.to_owned()),
            name: Some("Transition Completion".to_owned()),
            is_time_varying: None,
            discontinuous_interpolate: None,
            parameter_control_type: Some("2".to_owned()),
            start_keyframe: scalar_start_keyframe(initial),
            current_value: None,
            keyframes: if wipe.completion.is_empty() {
                None
            } else {
                Some(scalar_keyframes(&wipe.completion)?)
            },
            lower_bound: Some("0".to_owned()),
            upper_bound: Some("100".to_owned()),
            parameter_id: "1".to_owned(),
            lower_ui_bound: None,
            upper_ui_bound: None,
            bypass: None,
        }),
        Record::VideoComponentParam(VideoComponentParam {
            object_id: ids.params[1],
            class_id: Some(records::VIDEO_COMPONENT_PARAM.class_id.to_owned()),
            version: Some(records::VIDEO_COMPONENT_PARAM.version.to_owned()),
            name: Some("Wipe Angle".to_owned()),
            is_time_varying: Some("false".to_owned()),
            discontinuous_interpolate: None,
            parameter_control_type: Some("3".to_owned()),
            start_keyframe: scalar_start_keyframe(wipe.angle_degrees),
            current_value: None,
            keyframes: None,
            lower_bound: Some("-32768".to_owned()),
            upper_bound: Some("32767".to_owned()),
            parameter_id: "2".to_owned(),
            lower_ui_bound: None,
            upper_ui_bound: None,
            bypass: None,
        }),
        Record::VideoComponentParam(VideoComponentParam {
            object_id: ids.params[2],
            class_id: Some(records::VIDEO_COMPONENT_PARAM.class_id.to_owned()),
            version: Some(records::VIDEO_COMPONENT_PARAM.version.to_owned()),
            name: Some("Feather".to_owned()),
            is_time_varying: Some("false".to_owned()),
            discontinuous_interpolate: None,
            parameter_control_type: Some("2".to_owned()),
            start_keyframe: scalar_start_keyframe(wipe.feather),
            current_value: None,
            keyframes: None,
            lower_bound: Some("0".to_owned()),
            upper_bound: Some("32000".to_owned()),
            parameter_id: "3".to_owned(),
            lower_ui_bound: None,
            upper_ui_bound: Some("100".to_owned()),
            bypass: None,
        }),
    ])
}

/// One Track Matte Key in the corpus 12.x form (`VideoFilterComponent` 7,
/// `Component` 5, the generation of every component the writer writes), with
/// the three static parameters of [`TRACK_MATTE_KEY`] in `Params` order: the
/// Matte as the matte track's written ID, Composite Using and Reverse from
/// the channel. Premiere 26.5.1 opens this form and re-saves the same three
/// values in its own 9/7 record (fixture G6; the export gate measured it).
pub(super) fn track_matte_records(matte: PrTrackMatte, ids: &TrackMatteIds) -> Vec<Record> {
    let values = [
        video_track_id(matte.track_index).to_string(),
        matte.channel.composite_value().to_owned(),
        matte.channel.reverse_value().to_owned(),
    ];
    let mut output = vec![Record::VideoFilterComponent(VideoFilterComponent {
        object_id: ids.component,
        class_id: Some(records::VIDEO_FILTER_COMPONENT.class_id.to_owned()),
        version: Some(records::VIDEO_FILTER_COMPONENT.version.to_owned()),
        component: Some(MotionBody {
            version: Some(records::VIDEO_FILTER_COMPONENT_BODY_VERSION.to_owned()),
            params: Some(MotionParams::from_ids(ids.params)),
            id: Some("3".to_owned()),
            display_name: Some(TRACK_MATTE_KEY.display_name.to_owned()),
            instance_name: None,
            bypass: Some("false".to_owned()),
            intrinsic: Some("false".to_owned()),
        }),
        premiere_filter_private_data: None,
        sub_components: None,
        match_name: Some(TRACK_MATTE_KEY.match_name.to_owned()),
        video_filter_type: Some(TRACK_MATTE_KEY.filter_type.to_owned()),
    })];
    for ((&object_id, spec), value) in ids.params.iter().zip(TRACK_MATTE_KEY.params).zip(values) {
        output.push(Record::VideoComponentParam(VideoComponentParam {
            object_id,
            class_id: Some(spec.record.class_id.to_owned()),
            version: Some(spec.record.version.to_owned()),
            name: Some(spec.name.to_owned()),
            is_time_varying: Some("false".to_owned()),
            discontinuous_interpolate: spec.discontinuous_interpolate.then(|| "true".to_owned()),
            parameter_control_type: Some(spec.control.to_owned()),
            start_keyframe: scalar_start_keyframe(&value),
            current_value: None,
            keyframes: None,
            lower_bound: Some(spec.lower_bound.to_owned()),
            upper_bound: Some(spec.upper_bound.to_owned()),
            parameter_id: spec.id.to_string(),
            lower_ui_bound: None,
            upper_ui_bound: None,
            bypass: None,
        }));
    }
    output
}
