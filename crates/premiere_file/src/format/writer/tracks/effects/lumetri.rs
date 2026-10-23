//! Native Lumetri Color records for an FX grade, vignette, or a grade merged
//! with the vignette that directly followed it, RGB Curves, Hue Saturation
//! Curves, Color Wheels, an Input LUT or a Look ([`LUMETRI`]): the pinned
//! 130-parameter layout, with the mapped sections' current values and keys
//! and every other parameter at its pinned default
//! ([`LUMETRI_DEFAULTS`]), as Premiere 26.5.2 saves `color_lumetri_basic`.
//!
//! The scalars carry the grade: a Premiere 26.5.2 probe rendered an edited
//! Exposure scalar beside an unchanged native `Blob`. Premiere refuses to open
//! a Lumetri whose `Blob` (ParameterID 1) has no value, so every component
//! writes it as natively saved, the constant `FE FE` sentinel, and the
//! component's constant private data ([`LUMETRI_PRIVATE_DATA`]); neither holds
//! grade values. Premiere 26.5.2 accepted a single written grade component
//! (29.97 fps export at `color_lumetri_basic`'s values): all 130 controls read
//! back equal to the native save and the render is identical to its native
//! reference. A separate grade and vignette component pair opened and
//! rendered (no native vignette reference). The merged grade + vignette
//! component and other values are not Adobe-checked
//! (`docs/formats/premiere.md`, Lumetri Color).

use super::{
    super::{animation::scalar_keyframes, scalar_start_keyframe},
    native_number,
};
use crate::format::{invalid, writer::graph::EffectIds, Result};
use crate::schema::{
    native::{
        ArbVideoComponentParam, EncodedValue, MotionBody, MotionParams, MotionPrivateData, Record,
        RetainedOrSkipped, VideoComponentParam, VideoFilterComponent,
    },
    records, LumetriDefault, LutFile, PrEffect, PrEffectParamKeys, PrEffectParams, LUMETRI,
    LUMETRI_DEFAULTS, LUMETRI_GRADE_PARAMS, LUMETRI_PRIVATE_DATA, LUMETRI_VIGNETTE_PARAMS,
};
use aftereffects_file::lumetri::{luts, RgbCurves, COLOR_WHEELS_ID, RGB_CURVES_ID};
use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};

/// Component and body versions of the pinned Lumetri Color.
const LUMETRI_VERSIONS: [&str; 2] = ["9", "7"];

