//! Offset's saved center becomes an explicitly fully-wet MotionTile replacement.
use super::*;
use crate::schema::OFFSET_CENTER;

pub(super) const REPLACEMENT: &str = "Offset approximated by MotionTile with the saved normalized center, tile/output sizes 100%, phase 0 and no mirroring. Blend With Original (including its saved/cached values and keys) is ignored: replacement is fully wet. Spatial center tangents are discarded, retaining point values, times and temporal easing as independent coordinates. Wrapping seams, clipping, alpha and native sampling can differ; edited export writes MotionTile through the linked-AEP route, not original Offset state";

pub(super) fn reject_interpretation(
    _: &Graph<'_>,
    _: Record<'_>,
    _: &NativeEffect<'_>,
) -> Result<PrEffect> {
    Err(unsupported(
        "Offset replacement does not prove original controls static for source interpretation",
    ))
}

pub(super) fn read(
    graph: &Graph<'_>,
    record: Record<'_>,
    native: &NativeEffect<'_>,
) -> Result<PrEffect> {
    let enabled = native.enabled()?;
    let filter = graph.decode::<VideoFilterComponent>(record)?;
    ensure!(
        filter.value.video_filter_type.as_deref() == Some("2"),
        "unsupported Offset VideoFilterType"
    );
    let references = filter
        .value
        .component
        .and_then(|body| body.params)
        .ok_or_else(|| unsupported("missing Offset parameters"))?;
    let mut center = None;
    let mut ids = BTreeSet::new();
    for reference in &references.items {
        let record = graph.locate(reference, &native.identity)?;
        let element = record.element();
        let id = element
            .child("ParameterID")
            .and_then(Element::text)
            .ok_or_else(|| unsupported("missing Offset ParameterID"))?;
        ensure!(ids.insert(id), "duplicate Offset ParameterID {id}");
        ensure!(matches!(id, "1" | "2"), "unknown Offset ParameterID {id}");
        if id == "2" {
            ensure!(
                record.tag() == "VideoComponentParam"
                    && element.child("Name").and_then(Element::text) == Some("Blend With Original"),
                "unexpected Offset blend record"
            );
            // This control is deliberately not evaluated, including a stale
            // CurrentValue or unsupported animation. Its loss is always reported.
            continue;
        }
        ensure!(
            record.tag() == "PointComponentParam",
            "unexpected Offset center record"
        );
        only_children(element, &PARAM_CHILDREN, "Offset center ")?;
        let param = graph.decode::<VideoComponentParam>(record)?;
        ensure!(
            param.value.name.as_deref() == Some(OFFSET_CENTER.name),
            "unexpected Offset center name"
        );
        center = Some(
            if let Some(keys) = param
                .value
                .keyframes
                .as_deref()
                .filter(|keys| !keys.is_empty())
            {
                ensure!(
                    matches!(param.value.is_time_varying.as_deref(), None | Some("true")),
                    "Offset center keys conflict with IsTimeVarying"
                );
                ParamValue::Keyed(PrEffectParamKeys::Point(
                    super::super::animation::offset_point_keys(keys, &param.identity)?,
                ))
            } else {
                param_value(
                    &param.value,
                    element,
                    &OFFSET_CENTER,
                    &param.identity,
                    true,
                    false,
                )?
            },
        );
    }
    ensure!(ids.contains("2"), "missing Offset Blend With Original");
    let mut values = ParamValues {
        flattened_controls: Vec::new(),
        statics: BTreeMap::new(),
        animations: Vec::new(),
        start_keyframes: BTreeMap::new(),
    };
    match center.ok_or_else(|| unsupported("missing Offset center"))? {
        ParamValue::Static(value) => {
            values.statics.insert(1, value);
        }
        ParamValue::Keyed(PrEffectParamKeys::Point(mut keys)) => {
            for key in &mut keys {
                key.spatial_in_tangent = None;
                key.spatial_out_tangent = None;
            }
            values.animations.push(PrEffectParamAnimation {
                param: &OFFSET_CENTER,
                keys: PrEffectParamKeys::Point(keys),
            });
        }
        ParamValue::Keyed(_) => return Err(unsupported("Offset center keys must be points")),
    }
    Ok(PrEffect {
        mask: None,
        enabled,
        params: PrEffectParams::Offset(values.point(&OFFSET_CENTER)?),
        animations: values.animations,
    })
}
