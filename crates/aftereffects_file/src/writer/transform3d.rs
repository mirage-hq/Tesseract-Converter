//! Fresh native layer 3D Transform records built only from current FX values.
//!
//! This module replaces the Transform group on a layer emitted by this writer;
//! it never accepts or replays an imported layer record.

use crate::{rifx::Chunk, schema::layer_records::LayerRecord};

use super::{AepWriteError, KeyframeEasing, NumericTrack, views};
use views::ValueKind;

/// Static native layer Transform values for the 3D/separated-position sidecar.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeTransform3d {
    /// Whether the native layer switch enables AE's 3D transform/depth semantics.
    pub is_three_d: bool,
    pub anchor: [f64; 3],
    pub position: [f64; 3],
    /// Native scale ratios (`1.0` is 100%). FX percentages are lowered first.
    pub scale: [f64; 3],
    pub orientation: [f64; 3],
    pub rotation_x: f64,
    pub rotation_y: f64,
    pub rotation_z: f64,
    /// Native opacity ratio in `0.0..=1.0`.
    pub opacity: f64,
}

/// Authored native tracks belonging to one sidecar Transform group.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Transform3dAnimations {
    pub anchor: Option<NumericTrack>,
    pub position: Option<NumericTrack>,
    /// Independent scalar X/Y/Z followers under a separated Position leader.
    pub position_separated: Option<[Option<NumericTrack>; 3]>,
    pub scale: Option<NumericTrack>,
    pub orientation: Option<NumericTrack>,
    pub rotation_x: Option<NumericTrack>,
    pub rotation_y: Option<NumericTrack>,
    pub rotation_z: Option<NumericTrack>,
    pub opacity: Option<NumericTrack>,
}

/// Replaces the generated Transform group and writes its native 3D switch.
///
/// The caller must pass a freshly generated timeline layer. This helper does
/// not preserve unknown records and must never be used as an imported-record
/// patcher.
#[cfg(test)]
pub(crate) fn replace_fresh_layer_transform(
    layer: &mut Chunk,
    transform: &NativeTransform3d,
    animations: &Transform3dAnimations,
) -> Result<(), AepWriteError> {
    replace_fresh_layer_transform_with_clock(
        layer,
        transform,
        animations,
        super::keyframes::PropertyClock::DEFAULT,
    )
}

pub(crate) fn replace_fresh_layer_transform_with_clock(
    layer: &mut Chunk,
    transform: &NativeTransform3d,
    animations: &Transform3dAnimations,
    clock: super::keyframes::PropertyClock,
) -> Result<(), AepWriteError> {
    validate(transform)?;
    let replacement = transform_group_with_clock(transform, animations, clock)?;
    let children = layer
        .children_mut()
        .ok_or(AepWriteError::Invalid("native 3D layer is not a LIST"))?;

    let record = children
        .iter_mut()
        .find(|child| child.id() == *b"ldta")
        .ok_or(AepWriteError::Invalid("native 3D layer has no ldta"))?;
    let updated = LayerRecord::decode(
        record
            .data_payload()
            .ok_or(AepWriteError::Invalid("invalid native 3D ldta"))?,
    )?
    .with_three_d_layer(transform.is_three_d)?;
    *record = Chunk::data(*b"ldta", updated.encode())?;

    let properties = children
        .iter_mut()
        .find(|child| child.list_kind() == Some(*b"tdgp"))
        .ok_or(AepWriteError::Invalid(
            "native 3D layer has no root property group",
        ))?;
    replace_named_child(properties, "ADBE Transform Group", replacement)
}

fn validate(transform: &NativeTransform3d) -> Result<(), AepWriteError> {
    if transform
        .anchor
        .iter()
        .chain(&transform.position)
        .chain(&transform.scale)
        .chain(&transform.orientation)
        .chain(&[
            transform.rotation_x,
            transform.rotation_y,
            transform.rotation_z,
            transform.opacity,
        ])
        .any(|value| !value.is_finite())
        || !(0.0..=1.0).contains(&transform.opacity)
    {
        return Err(AepWriteError::Invalid("invalid native 3D Transform value"));
    }
    Ok(())
}

