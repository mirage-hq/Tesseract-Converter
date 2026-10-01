//! Standard clip effect records, written in stack order after intrinsic Motion.

use super::{
    animation::{point_keyframes, scalar_keyframes},
    point_start_keyframe, scalar_start_keyframe,
};
use crate::format::{
    writer::graph::{uuid, EffectIds},
    Result,
};
use crate::schema::{
    native::{
        MotionBody, MotionParams, MotionPrivateData, PointComponentParam, Record,
        RetainedOrSkipped, VideoComponentParam, VideoFilterComponent,
    },
    records, EffectParamSpec, EffectSpec, PrColourKeyframe, PrEffect, PrEffectParamKeys,
    PrEffectParams, PrKeyframeEasing, BLUR_DIMENSIONS_HORIZONTAL_AND_VERTICAL,
    FILM_IMPACT_BLUR_AMOUNT, FILM_IMPACT_BLUR_ANGLE, FILM_IMPACT_BLUR_DEFAULTS,
    FILM_IMPACT_BLUR_EDGE, FILM_IMPACT_DIRECTIONAL_BLUR_DEFAULTS, INVERT_CHANNEL_RGB,
    PREMIERE_NATIVE_FILTER_VERSIONS, PREMIERE_NATIVE_PARAMETER_ID, RAMP_SHAPE_LINEAR,
    TRANSFORM_SAMPLING_BICUBIC, TRANSFORM_SAMPLING_BILINEAR,
};
use base64::{engine::general_purpose::STANDARD, Engine};

/// Component `ID` of the first standard effect in a chain. Motion and Opacity
/// own IDs 1 and 2 (`DefaultMotionComponentID`, `DefaultOpacityComponentID`).
/// Corpus chains number their first standard effect 3 (140 chains) or 4 (76
/// chains); 4 is an inferred choice that no Premiere reopen has checked.
const FIRST_EFFECT_COMPONENT_ID: usize = 4;

