//! Fresh AE26 numeric keyframe records for the bounded FX exporter.

use std::borrow::Cow;

use crate::rifx::{Chunk, RifxError};

/// Native property key ticks belong to the composition, not the source/layer clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PropertyClock(u32);

impl PropertyClock {
    pub(super) const DEFAULT: Self = Self(24_576);

    pub(super) fn for_rate(rate: crate::timing::FrameRate) -> Result<Self, RifxError> {
        let (integer, fractional) = rate.parts();
        // Exact 16.16 rates from the six-comp independently authored AE probe.
        // All four fractional controls use 23,976 ticks, even the 29.97 comps;
        // refuse rates not independently observed rather than rounding FPS.
        if fractional != 0 {
            return match (integer, fractional) {
                (29, 63_570 | 63_572) | (23, 63_963 | 63_965) => Ok(Self(23_976)),
                _ => Err(RifxError::Invalid(
                    "fractional property clock needs native evidence",
                )),
            };
        }
        Ok(Self(u32::from(integer) * 1024))
    }

    pub(super) fn ticks(self) -> u32 {
        self.0
    }

    pub(super) fn units(self, millis: i64) -> Result<i32, RifxError> {
        let units = i128::from(millis) * i128::from(self.0);
        let rounded = if units >= 0 {
            (units + 500) / 1000
        } else {
            (units - 500) / 1000
        };
        i32::try_from(rounded).map_err(|_| RifxError::Limit("native keyframe time"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Easing {
    Hold,
    Linear,
    CubicBezier { x1: f64, y1: f64, x2: f64, y2: f64 },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Keyframe {
    pub time_millis: i64,
    pub values: Vec<f64>,
    /// Incoming easing from the previous key, one value per component. Spatial
    /// properties require one shared value.
    pub easing: Vec<Easing>,
    pub spatial_in: Vec<f64>,
    pub spatial_out: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Track {
    pub keys: Vec<Keyframe>,
}

pub(super) fn animated_vector_descriptor(
    mut bytes: [u8; 124],
    selection: u16,
    variant: u16,
    flags: u32,
    mode: u32,
    subtype: u8,
) -> [u8; 124] {
    bytes[4..6].copy_from_slice(&selection.to_be_bytes());
    bytes[6..8].copy_from_slice(&variant.to_be_bytes());
    bytes[8..12].copy_from_slice(&flags.to_be_bytes());
    bytes[56..60].copy_from_slice(&mode.to_be_bytes());
    bytes[60] = subtype;
    bytes[68] = 1;
    bytes[76..80].fill(0);
    bytes
}

pub(super) fn spatial_2d_track(track: &Track) -> Result<Cow<'_, Track>, RifxError> {
    let already_spatial = track.keys.iter().all(|key| {
        key.easing.len() == 1 && key.spatial_in.len() == 2 && key.spatial_out.len() == 2
    });
    if already_spatial {
        return Ok(Cow::Borrowed(track));
    }

    let paired_nonspatial = track.keys.iter().all(|key| {
        key.easing.len() == 2 && key.spatial_in.is_empty() && key.spatial_out.is_empty()
    });
    if !paired_nonspatial {
        return Err(RifxError::Invalid(
            "2D vector position keys are neither paired nor spatial",
        ));
    }

    let mut spatial = track.clone();
    for key in &mut spatial.keys {
        if key.easing[0] != key.easing[1] {
            return Err(RifxError::Invalid(
                "2D vector position component easings differ",
            ));
        }
        key.easing.truncate(1);
        key.spatial_in = vec![0.0; 2];
        key.spatial_out = vec![0.0; 2];
    }
    Ok(Cow::Owned(spatial))
}

pub(super) fn animated_descriptor(mut bytes: [u8; 124], spatial: bool) -> [u8; 124] {
    bytes[68] = 1;
    bytes[10] = 0xff;
    bytes[11] = 0xff;
    bytes[59] |= 8;
    bytes[60] = 9;
    if spatial {
        bytes[5] = 14;
        bytes[6..8].copy_from_slice(&3_u16.to_be_bytes());
        bytes[8..10].copy_from_slice(&u16::MAX.to_be_bytes());
    } else {
        bytes[5] = 0;
        bytes[6..8].fill(0);
        bytes[8..10].copy_from_slice(&1_u16.to_be_bytes());
    }
    bytes
}

#[expect(dead_code, reason = "default-clock wrapper; callers pass a clock")]
pub(super) fn vector_color_list(track: &Track) -> Result<Chunk, RifxError> {
    vector_color_list_with_clock(track, PropertyClock::DEFAULT)
}

pub(super) fn vector_color_list_with_clock(
    track: &Track,
    clock: PropertyClock,
) -> Result<Chunk, RifxError> {
    validate(track, 4, false)?;
    if track.keys.iter().any(|key| {
        key.easing
            .first()
            .is_some_and(|first| key.easing.iter().any(|easing| easing != first))
    }) {
        return Err(RifxError::Invalid("vector color component easings differ"));
    }
    let mut items = vec![Item::new(4, false); track.keys.len()];
    for (item, key) in items.iter_mut().zip(&track.keys) {
        item.time_units = clock.units(key.time_millis)?;
        item.values.clone_from(&key.values);
    }
    reject_colliding_units(&items)?;
    for index in 1..track.keys.len() {
        let (before, after) = items.split_at_mut(index);
        apply_segment(
            &track.keys[index - 1],
            &track.keys[index],
            &mut before[index - 1],
            &mut after[0],
            true,
        )?;
    }
    keyframe_list(
        track.keys.len(),
        152,
        items.into_iter().map(Item::encode_color),
    )
}

/// Color keeps its native color value class when animated; the generic numeric
/// descriptor would make Adobe reject the project, even with 152-byte keys.
pub(super) fn animated_color_descriptor(mut bytes: [u8; 124]) -> [u8; 124] {
    bytes[5] = 6;
    bytes[6..8].copy_from_slice(&1_u16.to_be_bytes());
    bytes[68] = 1;
    bytes[79] = 0;
    bytes
}

#[cfg(test)]
pub(super) fn list(track: &Track, dimensions: usize, spatial: bool) -> Result<Chunk, RifxError> {
    list_with_clock(track, dimensions, spatial, PropertyClock::DEFAULT)
}

pub(super) fn list_with_clock(
    track: &Track,
    dimensions: usize,
    spatial: bool,
    clock: PropertyClock,
) -> Result<Chunk, RifxError> {
    list_with_flags(
        track,
        dimensions,
        spatial,
        if spatial { 7 } else { 0 },
        clock,
    )
}

fn list_with_flags(
    track: &Track,
    dimensions: usize,
    spatial: bool,
    flags: u16,
    clock: PropertyClock,
) -> Result<Chunk, RifxError> {
    validate(track, dimensions, spatial)?;
    let stride = if spatial {
        56 + 24 * dimensions
    } else {
        8 + 40 * dimensions
    };
    let mut items = vec![Item::new(dimensions, spatial); track.keys.len()];
    for (item, key) in items.iter_mut().zip(&track.keys) {
        item.time_units = clock.units(key.time_millis)?;
        item.flags = flags;
        item.values.clone_from(&key.values);
        item.spatial_in.clone_from(&key.spatial_in);
        item.spatial_out.clone_from(&key.spatial_out);
    }
    reject_colliding_units(&items)?;
    for index in 1..track.keys.len() {
        let (before, after) = items.split_at_mut(index);
        apply_segment(
            &track.keys[index - 1],
            &track.keys[index],
            &mut before[index - 1],
            &mut after[0],
            spatial,
        )?;
    }
    keyframe_list(
        track.keys.len(),
        stride,
        items.into_iter().map(Item::encode),
    )
}

pub(super) fn keyframe_list(
    count: usize,
    stride: usize,
    items: impl Iterator<Item = Vec<u8>>,
) -> Result<Chunk, RifxError> {
    let count = u16::try_from(count).map_err(|_| RifxError::Limit("native keyframe count"))?;
    let stride = u16::try_from(stride).map_err(|_| RifxError::Limit("keyframe stride"))?;
    let blocks = usize::from(count).div_ceil(4).max(1);
    let blocks = u32::try_from(blocks).map_err(|_| RifxError::Limit("keyframe blocks"))?;
    let capacity = blocks
        .checked_mul(4)
        .ok_or(RifxError::Limit("keyframe capacity"))?;
    let mut header = vec![0_u8; 52];
    header[..10].copy_from_slice(&[0, 0xd0, 0x0b, 0xee, 0, 0, 0, 0, 0, 0]);
    header[10..12].copy_from_slice(&count.to_be_bytes());
    header[12..16].copy_from_slice(&blocks.to_be_bytes());
    header[18..20].copy_from_slice(&stride.to_be_bytes());
    header[23] = 4;
    header[24..28].copy_from_slice(&1_u32.to_be_bytes());
    header[28..32].copy_from_slice(&capacity.to_be_bytes());
    let data: Vec<u8> = items.flatten().collect();
    Ok(Chunk::list(
        *b"list",
        vec![Chunk::data(*b"lhd3", header)?, Chunk::data(*b"ldat", data)?],
    ))
}

/// Native color keys use the 152-byte, four-value layout with one temporal
/// ease pair, not the 168-byte independent-component numeric layout. The
/// reserved vectors share the spatial record layout, but the property itself
/// remains non-spatial. Only common Linear/Hold segments are established here.
#[expect(dead_code, reason = "default-clock wrapper; callers pass a clock")]
pub(super) fn color_list(track: &Track) -> Result<Chunk, RifxError> {
    color_list_with_clock(track, PropertyClock::DEFAULT)
}

pub(super) fn color_list_with_clock(
    track: &Track,
    clock: PropertyClock,
) -> Result<Chunk, RifxError> {
    validate(track, 4, false)?;
    if track
        .keys
        .iter()
        .flat_map(|key| &key.easing)
        .any(|easing| matches!(easing, Easing::CubicBezier { .. }))
    {
        return Err(RifxError::Invalid(
            "native color cubic easing is not established",
        ));
    }
    let mut color = track.clone();
    for key in &mut color.keys {
        key.easing.truncate(1);
        key.spatial_in = vec![0.0; 4];
        key.spatial_out = vec![0.0; 4];
    }
    list_with_flags(&color, 4, true, 1, clock)
}

fn reject_colliding_units(items: &[Item]) -> Result<(), RifxError> {
    if items
        .windows(2)
        .any(|pair| pair[0].time_units >= pair[1].time_units)
    {
        return Err(RifxError::Invalid("native keyframe clock collision"));
    }
    Ok(())
}

fn validate(track: &Track, dimensions: usize, spatial: bool) -> Result<(), RifxError> {
    if track.keys.is_empty()
        || u16::try_from(track.keys.len()).is_err()
        || !(1..=4).contains(&dimensions)
    {
        return Err(RifxError::Invalid(
            "invalid native numeric keyframe count/dimensions",
        ));
    }
    let ease_dimensions = if spatial { 1 } else { dimensions };
    let mut previous = None;
    for key in &track.keys {
        if key.values.len() != dimensions
            || key.easing.len() != ease_dimensions
            || key.values.iter().any(|value| !value.is_finite())
            || key.easing.first().is_some_and(|first| {
                key.easing
                    .iter()
                    .any(|easing| std::mem::discriminant(easing) != std::mem::discriminant(first))
            })
            || (spatial
                && (key.spatial_in.len() != dimensions
                    || key.spatial_out.len() != dimensions
                    || key
                        .spatial_in
                        .iter()
                        .chain(&key.spatial_out)
                        .any(|value| !value.is_finite())))
            || (!spatial && (!key.spatial_in.is_empty() || !key.spatial_out.is_empty()))
            || previous.is_some_and(|time| time >= key.time_millis)
        {
            return Err(RifxError::Invalid("invalid native numeric keyframe track"));
        }
        previous = Some(key.time_millis);
    }
    Ok(())
}

pub(super) fn time_units(millis: i64) -> Result<i32, RifxError> {
    PropertyClock::DEFAULT.units(millis)
}

fn apply_segment(
    previous: &Keyframe,
    current: &Keyframe,
    previous_item: &mut Item,
    current_item: &mut Item,
    spatial: bool,
) -> Result<(), RifxError> {
    let duration = (current.time_millis - previous.time_millis) as f64 / 1000.0;
    let easings = &current.easing;
    for (component, easing) in easings.iter().enumerate() {
        let value_component = if spatial { 0 } else { component };
        match easing {
            Easing::Hold => {
                previous_item.out_interpolation = 3;
                current_item.in_interpolation = 3;
            }
            Easing::Linear => {}
            Easing::CubicBezier { x1, y1, x2, y2 } => {
                if ![x1, y1, x2, y2].into_iter().all(|value| value.is_finite())
                    || !(0.0..=1.0).contains(x1)
                    || !(0.0..=1.0).contains(x2)
                    || (*x1 == 0.0 && *y1 != 0.0)
                    || (*x2 == 1.0 && *y2 != 1.0)
                {
                    return Err(RifxError::Invalid(
                        "cubic easing is not representable by native temporal ease",
                    ));
                }
                previous_item.out_interpolation = 2;
                current_item.in_interpolation = 2;
                let delta = if spatial {
                    previous
                        .values
                        .iter()
                        .zip(&current.values)
                        .map(|(from, to)| (to - from).powi(2))
                        .sum::<f64>()
                        .sqrt()
                } else {
                    current.values[value_component] - previous.values[value_component]
                };
                previous_item.out_influence[value_component] = *x1;
                current_item.in_influence[value_component] = 1.0 - *x2;
                previous_item.out_speed[value_component] = if *x1 == 0.0 || duration == 0.0 {
                    0.0
                } else {
                    y1 / x1 * delta / duration
                };
                current_item.in_speed[value_component] = if *x2 == 1.0 || duration == 0.0 {
                    0.0
                } else {
                    (1.0 - y2) / (1.0 - x2) * delta / duration
                };
                if !previous_item.out_speed[value_component].is_finite()
                    || !current_item.in_speed[value_component].is_finite()
                {
                    return Err(RifxError::Invalid(
                        "non-finite native numeric keyframe speed",
                    ));
                }
            }
        }
    }
    Ok(())
}

#[derive(Clone)]
struct Item {
    time_units: i32,
    in_interpolation: u8,
    out_interpolation: u8,
    flags: u16,
    values: Vec<f64>,
    in_speed: Vec<f64>,
    in_influence: Vec<f64>,
    out_speed: Vec<f64>,
    out_influence: Vec<f64>,
    spatial_in: Vec<f64>,
    spatial_out: Vec<f64>,
    spatial: bool,
}

impl Item {
    fn new(dimensions: usize, spatial: bool) -> Self {
        let ease_dimensions = if spatial { 1 } else { dimensions };
        Self {
            time_units: 0,
            in_interpolation: 1,
            out_interpolation: 1,
            flags: 0,
            values: vec![0.0; dimensions],
            in_speed: vec![0.0; ease_dimensions],
            in_influence: vec![0.0; ease_dimensions],
            out_speed: vec![0.0; ease_dimensions],
            out_influence: vec![0.0; ease_dimensions],
            spatial_in: if spatial {
                vec![0.0; dimensions]
            } else {
                Vec::new()
            },
            spatial_out: if spatial {
                vec![0.0; dimensions]
            } else {
                Vec::new()
            },
            spatial,
        }
    }

    fn encode(self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.time_units.to_be_bytes());
        bytes.extend_from_slice(&[self.in_interpolation, self.out_interpolation]);
        bytes.extend_from_slice(&self.flags.to_be_bytes());
        if self.spatial {
            bytes.extend_from_slice(&[
                0,
                0,
                0,
                u8::from(
                    self.spatial_in
                        .iter()
                        .chain(&self.spatial_out)
                        .any(|value| *value != 0.0),
                ),
            ]);
            bytes.extend_from_slice(&[0; 4]);
            bytes.extend_from_slice(&0.0_f64.to_be_bytes());
            for value in [
                self.in_speed[0],
                self.in_influence[0],
                self.out_speed[0],
                self.out_influence[0],
            ] {
                bytes.extend_from_slice(&value.to_be_bytes());
            }
            for value in self
                .values
                .into_iter()
                .chain(self.spatial_in)
                .chain(self.spatial_out)
            {
                bytes.extend_from_slice(&value.to_be_bytes());
            }
        } else {
            for values in [
                self.values,
                self.in_speed,
                self.in_influence,
                self.out_speed,
                self.out_influence,
            ] {
                for value in values {
                    bytes.extend_from_slice(&value.to_be_bytes());
                }
            }
        }
        bytes
    }

    fn encode_color(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(152);
        bytes.extend_from_slice(&self.time_units.to_be_bytes());
        bytes.extend_from_slice(&[self.in_interpolation, self.out_interpolation, 0, 1]);
        bytes.extend_from_slice(&[0; 16]);
        for value in [
            self.in_speed[0],
            self.in_influence[0],
            self.out_speed[0],
            self.out_influence[0],
        ] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        for value in self.values {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(&[0; 64]);
        bytes
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_key_clock_collision_and_overflow_are_errors() {
        let rate = crate::timing::FrameRate::new(30.0).unwrap();
        let clock = super::PropertyClock::for_rate(rate).unwrap();
        let make_key = |time_millis| super::Keyframe {
            time_millis,
            values: vec![1.0],
            easing: vec![super::Easing::Hold],
            spatial_in: vec![],
            spatial_out: vec![],
        };
        // Artificial one-tick clock exercises the validator independently
        // of selectable native composition rates (all observed >= 23,976).
        let low = super::PropertyClock(1);
        let track = super::Track {
            keys: vec![make_key(0), make_key(1)],
        };
        assert!(super::list_with_clock(&track, 1, false, low).is_err());
        assert!(super::list_with_clock(&track, 1, false, clock).is_ok());
        let too_large = super::Track {
            keys: vec![make_key(i64::MAX)],
        };
        assert!(super::list_with_clock(&too_large, 1, false, clock).is_err());
    }

    #[test]
    fn independent_native_property_clocks_are_selected_by_exact_rate() {
        use super::PropertyClock;
        for (fps, expected) in [
            (24.0, 24_576),
            (30.0, 30_720),
            (29.97, 23_976),
            (30_000.0 / 1_001.0, 23_976),
            (23.976, 23_976),
            (24_000.0 / 1_001.0, 23_976),
        ] {
            let rate = crate::timing::FrameRate::new(fps).unwrap();
            let clock = PropertyClock::for_rate(rate).unwrap();
            assert_eq!(clock.ticks(), expected, "{fps}");
            assert_eq!(
                clock.units(1_100).unwrap(),
                ((1_100_i64 * i64::from(expected) + 500) / 1000) as i32
            );
        }
        assert!(PropertyClock::for_rate(crate::timing::FrameRate::new(25.5).unwrap()).is_err());
    }
    use super::*;

    fn cubic() -> Easing {
        Easing::CubicBezier {
            x1: 0.5,
            y1: 0.5,
            x2: 0.5,
            y2: 0.5,
        }
    }

    fn key(time_millis: i64, values: Vec<f64>, easing: Vec<Easing>, spatial: bool) -> Keyframe {
        let spatial_values = if spatial {
            vec![0.0; values.len()]
        } else {
            Vec::new()
        };
        Keyframe {
            time_millis,
            values,
            easing,
            spatial_in: spatial_values.clone(),
            spatial_out: spatial_values,
        }
    }

    fn two_key_track(start: Vec<f64>, end: Vec<f64>, easing: Vec<Easing>, spatial: bool) -> Track {
        Track {
            keys: vec![
                key(0, start, vec![Easing::Linear; easing.len()], spatial),
                key(1_000, end, easing, spatial),
            ],
        }
    }

    fn assert_nonfinite_speed_rejected(track: &Track, dimensions: usize, spatial: bool) {
        assert!(matches!(
            list(track, dimensions, spatial),
            Err(RifxError::Invalid(
                "non-finite native numeric keyframe speed"
            ))
        ));
    }

    #[test]
    fn rejects_nonfinite_scalar_speed_from_tiny_x_handle() {
        let easing = Easing::CubicBezier {
            x1: f64::MIN_POSITIVE,
            y1: 1.0,
            x2: 0.5,
            y2: 0.5,
        };
        let track = two_key_track(vec![0.0], vec![f64::MAX], vec![easing], false);

        assert_nonfinite_speed_rejected(&track, 1, false);
    }

    #[test]
    fn rejects_nonfinite_pair_speed_from_finite_delta_overflow() {
        let track = two_key_track(
            vec![-f64::MAX, 0.0],
            vec![f64::MAX, 1.0],
            vec![cubic(); 2],
            false,
        );

        assert_nonfinite_speed_rejected(&track, 2, false);
    }

    #[test]
    fn rejects_nonfinite_spatial_speed_from_distance_overflow() {
        let track = two_key_track(vec![f64::MAX, 0.0], vec![0.0, 0.0], vec![cubic()], true);

        assert_nonfinite_speed_rejected(&track, 2, true);
    }

    #[test]
    fn accepts_ordinary_finite_scalar_pair_and_spatial_speeds() {
        let scalar = two_key_track(vec![0.0], vec![10.0], vec![cubic()], false);
        let pair = two_key_track(vec![0.0, 1.0], vec![10.0, 11.0], vec![cubic(); 2], false);
        let spatial = two_key_track(vec![0.0, 1.0], vec![10.0, 11.0], vec![cubic()], true);

        assert!(list(&scalar, 1, false).is_ok());
        assert!(list(&pair, 2, false).is_ok());
        assert!(list(&spatial, 2, true).is_ok());
    }

    #[test]
    fn paired_2d_keys_become_spatial_without_changing_times_or_values() {
        let paired = two_key_track(
            vec![1.0, 2.0],
            vec![30.0, 40.0],
            vec![cubic(), cubic()],
            false,
        );
        let spatial = spatial_2d_track(&paired).expect("identical component easing");

        assert_eq!(
            spatial
                .keys
                .iter()
                .map(|key| (key.time_millis, key.values.clone()))
                .collect::<Vec<_>>(),
            [(0, vec![1.0, 2.0]), (1_000, vec![30.0, 40.0])]
        );
        assert!(spatial.keys.iter().all(|key| key.easing.len() == 1));
        assert!(
            spatial
                .keys
                .iter()
                .all(|key| { key.spatial_in == [0.0, 0.0] && key.spatial_out == [0.0, 0.0] })
        );
        assert!(list(&spatial, 2, true).is_ok());
    }

    #[test]
    fn spatial_2d_keys_are_borrowed_unchanged() {
        let spatial = two_key_track(vec![1.0, 2.0], vec![30.0, 40.0], vec![cubic()], true);
        let adapted = spatial_2d_track(&spatial).expect("already spatial");

        assert!(matches!(adapted, Cow::Borrowed(_)));
        assert_eq!(&*adapted, &spatial);
    }

    #[test]
    fn paired_2d_keys_reject_different_component_easing() {
        let paired = two_key_track(
            vec![1.0, 2.0],
            vec![30.0, 40.0],
            vec![Easing::Linear, cubic()],
            false,
        );

        assert!(matches!(
            spatial_2d_track(&paired),
            Err(RifxError::Invalid(
                "2D vector position component easings differ"
            ))
        ));
    }

    #[test]
    fn numeric_keys_exceed_old_policy_boundary_but_retain_native_u16_limit() {
        let track = Track {
            keys: (0..=10_000)
                .map(|index| key(index, vec![index as f64], vec![Easing::Linear], false))
                .collect(),
        };
        list(&track, 1, false).expect("keys above the old policy limit");

        let mut too_many = track;
        too_many.keys.resize_with(usize::from(u16::MAX) + 1, || {
            key(0, vec![0.0], vec![Easing::Linear], false)
        });
        assert!(list(&too_many, 1, false).is_err());
    }
}
