//! Apply planar geometry before the opacity composite, avoiding a second raster transform.

use fx_schema::{GroupLayer, Layer, LayerData, LayerId, PropType};

use super::{Demand, PrecompositionPlan, native_transform, subtree_needs_projection};
use crate::writer::{NativeLayerOptions, NullLayerSpec, SolidTransform, TransformAnimations};

impl PrecompositionPlan {
    pub(in super::super) fn move_transform_inside(
        &mut self,
        group: &GroupLayer,
        dynamics: &crate::export_document::AnimationIndex<'_>,
        demand: &Demand,
        helper_id: LayerId,
        options: &NativeLayerOptions,
    ) -> Result<bool, &'static str> {
        if self.collapsed.is_some()
            || self.inner_parent.is_some()
            || self.camera.is_some()
            || self.consumer_3d
            || group.layers.len() < 2
            || group.motion_blur
            || group.layers.iter().any(has_motion_blur)
            || !group.effects.is_empty()
            || !group.masks.is_empty()
            || !group.fills.is_empty()
            || subtree_needs_projection(&group.layers, dynamics)
        {
            return Ok(false);
        }
        let (mut transform, mut animations) = if let Some((native, tracks)) = &options.transform_3d
        {
            (
                SolidTransform {
                    anchor: [native.anchor[0], native.anchor[1]],
                    position: [native.position[0], native.position[1]],
                    scale: [native.scale[0] * 100.0, native.scale[1] * 100.0],
                    rotation: native.rotation_z,
                    opacity: native.opacity * 100.0,
                },
                TransformAnimations {
                    opacity: tracks.opacity.clone(),
                    ..Default::default()
                },
            )
        } else {
            native_transform(group, dynamics)?
        };
        let geometric_keys = dynamics.for_layer(group.id).any(|entry| {
            entry.target.as_property().is_some_and(|property| {
                super::super::is_transform_property(property.property_type())
                    && property.property_type() != PropType::Opacity
            })
        });
        if !geometric_keys
            && transform.anchor == transform.position
            && transform.scale == [100.0; 2]
            && transform.rotation == 0.0
        {
            return Ok(false);
        }
        // Only opacity requires this isolation. Masks/effects have their own source spaces.
        if transform.opacity == 100.0 && animations.opacity.is_none() {
            return Ok(false);
        }
        // Geometry now precedes this crop: certify output demand, not child effect inputs.
        let bounds = demand.finite_union()?;
        let origin = bounds.min.map(f64::floor);
        let extent = std::array::from_fn::<_, 2, _>(|axis| bounds.max[axis].ceil() - origin[axis]);
        if extent
            .iter()
            .any(|value| !value.is_finite() || *value < 1.0 || *value > f64::from(u16::MAX))
        {
            return Ok(false);
        }
        let opacity = transform.opacity;
        let opacity_keys = animations.opacity.take();
        transform.opacity = 100.0;
        self.composite_parent = Some((
            helper_id,
            NullLayerSpec {
                name: format!("{} geometry", group.name),
                transform,
                transform_animations: animations,
            },
        ));
        self.origin = origin;
        self.width = extent[0] as u16;
        self.height = extent[1] as u16;
        self.transform = SolidTransform {
            anchor: [0.0; 2],
            position: origin,
            scale: [100.0; 2],
            rotation: 0.0,
            opacity,
        };
        self.transform_animations = TransformAnimations {
            opacity: opacity_keys,
            ..TransformAnimations::default()
        };
        Ok(true)
    }
}

fn has_motion_blur(layer: &Layer) -> bool {
    super::super::native_layer_options(layer).is_ok_and(|options| options.motion_blur)
        || matches!(layer.data(), LayerData::Group(group) if group.layers.iter().any(has_motion_blur))
}
