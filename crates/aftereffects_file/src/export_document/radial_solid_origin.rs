//! Keep an edited Rect-local radial center fixed when selecting a native Solid.
use fx_schema::{EffectData, PropertyTarget, RectLayer, Transform, animator::AnimationGraphEntry};

use crate::writer::NativeLayerOptions;

/// The caller has selected a real static, unparented root Solid. Its source
/// starts at zero, while FX radial points and Anchor use the Rect's local plane.
/// This translation does not equate native Zoom and FX sampling kernels.
pub(super) fn rebase_static_solid(
    rect: &RectLayer,
    transform: &Transform,
    options: &mut NativeLayerOptions,
    dynamics: &[AnimationGraphEntry],
) -> bool {
    if rect.effects.len() != 1
        || !rect.masks.is_empty()
        || options.parent.is_some()
        || options.matte.is_some()
        || !options.masks.is_empty()
        || !options.styles.is_empty()
        || options.transform_3d.is_some()
        || rect.blend_mode != Default::default()
        || transform.scale[0] != transform.scale[1]
        || !transform.scale[0].is_finite()
        || transform.scale[0] <= 0.0
        || !transform.rotation.is_finite()
        || transform.opacity.value() != 100.0
        || dynamics
            .iter()
            .any(|entry| entry.target.layer_id() == Some(rect.id))
    {
        return false;
    }
    if let EffectData::Identified { id, .. } = rect.effects[0].data()
        && dynamics.iter().any(|entry| {
            matches!(&entry.target, PropertyTarget::EffectProperty(target) if target.effect_id() == *id)
        })
    {
        return false;
    }
    let [effect] = options.effects.as_mut_slice() else {
        return false;
    };
    if !effect.enabled
        || effect.match_name != "ADBE Radial Blur"
        || effect
            .properties
            .iter()
            .any(|property| property.animation.is_some())
    {
        return false;
    }
    let Some(point) = effect.properties.iter_mut().find(|property| {
        property.match_name == "ADBE Radial Blur-0002" && property.values.len() == 2
    }) else {
        return false;
    };
    let center = [
        point.values[0] - rect.rect.position[0],
        point.values[1] - rect.rect.position[1],
    ];
    if center.iter().any(|value| !value.is_finite()) {
        return false;
    }
    point.values.copy_from_slice(&center);
    true
}
