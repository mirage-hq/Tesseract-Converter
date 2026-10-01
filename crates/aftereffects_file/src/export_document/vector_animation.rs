//! Native vector-group animation lowering from current editable FX tracks.

use fx_schema::{LayerId, Position, PropType, Transform, animator::AnimationGraphEntry};

use crate::writer::{NumericTrack, VectorGroupAnimations};

use super::{NativeTrack, is_transform_property, paired_track, scalar_track, track};

#[cfg(test)]
#[path = "vector_animation/tests.rs"]
mod tests;

/// Even a zero-valued static skew needs a native vector Transform when its
/// source owner has an authored Skew or Skew Axis track. A layer Transform has
/// no matching native properties, so it must never consume those keys.
pub(super) fn has_skew_tracks(entries: &[AnimationGraphEntry], owner: LayerId) -> bool {
    entries.iter().any(|entry| {
        entry.target.as_property().is_some_and(|property| {
            property.layer_id() == owner
                && matches!(
                    property.property_type(),
                    PropType::Skew | PropType::SkewAxis
                )
        })
    })
}

/// Rebuild layer-transform keys in native vector units for a skewed paint
/// program. The enclosing layer can also own geometry, paint and modifier
/// keys, which belong to separate native operators, not this Transform.
/// The caller must first validate those targets via its normal partitioner.
pub(super) fn program_transform_animations(
    entries: &[AnimationGraphEntry],
    owner: LayerId,
    base: &Transform,
) -> Result<VectorGroupAnimations, &'static str> {
    let mut seen = Vec::new();
    let transform_entries = entries
        .iter()
        .filter(|entry| {
            entry.target.as_property().is_some_and(|property| {
                property.layer_id() == owner && is_transform_property(property.property_type())
            })
        })
        .map(|entry| {
            let property = entry
                .target
                .as_property()
                .expect("filtered property target");
            if seen.contains(&property.property_type()) {
                return Err("Duplicate source-owned Transform animator cannot be written natively");
            }
            seen.push(property.property_type());
            Ok(entry.clone())
        })
        .collect::<Result<Vec<_>, _>>()?;
    group_animations(&transform_entries, owner, base)
}

/// Lowers animation owned by one FX group or Boolean operand into the matching
/// native vector Transform operator. Callers must attach the result to that
/// layer's `VectorContent::AnimatedGroup`; retargeting it to a leaf changes the
/// transform order.
pub(super) fn group_animations(
    entries: &[AnimationGraphEntry],
    layer_id: LayerId,
    base: &Transform,
) -> Result<VectorGroupAnimations, &'static str> {
    const ALLOWED: [PropType; 9] = [
        PropType::AnchorPointX,
        PropType::AnchorPointY,
        PropType::PositionX,
        PropType::PositionY,
        PropType::ScaleX,
        PropType::ScaleY,
        PropType::Rotation,
        PropType::Skew,
        PropType::SkewAxis,
    ];
    if entries
        .iter()
        .filter(|entry| entry.target.layer_id() == Some(layer_id))
        .any(|entry| {
            entry.target.as_property().is_none_or(|property| {
                property.property_type() != PropType::Opacity
                    && !ALLOWED.contains(&property.property_type())
            })
        })
    {
        return Err("Vector Group has animator targets outside native Transform support");
    }
    let Position::TwoD(position) = base.position else {
        return Err("Animated vector Group 3D Position needs native 3D child records");
    };
    if base.rotation_x != 0.0 || base.rotation_y != 0.0 || base.orientation != [0.0; 3] {
        return Err("Animated vector Group 3D Rotation/Orientation needs native 3D child records");
    }

    Ok(VectorGroupAnimations {
        anchor: pair_track(
            track(entries, layer_id, PropType::AnchorPointX)?,
            track(entries, layer_id, PropType::AnchorPointY)?,
            base.anchor_point,
        )?,
        position: pair_track(
            track(entries, layer_id, PropType::PositionX)?,
            track(entries, layer_id, PropType::PositionY)?,
            position,
        )?,
        scale: pair_track(
            track(entries, layer_id, PropType::ScaleX)?,
            track(entries, layer_id, PropType::ScaleY)?,
            base.scale,
        )?,
        rotation: scalar_track(track(entries, layer_id, PropType::Rotation)?, 1.0)?,
        skew: scalar_track(track(entries, layer_id, PropType::Skew)?, 1.0)?,
        skew_axis: scalar_track(track(entries, layer_id, PropType::SkewAxis)?, 1.0)?,
        opacity: scalar_track(track(entries, layer_id, PropType::Opacity)?, 1.0)?,
    })
}

/// Vector Transform pairs have two native components, unlike the three-slot
/// layer Transform tracks produced by the shared paired-track lowerer.
fn pair_track(
    x: Option<NativeTrack<'_>>,
    y: Option<NativeTrack<'_>>,
    base: [f64; 2],
) -> Result<Option<NumericTrack>, &'static str> {
    let mut lowered = paired_track(x, y, base, 1.0, false)?;
    if let Some(track) = &mut lowered {
        for key in &mut track.keys {
            key.values.truncate(2);
            key.easing.truncate(2);
        }
    }
    Ok(lowered)
}
