//! Current-FX lowering for a freshly authored native 3D layer Transform.

use fx_schema::animator::{AnimationGraphEntry, PropertyKeyframeTrack};
use fx_schema::{LayerId, Position, PropType, PropertyKeyframeEasing, Transform};

use crate::writer::{
    KeyframeEasing, NativeTransform3d, NumericKeyframe, NumericTrack, Transform3dAnimations,
};

use super::{NativeTrack, float_value, native_easing, scalar_track, track};

/// Explicit XY source-coordinate adjustment applied before native authoring.
///
/// Media callers must use `source`; silently using `IDENTITY` would discard the
/// source fit geometry when a media layer becomes 3D.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Native2dGeometry {
    anchor_origin: [f64; 2],
    anchor_divisor: [f64; 2],
    scale_multiplier: [f64; 2],
}

impl Native2dGeometry {
    pub(super) const IDENTITY: Self = Self {
        anchor_origin: [0.0; 2],
        anchor_divisor: [1.0; 2],
        scale_multiplier: [1.0; 2],
    };

    /// Converts current layer-local media coordinates to natural-source space.
    pub(super) const fn source(origin: [f64; 2], scale: [f64; 2]) -> Self {
        Self {
            anchor_origin: origin,
            anchor_divisor: scale,
            scale_multiplier: scale,
        }
    }

    /// Rebases an FX anchor against a known native source/content origin.
    pub(super) const fn centered(origin: [f64; 2]) -> Self {
        Self {
            anchor_origin: origin,
            ..Self::IDENTITY
        }
    }
}

/// A typed native Transform plus the 3D projection fidelity disclosure.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct LoweredTransform3d {
    pub transform: NativeTransform3d,
    pub animations: Transform3dAnimations,
    pub projection_diagnostic: &'static str,
}

/// Lowers the transform selected by the parent hierarchy policy.
///
/// `transform_owner_id` is deliberately separate from a content leaf ID: keys
/// owned by a single-child wrapper must remain attached to that selected owner.
pub(super) fn lower(
    entries: &[AnimationGraphEntry],
    selected_transform: &Transform,
    transform_owner_id: LayerId,
    geometry: Native2dGeometry,
) -> Result<Option<LoweredTransform3d>, &'static str> {
    let is_three_d = requires_native_3d(entries, selected_transform, transform_owner_id);
    let position = match selected_transform.position {
        Position::TwoD([x, y]) => [x, y, 0.0],
        Position::ThreeD(value) => value,
    };
    let (position_animation, position_separated) =
        position_tracks(entries, transform_owner_id, position)?;
    // Keep ordinary 2D transforms on their established writer path. This
    // sidecar is needed only when exact independent X/Y curves require native
    // separated Position followers.
    if !is_three_d && position_separated.is_none() {
        return Ok(None);
    }
    validate_geometry(geometry)?;
    if selected_transform.skew != 0.0
        || selected_transform.skew_axis != 0.0
        || has_target(entries, transform_owner_id, PropType::Skew)
        || has_target(entries, transform_owner_id, PropType::SkewAxis)
    {
        return Err(
            "The native Transform sidecar has no Skew/Skew Axis leaves; values or keys cannot be stored without changing semantics",
        );
    }

    let mut transform = NativeTransform3d {
        is_three_d,
        anchor: [
            selected_transform.anchor_point[0],
            selected_transform.anchor_point[1],
            0.0,
        ],
        position,
        scale: [
            selected_transform.scale[0] / 100.0,
            selected_transform.scale[1] / 100.0,
            1.0,
        ],
        orientation: selected_transform.orientation,
        rotation_x: selected_transform.rotation_x,
        rotation_y: selected_transform.rotation_y,
        rotation_z: selected_transform.rotation,
        opacity: selected_transform.opacity.value() / 100.0,
    };

    let mut animations = Transform3dAnimations {
        anchor: triple_track(
            entries,
            transform_owner_id,
            [
                Some(PropType::AnchorPointX),
                Some(PropType::AnchorPointY),
                None,
            ],
            transform.anchor,
            [1.0; 3],
            true,
            true,
        )?,
        position: position_animation,
        position_separated,
        scale: triple_track(
            entries,
            transform_owner_id,
            [Some(PropType::ScaleX), Some(PropType::ScaleY), None],
            [
                selected_transform.scale[0],
                selected_transform.scale[1],
                100.0,
            ],
            [100.0; 3],
            false,
            false,
        )?,
        orientation: triple_track(
            entries,
            transform_owner_id,
            [
                Some(PropType::OrientationX),
                Some(PropType::OrientationY),
                Some(PropType::OrientationZ),
            ],
            transform.orientation,
            [1.0; 3],
            false,
            true,
        )?,
        rotation_x: scalar_track(
            track(entries, transform_owner_id, PropType::RotationX)?,
            1.0,
        )?,
        rotation_y: scalar_track(
            track(entries, transform_owner_id, PropType::RotationY)?,
            1.0,
        )?,
        rotation_z: scalar_track(track(entries, transform_owner_id, PropType::Rotation)?, 1.0)?,
        opacity: scalar_track(
            track(entries, transform_owner_id, PropType::Opacity)?,
            100.0,
        )?,
    };
    if animations.orientation.as_ref().is_some_and(|track| {
        track.keys.iter().any(|key| {
            key.easing
                .iter()
                .any(|easing| matches!(easing, KeyframeEasing::CubicBezier { .. }))
        })
    }) {
        return Err(
            "ADBE Orientation otst/otky key values are established, but cubic quaternion temporal-speed metadata is not; Hold/Linear Orientation keys only",
        );
    }

    apply_geometry(&mut transform, &mut animations, geometry);
    Ok(Some(LoweredTransform3d {
        transform,
        animations,
        projection_diagnostic: "FX 3D is projected through the renderer's implicit default camera; this export stores native AE 3D Transform values, but AE's active/default camera projection may differ, so equal rendering is not claimed.",
    }))
}

