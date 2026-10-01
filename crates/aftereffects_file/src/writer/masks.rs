//! Fresh static native AE masks attached to an already-authored timeline layer.
//!
//! [`NativeMaskSpec::path`] uses owner-local pixels. AE stores mask contours
//! normalized to the owning source bounds, so this module performs that one
//! conversion before invoking the shared checked static-path encoder.

use fx_schema::{ShapePath, ShapePathCommand};

use crate::rifx::Chunk;

use super::{AepWriteError, NumericTrack, path_geometry, views};

const MAX_MASKS: usize = u16::MAX as usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeMaskMode {
    None,
    Add,
    Subtract,
    Intersect,
    Lighten,
    Darken,
    Difference,
}

impl NativeMaskMode {
    fn ordinal(self) -> u16 {
        match self {
            Self::None => 0,
            Self::Add => 1,
            Self::Subtract => 2,
            Self::Intersect => 3,
            Self::Lighten => 4,
            Self::Darken => 5,
            Self::Difference => 6,
        }
    }
}

/// One editable native mask. Geometry and Feather/Expansion are owner-local
/// pixels; opacity is normalized to `0..=1`. Numeric tracks use the same units
/// except opacity tracks, whose values are native percentages (`0..=100`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeMaskSpec {
    pub name: String,
    pub path: ShapePath,
    pub path_track: Option<super::PathTrack>,
    pub source_size: [u32; 2],
    pub mode: NativeMaskMode,
    pub inverted: bool,
    pub feather: [f64; 2],
    pub opacity: f64,
    pub expansion: f64,
    pub feather_track: Option<NumericTrack>,
    pub opacity_track: Option<NumericTrack>,
    pub expansion_track: Option<NumericTrack>,
}

impl NativeMaskSpec {
    /// Constructs a closed axis-aligned crop contour. `rect` is
    /// `[x, y, width, height]` in owner-local pixels with a top-left origin;
    /// width and height must be positive. No composition/canvas offset is
    /// applied implicitly.
    pub(crate) fn crop_rectangle(
        name: impl Into<String>,
        source_size: [u32; 2],
        rect: [f64; 4],
    ) -> Result<Self, &'static str> {
        let [x, y, width, height] = rect;
        if rect.iter().any(|value| !value.is_finite()) || width <= 0.0 || height <= 0.0 {
            return Err("crop rectangle requires finite coordinates and positive size");
        }
        let path = ShapePath {
            commands: vec![
                point(x, y, true),
                point(x + width, y, false),
                point(x + width, y + height, false),
                point(x, y + height, false),
                ShapePathCommand::Close,
            ],
        };
        Ok(Self {
            name: name.into(),
            path,
            path_track: None,
            source_size,
            mode: NativeMaskMode::Add,
            inverted: false,
            feather: [0.0; 2],
            opacity: 1.0,
            expansion: 0.0,
            feather_track: None,
            opacity_track: None,
            expansion_track: None,
        })
    }
}

fn point(x: f64, y: f64, first: bool) -> ShapePathCommand {
    if first {
        ShapePathCommand::MoveTo {
            x,
            y,
            mirror: None,
            corner_radius: None,
        }
    } else {
        ShapePathCommand::LineTo {
            x,
            y,
            mirror: None,
            corner_radius: None,
        }
    }
}

/// Inserts a single ordered mask parade into a fresh timeline `Layr`.
/// Construction and validation finish before the layer is mutated.
#[expect(dead_code, reason = "default-clock wrapper; callers pass a clock")]
pub(crate) fn apply(layer: &mut Chunk, masks: &[NativeMaskSpec]) -> Result<(), AepWriteError> {
    apply_with_clock(layer, masks, super::keyframes::PropertyClock::DEFAULT)
}

pub(crate) fn apply_with_clock(
    layer: &mut Chunk,
    masks: &[NativeMaskSpec],
    clock: super::keyframes::PropertyClock,
) -> Result<(), AepWriteError> {
    if masks.is_empty() {
        return Ok(());
    }
    if masks.len() > MAX_MASKS {
        return Err(AepWriteError::Invalid("native mask count exceeds u16"));
    }
    let parade = parade_with_clock(masks, clock)?;
    let records = layer.children_mut().ok_or(AepWriteError::Invalid(
        "native timeline layer is not a LIST",
    ))?;
    let root_count = records
        .iter()
        .filter(|record| record.list_kind() == Some(*b"tdgp"))
        .count();
    if root_count != 1 {
        return Err(AepWriteError::Invalid(
            "native timeline layer property root is not unique",
        ));
    }
    let root = records
        .iter_mut()
        .find(|record| record.list_kind() == Some(*b"tdgp"))
        .ok_or(AepWriteError::Invalid(
            "native timeline layer has no property root",
        ))?;
    let children = root
        .children_mut()
        .ok_or(AepWriteError::Invalid("native property root is opaque"))?;
    if has_match_name(children, "ADBE Mask Parade") {
        return Err(AepWriteError::Invalid(
            "native timeline layer already has masks",
        ));
    }
    let end = children
        .iter()
        .position(|chunk| match_name_is(chunk, "ADBE Group End"))
        .ok_or(AepWriteError::Invalid(
            "native property root has no group terminator",
        ))?;
    children.insert(end, match_name("ADBE Mask Parade")?);
    children.insert(end + 1, parade);
    Ok(())
}