/// One `VideoFilterComponent` and its parameter records per effect. A keyed
/// parameter is written like a keyed Motion parameter: its keys, no
/// `IsTimeVarying`, and its static value, the first key's, as `StartKeyframe`.
/// A keyed Premiere-native or Film Impact parameter is marked `IsTimeVarying`
/// `true`, as Premiere 26.5.1 saves a keyed Levels (Oracle run E4) and Amount.
pub(super) fn records(effects: &[PrEffect], ids: &[EffectIds]) -> Result<Vec<Record>> {
    let mut output = Vec::new();
    for (position, (effect, ids)) in effects.iter().zip(ids).enumerate() {
        let spec = effect.spec();
        let film_impact = matches!(
            effect.params,
            PrEffectParams::FilmImpactBlur(_) | PrEffectParams::FilmImpactDirectionalBlur(_)
        );
        // A Premiere-native filter is written as Premiere 26.5.1 saves Levels,
        // without flags: export omits a disabled Levels.
        let native = spec.premiere_native;
        let [version, body_version] = if native || film_impact {
            PREMIERE_NATIVE_FILTER_VERSIONS
        } else {
            [
                records::VIDEO_FILTER_COMPONENT.version,
                records::VIDEO_FILTER_COMPONENT_BODY_VERSION,
            ]
        };
        output.push(Record::VideoFilterComponent(VideoFilterComponent {
            object_id: ids.component,
            class_id: Some(records::VIDEO_FILTER_COMPONENT.class_id.to_owned()),
            version: Some(version.to_owned()),
            component: Some(MotionBody {
                version: Some(body_version.to_owned()),
                // Without parameters there is no `Params` element, as Premiere
                // 26.5.1 saves a Black & White (Oracle run E7).
                params: (!spec.params.is_empty())
                    .then(|| MotionParams::from_ids(ids.params.iter().copied())),
                id: Some((FIRST_EFFECT_COMPONENT_ID + position).to_string()),
                display_name: Some(spec.display_name.to_owned()),
                instance_name: None,
                bypass: if film_impact {
                    (!effect.enabled).then(|| "true".to_owned())
                } else {
                    (!native).then(|| (!effect.enabled).to_string())
                },
                intrinsic: (!native && !film_impact).then(|| "false".to_owned()),
            }),
            premiere_filter_private_data: levels_private_data(effect)?,
            sub_components: None,
            match_name: Some(spec.match_name.to_owned()),
            video_filter_type: Some(spec.filter_type.to_owned()),
        }));
        // Static values in the native `Params` order of `spec`.
        let values = match &effect.params {
            PrEffectParams::GaussianBlur(blur) => vec![
                native_number(blur.blurriness),
                BLUR_DIMENSIONS_HORIZONTAL_AND_VERTICAL.to_owned(),
                blur.repeat_edge_pixels.to_string(),
            ],
            PrEffectParams::FilmImpactBlur(blur) => {
                film_impact_values(spec, &FILM_IMPACT_BLUR_DEFAULTS, |param| {
                    if param.id == FILM_IMPACT_BLUR_AMOUNT.id {
                        Some(native_number(blur.amount))
                    } else if param.id == FILM_IMPACT_BLUR_EDGE.id {
                        Some(if blur.repeat_edge_pixels { "1" } else { "2" }.to_owned())
                    } else {
                        None
                    }
                })
            }
            PrEffectParams::FilmImpactDirectionalBlur(blur) => {
                film_impact_values(spec, &FILM_IMPACT_DIRECTIONAL_BLUR_DEFAULTS, |param| {
                    if param.id == FILM_IMPACT_BLUR_ANGLE.id {
                        Some(native_number(blur.angle))
                    } else if param.id == FILM_IMPACT_BLUR_AMOUNT.id {
                        Some(native_number(blur.amount))
                    } else if param.id == FILM_IMPACT_BLUR_EDGE.id {
                        Some("2".to_owned())
                    } else {
                        None
                    }
                })
            }
            PrEffectParams::CornerPin(pin) => pin
                .corners
                .iter()
                .map(|[x, y]| format!("{x}:{y}"))
                .collect(),
            PrEffectParams::DirectionalBlur(blur) => vec![
                native_number(blur.direction),
                native_number(blur.blur_length),
            ],
            PrEffectParams::Levels(levels) => {
                levels.start_values().iter().map(f64::to_string).collect()
            }
            PrEffectParams::BrightnessContrast(values) => vec![
                native_number(values.brightness),
                native_number(values.contrast),
            ],
            // Without Invert's opaque private data (`EffectSpec::opaque_private_data`).
            PrEffectParams::Invert(invert) => {
                vec![INVERT_CHANNEL_RGB.to_owned(), native_number(invert.blend)]
            }
            PrEffectParams::Tint(tint) => vec![
                tint.black.native().to_string(),
                tint.white.native().to_string(),
                native_number(tint.amount),
            ],
            PrEffectParams::BlackWhite => Vec::new(),
            // A linear ramp without scatter, the only form that converts.
            PrEffectParams::Ramp(ramp) => vec![
                format!("{}:{}", ramp.start[0], ramp.start[1]),
                ramp.start_colour.native().to_string(),
                format!("{}:{}", ramp.end[0], ramp.end[1]),
                ramp.end_colour.native().to_string(),
                RAMP_SHAPE_LINEAR.to_owned(),
                native_number(0.0),
                native_number(ramp.blend),
            ],
            // Whole counts, as Premiere writes them, and Sharp Colors on.
            PrEffectParams::Mosaic(mosaic) => vec![
                mosaic.horizontal.to_string(),
                mosaic.vertical.to_string(),
                mosaic.sharp_colors.to_string(),
            ],
            PrEffectParams::Transform(transform) => vec![
                format!(
                    "{}:{}",
                    transform.anchor_point[0], transform.anchor_point[1]
                ),
                format!("{}:{}", transform.position[0], transform.position[1]),
                transform.uniform_scale.to_string(),
                native_number(transform.scale_height),
                native_number(transform.scale_width),
                native_number(transform.skew),
                native_number(transform.skew_axis),
                native_number(transform.rotation),
                native_number(transform.opacity),
                transform.composition_shutter_angle.to_string(),
                native_number(transform.shutter_angle),
                if transform.bicubic_sampling {
                    TRANSFORM_SAMPLING_BICUBIC
                } else {
                    TRANSFORM_SAMPLING_BILINEAR
                }
                .to_owned(),
            ],
        };
        for ((&object_id, param), value) in ids.params.iter().zip(spec.params).zip(values) {
            let keys = effect.keys(param);
            if param.record.tag == records::POINT_COMPONENT_PARAM.tag {
                // Written as the Motion writer writes Position; Premiere 26.5.1
                // saves the same static `StartKeyframe`.
                output.push(Record::PointComponentParam(PointComponentParam {
                    object_id,
                    class_id: param.record.class_id,
                    version: param.record.version,
                    name: param.name,
                    is_time_varying: keys.is_none().then_some("false"),
                    parameter_control_type: Some(param.control),
                    start_keyframe: point_start_keyframe(&value),
                    keyframes: keys
                        .and_then(PrEffectParamKeys::point)
                        .map(point_keyframes)
                        .transpose()?,
                    parameter_id: param.id,
                }));
                continue;
            }
            output.push(Record::VideoComponentParam(VideoComponentParam {
                object_id,
                class_id: Some(param.record.class_id.to_owned()),
                version: Some(param.record.version.to_owned()),
                // A blank spec name is a checkbox saved without the element
                // (`EffectParamSpec::accepts_name`).
                name: (!param.name.is_empty()).then(|| param.name.to_owned()),
                is_time_varying: match keys {
                    None => (!film_impact).then(|| "false".to_owned()),
                    Some(_) => (native || film_impact).then(|| "true".to_owned()),
                },
                discontinuous_interpolate: param
                    .discontinuous_interpolate
                    .then(|| "true".to_owned()),
                parameter_control_type: (!param.control.is_empty())
                    .then(|| param.control.to_owned()),
                start_keyframe: scalar_start_keyframe(&value),
                current_value: None,
                keyframes: match keys {
                    Some(PrEffectParamKeys::Scalar(keys)) => Some(scalar_keyframes(keys)?),
                    Some(PrEffectParamKeys::Colour(keys)) => Some(colour_keyframes(keys)?),
                    Some(PrEffectParamKeys::Point(_)) | None => None,
                },
                lower_bound: (!param.lower_bound.is_empty()).then(|| param.lower_bound.to_owned()),
                upper_bound: (!param.upper_bound.is_empty()).then(|| param.upper_bound.to_owned()),
                parameter_id: if native {
                    PREMIERE_NATIVE_PARAMETER_ID.to_owned()
                } else {
                    param.id.to_string()
                },
                lower_ui_bound: param.lower_ui_bound.map(str::to_owned),
                upper_ui_bound: param.upper_ui_bound.map(str::to_owned),
                bypass: None,
            }));
        }
    }
    Ok(output)
}

