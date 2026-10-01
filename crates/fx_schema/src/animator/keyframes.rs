use std::{
    collections::BTreeSet,
    io::{self, Write},
};

use crate::time::TimeOffset;
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use ts_rs::TS;

use crate::{
    property::{PropType, PropertyTarget, PropertyValue},
    ShapePath, ShapePathCommand,
};

/// Maximum UTF-8 byte length of one caller-assigned keyframe identifier.
pub const MAX_KEYFRAME_ID_BYTES: usize = 128;

/// Largest signed millisecond coordinate represented exactly by JSON clients.
const MAX_EXACT_KEYFRAME_TIME_MILLIS: u64 = (1_u64 << 53) - 1;

crate::define_property_keyframe_schema!();

impl KeyframeId {
    /// Creates an opaque caller-assigned keyframe identity.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the opaque identifier string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PropertyKeyframeEasing {
    fn validate(self, keyframe_id: &KeyframeId) -> Result<(), PropertyKeyframeError> {
        match self {
            Self::Hold | Self::Linear => Ok(()),
            Self::CubicBezier { x1, y1, x2, y2 }
                if x1.is_finite()
                    && y1.is_finite()
                    && x2.is_finite()
                    && y2.is_finite()
                    && (0.0..=1.0).contains(&x1)
                    && (0.0..=1.0).contains(&x2) =>
            {
                Ok(())
            }
            Self::CubicBezier { .. } => Err(PropertyKeyframeError::InvalidEasing {
                keyframe_id: keyframe_id.0.clone(),
            }),
        }
    }
}

impl PropertyKeyframe {
    /// Creates one authored keyframe.
    #[must_use]
    pub const fn new(
        id: KeyframeId,
        layer_time: TimeOffset,
        value: PropertyValue,
        easing: PropertyKeyframeEasing,
    ) -> Self {
        Self {
            id,
            layer_time,
            value,
            easing,
            spatial_in_tangent: None,
            spatial_out_tangent: None,
        }
    }

    /// Sets the scalar components of this key's two-dimensional spatial
    /// tangents.
    ///
    /// Each value is an offset from the key's position, in composition pixels.
    /// The matching `PositionX` / `PositionY` key supplies the other component.
    #[must_use]
    pub const fn with_spatial_tangents(
        mut self,
        spatial_in_tangent: Option<f64>,
        spatial_out_tangent: Option<f64>,
    ) -> Self {
        self.spatial_in_tangent = spatial_in_tangent;
        self.spatial_out_tangent = spatial_out_tangent;
        self
    }

    /// Stable identity of this keyframe.
    #[must_use]
    pub fn id(&self) -> &KeyframeId {
        &self.id
    }

    /// Time on the owning layer's local clock.
    #[must_use]
    pub const fn layer_time(&self) -> TimeOffset {
        self.layer_time
    }

    /// Authored value at this keyframe.
    #[must_use]
    pub const fn value(&self) -> &PropertyValue {
        &self.value
    }

    /// Incoming easing applied from the previous keyframe.
    #[must_use]
    pub const fn easing(&self) -> PropertyKeyframeEasing {
        self.easing
    }

    /// This axis's incoming spatial-control offset, in composition pixels.
    #[must_use]
    pub const fn spatial_in_tangent(&self) -> Option<f64> {
        self.spatial_in_tangent
    }

    /// This axis's outgoing spatial-control offset, in composition pixels.
    #[must_use]
    pub const fn spatial_out_tangent(&self) -> Option<f64> {
        self.spatial_out_tangent
    }

    /// Returns this keyframe's serialized JSON byte size without allocating its payload.
    #[doc(hidden)]
    pub fn serialized_json_size(&self) -> Result<usize, PropertyKeyframeError> {
        serialized_json_size(self)
    }
}

impl<'de> Deserialize<'de> for PropertyKeyframeTrack {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        macro_rules! stored_track_reader {
            ($(#[$attr:meta])* struct $name:ident {
                $field:ident: $type:ty,
            }) => {
                #[derive(Deserialize)]
                $(#[$attr])*
                struct $name {
                    $field: $type,
                }
            };
        }
        crate::define_keyframe_track_reader_schema!(stored_track_reader,);

        let track = Self {
            keyframes: Wire::deserialize(deserializer)?.keyframes,
        };
        track.validate_shape().map_err(D::Error::custom)?;
        Ok(track)
    }
}

impl PropertyKeyframeTrack {
    pub(super) fn from_wire(
        keyframes: Vec<PropertyKeyframe>,
    ) -> Result<Self, PropertyKeyframeError> {
        let track = Self { keyframes };
        track.validate_shape()?;
        Ok(track)
    }

    /// Builds a canonical track, sorting keys by their layer-local time before
    /// validating identity, value, and easing invariants.
    pub fn new(mut keyframes: Vec<PropertyKeyframe>) -> Result<Self, PropertyKeyframeError> {
        keyframes.sort_by_key(PropertyKeyframe::layer_time);
        let track = Self { keyframes };
        track.validate_shape()?;
        Ok(track)
    }

    /// Returns the canonical time-sorted keys.
    #[must_use]
    pub fn keyframes(&self) -> &[PropertyKeyframe] {
        &self.keyframes
    }

    /// Whether any key carries an incoming or outgoing spatial tangent.
    #[must_use]
    pub fn has_spatial_tangents(&self) -> bool {
        self.keyframes.iter().any(|keyframe| {
            keyframe.spatial_in_tangent.is_some() || keyframe.spatial_out_tangent.is_some()
        })
    }