#[expect(dead_code, reason = "default-clock wrapper; callers pass a clock")]
fn parade(masks: &[NativeMaskSpec]) -> Result<Chunk, AepWriteError> {
    parade_with_clock(masks, super::keyframes::PropertyClock::DEFAULT)
}

fn parade_with_clock(
    masks: &[NativeMaskSpec],
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let mut children = vec![
        Chunk::data(*b"tdsb", 1_u32.to_be_bytes())?,
        views::name_payload("-_0_/-")?,
    ];
    for (offset, mask) in masks.iter().enumerate() {
        validate(mask)?;
        let index = u16::try_from(offset + 1)
            .map_err(|_| AepWriteError::Invalid("native mask index exceeds u16"))?;
        children.push(match_name("ADBE Mask Atom")?);
        children.push(Chunk::data(*b"mkif", mask_info(mask, index))?);
        children.push(mask_properties_with_clock(mask, clock)?);
    }
    children.push(match_name("ADBE Group End")?);
    Ok(Chunk::list(*b"tdgp", children))
}

fn validate(mask: &NativeMaskSpec) -> Result<(), AepWriteError> {
    if mask.name.is_empty() || mask.name.len() > 255 || mask.name.contains('\0') {
        return Err(AepWriteError::Invalid(
            "native mask name must be 1..=255 UTF-8 bytes without NUL",
        ));
    }
    if mask.source_size.contains(&0) {
        return Err(AepWriteError::Invalid(
            "native mask source dimensions are zero",
        ));
    }
    if !mask.path.is_finite()
        || mask
            .feather
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
        || !mask.opacity.is_finite()
        || !(0.0..=1.0).contains(&mask.opacity)
        || !mask.expansion.is_finite()
    {
        return Err(AepWriteError::Invalid(
            "native mask has invalid static values",
        ));
    }
    if let Some(track) = &mask.path_track {
        super::validate_path_track(track)?;
    }
    validate_track(mask.feather_track.as_ref(), 2, |value| value >= 0.0)?;
    validate_track(mask.opacity_track.as_ref(), 1, |value| {
        (0.0..=100.0).contains(&value)
    })?;
    validate_track(mask.expansion_track.as_ref(), 1, |_| true)?;
    Ok(())
}

fn validate_track(
    track: Option<&NumericTrack>,
    dimensions: usize,
    accepts: impl Fn(f64) -> bool,
) -> Result<(), AepWriteError> {
    let Some(track) = track else { return Ok(()) };
    if track.keys.iter().any(|key| {
        key.values.len() != dimensions
            || key
                .values
                .iter()
                .any(|value| !value.is_finite() || !accepts(*value))
            || !key.spatial_in.is_empty()
            || !key.spatial_out.is_empty()
    }) {
        return Err(AepWriteError::Invalid(
            "native mask numeric track has invalid values",
        ));
    }
    Ok(())
}

fn mask_info(mask: &NativeMaskSpec, index: u16) -> [u8; 48] {
    // The reader-owned layout establishes these typed fields. Unused/reserved
    // fields are canonical zeroes in fresh records; no source payload is copied.
    let mut bytes = [0_u8; 48];
    bytes[0] = u8::from(mask.inverted);
    bytes[6..8].copy_from_slice(&mask.mode.ordinal().to_be_bytes());
    bytes[8..12].copy_from_slice(&u32::from(index).to_be_bytes());
    bytes
}

#[cfg(test)]
fn mask_properties(mask: &NativeMaskSpec) -> Result<Chunk, AepWriteError> {
    mask_properties_with_clock(mask, super::keyframes::PropertyClock::DEFAULT)
}