/// Levels' private data: its 20 `StartKeyframe` values as little-endian u16s,
/// as Premiere 26.5.1 saves it (Oracle run E4). Its `BinaryHash` is a random
/// UUID, as for file media (inferred).
fn levels_private_data(effect: &PrEffect) -> Result<Option<RetainedOrSkipped<MotionPrivateData>>> {
    let PrEffectParams::Levels(levels) = &effect.params else {
        return Ok(None);
    };
    let mut bytes = Vec::with_capacity(40);
    for value in levels.start_values() {
        // Export rounds every Levels value to a whole native value.
        ensure_valid!(
            value.fract() == 0.0 && (0.0..=f64::from(u16::MAX)).contains(&value),
            "Levels value {value} is not a whole number that its private data can store"
        );
        bytes.extend_from_slice(&(value as u16).to_le_bytes());
    }
    Ok(Some(
        MotionPrivateData {
            encoding: records::ENCODING,
            binary_hash: uuid(),
            value: STANDARD.encode(bytes),
        }
        .into(),
    ))
}

/// Colour keys in the 8-field scalar key form with a native colour value and
/// zero handles: a Linear or Hold segment has no velocity. Premiere 26.5.1
/// saves Linear colour keys this way apart from its automatic influence
/// (Oracle run E6, Tint clip E).
fn colour_keyframes(keys: &[PrColourKeyframe]) -> Result<String> {
    let mut wire = String::new();
    for (index, key) in keys.iter().enumerate() {
        let mode = keys
            .get(index + 1)
            .map_or(PrKeyframeEasing::Linear, |next| next.easing);
        ensure_valid!(
            !matches!(mode, PrKeyframeEasing::CubicBezier { .. }),
            "Bezier colour keys have no Premiere form"
        );
        wire.push_str(&format!(
            "{},{},{},0,0,0,0,0;",
            key.source_ticks,
            key.value.native(),
            mode.native_outgoing_mode()
        ));
    }
    Ok(wire)
}

/// Premiere writes whole-number doubles with a trailing point (`25.`).
fn native_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}.")
    } else {
        value.to_string()
    }
}

/// A Film Impact effect's static values in `spec`'s `Params` order: `value`
/// of a parameter that the effect sets, and Premiere 26.5.1's `defaults` of
/// the others.
fn film_impact_values(
    spec: &EffectSpec,
    defaults: &[&str],
    value: impl Fn(&EffectParamSpec) -> Option<String>,
) -> Vec<String> {
    spec.params
        .iter()
        .zip(defaults)
        .map(|(param, default)| value(param).unwrap_or_else(|| (*default).to_owned()))
        .collect()
}