    /// Validates two scalar tracks as one editable two-dimensional position
    /// path.
    ///
    /// The tracks may otherwise use independent key times and easing. A
    /// spatially controlled segment must have the same adjacent endpoints and
    /// temporal easing on both axes, and every tangent side must provide both
    /// scalar components.
    pub fn validate_position_pair(
        position_x: &Self,
        position_y: &Self,
    ) -> Result<(), PropertyKeyframeError> {
        position_x.validate_shape()?;
        position_y.validate_shape()?;
        validate_position_axis_values(position_x, "x")?;
        validate_position_axis_values(position_y, "y")?;
        validate_spatial_endpoints(position_x, "x")?;
        validate_spatial_endpoints(position_y, "y")?;
        validate_spatial_segments(position_x, position_y)?;
        validate_spatial_segments(position_y, position_x)
    }

    /// Validates that this canonical track can drive the requested property target.
    #[doc(hidden)]
    pub fn validate_for_target(
        &self,
        target: &PropertyTarget,
    ) -> Result<(), PropertyKeyframeError> {
        let Some(keyframe) = self.keyframes.first() else {
            return Err(PropertyKeyframeError::EmptyTrack);
        };
        if self.has_spatial_tangents()
            && !target.as_property().is_some_and(|property| {
                matches!(
                    property.property_type(),
                    PropType::PositionX | PropType::PositionY
                )
            })
        {
            return Err(PropertyKeyframeError::SpatialTangentsUnsupportedByTarget {
                target: target.to_string(),
            });
        }
        let Some(actual) = script_value_type_name(&keyframe.value) else {
            return Err(PropertyKeyframeError::UnsupportedValue {
                keyframe_id: keyframe.id.0.clone(),
                value_kind: property_value_kind_label(&keyframe.value),
            });
        };
        // Resource-backed string properties reject JavaScript animators because
        // their values must be enumerable before playback. Native keyframes are
        // already finite and therefore validate as direct string values.
        let normalized = match target.as_property() {
            Some(property)
                if property.property_type().finite_range_value_kind().is_some()
                    && matches!(keyframe.value, PropertyValue::String(_)) =>
            {
                keyframe.value.clone()
            }
            _ => property_value_from_typed(target, &keyframe.value, actual).map_err(|error| {
                PropertyKeyframeError::ValueUnsupportedByTarget {
                    keyframe_id: keyframe.id.0.clone(),
                    target: target.to_string(),
                    reason: error.to_string(),
                }
            })?,
        };
        if normalized != keyframe.value {
            return Err(PropertyKeyframeError::ValueUnsupportedByTarget {
                keyframe_id: keyframe.id.0.clone(),
                target: target.to_string(),
                reason: "the target property normalizes the value to a different type".to_owned(),
            });
        }
        Ok(())
    }

    fn validate_shape(&self) -> Result<(), PropertyKeyframeError> {
        if self.keyframes.is_empty() {
            return Err(PropertyKeyframeError::EmptyTrack);
        }
        let expected_kind = property_value_kind_label(&self.keyframes[0].value);
        let mut ids = BTreeSet::new();
        let mut previous_time = None;
        let mut previous_path: Option<(&ShapePath, Vec<(usize, bool)>)> = None;
        for (index, keyframe) in self.keyframes.iter().enumerate() {
            if keyframe.id.0.is_empty() {
                return Err(PropertyKeyframeError::EmptyId);
            }
            if keyframe.id.0.len() > MAX_KEYFRAME_ID_BYTES {
                return Err(PropertyKeyframeError::IdTooLong {
                    keyframe_id: keyframe.id.0.clone(),
                    actual: keyframe.id.0.len(),
                    maximum: MAX_KEYFRAME_ID_BYTES,
                });
            }
            if !ids.insert(keyframe.id.0.as_str()) {
                return Err(PropertyKeyframeError::DuplicateId {
                    keyframe_id: keyframe.id.0.clone(),
                });
            }
            if keyframe.layer_time.as_millis().unsigned_abs() > MAX_EXACT_KEYFRAME_TIME_MILLIS {
                return Err(PropertyKeyframeError::InvalidTime {
                    keyframe_id: keyframe.id.0.clone(),
                });
            }
            if previous_time.is_some_and(|previous| previous >= keyframe.layer_time) {
                return Err(PropertyKeyframeError::TimesNotStrictlyIncreasing {
                    keyframe_id: keyframe.id.0.clone(),
                    layer_time: keyframe.layer_time,
                });
            }
            previous_time = Some(keyframe.layer_time);
            if !keyframe.value.is_finite() {
                return Err(PropertyKeyframeError::NonFiniteValue {
                    keyframe_id: keyframe.id.0.clone(),
                });
            }
            if keyframe
                .spatial_in_tangent
                .is_some_and(|tangent| !tangent.is_finite())
                || keyframe
                    .spatial_out_tangent
                    .is_some_and(|tangent| !tangent.is_finite())
            {
                return Err(PropertyKeyframeError::NonFiniteSpatialTangent {
                    keyframe_id: keyframe.id.0.clone(),
                });
            }
            let actual_kind = property_value_kind_label(&keyframe.value);
            if actual_kind != expected_kind {
                return Err(PropertyKeyframeError::MixedValueKinds {
                    keyframe_id: keyframe.id.0.clone(),
                    expected: expected_kind,
                    found: actual_kind,
                });
            }
            if matches!(
                keyframe.value,
                PropertyValue::Integer(_) | PropertyValue::TimeRange(_)
            ) {
                return Err(PropertyKeyframeError::UnsupportedValue {
                    keyframe_id: keyframe.id.0.clone(),
                    value_kind: actual_kind,
                });
            }
            if index > 0
                && keyframe.easing != PropertyKeyframeEasing::Hold
                && !matches!(
                    keyframe.value,
                    PropertyValue::Float(_)
                        | PropertyValue::Vector2(_)
                        | PropertyValue::Color(_)
                        | PropertyValue::Path(_)
                )
            {
                return Err(
                    PropertyKeyframeError::ContinuousEasingRequiresNumericValue {
                        keyframe_id: keyframe.id.0.clone(),
                        value_kind: actual_kind,
                    },
                );
            }
            keyframe.easing.validate(&keyframe.id)?;
            if let PropertyValue::Path(path) = &keyframe.value {
                let contours =
                    path_contours(path).ok_or_else(|| PropertyKeyframeError::UnsupportedValue {
                        keyframe_id: keyframe.id.0.clone(),
                        value_kind: "malformed path",
                    })?;
                if keyframe.easing != PropertyKeyframeEasing::Hold
                    && previous_path
                        .as_ref()
                        .is_some_and(|(previous, previous_contours)| {
                            !path_pair_fits_budget(previous, previous_contours, path, &contours)
                        })
                {
                    return Err(PropertyKeyframeError::UnsupportedValue {
                        keyframe_id: keyframe.id.0.clone(),
                        value_kind: "path interpolation exceeds the evaluation budget",
                    });
                }
                previous_path = Some((path, contours));
            }
        }
        Ok(())
    }
}

