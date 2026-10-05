//! Fail-closed reader for the one mask on a clip's intrinsic Opacity
//! (`schema::mask`). Its Mask Path may be keyed; every control that has no FX
//! counterpart or no measurement converts only at its saved default, mask
//! Position and Anchor Point only as one centre (`MaskControl::Centre`), and
//! numeric Feather/Expansion/Opacity keys use bounded scalar timing. Any other
//! changing control, or any record outside the four saved forms, omits
//! the occurrence with its reason. Affine Tracker samples become Path keys;
//! redundant scalar keys at supported defaults remain those defaults.

use super::{
    animation::{bool_start, point_keys, point_start, scalar_keys, scalar_start, start_field},
    graphic::{binary_param, keyed_binary_param},
};
use crate::{
    error::{ensure, unsupported, Result},
    format::{graph::Element, Graph, Located},
    omit,
    schema::{
        at_default, decode_mask_tracker, is_saved_tracker_state,
        native::{SubComponents, VideoComponentParam, VideoFilterComponent},
        MaskControl, MaskForm, MaskParamRole, MaskTrackerTransform, PrMask, PrMaskPathKey,
        PrScalarKeyframe, MASK_MATCH_NAME_26_5, MASK_TYPE_26_5, OBJECT_MASK_TYPE,
    },
    Omission, OmissionScope,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::collections::BTreeSet;

/// Read the mask that the native component `owner` names in
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
        body.params.as_ref().map_or(0, |params| params.items.len()),
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
            )));
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
    // Type must be inspected before the opaque Tracker values: an Object
    // Mask stores selection/propagation there, not a vector Mask Path. Retain
    // typed references for source-bound raster recovery; never substitute
    // automatic PersonMatte or reveal an unmasked source on recovery failure.
    let mut object_mask = false;
    if form.match_name == MASK_MATCH_NAME_26_5 {
        let spec = &MASK_TYPE_26_5;
        for reference in &params.items {
            let record = graph.locate(reference, &mask.identity)?;
            if record.tag() != spec.tag
                || record
                    .element()
                    .child("ParameterID")
                    .and_then(Element::text)
                    .and_then(|id| id.parse::<usize>().ok())
                    != Some(spec.id)
            {
                continue;
            }
            let input = graph.decode::<VideoComponentParam>(record)?;
            if input.value.name.as_deref() == spec.name
                && input.value.class_id.as_deref() == Some(spec.class_id)
                && input.value.parameter_control_type.as_deref() == spec.control
                && (
                    input.value.lower_bound.clone(),
                    input.value.upper_bound.clone(),
                ) == spec.bounds(form)
                && scalar_start(&input.value.start_keyframe, &input.identity)? == OBJECT_MASK_TYPE
            {
                object_mask = true;
            }
        }
    }
    let mut raster = None;
    let mut seen = BTreeSet::new();
    let mut path = None;
    let mut path_keys = Vec::new();
    let mut tracker = None;
    let mut tracked_profile = false;
    let mut feather = None;
    let mut opacity = None;
    let mut expansion = None;
    let mut feather_keys = Vec::new();
    let mut opacity_keys = Vec::new();
    let mut expansion_keys = Vec::new();
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
            MaskParamRole::Path | MaskParamRole::Binary(_) | MaskParamRole::TrackerState(_) => {
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
                if object_mask && id == 24 {
                    raster = Some(crate::schema::RasterMask::Saved(binary_param(
                        graph,
                        reference,
                        &mask.identity,
                        param,
                        crate::format::object_mask::Tracker::decode,
                    )?));
                    continue;
                }
                if object_mask && id == 23 {
                    // User Interactions drive the AI controller, not the saved raster.
                    // Still validate static binary framing; never replay these bytes.
                    binary_param(graph, reference, &mask.identity, param, |_| Ok(()))?;
                    continue;
                }
                if object_mask && id == 7 {
                    binary_param(graph, reference, &mask.identity, param, |payload| {
                        ensure!(
                            payload == [2, 0, 0, 0, 0, 0, 0, 0],
                            "Object Mask Path must be the saved empty path"
                        );
                        Ok(())
                    })?;
                    path = Some(crate::schema::text::PrShapePath {
                        vertices: Vec::new(),
                        closed: true,
                    });
                    continue;
                }
                match spec.role {
                    MaskParamRole::Binary(_)
                        if !object_mask && form.match_name == MASK_MATCH_NAME_26_5 && id == 6 =>
                    {
                        tracker = Some(keyed_binary_param(
                            graph,
                            reference,
                            &mask.identity,
                            param,
                            decode_mask_tracker,
                        )?);
                    }
                    MaskParamRole::Binary(expected) => {
                        binary_param(graph, reference, &mask.identity, param, |payload| {
                            ensure!(
                                Some(payload) == STANDARD.decode(expected).ok().as_deref(),
                                "mask control {id} ({label}) holds an unknown value; only the saved default converts"
                            );
                            Ok(())
                        })?;
                    }
                    MaskParamRole::TrackerState(expected) => {
                        binary_param(graph, reference, &mask.identity, param, |payload| {
                            ensure!(
                                is_saved_tracker_state(payload, expected),
                                "mask control {id} ({label}) holds an unknown value; only the saved default, naming any tracker, converts"
                            );
                            Ok(())
                        })?;
                    }
                    _ => {
                        let (stored, keys) = keyed_binary_param(
                            graph,
                            reference,
                            &mask.identity,
                            param,
                            form.decode_path,
                        )?;
                        path_keys = keys
                            .into_iter()
                            .map(|(source_ticks, path)| PrMaskPathKey { source_ticks, path })
                            .collect();
                        // Premiere 26.5.1 draws the first key's outline before
                        // that key, never a keyed record's stored value.
                        path = Some(path_keys.first().map_or(stored, |first| first.path.clone()));
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
        // The centre is a pair of identical records; the three numeric
        // controls below use the scalar contract. Other controls stay at their
        // defaults, including the redundant transform keys a tracker saves.
        ensure!(
            matches!(
                control,
                MaskControl::Centre
                    | MaskControl::Feather
                    | MaskControl::Opacity
                    | MaskControl::Expansion
            ) || (matches!(control, MaskControl::Default(_)) && spec.key_fields() == 8)
                || (input.value.keyframes.as_deref().is_none_or(str::is_empty)
                    && input.value.is_time_varying.as_deref() != Some("true")),
            "{}: keyframed {label} is not supported; only static values convert",
            input.identity
        );
        let wire = &input.value.start_keyframe;
        match control {
            MaskControl::Feather | MaskControl::Opacity | MaskControl::Expansion => {
                let stored = scalar_start(wire, &input.identity)?;
                let (lower, upper) = match control {
                    MaskControl::Feather => (0.0, 1000.0),
                    MaskControl::Opacity => (0.0, 100.0),
                    _ => (-1000.0, 1000.0),
                };
                ensure!(
                    stored.is_finite() && (lower..=upper).contains(&stored),
                    "{}: {label} must be finite and within {lower}..={upper}",
                    input.identity
                );
                let keys = numeric_keys(&input, label)?;
                let value = keys.first().map_or(stored, |key| key.value);
                match control {
                    MaskControl::Feather => {
                        feather = Some(value);
                        feather_keys = keys;
                    }
                    MaskControl::Opacity => {
                        opacity = Some(value);
                        opacity_keys = keys;
                    }
                    MaskControl::Expansion => {
                        expansion = Some(value);
                        expansion_keys = keys;
                    }
                    _ => unreachable!("numeric mask control matched above"),
                }
            }
            MaskControl::Inverted => inverted = Some(bool_start(wire, &input.identity)?),
            MaskControl::Centre => {
                let read = MaskCentre::read(&input, label)?;
                ensure!(
                    centre.as_ref().is_none_or(|earlier| *earlier == read),
                    "{}: mask Position and Anchor Point differ; the mask transform is not converted",
                    input.identity
                );
                centre = Some(read);
            }
            MaskControl::Default(_)
                if !object_mask && form.match_name == MASK_MATCH_NAME_26_5 && id == 20 =>
            {
                let value = start_field(wire, spec.key_fields(), &input.identity)?;
                tracked_profile = at_default(value, "true");
                ensure!(
                    (tracked_profile || at_default(value, "false"))
                        && input.value.keyframes.as_deref().is_none_or(str::is_empty)
                        && input.value.is_time_varying.as_deref() != Some("true"),
                    "{}: unsupported saved tracker-profile marker",
                    input.identity
                );
            }
            MaskControl::Default(_)
                if !object_mask
                    && form.match_name == MASK_MATCH_NAME_26_5
                    && id == MASK_TYPE_26_5.id =>
            {
                // The tracked native ellipse uses Type 1 and still stores its
                // complete outline. Export retains that outline as a generic path.
                let value = scalar_start(wire, &input.identity)?;
                ensure!(
                    (value == 0.0 || value == 1.0)
                        && numeric_keys(&input, "mask Type")?
                            .iter()
                            .all(|key| key.value == value),
                    "{}: unsupported vector mask Type",
                    input.identity
                );
            }
            MaskControl::Default(default) => {
                let default = if object_mask && id == 20 {
                    "true"
                } else if object_mask && id == 22 {
                    "4"
                } else {
                    default
                };
                ensure!(
                    at_default(
                        start_field(wire, spec.key_fields(), &input.identity)?,
                        default
                    ),
                    "{}: mask control {id} at a value other than {default} is not converted",
                    input.identity
                );
                if spec.key_fields() == 8 {
                    ensure!(
                        numeric_keys(&input, label)?
                            .iter()
                            .all(|key| at_default(&key.value.to_string(), default)),
                        "{}: animated {label} differs from its supported default {default}",
                        input.identity
                    );
                }
            }
        }
    }
    ensure!(
        seen.len() == form.params().len(),
        "{}: missing mask parameters",
        mask.identity
    );
    let mut path = path.ok_or_else(|| unsupported("missing Mask Path"))?;
    if let Some((stored, keys)) = tracker
        .filter(|(stored, keys)| !keys.is_empty() || *stored != MaskTrackerTransform::IDENTITY)
    {
        ensure!(
            tracked_profile,
            "{}: Tracker motion is outside the saved tracked-mask profile",
            mask.identity
        );
        ensure!(
            path_keys.iter().all(|key| key.path == path),
            "{}: combined moving Mask Path and Tracker reference outlines are unsupported",
            mask.identity
        );
        if keys.is_empty() {
            path = stored.apply(&path);
            for key in &mut path_keys {
                key.path = stored.apply(&key.path);
            }
        } else {
            path_keys = keys
                .into_iter()
                .map(|(source_ticks, transform)| PrMaskPathKey {
                    source_ticks,
                    path: transform.apply(&path),
                })
                .collect();
            path = path_keys[0].path.clone();
        }
        omit(
            omissions,
            OmissionScope::Feature,
            &mask.identity,
            "Saved affine mask tracking converts to editable Mask Path keys on the source clock; linear path interpolation replaces tracker interpolation, and tracking analysis is not retained",
        );
    }
    let mask = PrMask {
        raster,
        path,
        path_keys,
        feather_keys,
        opacity_keys,
        expansion_keys,
        expansion: expansion.ok_or_else(|| unsupported("missing Mask Expansion"))?,
        feather: feather.ok_or_else(|| unsupported("missing Mask Feather"))?,
        opacity: opacity.ok_or_else(|| unsupported("missing Mask Opacity"))?,
        inverted: inverted.ok_or_else(|| unsupported("missing mask Inverted"))?,
    };
    ensure!(
        !object_mask || mask.raster.is_some(),
        "missing Object Mask saved Tracker"
    );
    mask.validate()?;
    Ok(Some(mask))
}

fn numeric_keys(
    input: &Located<VideoComponentParam>,
    label: &str,
) -> Result<Vec<PrScalarKeyframe>> {
    let wire = input.value.keyframes.as_deref().unwrap_or_default();
    ensure!(
        if wire.is_empty() {
            input.value.is_time_varying.as_deref() != Some("true")
        } else {
            matches!(input.value.is_time_varying.as_deref(), None | Some("true"))
        },
        "{}: {label} keys conflict with IsTimeVarying",
        input.identity
    );
    let keys = scalar_keys(wire, &input.identity)?;
    // Nonzero speeds do not affect all-Linear tracks. Mixed Linear/Bezier
    // segments can use both handles, whose mask velocity units are unverified.
    let all_linear = wire
        .split_terminator(';')
        .all(|key| key.split(',').nth(2) == Some("0"));
    for key in wire.split_terminator(';') {
        let fields: Vec<_> = key.split(',').collect();
        ensure!(
            fields[3] == "0"
                && (all_linear
                    || [4, 6]
                        .into_iter()
                        .all(|i| fields[i].parse::<f64>() == Ok(0.0)))
                && [5, 7].into_iter().all(|i| fields[i]
                    .parse::<f64>()
                    .is_ok_and(|v| (0.0..=1.0).contains(&v))),
            "{}: {label} supports only all-Linear keys or zero-speed handles, with temporal flags 0",
            input.identity
        );
    }
    Ok(keys)
}

/// One mask Position or Anchor Point record ([`MaskControl::Centre`]): its
/// static point, or its keyed record's native start and key fields and
/// `IsTimeVarying`, which the point parsers checked first. Two centres agree
/// when their static points are equal, or when both are keyed with identical
/// records: either way Position minus Anchor Point stays zero, so the mask
/// transform stays the identity. Parsed keys alone would not tell: the parser
/// does not keep the temporal flags and resolves automatic spatial tangents,
/// which shape the motion between keys.
#[derive(Debug, PartialEq)]
enum MaskCentre {
    Static([f64; 2]),
    Keyed {
        start: String,
        keys: String,
        is_time_varying: Option<String>,
    },
}

impl MaskCentre {
    /// Read the centre record `input`, named `label` in reasons. Keys need an
    /// absent or true `IsTimeVarying`, as Motion keys do; without keys a true
    /// flag still fails closed.
    fn read(input: &Located<VideoComponentParam>, label: &str) -> Result<Self> {
        let value = &input.value;
        let point = point_start(&value.start_keyframe, &input.identity)?;
        let keys = value.keyframes.as_deref().unwrap_or_default();
        let is_time_varying = value.is_time_varying.as_deref();
        if keys.is_empty() {
            ensure!(
                is_time_varying != Some("true"),
                "{}: keyframed {label} is not supported; only static values convert",
                input.identity
            );
            return Ok(Self::Static(point));
        }
        ensure!(
            matches!(is_time_varying, None | Some("true")),
            "{}: keyframes conflict with a disabled or invalid IsTimeVarying",
            input.identity
        );
        // Native tracking saves linear centre keys with the automatic spatial
        // flag. Validate their zero handles as linear, but compare the original
        // records below: only identical Position/Anchor curves can cancel.
        // This does not admit that flag for independent Motion animation.
        let validation_keys = keys
            .split(';')
            .map(|key| {
                let mut fields: Vec<_> = key.split(',').collect();
                if fields.len() == 14 && fields[8] == "0" && fields[9] == "4" {
                    fields[9] = "0";
                }
                fields.join(",")
            })
            .collect::<Vec<_>>()
            .join(";");
        point_keys(&validation_keys, &input.identity)?;
        Ok(Self::Keyed {
            start: value.start_keyframe.clone(),
            keys: keys.to_owned(),
            is_time_varying: is_time_varying.map(str::to_owned),
        })
    }
}