/// The Lumetri Color component of `effect`, numbered `component_id` in its
/// chain, and its parameter records in native `Params` order.
pub(super) fn records(
    effect: &PrEffect,
    ids: &EffectIds,
    component_id: usize,
) -> Result<Vec<Record>> {
    ensure_valid!(
        effect.mask.is_none() && ids.mask.is_none(),
        "a masked Lumetri Color has no established written form"
    );
    ensure_valid!(
        ids.params.len() == LUMETRI.params.len(),
        "Lumetri Color requires one record per pinned layout parameter"
    );
    effect.validate()?;
    let values: Vec<(usize, f64)> = match &effect.params {
        // A grade merged with the vignette that followed it fills both
        // sections of the one component.
        PrEffectParams::LumetriGrade(grade) => LUMETRI_GRADE_PARAMS
            .iter()
            .map(|param| param.id)
            .zip(grade.values)
            .chain(grade.vignette.into_iter().flat_map(|vignette| {
                LUMETRI_VIGNETTE_PARAMS
                    .iter()
                    .map(|param| param.id)
                    .zip(vignette)
            }))
            .collect(),
        PrEffectParams::LumetriVignette(values) => LUMETRI_VIGNETTE_PARAMS
            .iter()
            .map(|param| param.id)
            .zip(*values)
            .collect(),
        PrEffectParams::LumetriCurves(_)
        | PrEffectParams::LumetriHueCurves(_)
        | PrEffectParams::LumetriWheels(_) => Vec::new(),
        PrEffectParams::LumetriLook(look) => vec![(luts::LOOK_INTENSITY, look.intensity)],
        PrEffectParams::LumetriInputLut(_) => Vec::new(),
        _ => {
            return Err(invalid(
                "only a grade, vignette, RGB Curves, Hue Saturation Curves, Color Wheels, Input LUT or Look is a Lumetri Color",
            ))
        }
    };
    // Saved binary values replacing the pinned defaults, by `ParameterID`.
    let mut binaries: Vec<(usize, (String, String))> = match &effect.params {
        PrEffectParams::LumetriCurves(curves) => {
            vec![(RGB_CURVES_ID, rgb_curves_value(curves)?)]
        }
        PrEffectParams::LumetriHueCurves(curves) => curves
            .encode()
            .ok_or_else(|| invalid("Lumetri hue curve has more than 32 points"))?
            .into_iter()
            .map(|(id, bytes)| (id, binary_value(&bytes)))
            .collect(),
        PrEffectParams::LumetriWheels(wheels) => {
            vec![(COLOR_WHEELS_ID, binary_value(&wheels.encode()))]
        }
        _ => Vec::new(),
    };
    binaries.extend(lut_path_slot(effect)?);
    let [version, body_version] = LUMETRI_VERSIONS;
    let mut output = vec![Record::VideoFilterComponent(VideoFilterComponent {
        object_id: ids.component,
        class_id: Some(records::VIDEO_FILTER_COMPONENT.class_id.to_owned()),
        version: Some(version.to_owned()),
        component: Some(MotionBody {
            version: Some(body_version.to_owned()),
            params: Some(MotionParams::from_ids(ids.params.iter().copied())),
            id: Some(component_id.to_string()),
            display_name: Some(LUMETRI.display_name.to_owned()),
            instance_name: None,
            // The pinned save omits Bypass and Intrinsic on an active Lumetri.
            bypass: (!effect.enabled).then(|| "true".to_owned()),
            intrinsic: None,
        }),
        premiere_filter_private_data: Some(
            MotionPrivateData {
                encoding: records::ENCODING,
                binary_hash: LUMETRI_PRIVATE_DATA.0.to_owned(),
                value: LUMETRI_PRIVATE_DATA.1.to_owned(),
            }
            .into(),
        ),
        media_dependency_map: None,
        sub_components: None,
        match_name: Some(LUMETRI.match_name.to_owned()),
        video_filter_type: Some(LUMETRI.filter_type.to_owned()),
    })];
    for ((&object_id, param), default) in
        ids.params.iter().zip(LUMETRI.params).zip(LUMETRI_DEFAULTS)
    {
        let name = (!param.name.is_empty()).then(|| param.name.to_owned());
        let control = (!param.control.is_empty()).then(|| param.control.to_owned());
        match default {
            // Written as saved: the Blob sentinel, and a value that Premiere
            // stores once per hash and leaves empty in later parameters.
            LumetriDefault::Binary { hash, value } => {
                let (hash, value) = binaries
                    .iter()
                    .find(|(id, _)| *id == param.id)
                    .map_or((hash, value), |(_, (hash, value))| {
                        (hash.as_str(), value.as_str())
                    });
                output.push(Record::ArbVideoComponentParam(ArbVideoComponentParam {
                    object_id,
                    class_id: Some(param.record.class_id.to_owned()),
                    version: Some(param.record.version.to_owned()),
                    node: RetainedOrSkipped::Skipped,
                    name,
                    parameter_control_type: control,
                    is_time_varying: None,
                    parameter_id: param.id.to_string(),
                    start_keyframe_position: Some(records::STATIC_KEYFRAME_TIME.to_owned()),
                    keyframes: None,
                    start_keyframe_value: Some(EncodedValue {
                        encoding: records::ENCODING.to_owned(),
                        binary_hash: Some(hash.to_owned()),
                        value: value.to_owned(),
                    }),
                }));
            }
            LumetriDefault::Static(start) => {
                let keys = effect.keys(param);
                // A keyed parameter's static value is its first key's.
                let start_keyframe = values.iter().find(|(id, _)| *id == param.id).map_or_else(
                    || start.to_owned(),
                    |(_, value)| scalar_start_keyframe(native_number(*value)),
                );
                let keyframes = match keys {
                    None => None,
                    Some(PrEffectParamKeys::Scalar(keys)) => Some(scalar_keyframes(keys)?),
                    Some(_) => return Err(invalid("Lumetri Color keys must be scalar")),
                };
                output.push(Record::VideoComponentParam(VideoComponentParam {
                    object_id,
                    class_id: Some(param.record.class_id.to_owned()),
                    version: Some(param.record.version.to_owned()),
                    name,
                    // The pinned keyed Saturation is marked time-varying.
                    is_time_varying: keyframes.as_ref().map(|_| "true".to_owned()),
                    discontinuous_interpolate: param
                        .discontinuous_interpolate
                        .then(|| "true".to_owned()),
                    parameter_control_type: control,
                    start_keyframe,
                    current_value: None,
                    keyframes,
                    lower_bound: (!param.lower_bound.is_empty())
                        .then(|| param.lower_bound.to_owned()),
                    upper_bound: (!param.upper_bound.is_empty())
                        .then(|| param.upper_bound.to_owned()),
                    parameter_id: param.id.to_string(),
                    lower_ui_bound: param.lower_ui_bound.map(str::to_owned),
                    upper_ui_bound: param.upper_ui_bound.map(str::to_owned),
                    bypass: None,
                }));
            }
        }
    }
    Ok(output)
}