// Keep these bounds aligned with fx_composition::animator::path_interpolation.
const MAX_INTERIOR_PATH_COMMANDS: usize = 8_192;
const MAX_PATH_CONTOURS: usize = 512;
const MAX_FLATTENED_PATH_POINTS: usize = 16_384;
const MAX_INTERPOLATED_PATH_COMMANDS: usize = 8_192;

fn path_contours(path: &ShapePath) -> Option<Vec<(usize, bool)>> {
    let mut contours = Vec::new();
    for command in &path.commands {
        match command {
            ShapePathCommand::MoveTo { .. } => contours.push((1usize, false)),
            ShapePathCommand::LineTo { .. } | ShapePathCommand::CubicTo { .. } => {
                let (samples, closed) = contours.last_mut()?;
                if *closed {
                    return None;
                }
                let count = if matches!(command, ShapePathCommand::CubicTo { .. }) {
                    8
                } else {
                    1
                };
                *samples = samples.checked_add(count)?;
            }
            ShapePathCommand::Close => {
                let (_, closed) = contours.last_mut()?;
                if *closed {
                    return None;
                }
                *closed = true;
            }
        }
    }
    Some(contours)
}

fn path_pair_fits_budget(
    from: &ShapePath,
    from_contours: &[(usize, bool)],
    to: &ShapePath,
    to_contours: &[(usize, bool)],
) -> bool {
    let matching_commands = from.commands.len() == to.commands.len()
        && from
            .commands
            .iter()
            .zip(&to.commands)
            .all(|(left, right)| std::mem::discriminant(left) == std::mem::discriminant(right));
    if matching_commands {
        return from.commands.len() <= MAX_INTERPOLATED_PATH_COMMANDS;
    }
    if from_contours.len().max(to_contours.len()) > MAX_PATH_CONTOURS {
        return false;
    }
    if from
        .commands
        .len()
        .checked_add(to.commands.len())
        .is_none_or(|count| count > MAX_INTERIOR_PATH_COMMANDS)
    {
        return false;
    }
    let flattened_points = from_contours
        .iter()
        .chain(to_contours)
        .try_fold(0usize, |total, (samples, closed)| {
            total.checked_add(samples.checked_add(usize::from(*closed))?)
        });
    if flattened_points.is_none_or(|count| count > MAX_FLATTENED_PATH_POINTS) {
        return false;
    }
    (0..from_contours.len().max(to_contours.len()))
        .try_fold(0usize, |total, index| {
            let from = from_contours.get(index);
            let to = to_contours.get(index);
            let samples = from
                .map_or(1, |(count, _)| *count)
                .max(to.map_or(1, |(count, _)| *count));
            let closed =
                from.is_some_and(|(_, closed)| *closed) || to.is_some_and(|(_, closed)| *closed);
            total.checked_add(samples.checked_add(usize::from(closed))?)
        })
        .is_some_and(|count| count <= MAX_INTERPOLATED_PATH_COMMANDS)
}

fn serialized_json_size(value: &impl Serialize) -> Result<usize, PropertyKeyframeError> {
    let mut writer = CountingWriter::default();
    serde_json::to_writer(&mut writer, value).map_err(|error| {
        PropertyKeyframeError::Serialization {
            message: error.to_string(),
        }
    })?;
    Ok(writer.bytes)
}

#[derive(Default)]
struct CountingWriter {
    bytes: usize,
}

impl Write for CountingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(buffer.len())
            .ok_or_else(|| io::Error::other("serialized keyframe size overflow"))?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn validate_position_axis_values(
    track: &PropertyKeyframeTrack,
    axis: &'static str,
) -> Result<(), PropertyKeyframeError> {
    for keyframe in &track.keyframes {
        if !matches!(keyframe.value, PropertyValue::Float(_)) {
            return Err(PropertyKeyframeError::PositionPairRequiresFloat {
                axis,
                keyframe_id: keyframe.id.0.clone(),
            });
        }
    }
    Ok(())
}

fn validate_spatial_endpoints(
    track: &PropertyKeyframeTrack,
    axis: &'static str,
) -> Result<(), PropertyKeyframeError> {
    let Some(first) = track.keyframes.first() else {
        return Err(PropertyKeyframeError::EmptyTrack);
    };
    if first.spatial_in_tangent.is_some() {
        return Err(PropertyKeyframeError::PositionPairTangentHasNoSegment {
            axis,
            keyframe_id: first.id.0.clone(),
            side: "in",
        });
    }
    let Some(last) = track.keyframes.last() else {
        return Err(PropertyKeyframeError::EmptyTrack);
    };
    if last.spatial_out_tangent.is_some() {
        return Err(PropertyKeyframeError::PositionPairTangentHasNoSegment {
            axis,
            keyframe_id: last.id.0.clone(),
            side: "out",
        });
    }
    Ok(())
}