pub(super) fn requires_native_3d(
    entries: &[AnimationGraphEntry],
    transform: &Transform,
    owner: LayerId,
) -> bool {
    matches!(transform.position, Position::ThreeD(_))
        || transform.rotation_x != 0.0
        || transform.rotation_y != 0.0
        || transform.orientation != [0.0; 3]
        || [
            PropType::PositionZ,
            PropType::RotationX,
            PropType::RotationY,
            PropType::OrientationX,
            PropType::OrientationY,
            PropType::OrientationZ,
        ]
        .into_iter()
        .any(|property| has_target(entries, owner, property))
}

fn has_target(entries: &[AnimationGraphEntry], owner: LayerId, property: PropType) -> bool {
    entries.iter().any(|entry| {
        entry
            .target
            .as_property()
            .is_some_and(|target| target.layer_id() == owner && target.property_type() == property)
    })
}

fn validate_geometry(value: Native2dGeometry) -> Result<(), &'static str> {
    if value
        .anchor_origin
        .iter()
        .chain(&value.anchor_divisor)
        .chain(&value.scale_multiplier)
        .any(|component| !component.is_finite())
        || value
            .anchor_divisor
            .iter()
            .chain(&value.scale_multiplier)
            .any(|component| *component <= 0.0)
    {
        return Err("Native 3D source geometry is non-finite or non-positive");
    }
    Ok(())
}

fn apply_geometry(
    transform: &mut NativeTransform3d,
    animations: &mut Transform3dAnimations,
    geometry: Native2dGeometry,
) {
    for axis in 0..2 {
        transform.anchor[axis] =
            (transform.anchor[axis] - geometry.anchor_origin[axis]) / geometry.anchor_divisor[axis];
        transform.scale[axis] *= geometry.scale_multiplier[axis];
    }
    if let Some(track) = &mut animations.anchor {
        for key in &mut track.keys {
            for axis in 0..2 {
                key.values[axis] = (key.values[axis] - geometry.anchor_origin[axis])
                    / geometry.anchor_divisor[axis];
                key.spatial_in[axis] /= geometry.anchor_divisor[axis];
                key.spatial_out[axis] /= geometry.anchor_divisor[axis];
            }
        }
    }
    if let Some(track) = &mut animations.scale {
        for key in &mut track.keys {
            for axis in 0..2 {
                key.values[axis] *= geometry.scale_multiplier[axis];
            }
        }
    }
}

