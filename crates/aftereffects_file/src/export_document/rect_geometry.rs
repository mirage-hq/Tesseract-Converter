//! Typed FX Rectangle geometry controls lowered to fresh native Rectangle keys.
//!
//! FX stores a Rectangle position as its top-left origin. AE stores the native
//! Rectangle Position at its center, so every Size key owns a coupled Position
//! key. Layer/group Transform and anchor tracks remain outside this helper.

use fx_schema::{LayerId, PropType, RectLayer, animator::AnimationGraphEntry};

use crate::writer::{GeometryAnimations, VectorGeometry};

/// Fresh Rectangle geometry and the key tracks owned by that same FX Rectangle.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RectGeometryOutput {
    pub geometry: VectorGeometry,
    pub animations: GeometryAnimations,
}

/// Lowers current typed Rectangle geometry without sampling or retargeting keys.
///
/// Absent and disabled keyframe animators retain the current static geometry.
/// Constant and enabled keyframe animators become native parametric controls.
/// Only exact property targets owned by `layer.id` are considered.
pub(super) fn lower(
    layer: &RectLayer,
    dynamics: &[AnimationGraphEntry],
) -> Result<RectGeometryOutput, &'static str> {
    let origin = layer.rect.position;
    let size = layer.rect.size;
    let roundness = layer.rect.roundness;
    let center = [origin[0] + size[0] / 2.0, origin[1] + size[1] / 2.0];
    if size
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0 || *value > 65535.0)
        || origin.iter().any(|value| !value.is_finite())
        || center.iter().any(|value| !value.is_finite())
        || !roundness.is_finite()
        || !(0.0..=100000.0).contains(&roundness)
    {
        return Err("Rectangle static geometry exceeds native bounds");
    }

    let size_track = super::vector_track(owned_track(dynamics, layer.id, PropType::RectSize)?)?;
    if size_track.as_ref().is_some_and(|track| {
        track.keys.iter().any(|key| {
            key.values.len() != 2
                || key
                    .values
                    .iter()
                    .any(|value| !value.is_finite() || *value <= 0.0 || *value > 65535.0)
                || origin
                    .iter()
                    .zip(&key.values)
                    .any(|(start, extent)| !(start + extent / 2.0).is_finite())
        })
    }) {
        return Err("Animated Rectangle size or derived native center exceeds supported bounds");
    }
    let position_track = size_track
        .as_ref()
        .map(|track| crate::writer::NumericTrack {
            keys: track
                .keys
                .iter()
                .map(|key| {
                    let mut key = key.clone();
                    key.values = origin
                        .iter()
                        .zip(&key.values)
                        .map(|(start, extent)| start + extent / 2.0)
                        .collect();
                    key
                })
                .collect(),
        });

    let roundness_track = super::scalar_track(
        owned_track(dynamics, layer.id, PropType::RectRoundness)?,
        1.0,
    )?;
    if roundness_track.as_ref().is_some_and(|track| {
        track.keys.iter().any(|key| {
            key.values.len() != 1
                || key
                    .values
                    .iter()
                    .any(|value| !value.is_finite() || !(0.0..=100000.0).contains(value))
        })
    }) {
        return Err("Animated Rectangle roundness exceeds native bounds");
    }

    Ok(RectGeometryOutput {
        geometry: VectorGeometry::Rect {
            size,
            position: origin,
            roundness,
        },
        animations: GeometryAnimations {
            rect_size: size_track,
            rect_position: position_track,
            rect_roundness: roundness_track,
            ..Default::default()
        },
    })
}