fn validate_spatial_segments(
    source: &PropertyKeyframeTrack,
    paired: &PropertyKeyframeTrack,
) -> Result<(), PropertyKeyframeError> {
    for (left, right) in source.keyframes.iter().zip(source.keyframes.iter().skip(1)) {
        if left.spatial_out_tangent.is_none() && right.spatial_in_tangent.is_none() {
            continue;
        }
        let paired_left = paired
            .keyframes
            .binary_search_by_key(&left.layer_time, PropertyKeyframe::layer_time)
            .ok();
        let paired_right = paired
            .keyframes
            .binary_search_by_key(&right.layer_time, PropertyKeyframe::layer_time)
            .ok();
        let (Some(paired_left), Some(paired_right)) = (paired_left, paired_right) else {
            return Err(PropertyKeyframeError::PositionPairSpatialSegmentMismatch {
                start_time: left.layer_time,
                end_time: right.layer_time,
            });
        };
        if paired_right != paired_left + 1 {
            return Err(PropertyKeyframeError::PositionPairSpatialSegmentMismatch {
                start_time: left.layer_time,
                end_time: right.layer_time,
            });
        }
        let (Some(paired_left), Some(paired_right)) = (
            paired.keyframes.get(paired_left),
            paired.keyframes.get(paired_right),
        ) else {
            return Err(PropertyKeyframeError::PositionPairSpatialSegmentMismatch {
                start_time: left.layer_time,
                end_time: right.layer_time,
            });
        };
        if left.spatial_out_tangent.is_some() != paired_left.spatial_out_tangent.is_some() {
            return Err(PropertyKeyframeError::PositionPairTangentMismatch {
                layer_time: left.layer_time,
                side: "out",
            });
        }
        if right.spatial_in_tangent.is_some() != paired_right.spatial_in_tangent.is_some() {
            return Err(PropertyKeyframeError::PositionPairTangentMismatch {
                layer_time: right.layer_time,
                side: "in",
            });
        }
        if right.easing != paired_right.easing {
            return Err(PropertyKeyframeError::PositionPairEasingMismatch {
                layer_time: right.layer_time,
            });
        }
    }
    Ok(())
}

/// Invalid persisted or staged property-keyframe data.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PropertyKeyframeError {
    /// The requested animator is not backed by editable keyframes.
    #[error("property animator is not an editable keyframe animator")]
    NotKeyframeAnimator,
    /// Native keyframes do not consume animation-graph dependencies.
    #[error("native property keyframes cannot declare graph dependencies")]
    UnexpectedDependencies,
    /// Disabling requires the static property value rendered in place of keys.
    #[error("disabling a property keyframe animator requires a static value")]
    DisabledValueRequired,
    /// Enabling must discard the parked static value.
    #[error("an enabled property keyframe animator cannot carry a disabled value")]
    UnexpectedDisabledValue,
    /// The parked disabled value must obey the same target contract as keys.
    #[error("disabled property keyframe value is invalid for {target}: {reason}")]
    InvalidDisabledValue {
        /// Rejected target.
        target: String,
        /// Existing animator contract rejection.
        reason: String,
    },
    /// A keyframe track must contain at least one value.
    #[error("property keyframe track is empty")]
    EmptyTrack,
    /// The keyframe payload could not be serialized for byte accounting.
    #[error("property keyframe track could not be serialized: {message}")]
    Serialization {
        /// Serialization failure detail.
        message: String,
    },
    /// A keyframe identity cannot be empty.
    #[error("property keyframe id is empty")]
    EmptyId,
    /// A keyframe identity exceeds its storage bound.
    #[error("property keyframe id {keyframe_id:?} has {actual} bytes; maximum is {maximum}")]
    IdTooLong {
        /// Rejected identity.
        keyframe_id: String,
        /// UTF-8 byte length.
        actual: usize,
        /// Maximum supported bytes.
        maximum: usize,
    },
    /// IDs must be unique within a track.
    #[error("property keyframe id {keyframe_id:?} is duplicated")]
    DuplicateId {
        /// Duplicated identity.
        keyframe_id: String,
    },
    /// A requested keyframe identity does not exist in the target track.
    #[error("property keyframe id {keyframe_id:?} does not exist")]
    UnknownId {
        /// Missing identity.
        keyframe_id: String,
    },
    /// Layer time must remain exactly representable by JSON clients.
    #[error(
        "property keyframe {keyframe_id:?} has a layer time outside the exact JSON integer range"
    )]
    InvalidTime {
        /// Rejected keyframe identity.
        keyframe_id: String,
    },
    /// Persisted tracks must already be in canonical strict time order.
    #[error("property keyframe {keyframe_id:?} at {layer_time:?} is not strictly after the preceding key")]
    TimesNotStrictlyIncreasing {
        /// Rejected keyframe identity.
        keyframe_id: String,
        /// Rejected layer-local time.
        layer_time: TimeOffset,
    },
    /// Numeric payloads must be finite.
    #[error("property keyframe {keyframe_id:?} has a non-finite value")]
    NonFiniteValue {
        /// Rejected keyframe identity.
        keyframe_id: String,
    },
    /// Spatial tangent components must remain finite.
    #[error("property keyframe {keyframe_id:?} has a non-finite spatial tangent")]
    NonFiniteSpatialTangent {
        /// Rejected keyframe identity.
        keyframe_id: String,
    },
    /// Position tracks carry one scalar component per axis.
    #[error("position {axis}-axis keyframe {keyframe_id:?} is not a float")]
    PositionPairRequiresFloat {
        /// Axis carrying the invalid value.
        axis: &'static str,
        /// Rejected keyframe identity.
        keyframe_id: String,
    },
    /// A tangent on an endpoint has no adjacent segment to control.
    #[error("position {axis}-axis keyframe {keyframe_id:?} has a spatial {side} tangent without a segment")]
    PositionPairTangentHasNoSegment {
        /// Axis carrying the invalid tangent.
        axis: &'static str,
        /// Rejected keyframe identity.
        keyframe_id: String,
        /// Incoming or outgoing tangent side.
        side: &'static str,
    },
    /// Both axes must contain the same adjacent keys for a spatial segment.
    #[error(
        "position spatial segment from {start_time:?} to {end_time:?} is not adjacent on both axes"
    )]
    PositionPairSpatialSegmentMismatch {
        /// Segment start on the layer-local clock.
        start_time: TimeOffset,
        /// Segment end on the layer-local clock.
        end_time: TimeOffset,
    },
    /// A spatial tangent must provide both x and y components.
    #[error("position keyframe at {layer_time:?} has an unmatched spatial {side} tangent")]
    PositionPairTangentMismatch {
        /// Time of the mismatched key.
        layer_time: TimeOffset,
        /// Incoming or outgoing tangent side.
        side: &'static str,
    },
    /// Spatial interpolation needs one temporal parameter shared by both axes.
    #[error("position keyframe pair at {layer_time:?} has different x/y temporal easing")]
    PositionPairEasingMismatch {
        /// Start time of the rejected segment.
        layer_time: TimeOffset,
    },
    /// One track cannot change its value shape over time.
    #[error("property keyframe {keyframe_id:?} has {found}; expected {expected}")]
    MixedValueKinds {
        /// Rejected keyframe identity.
        keyframe_id: String,
        /// First keyframe's value kind.
        expected: &'static str,
        /// Rejected value kind.
        found: &'static str,
    },
    /// The first release does not keyframe this value shape.
    #[error("property keyframe {keyframe_id:?} uses unsupported {value_kind} values")]
    UnsupportedValue {
        /// Rejected keyframe identity.
        keyframe_id: String,
        /// Rejected value kind.
        value_kind: &'static str,
    },
    /// Discrete values can only use hold easing.
    #[error("property keyframe {keyframe_id:?} uses continuous easing for {value_kind}")]
    ContinuousEasingRequiresNumericValue {
        /// Rejected keyframe identity.
        keyframe_id: String,
        /// Rejected value kind.
        value_kind: &'static str,
    },
    /// Cubic Bézier x coordinates or numeric coordinates are invalid.
    #[error("property keyframe {keyframe_id:?} has invalid cubic Bézier easing")]
    InvalidEasing {
        /// Rejected keyframe identity.
        keyframe_id: String,
    },
    /// Spatial tangents apply only to the paired scalar position properties.
    #[error("property keyframe spatial tangents are invalid for {target}")]
    SpatialTangentsUnsupportedByTarget {
        /// Rejected property target.
        target: String,
    },
    /// The target cannot consume the authored keyframe value.
    #[error("property keyframe {keyframe_id:?} is invalid for {target}: {reason}")]
    ValueUnsupportedByTarget {
        /// Rejected keyframe identity.
        keyframe_id: String,
        /// Target diagnostic label.
        target: String,
        /// Existing animator contract rejection.
        reason: String,
    },
}

