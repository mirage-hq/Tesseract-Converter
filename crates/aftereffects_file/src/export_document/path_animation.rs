//! Typed Path tracks shared by Shape and mask-guide export. Native records own
//! authored or explicitly baked keys; this writer does not resample the curve.

use fx_schema::{PropertyValue, ShapePath, ShapePathCommand};

use super::{NativeTrack, native_easing};
use crate::writer::{
    KeyframeEasing, NumericKeyframe, NumericTrack, PathKeyframe, PathTrack, split_path_track,
};

fn drawable_contours(path: &ShapePath) -> usize {
    path.commands
        .split(|command| matches!(command, ShapePathCommand::MoveTo { .. }))
        .filter(|commands| {
            commands.iter().any(|command| {
                matches!(
                    command,
                    ShapePathCommand::LineTo { .. } | ShapePathCommand::CubicTo { .. }
                )
            })
        })
        .count()
}

/// Native round-cap strokes can draw dots for an otherwise empty Path. Hide
/// the whole shared paint group when there are no segments, without changing
/// the layer's authored opacity or moving a fake shape outside the canvas.
pub(super) fn visibility(
    track: &PathTrack,
    has_stroke: bool,
) -> Result<Option<NumericTrack>, &'static str> {
    let count = |path: &ShapePath| {
        path.commands
            .iter()
            .filter(|command| matches!(command, ShapePathCommand::MoveTo { .. }))
            .count()
    };
    let maximum = track
        .keyframes
        .iter()
        .map(|key| count(&key.path))
        .max()
        .unwrap_or(0);
    if has_stroke
        && track.keyframes.iter().any(|key| {
            let drawn = drawable_contours(&key.path);
            drawn != 0 && (drawn != maximum || drawn != count(&key.path))
        })
    {
        return Err(
            "Stroked Path loses only some contours; absent native round-cap slots may draw dots, so partial disappearance is not yet supported",
        );
    }
    if track
        .keyframes
        .iter()
        .all(|key| drawable_contours(&key.path) != 0)
    {
        return Ok(None);
    }
    let mut keys = Vec::<NumericKeyframe>::new();
    for key in &track.keyframes {
        let opacity = if drawable_contours(&key.path) == 0 {
            0.0
        } else {
            100.0
        };
        if keys.last().is_some_and(|last| last.values == [opacity]) {
            continue;
        }
        keys.push(NumericKeyframe {
            time_millis: key.time_millis,
            values: vec![opacity],
            easing: vec![KeyframeEasing::Hold],
            spatial_in: vec![],
            spatial_out: vec![],
        });
    }
    Ok(Some(NumericTrack { keys }))
}

pub(super) fn track(source: Option<NativeTrack<'_>>) -> Result<Option<PathTrack>, &'static str> {
    let Some(source) = source else {
        return Ok(None);
    };
    if let NativeTrack::Keyframes(track) = &source
        && u16::try_from(track.keyframes().len()).is_err()
    {
        return Err("Path track exceeds the native key field");
    }
    let path = |value: &PropertyValue| match value {
        PropertyValue::Path(path) if path.is_finite() => Ok(path.clone()),
        _ => Err("Path animator contains a non-finite or non-Path value"),
    };
    let keyframes = match source {
        NativeTrack::Constant(value) => vec![PathKeyframe {
            time_millis: 0,
            path: path(value)?,
            easing: KeyframeEasing::Hold,
        }],
        NativeTrack::Keyframes(track) => track
            .keyframes()
            .iter()
            .map(|key| {
                Ok(PathKeyframe {
                    time_millis: key.layer_time().as_millis(),
                    path: path(key.value())?,
                    easing: native_easing(key.easing()),
                })
            })
            .collect::<Result<Vec<_>, &'static str>>()?,
    };
    let track = PathTrack { keyframes };
    split_path_track(&track).map_err(|_| {
        "Path keys exceed native field/geometry/timing bounds or use unsupported topology/easing; no sampled geometry is substituted"
    })?;
    Ok(Some(track))
}