type PositionTracks = (Option<NumericTrack>, Option<[Option<NumericTrack>; 3]>);

fn position_tracks(
    entries: &[AnimationGraphEntry],
    owner: LayerId,
    base: [f64; 3],
) -> Result<PositionTracks, &'static str> {
    let sources = [
        track(entries, owner, PropType::PositionX)?,
        track(entries, owner, PropType::PositionY)?,
        track(entries, owner, PropType::PositionZ)?,
    ];
    if sources.iter().all(Option::is_none) {
        return Ok((None, None));
    }
    let has_spatial_tangents = sources.iter().flatten().any(|source| match source {
        NativeTrack::Keyframes(track) => track
            .keyframes()
            .iter()
            .any(|key| key.spatial_in_tangent().is_some() || key.spatial_out_tangent().is_some()),
        NativeTrack::Constant(_) => false,
    });
    let keyed = sources.map(|source| match source {
        Some(NativeTrack::Keyframes(track)) => Some(track),
        _ => None,
    });
    let mut tracks = keyed.into_iter().flatten();
    let compatible = tracks.next().is_none_or(|first| {
        tracks.all(|other| {
            key_times(first).eq(key_times(other))
                && first
                    .keyframes()
                    .iter()
                    .zip(other.keyframes())
                    .all(|(left, right)| left.easing() == right.easing())
        })
    });
    // Keep the established spatial leader representation when it can preserve
    // the authored curves. Only incompatible scalar channels need separation.
    if has_spatial_tangents || compatible {
        return triple_track(
            entries,
            owner,
            [
                Some(PropType::PositionX),
                Some(PropType::PositionY),
                Some(PropType::PositionZ),
            ],
            base,
            [1.0; 3],
            true,
            true,
        )
        .map(|track| (track, None));
    }

    let [x, y, z] = sources;
    Ok((
        None,
        Some([
            scalar_track(x, 1.0)?,
            scalar_track(y, 1.0)?,
            scalar_track(z, 1.0)?,
        ]),
    ))
}

