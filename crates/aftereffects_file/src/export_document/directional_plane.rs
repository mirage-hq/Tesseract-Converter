//! FX Directional Blur controls are screen-space, unlike native raster controls.

use fx_schema::{EffectData, PropertyTarget, RectLayer, Transform, animator::AnimationGraphEntry};

use crate::writer::NativeLayerOptions;

/// Called only after selecting a real native Solid source with a static Transform.
/// Other source kinds and mixed/gated stages keep their diagnosed approximation.
pub(super) fn compensate_static_solid(
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
        || effect.match_name != "ADBE Motion Blur"
        || effect
            .properties
            .iter()
            .any(|property| property.animation.is_some())
    {
        return false;
    }
    let direction = effect.properties.iter().position(|property| {
        property.match_name == "ADBE Motion Blur-0001" && property.values.len() == 1
    });
    let length = effect.properties.iter().position(|property| {
        property.match_name == "ADBE Motion Blur-0002" && property.values.len() == 1
    });
    let (Some(direction), Some(length)) = (direction, length) else {
        return false;
    };
    let source_direction = effect.properties[direction].values[0] - transform.rotation;
    let source_length = effect.properties[length].values[0] / (transform.scale[0] / 100.0);
    if !source_direction.is_finite() || !source_length.is_finite() || source_length < 0.0 {
        return false;
    }
    effect.properties[direction].values[0] = source_direction;
    effect.properties[length].values[0] = source_length;
    true
}