pub(super) fn validate_disabled_keyframe_value(
    target: &PropertyTarget,
    value: &PropertyValue,
) -> Result<(), PropertyKeyframeError> {
    if !value.is_finite() {
        return Err(PropertyKeyframeError::InvalidDisabledValue {
            target: target.to_string(),
            reason: "value must be finite".to_owned(),
        });
    }
    if let PropertyValue::Path(path) = value {
        if path_contours(path).is_none() {
            return Err(PropertyKeyframeError::InvalidDisabledValue {
                target: target.to_string(),
                reason: "path commands are malformed".to_owned(),
            });
        }
    }
    let actual = script_value_type_name(value).ok_or_else(|| {
        PropertyKeyframeError::InvalidDisabledValue {
            target: target.to_string(),
            reason: format!("unsupported {} value", property_value_kind_label(value)),
        }
    })?;
    let normalized = property_value_from_typed(target, value, actual).map_err(|error| {
        PropertyKeyframeError::InvalidDisabledValue {
            target: target.to_string(),
            reason: error.to_string(),
        }
    })?;
    if normalized != *value {
        return Err(PropertyKeyframeError::InvalidDisabledValue {
            target: target.to_string(),
            reason: "the target property normalizes the value to a different type".to_owned(),
        });
    }
    Ok(())
}

fn property_value_from_typed(
    target: &PropertyTarget,
    value: &PropertyValue,
    actual: &'static str,
) -> Result<PropertyValue, String> {
    let Some(property) = target.as_property() else {
        return Ok(value.clone());
    };
    let expected = match property.property_type() {
        PropType::DropShadowOffset
        | PropType::RectSize
        | PropType::PolyStarPosition
        | PropType::EllipseSize
        | PropType::EllipsePosition => "two-number array",
        PropType::FillColor
        | PropType::StrokeColor
        | PropType::DropShadowColor
        | PropType::MediaColor => "four-number array",
        PropType::TextContent => "string",
        PropType::FillEnabled
        | PropType::StrokeEnabled
        | PropType::DropShadowEnabled
        | PropType::Underline
        | PropType::Strikethrough
        | PropType::AllCaps => "boolean",
        PropType::FontFamily
        | PropType::FontStyle
        | PropType::MediaSourceAssetId
        | PropType::AudioSourceAssetId
        | PropType::StrokeJoin
        | PropType::ActiveRange
        | PropType::SourceRange => {
            return Err(format!(
                "JavaScript animator is not supported for target {target}"
            ));
        }
        PropType::ShapePath => "{ commands: [...] } path object",
        _ => "finite number",
    };
    let accepted = matches!(
        (expected, value),
        ("finite number", PropertyValue::Float(_))
            | ("two-number array", PropertyValue::Vector2(_))
            | ("four-number array", PropertyValue::Color(_))
            | ("string", PropertyValue::String(_))
            | ("boolean", PropertyValue::Bool(_))
            | ("{ commands: [...] } path object", PropertyValue::Path(_))
    );
    if accepted {
        Ok(value.clone())
    } else {
        Err(format!(
            "JavaScript animator for {target} returned {actual}; expected {expected}"
        ))
    }
}

