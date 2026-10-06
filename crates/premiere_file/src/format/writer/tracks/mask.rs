//! The mask record of a clip's Opacity mask, in the v7 form that the written
//! Opacity owner pairs with (`schema::mask`).

use super::{animation::scalar_keyframes, scalar_start_keyframe};
use crate::format::{writer::graph::MaskIds, Result};
use crate::schema::{
    encode_mask_path,
    native::{
        ArbVideoComponentParam, EncodedValue, MotionBody, MotionParams, MotionPrivateData, Record,
        RetainedOrSkipped, VideoComponentParam, VideoFilterComponent,
    },
    records, MaskControl, MaskParamRole, PrMask, MASK_FORM_V7, MASK_PATH_RECORD_VERSION,
    MASK_PRIVATE_DATA,
};
use base64::{engine::general_purpose::STANDARD, Engine};

/// The mask component and its 13 parameters, as `abstract_slideshow` and
/// `vhs_slideshow` save them: the tracking controls off, the path in unit
/// frame fractions, editable numeric controls, and the corpus constants. A keyed path is
/// written as Premiere 26.5.1 saves one: `ticks,base64;` per
/// key, `IsTimeVarying` true and the first key as the stored value. The v7
/// form is unobserved with keys, and Premiere's reopen of it is unverified.
pub(super) fn records(mask: &PrMask, ids: &MaskIds) -> Result<Vec<Record>> {
    mask.validate()?;
    let form = &MASK_FORM_V7;
    let mut output = vec![Record::VideoFilterComponent(VideoFilterComponent {
        object_id: ids.component,
        class_id: Some(records::VIDEO_FILTER_COMPONENT.class_id.to_owned()),
        version: Some(form.component_version.to_owned()),
        component: Some(MotionBody {
            version: Some(form.body_version.to_owned()),
            params: Some(MotionParams::from_ids(ids.params)),
            id: Some("0".to_owned()),
            display_name: Some(form.display_name.to_owned()),
            instance_name: Some("1".to_owned()),
            bypass: Some("false".to_owned()),
            intrinsic: Some("false".to_owned()),
        }),
        premiere_filter_private_data: Some(
            MotionPrivateData {
                encoding: records::ENCODING,
                binary_hash: ids.private_data_hash.clone(),
                value: MASK_PRIVATE_DATA.to_owned(),
            }
            .into(),
        ),
        sub_components: None,
        match_name: Some(form.match_name.to_owned()),
        video_filter_type: Some("2".to_owned()),
    })];
    for (&object_id, spec) in ids.params.iter().zip(form.params()) {
        let control = match spec.role {
            MaskParamRole::Path => {
                let keys = mask
                    .path_keys
                    .iter()
                    .map(|key| {
                        let payload = STANDARD.encode(encode_mask_path(&key.path)?);
                        Ok(format!("{},{payload};", key.source_ticks))
                    })
                    .collect::<Result<String>>()?;
                output.push(Record::ArbVideoComponentParam(ArbVideoComponentParam {
                    object_id,
                    class_id: Some(spec.class_id.to_owned()),
                    version: Some(MASK_PATH_RECORD_VERSION.to_owned()),
                    node: RetainedOrSkipped::Skipped,
                    name: spec.name.map(str::to_owned),
                    is_time_varying: Some((!keys.is_empty()).to_string()),
                    parameter_control_type: spec.control.map(str::to_owned),
                    parameter_id: spec.id.to_string(),
                    start_keyframe_position: Some(records::STATIC_KEYFRAME_TIME.to_owned()),
                    start_keyframe_value: Some(EncodedValue {
                        encoding: records::ENCODING.to_owned(),
                        binary_hash: Some(ids.path_hash.clone()),
                        value: STANDARD.encode(encode_mask_path(&mask.path)?),
                    }),
                    keyframes: (!keys.is_empty()).then_some(keys),
                }));
                continue;
            }
            MaskParamRole::Binary(_) | MaskParamRole::TrackerState(_) => {
                return Err(crate::format::invalid(
                    "the written mask form has no tracker values",
                ))
            }
            MaskParamRole::Control(control) => control,
        };
        let value = match control {
            MaskControl::Feather => mask.feather.to_string(),
            MaskControl::Opacity => mask.opacity.to_string(),
            MaskControl::Expansion => mask.expansion.to_string(),
            MaskControl::Inverted => mask.inverted.to_string(),
            MaskControl::Default(value) => value.to_owned(),
            MaskControl::Centre => {
                return Err(crate::format::invalid(
                    "the written mask form has no mask Position or Anchor Point",
                ))
            }
        };
        let keys = match control {
            MaskControl::Feather => mask.feather_keys.as_slice(),
            MaskControl::Opacity => mask.opacity_keys.as_slice(),
            MaskControl::Expansion => mask.expansion_keys.as_slice(),
            _ => &[],
        };
        let value = keys.first().map_or(value, |key| key.value.to_string());
        let (lower_bound, upper_bound) = spec.bounds(form);
        output.push(Record::VideoComponentParam(VideoComponentParam {
            object_id,
            class_id: Some(spec.class_id.to_owned()),
            version: Some(records::VIDEO_COMPONENT_PARAM.version.to_owned()),
            name: spec.name.map(str::to_owned),
            is_time_varying: Some((!keys.is_empty()).to_string()),
            discontinuous_interpolate: None,
            parameter_control_type: spec.control.map(str::to_owned),
            start_keyframe: scalar_start_keyframe(&value),
            current_value: None,
            keyframes: (!keys.is_empty())
                .then(|| scalar_keyframes(keys))
                .transpose()?,
            lower_bound,
            upper_bound,
            parameter_id: spec.id.to_string(),
            lower_ui_bound: spec.lower_ui.map(str::to_owned),
            upper_ui_bound: spec.upper_ui.map(str::to_owned),
            bypass: None,
        }));
    }
    Ok(output)
}
