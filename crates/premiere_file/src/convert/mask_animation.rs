//! Editable numeric controls on clip Opacity and Shape-attached mask owners.

use super::keyframes;
use crate::schema::{mask_numeric_easing, PrMask, PrScalarKeyframe};
use fx_schema::{
    animator::{AnimatorData, PropertyKeyframe, PropertyKeyframeTrack},
    AnimationGraph, FxItemId, KeyframeId, PropertyTarget, PropertyValue, TimeOffset,
};

pub(super) fn import_tracks(
    mask: &PrMask,
    id: FxItemId,
    source_in: i64,
) -> Result<Vec<(PropertyTarget, PropertyKeyframeTrack)>, String> {
    mask.validate().map_err(|e| e.to_string())?;
    mask.numeric_keys()
        .into_iter()
        .filter(|(_, keys)| !keys.is_empty())
        .map(|(name, keys)| {
            let keys = keys
                .iter()
                .enumerate()
                .map(|(index, key)| {
                    let value = match name {
                        "feather" => PropertyValue::Vector2([key.value; 2]),
                        "opacity" => PropertyValue::Float(key.value / 100.0),
                        _ => PropertyValue::Float(key.value),
                    };
                    Ok(PropertyKeyframe::new(
                        KeyframeId::new(format!("premiere-mask-{id}-{name}-{index}")),
                        TimeOffset::from_millis(
                            keyframes::layer_millis(key.source_ticks, source_in)
                                .map_err(|e| e.to_string())?,
                        ),
                        value,
                        keyframes::fx_easing(key.easing),
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok((
                PropertyTarget::fx_item(id, name),
                PropertyKeyframeTrack::new(keys).map_err(|e| e.to_string())?,
            ))
        })
        .collect()
}

pub(super) fn has_tracks(dynamics: &AnimationGraph, id: FxItemId) -> bool {
    dynamics
        .entries()
        .iter()
        .any(|entry| entry.target.fx_item_id() == Some(id))
}

/// Read only current editable keys. Unknown targets and non-keyframe animators
/// fail the whole mask, never leave its content unmasked or freeze a control.
pub(super) fn export_tracks(
    mask: &mut PrMask,
    id: FxItemId,
    dynamics: &AnimationGraph,
    source_in: i64,
) -> Result<(), String> {
    for entry in dynamics
        .entries()
        .iter()
        .filter(|entry| entry.target.fx_item_id() == Some(id))
    {
        let PropertyTarget::FxItemProperty(target) = &entry.target else {
            continue;
        };
        let name = target.property_name();
        let keys = match name {
            "feather" => &mut mask.feather_keys,
            "expansion" => &mut mask.expansion_keys,
            "opacity" => &mut mask.opacity_keys,
            _ => return Err(format!("the Opacity mask property {name} is not supported")),
        };
        let AnimatorData::Keyframes {
            track,
            enabled: true,
            ..
        } = entry.animator.data()
        else {
            return Err(format!(
                "the Opacity mask {name} is not one enabled keyframe track"
            ));
        };
        if !entry.dependencies.is_empty() || !keys.is_empty() {
            return Err(format!(
                "the Opacity mask {name} has dependencies or duplicate tracks"
            ));
        }
        for key in track.keyframes() {
            if key.spatial_in_tangent().is_some() || key.spatial_out_tangent().is_some() {
                return Err(format!("the Opacity mask {name} has spatial tangents"));
            }
            let value = match (name, key.value()) {
                ("feather", PropertyValue::Vector2([x, y])) if x == y => *x,
                ("feather", _) => {
                    return Err(
                        "the Opacity mask feather keys must have equal finite axes".to_owned()
                    )
                }
                ("opacity", PropertyValue::Float(value)) => value * 100.0,
                ("expansion", PropertyValue::Float(value)) => *value,
                _ => return Err(format!("the Opacity mask {name} keys must be floats")),
            };
            let easing = keyframes::native_easing(key.easing()).map_err(|e| e.to_string())?;
            if !mask_numeric_easing(easing) {
                return Err(format!(
                    "the Opacity mask {name} supports only Linear, Hold or zero-speed Bezier keys"
                ));
            }
            keys.push(PrScalarKeyframe {
                source_ticks: keyframes::source_ticks(source_in, key.layer_time().as_millis())
                    .map_err(|e| e.to_string())?,
                value,
                easing,
            });
        }
    }
    mask.validate().map_err(|e| e.to_string())
}