fn triple_track(
    entries: &[AnimationGraphEntry],
    owner: LayerId,
    properties: [Option<PropType>; 3],
    base: [f64; 3],
    divisors: [f64; 3],
    spatial: bool,
    shared_ease: bool,
) -> Result<Option<NumericTrack>, &'static str> {
    let sources = [
        properties[0]
            .map(|property| track(entries, owner, property))
            .transpose()?,
        properties[1]
            .map(|property| track(entries, owner, property))
            .transpose()?,
        properties[2]
            .map(|property| track(entries, owner, property))
            .transpose()?,
    ];
    let sources = sources.map(Option::flatten);
    if sources.iter().all(Option::is_none) {
        return Ok(None);
    }
    let keyed = sources.map(|source| match source {
        Some(NativeTrack::Keyframes(track)) => Some(track),
        _ => None,
    });
    let template = keyed.iter().flatten().next().copied();
    if !spatial && !shared_ease && template.is_some() {
        // A separated Position sidecar must not reintroduce the independent
        // Scale-knot restriction removed from ordinary planar Transforms.
        let values = sources
            .iter()
            .zip(base)
            .zip(divisors)
            .map(|((&source, fallback), divisor)| {
                component_value(source, 0, fallback).map(|value| value / divisor)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let tracks = keyed
            .iter()
            .zip(divisors)
            .map(|(&source, divisor)| scalar_track(source.map(NativeTrack::Keyframes), divisor))
            .collect::<Result<Vec<_>, _>>()?;
        return super::effects::merge_tracks(&values, &tracks);
    }
    if let Some(template) = template {
        for candidate in keyed.iter().flatten().skip(1) {
            if key_times(template).ne(key_times(candidate)) {
                return Err(
                    "Native 3D Transform components have different key times; resampling is not allowed",
                );
            }
        }
    }

    let key_count = template.map_or(1, |track| track.keyframes().len());
    let mut keys = Vec::with_capacity(key_count);
    for index in 0..key_count {
        let component_keys = keyed.map(|source| source.map(|track| &track.keyframes()[index]));
        let template_key = component_keys.iter().flatten().next().copied();
        let easings = component_keys.map(|key| {
            key.or(template_key)
                .map_or(PropertyKeyframeEasing::Hold, |key| key.easing())
        });
        if shared_ease && easings.iter().any(|easing| *easing != easings[0]) {
            return Err("Native 3D spatial/Orientation components require one shared easing curve");
        }
        let mut values = Vec::with_capacity(3);
        for axis in 0..3 {
            values.push(component_value(sources[axis], index, base[axis])? / divisors[axis]);
        }
        let (spatial_in, spatial_out) = if spatial {
            (
                (0..3)
                    .map(|axis| {
                        component_keys[axis]
                            .and_then(|key| key.spatial_in_tangent())
                            .unwrap_or(0.0)
                            / divisors[axis]
                    })
                    .collect(),
                (0..3)
                    .map(|axis| {
                        component_keys[axis]
                            .and_then(|key| key.spatial_out_tangent())
                            .unwrap_or(0.0)
                            / divisors[axis]
                    })
                    .collect(),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        keys.push(NumericKeyframe {
            time_millis: template_key.map_or(0, |key| key.layer_time().as_millis()),
            values,
            easing: if shared_ease {
                vec![native_easing(easings[0]); if spatial { 1 } else { 3 }]
            } else {
                easings.map(native_easing).to_vec()
            },
            spatial_in,
            spatial_out,
        });
    }
    Ok(Some(NumericTrack { keys }))
}

fn key_times(track: &PropertyKeyframeTrack) -> impl Iterator<Item = fx_schema::TimeOffset> + '_ {
    track.keyframes().iter().map(|key| key.layer_time())
}

fn component_value(
    source: Option<NativeTrack<'_>>,
    index: usize,
    fallback: f64,
) -> Result<f64, &'static str> {
    match source {
        Some(NativeTrack::Keyframes(track)) => float_value(track.keyframes()[index].value()),
        Some(NativeTrack::Constant(value)) => float_value(value),
        None => Ok(fallback),
    }
}

#[cfg(test)]
mod tests {
    use fx_schema::{
        PropertyTarget, PropertyValue, TimeOffset,
        animator::{
            AnimationGraphEntry, KeyframeId, PropertyAnimator, PropertyKeyframe,
            PropertyKeyframeTrack,
        },
    };

    use super::*;

    fn transform(position: [f64; 3]) -> Transform {
        transform_value(serde_json::json!(position))
    }

    fn transform_2d(position: [f64; 2]) -> Transform {
        transform_value(serde_json::json!(position))
    }

    fn transform_value(position: serde_json::Value) -> Transform {
        serde_json::from_value(serde_json::json!({
            "anchorPoint": [0.0, 0.0],
            "position": position,
            "scale": [100.0, 100.0],
            "rotation": 0.0,
            "opacity": 100.0
        }))
        .unwrap()
    }

    fn keyed_entry(
        owner: LayerId,
        property: PropType,
        keys: &[(i64, f64, PropertyKeyframeEasing)],
    ) -> AnimationGraphEntry {
        let track = PropertyKeyframeTrack::new(
            keys.iter()
                .enumerate()
                .map(|(index, (millis, value, easing))| {
                    PropertyKeyframe::new(
                        KeyframeId::new(format!("separated-3d-{property:?}-{index}")),
                        TimeOffset::from_millis(*millis),
                        PropertyValue::Float(*value),
                        *easing,
                    )
                })
                .collect(),
        )
        .unwrap();
        AnimationGraphEntry {
            target: PropertyTarget::layer(owner, property),
            animator: PropertyAnimator::keyframes(track),
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        }
    }

    #[test]
    fn public_export_keeps_differently_timed_xyz_position_tracks() {
        let native = crate::structure::read_project(include_bytes!(
            "../../tests/fixtures/properties/transform_unseparated.aep"
        ))
        .unwrap();
        let mut value = crate::structure_document::to_structural_fx_document(&native, Some(1))
            .unwrap()
            .document
            .to_json_value()
            .unwrap();
        let mut leaf =
            value["composition"]["layers"][0]["layers"][0]["layers"][0]["layers"][0].clone();
        let owner = LayerId::new(30100);
        leaf["id"] = serde_json::json!(30100);
        leaf["parent"] = serde_json::Value::Null;
        leaf["name"] = serde_json::json!("Independent XYZ position");
        leaf["transform"]["position"] = serde_json::json!([10.0, 30.0, 60.0]);
        leaf["transform"]["rotationX"] = serde_json::json!(5.0);
        let cubic = PropertyKeyframeEasing::CubicBezier {
            x1: 0.2,
            y1: 0.3,
            x2: 0.7,
            y2: 0.8,
        };
        let entries = vec![
            keyed_entry(
                owner,
                PropType::PositionX,
                &[
                    (0, 10.0, PropertyKeyframeEasing::Hold),
                    (1_000, 20.0, PropertyKeyframeEasing::Linear),
                ],
            ),
            keyed_entry(
                owner,
                PropType::PositionY,
                &[
                    (0, 30.0, PropertyKeyframeEasing::Linear),
                    (400, 40.0, cubic),
                    (1_200, 50.0, PropertyKeyframeEasing::Hold),
                ],
            ),
            keyed_entry(
                owner,
                PropType::PositionZ,
                &[
                    (0, 60.0, PropertyKeyframeEasing::Hold),
                    (200, 70.0, PropertyKeyframeEasing::Linear),
                    (700, 80.0, cubic),
                    (1_500, 90.0, PropertyKeyframeEasing::Hold),
                ],
            ),
        ];
        value["composition"]["layers"] = serde_json::json!([leaf]);
        value["composition"]["dynamics"] = serde_json::json!({"entries": entries});
        let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();

        let output = super::super::to_aep(&document).unwrap();
        let project = crate::structure::read_project(&output.bytes).unwrap();
        let crate::structure::ItemKind::Composition(composition) = &project.item(1).unwrap().kind
        else {
            panic!("fresh composition")
        };
        let cameras = composition
            .layers
            .iter()
            .filter(|layer| layer.record.layer_type() == 2)
            .count();
        let render_layers = composition
            .layers
            .iter()
            .filter(|layer| layer.record.layer_type() != 2)
            .collect::<Vec<_>>();
        assert_eq!(cameras, 1, "{:?}", output.diagnostics);
        assert_eq!(render_layers.len(), 1, "{:?}", output.diagnostics);
        let properties = crate::properties::read_transform(&render_layers[0].content).unwrap();
        let position = properties
            .iter()
            .find(|property| property.match_name == "ADBE Position")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        assert!(position.dimensions_separated);
        for (name, expected_count) in [
            ("ADBE Position_0", 2),
            ("ADBE Position_1", 3),
            ("ADBE Position_2", 4),
        ] {
            let follower = properties
                .iter()
                .find(|property| property.match_name == name)
                .unwrap()
                .numeric
                .as_ref()
                .unwrap();
            assert_eq!(follower.keyframes.len(), expected_count, "{name}");
        }
    }

    #[test]
    fn public_export_keeps_planar_independent_position_without_enabling_3d() {
        let native = crate::structure::read_project(include_bytes!(
            "../../tests/fixtures/properties/transform_unseparated.aep"
        ))
        .unwrap();
        let mut value = crate::structure_document::to_structural_fx_document(&native, Some(1))
            .unwrap()
            .document
            .to_json_value()
            .unwrap();
        let mut leaf =
            value["composition"]["layers"][0]["layers"][0]["layers"][0]["layers"][0].clone();
        let owner = LayerId::new(30103);
        leaf["id"] = serde_json::json!(30103);
        leaf["parent"] = serde_json::Value::Null;
        leaf["name"] = serde_json::json!("Independent planar position");
        leaf["transform"]["position"] = serde_json::json!([10.0, 30.0]);
        let entries = vec![
            keyed_entry(
                owner,
                PropType::PositionX,
                &[
                    (0, 10.0, PropertyKeyframeEasing::Hold),
                    (1_000, 20.0, PropertyKeyframeEasing::Linear),
                ],
            ),
            keyed_entry(
                owner,
                PropType::PositionY,
                &[
                    (0, 30.0, PropertyKeyframeEasing::Linear),
                    (400, 40.0, PropertyKeyframeEasing::Linear),
                    (1_200, 50.0, PropertyKeyframeEasing::Hold),
                ],
            ),
        ];
        value["composition"]["layers"] = serde_json::json!([leaf]);
        value["composition"]["dynamics"] = serde_json::json!({"entries": entries});
        let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();

        let output = super::super::to_aep(&document).unwrap();
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("implicit default camera"))
        );
        let project = crate::structure::read_project(&output.bytes).unwrap();
        let crate::structure::ItemKind::Composition(composition) = &project.item(1).unwrap().kind
        else {
            panic!("fresh composition")
        };
        assert_eq!(
            composition
                .layers
                .iter()
                .filter(|layer| layer.record.layer_type() == 2)
                .count(),
            0,
            "planar separated Position must not add a camera: {:?}",
            output.diagnostics
        );
        let render_layers = composition
            .layers
            .iter()
            .filter(|layer| layer.record.layer_type() != 2)
            .collect::<Vec<_>>();
        assert_eq!(render_layers.len(), 1, "{:?}", output.diagnostics);
        assert!(!render_layers[0].record.flags().three_d_layer);
        let properties = crate::properties::read_transform(&render_layers[0].content).unwrap();
        let position = properties
            .iter()
            .find(|property| property.match_name == "ADBE Position")
            .unwrap()
            .numeric
            .as_ref()
            .unwrap();
        assert!(position.dimensions_separated);
        for (name, expected_count) in [("ADBE Position_0", 2), ("ADBE Position_1", 3)] {
            let follower = properties
                .iter()
                .find(|property| property.match_name == name)
                .unwrap()
                .numeric
                .as_ref()
                .unwrap();
            assert_eq!(follower.keyframes.len(), expected_count, "{name}");
        }
    }

    #[test]
    fn planar_position_with_different_easing_uses_exact_scalar_followers() {
        let owner = LayerId::new(30104);
        let cubic = PropertyKeyframeEasing::CubicBezier {
            x1: 0.2,
            y1: 0.3,
            x2: 0.7,
            y2: 0.8,
        };
        let entries = vec![
            keyed_entry(
                owner,
                PropType::PositionX,
                &[
                    (0, 10.0, PropertyKeyframeEasing::Hold),
                    (1_000, 20.0, PropertyKeyframeEasing::Linear),
                ],
            ),
            keyed_entry(
                owner,
                PropType::PositionY,
                &[
                    (0, 30.0, PropertyKeyframeEasing::Linear),
                    (1_000, 40.0, cubic),
                ],
            ),
        ];

        let lowered = lower(
            &entries,
            &transform_2d([10.0, 30.0]),
            owner,
            Native2dGeometry::IDENTITY,
        )
        .unwrap()
        .unwrap();
        assert!(!lowered.transform.is_three_d);
        assert_eq!(lowered.transform.position, [10.0, 30.0, 0.0]);
        assert!(lowered.animations.position.is_none());
        let [x, y, z] = lowered.animations.position_separated.unwrap();
        assert_eq!(
            x.unwrap()
                .keys
                .iter()
                .map(|key| (key.time_millis, key.values[0], key.easing[0]))
                .collect::<Vec<_>>(),
            vec![
                (0, 10.0, KeyframeEasing::Hold),
                (1_000, 20.0, KeyframeEasing::Linear)
            ]
        );
        assert_eq!(
            y.unwrap()
                .keys
                .iter()
                .map(|key| (key.time_millis, key.values[0], key.easing[0]))
                .collect::<Vec<_>>(),
            vec![
                (0, 30.0, KeyframeEasing::Linear),
                (1_000, 40.0, native_easing(cubic))
            ]
        );
        assert!(z.is_none());
    }

    #[test]
    fn ordinary_planar_transform_does_not_enter_the_sidecar_or_reject_skew() {
        let owner = LayerId::new(30105);
        let entries = vec![
            keyed_entry(
                owner,
                PropType::PositionX,
                &[
                    (0, 10.0, PropertyKeyframeEasing::Linear),
                    (1_000, 20.0, PropertyKeyframeEasing::Linear),
                ],
            ),
            keyed_entry(
                owner,
                PropType::PositionY,
                &[
                    (0, 30.0, PropertyKeyframeEasing::Linear),
                    (1_000, 40.0, PropertyKeyframeEasing::Linear),
                ],
            ),
        ];
        let mut planar = transform_2d([10.0, 30.0]);
        planar.skew = 12.0;
        assert_eq!(
            lower(&entries, &planar, owner, Native2dGeometry::IDENTITY).unwrap(),
            None
        );

        let incompatible = vec![
            entries[0].clone(),
            keyed_entry(
                owner,
                PropType::PositionY,
                &[
                    (0, 30.0, PropertyKeyframeEasing::Hold),
                    (1_000, 40.0, PropertyKeyframeEasing::Hold),
                ],
            ),
        ];
        assert!(matches!(
            lower(&incompatible, &planar, owner, Native2dGeometry::IDENTITY),
            Err(message) if message.contains("no Skew/Skew Axis leaves")
        ));
    }

    #[test]
    fn independent_position_components_keep_exact_tracks_without_resampling() {
        let owner = LayerId::new(30100);
        let cubic = PropertyKeyframeEasing::CubicBezier {
            x1: 0.2,
            y1: 0.3,
            x2: 0.7,
            y2: 0.8,
        };
        let entries = vec![
            keyed_entry(
                owner,
                PropType::PositionX,
                &[
                    (0, 10.0, PropertyKeyframeEasing::Hold),
                    (1_000, 20.0, PropertyKeyframeEasing::Linear),
                ],
            ),
            keyed_entry(
                owner,
                PropType::PositionY,
                &[
                    (0, 30.0, PropertyKeyframeEasing::Linear),
                    (400, 40.0, cubic),
                    (1_200, 50.0, PropertyKeyframeEasing::Hold),
                ],
            ),
            keyed_entry(
                owner,
                PropType::PositionZ,
                &[
                    (0, 60.0, PropertyKeyframeEasing::Hold),
                    (200, 70.0, PropertyKeyframeEasing::Linear),
                    (700, 80.0, cubic),
                    (1_500, 90.0, PropertyKeyframeEasing::Hold),
                ],
            ),
        ];

        let lowered = lower(
            &entries,
            &transform([10.0, 30.0, 60.0]),
            owner,
            Native2dGeometry::IDENTITY,
        )
        .unwrap()
        .unwrap();
        assert!(lowered.transform.is_three_d);
        assert!(lowered.animations.position.is_none());
        let tracks = lowered.animations.position_separated.unwrap();
        let expected = [
            vec![
                (0, 10.0, KeyframeEasing::Hold),
                (1_000, 20.0, KeyframeEasing::Linear),
            ],
            vec![
                (0, 30.0, KeyframeEasing::Linear),
                (400, 40.0, native_easing(cubic)),
                (1_200, 50.0, KeyframeEasing::Hold),
            ],
            vec![
                (0, 60.0, KeyframeEasing::Hold),
                (200, 70.0, KeyframeEasing::Linear),
                (700, 80.0, native_easing(cubic)),
                (1_500, 90.0, KeyframeEasing::Hold),
            ],
        ];
        for (track, expected) in tracks.into_iter().zip(expected) {
            let track = track.unwrap();
            assert_eq!(track.keys.len(), expected.len());
            for (key, (time, value, easing)) in track.keys.iter().zip(expected) {
                assert_eq!(key.time_millis, time);
                assert_eq!(key.values, vec![value]);
                assert_eq!(key.easing, vec![easing]);
                assert!(key.spatial_in.is_empty());
                assert!(key.spatial_out.is_empty());
            }
        }
    }

    #[test]
    fn spatial_position_stays_combined_and_mismatched_spatial_times_are_rejected() {
        fn spatial_entry(
            owner: LayerId,
            property: PropType,
            end_millis: i64,
            tangent: f64,
        ) -> AnimationGraphEntry {
            let track = PropertyKeyframeTrack::new(vec![
                PropertyKeyframe::new(
                    KeyframeId::new(format!("spatial-{property:?}-0")),
                    TimeOffset::from_millis(0),
                    PropertyValue::Float(0.0),
                    PropertyKeyframeEasing::Linear,
                )
                .with_spatial_tangents(None, Some(tangent)),
                PropertyKeyframe::new(
                    KeyframeId::new(format!("spatial-{property:?}-1")),
                    TimeOffset::from_millis(end_millis),
                    PropertyValue::Float(100.0),
                    PropertyKeyframeEasing::Linear,
                )
                .with_spatial_tangents(Some(tangent), None),
            ])
            .unwrap();
            AnimationGraphEntry {
                target: PropertyTarget::layer(owner, property),
                animator: PropertyAnimator::keyframes(track),
                dependencies: Vec::new(),
                random_seed_target: None,
                layer_refs: Default::default(),
            }
        }

        let owner = LayerId::new(30102);
        let aligned = vec![
            spatial_entry(owner, PropType::PositionX, 1_000, 11.0),
            spatial_entry(owner, PropType::PositionY, 1_000, 22.0),
        ];
        let lowered = lower(
            &aligned,
            &transform([0.0, 0.0, 42.0]),
            owner,
            Native2dGeometry::IDENTITY,
        )
        .unwrap()
        .unwrap();
        assert!(lowered.animations.position_separated.is_none());
        let combined = lowered.animations.position.unwrap();
        assert_eq!(combined.keys[0].spatial_out, vec![11.0, 22.0, 0.0]);
        assert_eq!(combined.keys[1].spatial_in, vec![11.0, 22.0, 0.0]);

        let mismatched = vec![
            spatial_entry(owner, PropType::PositionX, 1_000, 11.0),
            spatial_entry(owner, PropType::PositionY, 500, 22.0),
        ];
        for transform in [transform([0.0, 0.0, 42.0]), transform_2d([0.0, 0.0])] {
            assert!(matches!(
                lower(
                    &mismatched,
                    &transform,
                    owner,
                    Native2dGeometry::IDENTITY,
                ),
                Err(message) if message.contains("different key times")
            ));
        }
    }

    #[test]
    fn separated_position_keeps_an_inactive_constant_axis_static() {
        let owner = LayerId::new(30101);
        let entries = vec![
            keyed_entry(
                owner,
                PropType::PositionX,
                &[
                    (0, 10.0, PropertyKeyframeEasing::Linear),
                    (1_000, 20.0, PropertyKeyframeEasing::Linear),
                ],
            ),
            keyed_entry(
                owner,
                PropType::PositionY,
                &[
                    (0, 30.0, PropertyKeyframeEasing::Hold),
                    (500, 40.0, PropertyKeyframeEasing::Linear),
                    (1_000, 50.0, PropertyKeyframeEasing::Linear),
                ],
            ),
        ];

        let lowered = lower(
            &entries,
            &transform([10.0, 30.0, 42.0]),
            owner,
            Native2dGeometry::IDENTITY,
        )
        .unwrap()
        .unwrap();
        assert_eq!(lowered.transform.position[2], 42.0);
        let [x, y, z] = lowered.animations.position_separated.unwrap();
        assert_eq!(x.unwrap().keys.len(), 2);
        assert_eq!(y.unwrap().keys.len(), 3);
        assert!(z.is_none());
    }
}