fn script_value_type_name(value: &PropertyValue) -> Option<&'static str> {
    match value {
        PropertyValue::Float(_) => Some("number"),
        PropertyValue::Vector2(_) => Some("array[length=2]"),
        PropertyValue::Color(_) => Some("array[length=4]"),
        PropertyValue::String(_) => Some("string"),
        PropertyValue::Bool(_) => Some("boolean"),
        PropertyValue::Path(_) => Some("object"),
        PropertyValue::Integer(_) | PropertyValue::TimeRange(_) => None,
    }
}

fn property_value_kind_label(value: &PropertyValue) -> &'static str {
    match value {
        PropertyValue::Integer(_) => "integer",
        PropertyValue::Float(_) => "float",
        PropertyValue::Vector2(_) => "vector2",
        PropertyValue::Color(_) => "color",
        PropertyValue::String(_) => "string",
        PropertyValue::Bool(_) => "bool",
        PropertyValue::TimeRange(_) => "timeRange",
        PropertyValue::Path(_) => "path",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        property::{PropType, Property},
        EffectId, LayerId,
    };

    fn keyframe(
        id: &str,
        milliseconds: i64,
        value: PropertyValue,
        easing: PropertyKeyframeEasing,
    ) -> PropertyKeyframe {
        PropertyKeyframe::new(
            KeyframeId::new(id),
            TimeOffset::from_millis(milliseconds),
            value,
            easing,
        )
    }

    fn float_track(easing: PropertyKeyframeEasing) -> PropertyKeyframeTrack {
        PropertyKeyframeTrack::new(vec![
            keyframe(
                "first",
                0,
                PropertyValue::Float(0.0),
                PropertyKeyframeEasing::Linear,
            ),
            keyframe("second", 1000, PropertyValue::Float(10.0), easing),
        ])
        .expect("test track should be valid")
    }

    fn spatial_float_track(easing: PropertyKeyframeEasing) -> PropertyKeyframeTrack {
        PropertyKeyframeTrack::new(vec![
            keyframe(
                "first",
                0,
                PropertyValue::Float(0.0),
                PropertyKeyframeEasing::Linear,
            )
            .with_spatial_tangents(None, Some(20.0)),
            keyframe("second", 1000, PropertyValue::Float(10.0), easing)
                .with_spatial_tangents(Some(5.0), None),
        ])
        .expect("spatial test track should be valid")
    }

    #[test]
    fn native_path_keyframes_round_trip_and_validate_for_shape_path() {
        let path = PropertyValue::Path(crate::ShapePath {
            commands: vec![crate::ShapePathCommand::MoveTo {
                x: 2.0,
                y: 3.0,
                mirror: None,
                corner_radius: None,
            }],
        });
        let track = PropertyKeyframeTrack::new(vec![
            keyframe("start", 0, path.clone(), PropertyKeyframeEasing::Linear),
            keyframe("end", 1000, path, PropertyKeyframeEasing::Linear),
        ])
        .expect("path keys should be accepted");
        track
            .validate_for_target(&PropertyTarget::layer(LayerId::new(1), PropType::ShapePath))
            .expect("shapePath should accept path keys");
        assert!(track
            .validate_for_target(&PropertyTarget::layer(LayerId::new(1), PropType::Opacity))
            .is_err());
        let json = serde_json::to_string(&track).expect("serialize path keys");
        let restored: PropertyKeyframeTrack =
            serde_json::from_str(&json).expect("deserialize path keys");
        assert_eq!(restored, track);
    }

    #[test]
    fn native_path_keys_reject_malformed_and_over_budget_segments_before_persistence() {
        let invalid = PropertyValue::Path(crate::ShapePath {
            commands: vec![crate::ShapePathCommand::Close],
        });
        assert!(PropertyKeyframeTrack::new(vec![keyframe(
            "invalid",
            0,
            invalid.clone(),
            PropertyKeyframeEasing::Linear,
        )])
        .is_err());
        assert!(validate_disabled_keyframe_value(
            &PropertyTarget::layer(LayerId::new(1), PropType::ShapePath),
            &invalid,
        )
        .is_err());

        let mut commands = vec![crate::ShapePathCommand::MoveTo {
            x: 0.0,
            y: 0.0,
            mirror: None,
            corner_radius: None,
        }];
        commands.extend((0..4096).map(|index| crate::ShapePathCommand::LineTo {
            x: index as f64,
            y: 0.0,
            mirror: None,
            corner_radius: None,
        }));
        let first = PropertyValue::Path(crate::ShapePath {
            commands: commands.clone(),
        });
        PropertyKeyframeTrack::new(vec![
            keyframe("first", 0, first.clone(), PropertyKeyframeEasing::Linear),
            keyframe(
                "matching",
                1000,
                first.clone(),
                PropertyKeyframeEasing::Linear,
            ),
        ])
        .expect("matching paths fit the output budget despite combined input count");
        let many_contours = (0..513)
            .map(|index| crate::ShapePathCommand::MoveTo {
                x: index as f64,
                y: 0.0,
                mirror: None,
                corner_radius: None,
            })
            .collect::<Vec<_>>();
        let many = PropertyValue::Path(crate::ShapePath {
            commands: many_contours.clone(),
        });
        PropertyKeyframeTrack::new(vec![
            keyframe("first", 0, many.clone(), PropertyKeyframeEasing::Linear),
            keyframe(
                "matching",
                1000,
                many.clone(),
                PropertyKeyframeEasing::Linear,
            ),
        ])
        .expect("matching paths do not require the sampled-contour limit");
        let mut changed = many_contours;
        changed.push(crate::ShapePathCommand::LineTo {
            x: 2.0,
            y: 0.0,
            mirror: None,
            corner_radius: None,
        });
        assert!(PropertyKeyframeTrack::new(vec![
            keyframe("first", 0, many, PropertyKeyframeEasing::Linear),
            keyframe(
                "too-many-contours",
                1000,
                PropertyValue::Path(crate::ShapePath { commands: changed }),
                PropertyKeyframeEasing::Linear,
            ),
        ])
        .is_err());
        commands.push(crate::ShapePathCommand::LineTo {
            x: 4096.0,
            y: 0.0,
            mirror: None,
            corner_radius: None,
        });
        let second = PropertyValue::Path(crate::ShapePath { commands });
        assert!(PropertyKeyframeTrack::new(vec![
            keyframe("first", 0, first.clone(), PropertyKeyframeEasing::Linear),
            keyframe(
                "too-large",
                1000,
                second.clone(),
                PropertyKeyframeEasing::Linear
            ),
        ])
        .is_err());
        PropertyKeyframeTrack::new(vec![
            keyframe("first", 0, first, PropertyKeyframeEasing::Linear),
            keyframe("held", 1000, second, PropertyKeyframeEasing::Hold),
        ])
        .expect("hold does not interpolate the oversized path pair");
    }

    #[test]
    fn effect_named_properties_accept_typed_keyframe_shapes() {
        let color_target = PropertyTarget::effect_param(EffectId::new(1), "color");
        let color_track = PropertyKeyframeTrack::new(vec![
            keyframe(
                "color-first",
                0,
                PropertyValue::Color([0.0, 0.0, 0.0, 1.0]),
                PropertyKeyframeEasing::Linear,
            ),
            keyframe(
                "color-second",
                1000,
                PropertyValue::Color([1.0, 0.5, 0.25, 1.0]),
                PropertyKeyframeEasing::Linear,
            ),
        ])
        .unwrap();
        color_track.validate_for_target(&color_target).unwrap();

        let enabled_target = PropertyTarget::effect_param(EffectId::new(1), "enabled");
        let enabled_track = PropertyKeyframeTrack::new(vec![
            keyframe(
                "enabled-first",
                0,
                PropertyValue::Bool(true),
                PropertyKeyframeEasing::Hold,
            ),
            keyframe(
                "enabled-second",
                1000,
                PropertyValue::Bool(false),
                PropertyKeyframeEasing::Hold,
            ),
        ])
        .unwrap();
        enabled_track.validate_for_target(&enabled_target).unwrap();
    }

    #[test]
    fn disabled_values_preserve_runtime_conversion_errors_and_finite_range_rejection() {
        let layer_id = LayerId::new(1);
        for property_type in [
            PropType::FontFamily,
            PropType::FontStyle,
            PropType::MediaSourceAssetId,
            PropType::AudioSourceAssetId,
            PropType::StrokeJoin,
        ] {
            let target = PropertyTarget::from(Property::new(layer_id, property_type));
            let value = PropertyValue::String("value".to_owned());
            let error = validate_disabled_keyframe_value(&target, &value)
                .expect_err("finite-range strings cannot be parked disabled values");
            assert_eq!(
                error.to_string(),
                format!(
                    "disabled property keyframe value is invalid for {target}: JavaScript animator is not supported for target {target}"
                )
            );

            let track = PropertyKeyframeTrack::new(vec![keyframe(
                "finite-range",
                0,
                value,
                PropertyKeyframeEasing::Hold,
            )])
            .expect("finite-range string track should be structurally valid");
            track
                .validate_for_target(&target)
                .expect("enabled finite-range string tracks remain supported");
        }

        let opacity = PropertyTarget::from(Property::new(layer_id, PropType::Opacity));
        let wrong_kind = PropertyValue::String("opaque".to_owned());
        assert_eq!(
            validate_disabled_keyframe_value(&opacity, &wrong_kind)
                .expect_err("a string cannot animate opacity")
                .to_string(),
            format!(
                "disabled property keyframe value is invalid for {opacity}: JavaScript animator for {opacity} returned string; expected finite number"
            )
        );

        let wrong_kind_track = PropertyKeyframeTrack::new(vec![keyframe(
            "wrong-kind",
            0,
            wrong_kind,
            PropertyKeyframeEasing::Hold,
        )])
        .expect("string keyframe should be structurally valid");
        assert_eq!(
            wrong_kind_track
                .validate_for_target(&opacity)
                .expect_err("a string track cannot animate opacity")
                .to_string(),
            format!(
                "property keyframe \"wrong-kind\" is invalid for {opacity}: JavaScript animator for {opacity} returned string; expected finite number"
            )
        );

        for unsupported in [
            PropertyValue::Integer(1),
            PropertyValue::TimeRange(crate::property::TimeRangeProperty::new(
                crate::time::Time::ZERO,
                crate::time::Duration::ZERO,
            )),
        ] {
            let kind = property_value_kind_label(&unsupported);
            assert_eq!(
                validate_disabled_keyframe_value(&opacity, &unsupported)
                    .expect_err("value kind has no script conversion")
                    .to_string(),
                format!(
                    "disabled property keyframe value is invalid for {opacity}: unsupported {kind} value"
                )
            );
        }

        let path = PropertyValue::Path(crate::ShapePath {
            commands: Vec::new(),
        });
        let shape_path = PropertyTarget::layer(layer_id, PropType::ShapePath);
        validate_disabled_keyframe_value(&shape_path, &path)
            .expect("path can be parked as a disabled shapePath value");
        assert_eq!(
            validate_disabled_keyframe_value(&opacity, &path)
                .expect_err("path cannot be parked as opacity")
                .to_string(),
            format!(
                "disabled property keyframe value is invalid for {opacity}: JavaScript animator for {opacity} returned object; expected finite number"
            )
        );

        assert_eq!(
            validate_disabled_keyframe_value(&opacity, &PropertyValue::Float(f64::INFINITY))
                .expect_err("finiteness is checked before target compatibility")
                .to_string(),
            format!(
                "disabled property keyframe value is invalid for {opacity}: value must be finite"
            )
        );
    }

    #[test]
    fn accepts_tracks_longer_than_the_removed_fixed_key_limit() {
        let track = PropertyKeyframeTrack::new(
            (0..=2_048)
                .map(|index| {
                    keyframe(
                        &format!("key-{index}"),
                        i64::from(index),
                        PropertyValue::Float(f64::from(index)),
                        PropertyKeyframeEasing::Linear,
                    )
                })
                .collect(),
        )
        .unwrap();

        assert_eq!(track.keyframes().len(), 2_049);
    }

    #[test]
    fn tracks_larger_than_the_former_one_mib_limit_round_trip() {
        let track = PropertyKeyframeTrack::new(vec![keyframe(
            "large",
            0,
            PropertyValue::String("x".repeat(1024 * 1024)),
            PropertyKeyframeEasing::Hold,
        )])
        .unwrap();

        let wire = serde_json::to_vec(&track).unwrap();
        assert!(wire.len() > 1024 * 1024);
        let restored: PropertyKeyframeTrack = serde_json::from_slice(&wire).unwrap();
        assert_eq!(restored, track);
    }

    #[test]
    fn rejects_inexact_time_and_target_incompatible_tracks() {
        let inexact_time = PropertyKeyframeTrack::new(vec![keyframe(
            "inexact-time",
            i64::try_from(MAX_EXACT_KEYFRAME_TIME_MILLIS + 1)
                .expect("JSON integer bound should fit in i64"),
            PropertyValue::Float(0.0),
            PropertyKeyframeEasing::Linear,
        )]);
        assert!(matches!(
            inexact_time,
            Err(PropertyKeyframeError::InvalidTime { .. })
        ));

        let target = PropertyTarget::from(Property::new(LayerId::new(1), PropType::TextContent));
        assert!(matches!(
            float_track(PropertyKeyframeEasing::Linear).validate_for_target(&target),
            Err(PropertyKeyframeError::ValueUnsupportedByTarget { .. })
        ));

        let opacity = PropertyTarget::from(Property::new(LayerId::new(1), PropType::Opacity));
        assert!(matches!(
            spatial_float_track(PropertyKeyframeEasing::Linear).validate_for_target(&opacity),
            Err(PropertyKeyframeError::SpatialTangentsUnsupportedByTarget { .. })
        ));

        let non_finite_tangent = PropertyKeyframeTrack::new(vec![keyframe(
            "non-finite-tangent",
            0,
            PropertyValue::Float(0.0),
            PropertyKeyframeEasing::Linear,
        )
        .with_spatial_tangents(None, Some(f64::INFINITY))]);
        assert!(matches!(
            non_finite_tangent,
            Err(PropertyKeyframeError::NonFiniteSpatialTangent { .. })
        ));
    }

    #[test]
    fn constructor_sorts_while_wire_rejects_unsorted_and_invalid_tracks() {
        let authored = PropertyKeyframeTrack::new(vec![
            keyframe(
                "later",
                10,
                PropertyValue::Float(1.0),
                PropertyKeyframeEasing::Linear,
            ),
            keyframe(
                "earlier",
                0,
                PropertyValue::Float(0.0),
                PropertyKeyframeEasing::Linear,
            ),
        ])
        .expect("the public constructor should canonicalize author order");
        assert_eq!(authored.keyframes()[0].id().as_str(), "earlier");
        assert_eq!(authored.keyframes()[1].id().as_str(), "later");

        let unsorted = serde_json::json!({
            "keyframes": [
                { "id": "later", "layerTime": 10, "value": { "type": "float", "value": 1 }, "easing": { "type": "linear" } },
                { "id": "earlier", "layerTime": 0, "value": { "type": "float", "value": 0 }, "easing": { "type": "linear" } }
            ]
        });
        assert!(serde_json::from_value::<PropertyKeyframeTrack>(unsorted).is_err());

        let duplicate = PropertyKeyframeTrack::new(vec![
            keyframe(
                "same",
                0,
                PropertyValue::Float(0.0),
                PropertyKeyframeEasing::Linear,
            ),
            keyframe(
                "same",
                1,
                PropertyValue::Float(1.0),
                PropertyKeyframeEasing::Linear,
            ),
        ]);
        assert!(matches!(
            duplicate,
            Err(PropertyKeyframeError::DuplicateId { .. })
        ));

        let duplicate_time = PropertyKeyframeTrack::new(vec![
            keyframe(
                "first",
                0,
                PropertyValue::Float(0.0),
                PropertyKeyframeEasing::Linear,
            ),
            keyframe(
                "second",
                0,
                PropertyValue::Float(1.0),
                PropertyKeyframeEasing::Linear,
            ),
        ]);
        assert!(matches!(
            duplicate_time,
            Err(PropertyKeyframeError::TimesNotStrictlyIncreasing { .. })
        ));

        let empty_id = PropertyKeyframeTrack::new(vec![keyframe(
            "",
            0,
            PropertyValue::Float(0.0),
            PropertyKeyframeEasing::Linear,
        )]);
        assert!(matches!(empty_id, Err(PropertyKeyframeError::EmptyId)));

        let mixed = PropertyKeyframeTrack::new(vec![
            keyframe(
                "float",
                0,
                PropertyValue::Float(0.0),
                PropertyKeyframeEasing::Hold,
            ),
            keyframe(
                "bool",
                1,
                PropertyValue::Bool(true),
                PropertyKeyframeEasing::Hold,
            ),
        ]);
        assert!(matches!(
            mixed,
            Err(PropertyKeyframeError::MixedValueKinds { .. })
        ));

        let invalid_easing = PropertyKeyframeTrack::new(vec![keyframe(
            "invalid-easing",
            0,
            PropertyValue::Float(0.0),
            PropertyKeyframeEasing::CubicBezier {
                x1: -0.1,
                y1: 0.0,
                x2: 1.0,
                y2: 1.0,
            },
        )]);
        assert!(matches!(
            invalid_easing,
            Err(PropertyKeyframeError::InvalidEasing { .. })
        ));
    }
}