#[cfg(test)]
fn transform_group(
    value: &NativeTransform3d,
    animations: &Transform3dAnimations,
) -> Result<Chunk, AepWriteError> {
    transform_group_with_clock(value, animations, super::keyframes::PropertyClock::DEFAULT)
}

fn transform_group_with_clock(
    value: &NativeTransform3d,
    animations: &Transform3dAnimations,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    if animations.position.is_some() && animations.position_separated.is_some() {
        return Err(AepWriteError::Invalid(
            "Position cannot be both combined and separated",
        ));
    }
    let mut entries = vec![(
        "ADBE Anchor Point",
        views::property_with_clock(
            ValueKind::Spatial,
            &value.anchor,
            None,
            animations.anchor.as_ref(),
            clock,
        )?,
    )];
    if let Some(followers) = &animations.position_separated {
        entries.push((
            "ADBE Position",
            property_with_flags(
                ValueKind::Spatial,
                &value.position,
                None,
                None,
                0x0000_0803,
                clock,
            )?,
        ));
        for (name, axis, animation) in [
            ("ADBE Position_0", 0, &followers[0]),
            ("ADBE Position_1", 1, &followers[1]),
            ("ADBE Position_2", 2, &followers[2]),
        ] {
            entries.push((
                name,
                property_with_flags(
                    ValueKind::Scalar,
                    &[value.position[axis]],
                    Some((0.0, 0.0)),
                    animation.as_ref(),
                    1,
                    clock,
                )?,
            ));
        }
    } else {
        entries.push((
            "ADBE Position",
            views::property_with_clock(
                ValueKind::Spatial,
                &value.position,
                None,
                animations.position.as_ref(),
                clock,
            )?,
        ));
    }
    entries.extend([
        (
            "ADBE Scale",
            views::property_with_clock(
                ValueKind::Scale,
                &value.scale,
                Some((0.0, 0.0)),
                animations.scale.as_ref(),
                clock,
            )?,
        ),
        (
            "ADBE Orientation",
            orientation_property(value.orientation, animations.orientation.as_ref(), clock)?,
        ),
        (
            "ADBE Rotate X",
            views::property_with_clock(
                ValueKind::Angle,
                &[value.rotation_x],
                None,
                animations.rotation_x.as_ref(),
                clock,
            )?,
        ),
        (
            "ADBE Rotate Y",
            views::property_with_clock(
                ValueKind::Angle,
                &[value.rotation_y],
                None,
                animations.rotation_y.as_ref(),
                clock,
            )?,
        ),
        (
            "ADBE Rotate Z",
            views::property_with_clock(
                ValueKind::Angle,
                &[value.rotation_z],
                None,
                animations.rotation_z.as_ref(),
                clock,
            )?,
        ),
        (
            "ADBE Opacity",
            views::property_with_clock(
                ValueKind::Scalar,
                &[value.opacity],
                Some((0.0, 100.0)),
                animations.opacity.as_ref(),
                clock,
            )?,
        ),
    ]);
    Ok(views::group(1, "-_0_/-", entries)?)
}

fn property_with_flags(
    kind: ValueKind,
    values: &[f64],
    bounds: Option<(f64, f64)>,
    animation: Option<&NumericTrack>,
    flags: u32,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let mut property = views::property_with_clock(kind, values, bounds, animation, clock)?;
    let children = property
        .children_mut()
        .ok_or(AepWriteError::Invalid("native property is not a LIST"))?;
    let mut matches = children.iter_mut().filter(|child| child.id() == *b"tdsb");
    let target = matches
        .next()
        .ok_or(AepWriteError::Invalid("native property has no tdsb"))?;
    if matches.next().is_some() {
        return Err(AepWriteError::Invalid("native property has duplicate tdsb"));
    }
    *target = Chunk::data(*b"tdsb", flags.to_be_bytes())?;
    Ok(property)
}

