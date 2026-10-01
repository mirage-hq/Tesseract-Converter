//! Fail-closed reader for the one static mask on a clip's intrinsic Opacity
//! (`schema::mask`). Every control that has no FX counterpart or no
//! measurement converts only at its saved default; anything keyed, and any
//! record outside the three saved forms, omits the occurrence with its reason.

use super::{
    animation::{bool_start, point_start, scalar_start, start_field},
    graphic::binary_param,
};
use crate::{
    error::{ensure, unsupported, Result},
    format::{graph::Element, Graph},
    omit,
    schema::{
        at_default,
        native::{SubComponents, VideoComponentParam, VideoFilterComponent},
        MaskControl, MaskForm, MaskParamRole, PrMask,
    },
    Omission, OmissionScope,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::collections::BTreeSet;

/// Read the mask that the Opacity component `owner` names in
/// `sub_components`, or `None` for a bypassed mask, which Premiere does not
/// render and which is reported in `omissions`.
pub(super) fn read_opacity_mask(
    graph: &Graph<'_>,
    owner: &str,
    sub_components: &SubComponents,
    omissions: &mut Vec<Omission>,
) -> Result<Option<PrMask>> {
    let [reference] = sub_components.items.as_slice() else {
        return Err(unsupported(format!(
            "{owner}: {} masks on Opacity are not converted; how Premiere combines them is unverified",
            sub_components.items.len()
        )));
    };
    let mask = graph.follow::<VideoFilterComponent>(reference, owner)?;
    let body = mask
        .value
        .component
        .as_ref()
        .ok_or_else(|| unsupported(format!("{}: missing mask Component", mask.identity)))?;
    let form = MaskForm::of(
        mask.value.match_name.as_deref(),
        mask.value.version.as_deref(),
        body.version.as_deref(),
    )
    .ok_or_else(|| {
        unsupported(format!(
            "{}: unsupported mask record form (MatchName {:?}, VideoFilterComponent {:?}, Component {:?})",
            mask.identity, mask.value.match_name, mask.value.version, body.version
        ))
    })?;
    ensure!(
        body.display_name.as_deref() == Some(form.display_name)
            && body.intrinsic.as_deref() == form.flags_written.then_some("false")
            && mask.value.sub_components.is_none(),
        "{}: unsupported mask component",
        mask.identity
    );
    match (form.flags_written, body.bypass.as_deref()) {
        (true, Some("false")) | (false, None) => {}
        (true, Some("true")) => {
            omit(
                omissions,
                OmissionScope::Feature,
                &mask.identity,
                "bypassed mask is not converted; Premiere renders the clip without it",
            );
            return Ok(None);
        }
        _ => {
            return Err(unsupported(format!(
                "{}: unsupported mask Bypass",
                mask.identity
            )))
        }
    }
    let params = body
        .params
        .as_ref()
        .ok_or_else(|| unsupported(format!("{}: missing mask Params", mask.identity)))?;
    ensure!(
        params.items.len() == form.params().len(),
        "{}: unsupported mask parameter layout",
        mask.identity
    );
    let mut seen = BTreeSet::new();
    let mut path = None;
    let mut feather = None;
    let mut opacity = None;
    let mut inverted = None;
    let mut centre = None;
    for reference in &params.items {
        let record = graph.locate(reference, &mask.identity)?;
        let identity = record.identity();
        let (id, spec) = record
            .element()
            .child("ParameterID")
            .and_then(Element::text)
            .and_then(|id| id.parse::<usize>().ok())
            .and_then(|id| {
                form.params()
                    .iter()
                    .find(|spec| spec.id == id)
                    .map(|spec| (id, spec))
            })
            .ok_or_else(|| unsupported(format!("{identity}: unknown mask parameter")))?;
        ensure!(
            seen.insert(id),
            "{identity}: duplicate mask ParameterID {id}"
        );
        ensure!(
            record.tag() == spec.tag,
            "{identity}: unsupported mask parameter type"
        );
        let label = spec.name.unwrap_or("mask control");
        let control = match spec.role {
            MaskParamRole::Path | MaskParamRole::Binary(_) => {
                ensure!(
                    record.element().attribute("ClassID") == Some(spec.class_id)
                        && record
                            .element()
                            .child("ParameterControlType")
                            .and_then(Element::text)
                            == spec.control,
                    "{identity}: unexpected {label} layout"
                );
                let id_text = id.to_string();
                let param = (id_text.as_str(), label);
                match spec.role {
                    MaskParamRole::Binary(expected) => {
                        binary_param(graph, reference, &mask.identity, param, |payload| {
                            ensure!(
                                Some(payload) == STANDARD.decode(expected).ok().as_deref(),
                                "mask control {id} ({label}) holds an unknown value; only the saved default converts"
                            );
                            Ok(())
                        })?;
                    }
                    _ => {
                        path = Some(binary_param(
                            graph,
                            reference,
                            &mask.identity,
                            param,
                            form.decode_path,
                        )?);
                    }
                }
                continue;
            }
            MaskParamRole::Control(control) => control,
        };
        let input = graph.decode::<VideoComponentParam>(record)?;
        ensure!(
            input.value.name.as_deref() == spec.name
                && input.value.class_id.as_deref() == Some(spec.class_id)
                && input.value.parameter_control_type.as_deref() == spec.control
                && (
                    input.value.lower_bound.clone(),
                    input.value.upper_bound.clone()
                ) == spec.bounds(form)
                && input.value.lower_ui_bound.as_deref() == spec.lower_ui
                && input.value.upper_ui_bound.as_deref() == spec.upper_ui,
            "{}: unexpected {label} layout",
            input.identity
        );
        ensure!(
            input.value.keyframes.as_deref().is_none_or(str::is_empty)
                && input.value.is_time_varying.as_deref() != Some("true"),
            "{}: keyframed {label} is not supported; only static values convert",
            input.identity
        );
        let wire = &input.value.start_keyframe;
        match control {
            MaskControl::Feather => feather = Some(scalar_start(wire, &input.identity)?),
            MaskControl::Opacity => opacity = Some(scalar_start(wire, &input.identity)?),
            MaskControl::Expansion => ensure!(
                scalar_start(wire, &input.identity)? == 0.0,
                "{}: Mask Expansion is not converted",
                input.identity
            ),
            MaskControl::Inverted => inverted = Some(bool_start(wire, &input.identity)?),
            MaskControl::Centre => {
                let point = point_start(wire, &input.identity)?;
                ensure!(
                    centre.replace(point).is_none_or(|earlier| earlier == point),
                    "{}: mask Position and Anchor Point differ; the mask transform is not converted",
                    input.identity
                );
            }
            MaskControl::Default(default) => ensure!(
                at_default(
                    start_field(wire, spec.key_fields(), &input.identity)?,
                    default
                ),
                "{}: mask control {id} at a value other than {default} is not converted",
                input.identity
            ),
        }
    }
    ensure!(
        seen.len() == form.params().len(),
        "{}: missing mask parameters",
        mask.identity
    );
    let mask = PrMask {
        path: path.ok_or_else(|| unsupported("missing Mask Path"))?,
        feather: feather.ok_or_else(|| unsupported("missing Mask Feather"))?,
        opacity: opacity.ok_or_else(|| unsupported("missing Mask Opacity"))?,
        inverted: inverted.ok_or_else(|| unsupported("missing mask Inverted"))?,
    };
    mask.validate()?;
    Ok(Some(mask))
}