fn owned_track<'a>(
    dynamics: &'a [AnimationGraphEntry],
    owner: LayerId,
    property: PropType,
) -> Result<Option<super::NativeTrack<'a>>, &'static str> {
    let count = dynamics
        .iter()
        .filter(|entry| {
            entry.target.as_property().is_some_and(|target| {
                target.layer_id() == owner && target.property_type() == property
            })
        })
        .count();
    if count > 1 {
        return Err("Multiple Rectangle geometry animators target one owned property");
    }
    super::track(dynamics, owner, property)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use fx_schema::{
        NonNegativeProperty, PropertyTarget, PropertyValue, RectShape, TimeOffset, Transform,
        animator::{
            AnimatorData, KeyframeId, PropertyAnimator, PropertyKeyframe, PropertyKeyframeTrack,
        },
        layer::ShapeLineJoin,
    };

    use super::*;

    fn rect() -> RectLayer {
        RectLayer {
            id: LayerId::new(7),
            name: "Rectangle".into(),
            description: String::new(),
            is_hidden: false,
            parent: None,
            blend_mode: Default::default(),
            track_matte: None,
            masks: Vec::new(),
            active_range: fx_schema::TimeRangeProperty::new(
                fx_schema::Time::ZERO,
                fx_schema::Duration::from_secs(2.0),
            ),
            effects: Vec::new(),
            motion_blur: false,
            transform: Transform {
                anchor_point: [0.0; 2],
                position: fx_schema::Position::xy(0.0, 0.0),
                scale: [100.0; 2],
                rotation: 0.0,
                skew: 0.0,
                skew_axis: 0.0,
                rotation_x: 0.0,
                rotation_y: 0.0,
                orientation: [0.0; 3],
                opacity: fx_schema::PercentageProperty::new(100.0).unwrap(),
            },
            rect: RectShape {
                size: [80.0, 40.0],
                position: [3.0, -4.0],
                roundness: 6.0,
                fill_enabled: true,
                fill_color: [1.0; 4],
                fill_paint: None,
                fill_blend_mode: None,
                stroke_enabled: false,
                stroke_color: None,
                stroke_width: NonNegativeProperty::default(),
                stroke_dashes: Vec::new(),
                stroke_dash_offset: 0.0,
                stroke_join: ShapeLineJoin::Miter,
                stroke_miter_limit: 4.0,
            },
        }
    }

    fn entry(
        owner: LayerId,
        property: PropType,
        animator: PropertyAnimator,
    ) -> AnimationGraphEntry {
        AnimationGraphEntry {
            target: PropertyTarget::layer(owner, property),
            animator,
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: BTreeMap::new(),
        }
    }

    fn keyed(values: [[f64; 2]; 2]) -> PropertyAnimator {
        PropertyAnimator::keyframes(
            PropertyKeyframeTrack::new(vec![
                PropertyKeyframe::new(
                    KeyframeId::new("first"),
                    TimeOffset::from_millis(0),
                    PropertyValue::Vector2(values[0]),
                    fx_schema::PropertyKeyframeEasing::Linear,
                ),
                PropertyKeyframe::new(
                    KeyframeId::new("second"),
                    TimeOffset::from_millis(500),
                    PropertyValue::Vector2(values[1]),
                    fx_schema::PropertyKeyframeEasing::Linear,
                ),
            ])
            .expect("valid Rectangle size keys"),
        )
    }

    #[test]
    fn static_constant_and_enabled_size_keep_top_left_origin_coupled_to_center() {
        let layer = rect();
        let static_output = lower(&layer, &[]).expect("static Rectangle");
        assert_eq!(static_output.animations, GeometryAnimations::default());

        let constant = entry(
            layer.id,
            PropType::RectSize,
            PropertyAnimator::constant(PropertyValue::Vector2([100.0, 60.0]))
                .expect("valid constant"),
        );
        let output = lower(&layer, &[constant]).expect("constant Rectangle size");
        assert_eq!(
            output.animations.rect_size.unwrap().keys[0].values,
            [100.0, 60.0]
        );
        assert_eq!(
            output.animations.rect_position.unwrap().keys[0].values,
            [53.0, 26.0]
        );

        let animated = entry(
            layer.id,
            PropType::RectSize,
            keyed([[80.0, 40.0], [140.0, 60.0]]),
        );
        let output = lower(&layer, &[animated]).expect("animated Rectangle size");
        let positions: Vec<_> = output
            .animations
            .rect_position
            .unwrap()
            .keys
            .into_iter()
            .map(|key| key.values)
            .collect();
        assert_eq!(positions, [vec![43.0, 16.0], vec![73.0, 26.0]]);
    }

    #[test]
    fn disabled_keys_emit_the_runtime_constant_and_unrelated_targets_are_not_retargeted() {
        let layer = rect();
        let track = keyed([[20.0, 20.0], [30.0, 30.0]]);
        let AnimatorData::Keyframes { track, .. } = track.data().clone() else {
            panic!("keyed test animator")
        };
        let disabled = PropertyAnimator::from_data(&AnimatorData::Keyframes {
            track,
            enabled: false,
            disabled_value: Some(PropertyValue::Vector2([80.0, 40.0])),
        })
        .expect("valid disabled animator");
        let entries = vec![
            entry(layer.id, PropType::RectSize, disabled),
            entry(
                LayerId::new(8),
                PropType::RectRoundness,
                PropertyAnimator::constant(PropertyValue::Float(90.0)).expect("valid constant"),
            ),
            entry(
                layer.id,
                PropType::StrokeMiterLimit,
                PropertyAnimator::constant(PropertyValue::Float(12.0)).expect("valid constant"),
            ),
            entry(
                layer.id,
                PropType::StrokeJoin,
                PropertyAnimator::constant(PropertyValue::String("round".into()))
                    .expect("valid constant"),
            ),
        ];
        let animations = lower(&layer, &entries)
            .expect("unrelated tracks are ignored")
            .animations;
        let size = animations
            .rect_size
            .expect("disabledValue is emitted as one native constant key");
        assert_eq!(size.keys.len(), 1);
        assert_eq!(size.keys[0].time_millis, 0);
        assert_eq!(size.keys[0].values, [80.0, 40.0]);
        let position = animations
            .rect_position
            .expect("the constant size keeps its coupled native center");
        assert_eq!(position.keys.len(), 1);
        assert_eq!(position.keys[0].time_millis, 0);
        assert_eq!(position.keys[0].values, [43.0, 16.0]);
        assert!(animations.rect_roundness.is_none());
    }
}