fn orientation_property(
    value: [f64; 3],
    animation: Option<&NumericTrack>,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let mut property =
        views::property_with_clock(ValueKind::Orientation, &value, None, None, clock)?;
    let children = property
        .children_mut()
        .ok_or(AepWriteError::Invalid("Orientation property is not a LIST"))?;

    if let Some(track) = animation {
        validate_orientation_track(track)?;
        let descriptor = children
            .iter_mut()
            .find(|child| child.id() == *b"tdb4")
            .ok_or(AepWriteError::Invalid("Orientation has no descriptor"))?;
        let bytes: [u8; 124] = descriptor
            .data_payload()
            .ok_or(AepWriteError::Invalid("invalid Orientation descriptor"))?
            .try_into()
            .map_err(|_| AepWriteError::Invalid("invalid Orientation descriptor length"))?;
        *descriptor = Chunk::data(
            *b"tdb4",
            super::keyframes::animated_descriptor(bytes, false),
        )?;
        children.retain(|child| child.id() != *b"cdat");
        children.push(orientation_key_list(track, clock)?);
    } else {
        let static_values: Vec<u8> = value.into_iter().flat_map(f64::to_le_bytes).collect();
        let stored = children
            .iter_mut()
            .find(|child| child.id() == *b"cdat")
            .ok_or(AepWriteError::Invalid("Orientation has no static value"))?;
        *stored = Chunk::data(*b"cdat", static_values)?;
    }

    let values = animation.map_or_else(
        || vec![orientation_value(value)],
        |track| {
            track
                .keys
                .iter()
                .map(|key| orientation_value([key.values[0], key.values[1], key.values[2]]))
                .collect()
        },
    );
    Ok(Chunk::list(
        *b"otst",
        vec![property, Chunk::list(*b"otky", values)],
    ))
}

fn validate_orientation_track(track: &NumericTrack) -> Result<(), AepWriteError> {
    if track.keys.is_empty()
        || track.keys.iter().any(|key| {
            key.values.len() != 3
                || key.easing.len() != 3
                || !key.spatial_in.is_empty()
                || !key.spatial_out.is_empty()
                || key.values.iter().any(|value| !value.is_finite())
                || key.easing.iter().any(|easing| {
                    matches!(easing, KeyframeEasing::CubicBezier { .. }) || *easing != key.easing[0]
                })
        })
    {
        return Err(AepWriteError::Invalid(
            "ADBE Orientation otst/otky supports finite Hold/Linear shared-ease keys only; cubic quaternion temporal-speed encoding is not established",
        ));
    }
    Ok(())
}

fn orientation_key_list(
    track: &NumericTrack,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let count = u16::try_from(track.keys.len())
        .map_err(|_| AepWriteError::Invalid("too many Orientation keys"))?;
    let blocks = u32::from(count).div_ceil(4).max(1);
    let mut header = vec![0_u8; 52];
    header[..10].copy_from_slice(&[0, 0xd0, 0x0b, 0xee, 0, 0, 0, 0, 0, 0]);
    header[10..12].copy_from_slice(&count.to_be_bytes());
    header[12..16].copy_from_slice(&blocks.to_be_bytes());
    header[18..20].copy_from_slice(&80_u16.to_be_bytes());
    header[23] = 4;
    header[24..28].copy_from_slice(&1_u32.to_be_bytes());
    header[28..32].copy_from_slice(&(blocks * 4).to_be_bytes());

    let mut in_interpolation = vec![1_u8; track.keys.len()];
    let mut out_interpolation = vec![1_u8; track.keys.len()];
    for index in 1..track.keys.len() {
        if track.keys[index].easing[0] == KeyframeEasing::Hold {
            out_interpolation[index - 1] = 3;
            in_interpolation[index] = 3;
        }
    }
    let mut data = Vec::with_capacity(usize::from(count) * 80);
    let mut previous = None;
    let mut previous_units = None;
    for (index, key) in track.keys.iter().enumerate() {
        if previous.is_some_and(|time| time >= key.time_millis) {
            return Err(AepWriteError::Invalid(
                "Orientation key times must be strictly increasing",
            ));
        }
        previous = Some(key.time_millis);
        let units = clock.units(key.time_millis)?;
        if previous_units.is_some_and(|previous| previous >= units) {
            return Err(AepWriteError::Invalid(
                "Orientation key times collide in native clock",
            ));
        }
        previous_units = Some(units);
        data.extend_from_slice(&units.to_be_bytes());
        data.extend_from_slice(&[in_interpolation[index], out_interpolation[index], 0, 0]);
        data.extend_from_slice(&[0; 16]);
        data.extend_from_slice(&[0; 32]);
        data.extend_from_slice(&[0; 24]);
    }
    Ok(Chunk::list(
        *b"list",
        vec![Chunk::data(*b"lhd3", header)?, Chunk::data(*b"ldat", data)?],
    ))
}