/// The `BinaryHash` and base64 value of RGB Curves `curves`, as Premiere
/// 26.5.2 renders them ([`RgbCurves::encode`], [`binary_value`]).
fn rgb_curves_value(curves: &RgbCurves) -> Result<(String, String)> {
    let bytes = curves
        .encode()
        .ok_or_else(|| invalid("Lumetri RGB curve has more than 32 points"))?;
    Ok(binary_value(&bytes))
}

/// The `BinaryHash` and base64 value of the saved bytes `bytes`. The hash
/// is content-derived like Premiere's: 24 hex digits of the value's
/// SHA-256, then the byte length plus 12 as eight hex digits.
fn binary_value(bytes: &[u8]) -> (String, String) {
    let digest: String = Sha256::digest(bytes)
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let hash = format!(
        "{}-{}-{}-{}-{}{:08x}",
        &digest[..8],
        &digest[8..12],
        &digest[12..16],
        &digest[16..20],
        &digest[20..24],
        bytes.len() + 12
    );
    (hash, STANDARD.encode(bytes))
}

/// The path slot of a custom-LUT Lumetri Color and its saved value
/// (`BinaryHash`, base64 of [`binary_value`]), as Premiere 26.5.2 saves a
/// browsed `.cube`: the absolute path in UTF-16LE with one NUL terminator.
/// `None` for a grade or vignette.
fn lut_path_slot(effect: &PrEffect) -> Result<Option<(usize, (String, String))>> {
    let (id, lut) = match &effect.params {
        PrEffectParams::LumetriInputLut(lut) => (luts::INPUT_LUT_PATH, lut),
        PrEffectParams::LumetriLook(look) => (luts::LOOK_PATH, &look.lut),
        _ => return Ok(None),
    };
    ensure_valid!(
        matches!(lut.file, LutFile::Found(_))
            && std::path::Path::new(&lut.native_path).is_absolute(),
        "a Lumetri Color LUT needs the absolute path of its packaged .cube file"
    );
    let bytes: Vec<u8> = lut
        .native_path
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    Ok(Some((id, binary_value(&bytes))))
}
