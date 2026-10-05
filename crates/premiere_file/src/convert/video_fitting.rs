//! Lower display-pixel fitting into the square coded frame used by native export.

use fx_schema::MediaFit;

use crate::schema::{
    records::PixelAspectRatio, PrPointKeyframe, PrPropertyAnimation, PrStaticTransform,
};

/// Content scale and offset in units of the full coded frame. The fixed
/// Contain viewport and an implicit natural frame are deliberately distinct.
#[derive(Clone, Copy)]
pub(super) struct VideoFitting {
    scale: [f64; 2],
    offset: [f64; 2],
}

impl VideoFitting {
    pub(super) fn new(aspect: PixelAspectRatio, fit: MediaFit, natural: bool) -> Option<Self> {
        let ratio = aspect.scale();
        let scale = if natural {
            [ratio, 1.0]
        } else if fit == MediaFit::Contain {
            let factor = (1.0 / ratio).min(1.0);
            [ratio * factor, factor]
        } else {
            return None;
        };
        if scale == [1.0, 1.0] {
            return None;
        }
        let offset = if natural {
            [0.0, 0.0]
        } else {
            scale.map(|value| (1.0 - value) * 0.5)
        };
        Some(Self { scale, offset })
    }

    fn anchor(self, point: &mut [f64; 2]) {
        for (axis, value) in point.iter_mut().enumerate() {
            *value = (*value - self.offset[axis]) / self.scale[axis];
        }
    }

    fn anchors(self, keys: &mut [PrPointKeyframe]) {
        for key in keys {
            self.anchor(&mut key.value);
            for tangent in [&mut key.spatial_in_tangent, &mut key.spatial_out_tangent]
                .into_iter()
                .flatten()
            {
                for (axis, value) in tangent.iter_mut().enumerate() {
                    *value /= self.scale[axis];
                }
            }
        }
    }

    /// Returns whether uniform scale keys had to retain their static value:
    /// Motion has no admitted nonuniform two-axis scale animation mapping.
    pub(super) fn motion(
        self,
        transform: &mut PrStaticTransform,
        animations: &mut Vec<PrPropertyAnimation>,
    ) -> bool {
        self.anchor(&mut transform.anchor_point);
        for (value, factor) in transform.scale.iter_mut().zip(self.scale) {
            *value *= factor;
        }
        let mut lost_scale = false;
        animations.retain_mut(|animation| {
            match animation {
                PrPropertyAnimation::AnchorPoint(keys) => self.anchors(keys),
                PrPropertyAnimation::ScaleWidth(keys) => {
                    for key in keys {
                        key.value *= self.scale[0];
                    }
                }
                PrPropertyAnimation::UniformScale(keys) => {
                    if self.scale[0] != self.scale[1] {
                        lost_scale = true;
                        return false;
                    }
                    for key in keys {
                        key.value *= self.scale[0];
                    }
                }
                PrPropertyAnimation::Opacity(_)
                | PrPropertyAnimation::Position(_)
                | PrPropertyAnimation::Rotation(_) => {}
            }
            true
        });
        lost_scale
    }

    /// A Transform stage owns the video's source-space transform; its parent
    /// Motion must not receive the fitting a second time.
    pub(super) fn stage(self, effect: &mut crate::schema::PrEffect) {
        use crate::schema::{
            PrEffectParamAnimation, PrEffectParamKeys, PrEffectParams, TRANSFORM_ANCHOR_POINT,
            TRANSFORM_SCALE_HEIGHT, TRANSFORM_SCALE_WIDTH,
        };
        let PrEffectParams::Transform(transform) = &mut effect.params else {
            unreachable!("fitting is applied only to the emitted Transform stage");
        };
        self.anchor(&mut transform.anchor_point);
        let uniform = transform.uniform_scale;
        let original_height = transform.scale_height;
        transform.scale_width = if uniform {
            original_height
        } else {
            transform.scale_width
        } * self.scale[0];
        transform.scale_height *= self.scale[1];
        transform.uniform_scale = false;
        if uniform {
            effect
                .animations
                .retain(|animation| animation.param.id != TRANSFORM_SCALE_WIDTH.id);
            if let Some(height) = effect
                .animations
                .iter()
                .find(|animation| animation.param.id == TRANSFORM_SCALE_HEIGHT.id)
            {
                effect.animations.push(PrEffectParamAnimation {
                    param: &TRANSFORM_SCALE_WIDTH,
                    keys: height.keys.clone(),
                });
            }
        }
        effect
            .animations
            .sort_by_key(|animation| animation.param.id);
        for animation in &mut effect.animations {
            match &mut animation.keys {
                PrEffectParamKeys::Point(keys)
                    if animation.param.id == TRANSFORM_ANCHOR_POINT.id =>
                {
                    self.anchors(keys)
                }
                PrEffectParamKeys::Scalar(keys)
                    if animation.param.id == TRANSFORM_SCALE_WIDTH.id =>
                {
                    for key in keys {
                        key.value *= self.scale[0];
                    }
                }
                PrEffectParamKeys::Scalar(keys)
                    if animation.param.id == TRANSFORM_SCALE_HEIGHT.id =>
                {
                    for key in keys {
                        key.value *= self.scale[1];
                    }
                }
                _ => {}
            }
        }
    }
}
