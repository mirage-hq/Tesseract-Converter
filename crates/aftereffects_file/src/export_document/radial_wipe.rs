//! Recognize an edited FX half-plane by geometry, not importer labels or IDs.

#[cfg(test)]
mod proof_tests;
#[cfg(test)]
mod tests;
use fx_schema::{
    GroupLayer, LayerData, LayerId, Position, PropType, PropertyValue, ShapePathCommand, Time,
    Transform, animator::AnimationGraphEntry, layer::MaskMode,
};

use super::{NativeTrack, scalar_track, track};
use crate::writer::effects::{self, NativeEffect};

const MATCH_NAME: &str = "ADBE Radial Wipe";

pub(super) struct Candidate {
    pub guide: LayerId,
    pub center: [f64; 2],
    pub extent: f64,
    pub angle: f64,
    pub angle_track: Option<crate::writer::NumericTrack>,
}

fn plain_transform(t: &Transform) -> bool {
    t.anchor_point == [0.0; 2]
        && t.scale == [100.0; 2]
        && t.skew == 0.0
        && t.skew_axis == 0.0
        && t.rotation_x == 0.0
        && t.rotation_y == 0.0
        && t.orientation == [0.0; 3]
        && t.opacity.value() == 100.0
}

fn extent(commands: &[ShapePathCommand]) -> Option<f64> {
    let [
        ShapePathCommand::MoveTo {
            x: left,
            y: top,
            mirror: None,
            corner_radius: None,
        },
        ShapePathCommand::LineTo {
            x: right,
            y: top_right,
            mirror: None,
            corner_radius: None,
        },
        ShapePathCommand::LineTo {
            x: bottom_right,
            y: bottom,
            mirror: None,
            corner_radius: None,
        },
        ShapePathCommand::LineTo {
            x: bottom_left,
            y: bottom_left_y,
            mirror: None,
            corner_radius: None,
        },
        ShapePathCommand::Close,
    ] = commands
    else {
        return None;
    };
    let d = -*left;
    (d.is_finite()
        && d > 0.0
        && *top == -d
        && *right == 0.0
        && *top_right == -d
        && *bottom_right == 0.0
        && *bottom == d
        && *bottom_left == -d
        && *bottom_left_y == d)
        .then_some(d)
}

fn child_masks_reference(layers: &[fx_schema::Layer], guide: LayerId) -> bool {
    layers.iter().any(|layer| {
        super::mask_and_transform(layer)
            .is_some_and(|(_, masks, _)| masks.iter().any(|mask| mask.layer == Some(guide)))
            || layer
                .child_layers()
                .is_some_and(|children| child_masks_reference(children, guide))
    })
}