fn orientation_value(value: [f64; 3]) -> Chunk {
    let bytes: Vec<u8> = value.into_iter().flat_map(f64::to_be_bytes).collect();
    Chunk::data(*b"otda", bytes).expect("otda is a fixed non-LIST identifier")
}

fn replace_named_child(
    group: &mut Chunk,
    name: &str,
    replacement: Chunk,
) -> Result<(), AepWriteError> {
    let children = group
        .children_mut()
        .ok_or(AepWriteError::Invalid("root property group is not a LIST"))?;
    let index = children
        .iter()
        .position(|child| child.id() == *b"tdmn" && match_name(child) == Some(name))
        .ok_or(AepWriteError::Invalid(
            "fresh layer has no generated Transform group",
        ))?;
    let child = children
        .get_mut(index + 1)
        .ok_or(AepWriteError::Invalid("Transform match name has no value"))?;
    *child = replacement;
    Ok(())
}

fn match_name(chunk: &Chunk) -> Option<&str> {
    let bytes = chunk.data_payload()?;
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end]).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_thirty_fps_orientation_has_matching_descriptor_and_hold_key_units() {
        let clock = super::super::keyframes::PropertyClock::for_rate(
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        let track = super::NumericTrack {
            keys: [250, 1100]
                .into_iter()
                .map(|time_millis| super::super::keyframes::Keyframe {
                    time_millis,
                    values: vec![10.0, 5.0, 0.0],
                    easing: vec![super::super::keyframes::Easing::Hold; 3],
                    spatial_in: vec![],
                    spatial_out: vec![],
                })
                .collect(),
        };
        let property = super::orientation_property([0.0; 3], Some(&track), clock).unwrap();
        let container = property.children().unwrap()[0].children().unwrap();
        let descriptor = crate::properties::data(container, *b"tdb4").unwrap();
        assert_eq!(
            u32::from_be_bytes(descriptor[12..16].try_into().unwrap()),
            30_720
        );
        let list = crate::properties::unique_list(container, *b"list").unwrap();
        let keys = crate::properties::data(list, *b"ldat").unwrap();
        assert_eq!(i32::from_be_bytes(keys[..4].try_into().unwrap()), 7680);
        assert_eq!(i32::from_be_bytes(keys[80..84].try_into().unwrap()), 33792);
    }
    use crate::properties::{data, read_numeric, runs, unique_list};

    use super::*;

    fn native_transform(position: [f64; 3]) -> NativeTransform3d {
        NativeTransform3d {
            is_three_d: true,
            anchor: [0.0; 3],
            position,
            scale: [1.0; 3],
            orientation: [0.0; 3],
            rotation_x: 0.0,
            rotation_y: 0.0,
            rotation_z: 0.0,
            opacity: 1.0,
        }
    }

    fn track(values: &[(i64, f64)]) -> NumericTrack {
        NumericTrack {
            keys: values
                .iter()
                .map(|(time_millis, value)| super::super::NumericKeyframe {
                    time_millis: *time_millis,
                    values: vec![*value],
                    easing: vec![KeyframeEasing::Linear],
                    spatial_in: Vec::new(),
                    spatial_out: Vec::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn replacement_preserves_the_planar_layer_switch() {
        let project = crate::structure::read_project(include_bytes!(
            "../../tests/fixtures/properties/transform_unseparated.aep"
        ))
        .unwrap();
        let crate::structure::ItemKind::Composition(composition) = &project.item(1).unwrap().kind
        else {
            panic!("fresh composition")
        };
        let mut layer = Chunk::list(*b"Layr", composition.layers[0].content.clone());
        let mut transform = native_transform([10.0, 30.0, 0.0]);
        transform.is_three_d = false;
        let animations = Transform3dAnimations {
            position_separated: Some([
                Some(track(&[(0, 10.0), (1_000, 20.0)])),
                Some(track(&[(0, 30.0), (400, 40.0), (1_200, 50.0)])),
                None,
            ]),
            ..Default::default()
        };

        replace_fresh_layer_transform(&mut layer, &transform, &animations).unwrap();

        let record = layer
            .children()
            .unwrap()
            .iter()
            .find(|child| child.id() == *b"ldta")
            .unwrap();
        let record = LayerRecord::decode(record.data_payload().unwrap()).unwrap();
        assert!(!record.flags().three_d_layer);
    }

    #[test]
    fn separated_position_uses_native_leader_and_scalar_follower_flags() {
        let animations = Transform3dAnimations {
            position_separated: Some([
                Some(track(&[(0, 10.0), (1_000, 20.0)])),
                Some(track(&[(0, 30.0), (400, 40.0), (1_200, 50.0)])),
                None,
            ]),
            ..Default::default()
        };
        let group = transform_group(&native_transform([10.0, 30.0, 42.0]), &animations).unwrap();
        let properties = runs(group.children().unwrap()).unwrap();
        let expected = [
            ("ADBE Position", [0, 0, 8, 3], true, 0),
            ("ADBE Position_0", [0, 0, 0, 1], false, 2),
            ("ADBE Position_1", [0, 0, 0, 1], false, 3),
            ("ADBE Position_2", [0, 0, 0, 1], false, 0),
        ];
        for (name, flags, separated, key_count) in expected {
            let run = properties
                .iter()
                .find_map(|(candidate, run)| (*candidate == name).then_some(*run))
                .unwrap();
            let property = unique_list(run, *b"tdbs").unwrap();
            assert_eq!(data(property, *b"tdsb").unwrap(), flags);
            let numeric = read_numeric(property).unwrap();
            assert_eq!(numeric.dimensions_separated, separated);
            assert_eq!(numeric.keyframes.len(), key_count);
        }
    }

    #[test]
    fn repeated_separated_position_writes_use_current_values_not_source_bytes() {
        let animations = Transform3dAnimations {
            position_separated: Some([Some(track(&[(0, 15.0), (500, 25.0)])), None, None]),
            ..Default::default()
        };
        let first = transform_group(&native_transform([15.0, 35.0, 45.0]), &animations).unwrap();
        let edited = transform_group(&native_transform([15.0, 35.0, 145.0]), &animations).unwrap();
        assert_ne!(first, edited);
        assert_eq!(
            edited,
            transform_group(&native_transform([15.0, 35.0, 145.0]), &animations).unwrap()
        );
        let properties = runs(edited.children().unwrap()).unwrap();
        let z = properties
            .iter()
            .find_map(|(name, run)| (*name == "ADBE Position_2").then_some(*run))
            .unwrap();
        assert_eq!(
            read_numeric(unique_list(z, *b"tdbs").unwrap())
                .unwrap()
                .values,
            vec![145.0]
        );
    }
}