fn mask_properties_with_clock(
    mask: &NativeMaskSpec,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    use views::ValueKind;
    let normalized = normalized_path(&mask.path, mask.source_size);
    let path_property = if let Some(track) = &mask.path_track {
        let mut track = track.clone();
        for key in &mut track.keyframes {
            key.path = normalized_path(&key.path, mask.source_size);
        }
        path_geometry::animated_property(&track)?
    } else {
        path_geometry::property_with_clock(&normalized, clock)?
    };
    // The authoring boundary uses percentage keys; AE stores mask opacity in
    // normalized units, unlike its displayed 0..100 bounds. Easing is unitless.
    let opacity_track = mask.opacity_track.clone().map(|mut track| {
        for key in &mut track.keys {
            for value in &mut key.values {
                *value /= 100.0;
            }
        }
        track
    });
    views::group(
        1,
        &mask.name,
        vec![
            ("ADBE Mask Shape", path_property),
            (
                "ADBE Mask Feather",
                views::property_with_clock(
                    ValueKind::MaskFeather,
                    &mask.feather,
                    Some((0.0, 32_000.0)),
                    mask.feather_track.as_ref(),
                    clock,
                )?,
            ),
            (
                "ADBE Mask Opacity",
                views::property_with_clock(
                    ValueKind::MaskOpacity,
                    &[mask.opacity],
                    Some((0.0, 100.0)),
                    opacity_track.as_ref(),
                    clock,
                )?,
            ),
            (
                "ADBE Mask Offset",
                views::property_with_clock(
                    ValueKind::MaskExpansion,
                    &[mask.expansion],
                    Some((-32_000.0, 32_000.0)),
                    mask.expansion_track.as_ref(),
                    clock,
                )?,
            ),
        ],
    )
    .map_err(Into::into)
}

fn normalized_path(path: &ShapePath, size: [u32; 2]) -> ShapePath {
    let [sx, sy] = [f64::from(size[0]), f64::from(size[1])];
    let commands = path
        .commands
        .iter()
        .map(|command| match *command {
            ShapePathCommand::MoveTo {
                x,
                y,
                mirror,
                corner_radius,
            } => ShapePathCommand::MoveTo {
                x: x / sx,
                y: y / sy,
                mirror,
                corner_radius,
            },
            ShapePathCommand::LineTo {
                x,
                y,
                mirror,
                corner_radius,
            } => ShapePathCommand::LineTo {
                x: x / sx,
                y: y / sy,
                mirror,
                corner_radius,
            },
            ShapePathCommand::CubicTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
                mirror,
                corner_radius,
            } => ShapePathCommand::CubicTo {
                c1x: c1x / sx,
                c1y: c1y / sy,
                c2x: c2x / sx,
                c2y: c2y / sy,
                x: x / sx,
                y: y / sy,
                mirror,
                corner_radius,
            },
            ShapePathCommand::Close => ShapePathCommand::Close,
        })
        .collect();
    ShapePath { commands }
}

fn match_name(value: &str) -> Result<Chunk, AepWriteError> {
    if value.len() > 40 || !value.is_ascii() {
        return Err(AepWriteError::Invalid("invalid native mask match name"));
    }
    let mut bytes = [0_u8; 40];
    bytes[..value.len()].copy_from_slice(value.as_bytes());
    Ok(Chunk::data(*b"tdmn", bytes)?)
}

fn match_name_is(chunk: &Chunk, value: &str) -> bool {
    chunk.id() == *b"tdmn"
        && chunk.data_payload().is_some_and(|bytes| {
            bytes.get(..value.len()) == Some(value.as_bytes())
                && bytes
                    .get(value.len()..)
                    .is_some_and(|tail| tail.iter().all(|byte| *byte == 0))
        })
}

fn has_match_name(children: &[Chunk], value: &str) -> bool {
    children.iter().any(|chunk| match_name_is(chunk, value))
}

#[cfg(test)]
#[path = "masks/native_tests.rs"]
mod native_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_constructor_pins_owner_local_top_left_convention() {
        let mask = NativeMaskSpec::crop_rectangle("Crop", [1920, 1080], [10.0, 20.0, 300.0, 200.0])
            .unwrap();
        assert_eq!(mask.path.commands[0].endpoint(), Some((10.0, 20.0)));
        assert_eq!(mask.path.commands[2].endpoint(), Some((310.0, 220.0)));
        assert_eq!(mask.path.commands.last(), Some(&ShapePathCommand::Close));
    }

    #[test]
    fn mask_info_uses_stable_one_based_index_and_mode() {
        let mask =
            NativeMaskSpec::crop_rectangle("Guide", [100, 100], [0.0, 0.0, 10.0, 10.0]).unwrap();
        let bytes = mask_info(&mask, 7);
        assert_eq!(u16::from_be_bytes([bytes[6], bytes[7]]), 1);
        assert_eq!(u32::from_be_bytes(bytes[8..12].try_into().unwrap()), 7);
    }
}