/// A candidate is deliberately narrower than a generic editable PathMask.
/// Rejected shapes continue through the ordinary mask lowering unchanged.
pub(super) fn recognize(
    group: &GroupLayer,
    dynamics: &[AnimationGraphEntry],
    composition_end: Time,
) -> Result<Option<Candidate>, &'static str> {
    let [mask] = group.masks.as_slice() else {
        return Ok(None);
    };
    let Some(guide_id) = mask.layer else {
        return Ok(None);
    };
    let Some(guide) = group.layers.iter().find(|child| child.id() == guide_id) else {
        return Ok(None);
    };
    let LayerData::Shape(shape) = guide.data() else {
        return Ok(None);
    };
    let Some(d) = extent(&shape.shape.path.commands) else {
        return Ok(None);
    };
    if mask.mode != MaskMode::Add
        || mask.inverted
        || mask.legacy_path.is_some()
        || mask.feather != [0.0; 2]
        || mask.expansion != 0.0
        || mask.opacity.value() != 1.0
        || guide.parent_id() != Some(group.id)
        || guide.active_range().start != Time::ZERO
        || guide.active_range().end() < composition_end
        || !shape.shape.fills.is_empty()
        || !shape.shape.strokes.is_empty()
        || shape.shape.ellipse.is_some()
        || shape.shape.poly_star.is_some()
        || shape.shape.round_corners.is_some()
        || shape.shape.offset_paths.is_some()
        || shape.shape.trim.is_some()
        || !shape.effects.is_empty()
        || !shape.masks.is_empty()
        || shape.track_matte.is_some()
        || shape.is_hidden
        || shape.motion_blur
        || !plain_transform(&shape.transform)
        || !matches!(shape.transform.position, Position::TwoD(_))
        || !shape.transform.rotation.is_finite()
        || dynamics
            .iter()
            .any(|entry| entry.target.fx_item_id() == Some(mask.id))
    {
        return Err("half-plane mask has unsupported edited controls or guide lifetime");
    }
    let Position::TwoD(center) = shape.transform.position else {
        return Err("half-plane center is not two-dimensional");
    };
    if center.iter().any(|v| !v.is_finite()) {
        return Err("half-plane center is not finite");
    }
    // Even disabled, dependent, or non-Rotation entries can change geometry or
    // export visibility. Do not consume a guide whose full animation is unknown.
    if dynamics.iter().any(|entry| {
        entry.target.layer_id() == Some(guide_id)
            && entry
                .target
                .as_property()
                .is_none_or(|property| property.property_type() != PropType::Rotation)
    }) {
        return Err("half-plane guide has unsupported animation");
    }
    let rotation_entries = dynamics
        .iter()
        .filter(|entry| {
            entry.target.as_property().is_some_and(|target| {
                target.layer_id() == guide_id && target.property_type() == PropType::Rotation
            })
        })
        .count();
    if rotation_entries > 1 {
        return Err("half-plane guide has ambiguous Rotation animation");
    }
    if child_masks_reference(&group.layers, guide_id) {
        return Err("half-plane guide is shared by another mask");
    }
    let angle_source = track(dynamics, guide_id, PropType::Rotation)?;
    let mut angle = shape.transform.rotation;
    if let Some(source) = &angle_source {
        angle = match source {
            NativeTrack::Constant(PropertyValue::Float(value)) if value.is_finite() => *value,
            NativeTrack::Keyframes(keys) => {
                let first = keys
                    .keyframes()
                    .first()
                    .ok_or("empty half-plane Rotation keys")?;
                super::float_value(first.value())?
            }
            _ => return Err("half-plane Rotation is not an editable finite scalar"),
        };
    }
    let angle_track = scalar_track(angle_source, 1.0)?;
    if angle_track.as_ref().is_some_and(|keys| {
        keys.keys.is_empty()
            || keys.keys.iter().any(|key| {
                key.time_millis < 0
                    || u64::try_from(key.time_millis)
                        .map_or(true, |time| time > composition_end.as_millis())
            })
    }) {
        return Err("half-plane Rotation keys exceed the identity source clock");
    }
    Ok(Some(Candidate {
        guide: guide_id,
        center,
        extent: d,
        angle,
        angle_track,
    }))
}

impl Candidate {
    /// The finite rectangle must contain every source pixel under every rotation.
    /// For a rotating half-plane this is the radius of the farthest canvas corner.
    pub(super) fn covers(&self, size: [u32; 2], origin: [f64; 2]) -> bool {
        let far = [0, 1].map(|axis| {
            (origin[axis] - self.center[axis])
                .abs()
                .max((origin[axis] + f64::from(size[axis]) - self.center[axis]).abs())
        });
        far[0].hypot(far[1]).is_finite()
            && self.extent > far[0].hypot(far[1])
            && size.iter().all(|size| *size > 0)
    }

    pub(super) fn native(
        &self,
        size: [u32; 2],
        origin: [f64; 2],
    ) -> Result<NativeEffect, &'static str> {
        let mut effect = effects::new_effect(MATCH_NAME, true, size.map(f64::from))
            .map_err(|_| "canonical Radial Wipe definition is unavailable")?;
        for (suffix, values) in [
            ("0001", vec![50.0]),
            ("0002", vec![self.angle]),
            (
                "0003",
                vec![self.center[0] - origin[0], self.center[1] - origin[1]],
            ),
            ("0004", vec![1.0]),
            ("0005", vec![0.0]),
        ] {
            let name = format!("{MATCH_NAME}-{suffix}");
            let property = effect
                .properties
                .iter_mut()
                .find(|property| property.match_name == name)
                .ok_or("canonical Radial Wipe control is missing")?;
            property.values = values;
            if suffix == "0002" {
                property.animation = self.angle_track.clone();
            }
        }
        Ok(effect)
    }
}
