//! Typed Path tracks shared by Shape and mask-guide export. Native records own
//! authored or explicitly baked keys; this writer does not resample the curve.

use fx_schema::{PropertyValue, ShapePath, ShapePathCommand};

use super::{NativeTrack, native_easing};
use crate::writer::{
    KeyframeEasing, NumericKeyframe, NumericTrack, PathKeyframe, PathTrack, split_path_track,
};

pub(super) const PARTIAL_STROKE_DIAGNOSTIC: &str = "Partial-contour disappearance approximated with per-contour native Stroke controls instead of the original shared Stroke; overlap/AA coverage may differ. Fill, when present, keeps one compound paint scope but native Fill/Stroke Path edits are separate. Best-effort editable export, not fidelity proof";

/// Preserve a Normal Fill stack together, then draw separately hidden contours
/// with one opaque static Stroke. Native Path/Stroke edit controls are separate;
/// reject modifiers and unproved paint-scope combinations.
pub(super) fn partial_stroke_slots(
    contents: &[crate::writer::VectorContent],
) -> Option<Vec<crate::writer::VectorContent>> {
    use crate::writer::{
        GeometryAnimations, VectorContent, VectorGeometry, VectorGroupAnimations, VectorGroupSpec,
        VectorGroupTransform, VectorPaintAnimations, VectorPaintSpec,
    };
    let (geometry, paints) = contents.split_first()?;
    let VectorContent::Geometry {
        geometry: VectorGeometry::Path(_),
        animations: geometry_keys,
    } = geometry
    else {
        return None;
    };
    let (last, fills) = paints.split_last()?;
    let VectorContent::Paint(
        stroke @ VectorPaintSpec::Stroke {
            paint: fx_schema::ShapePaint::Solid { color },
            blend_mode,
            opacity,
            dashes,
            animations,
            ..
        },
    ) = last
    else {
        return None;
    };
    if !fills.iter().all(|content| {
        matches!(content,
            VectorContent::Paint(VectorPaintSpec::Fill { blend_mode, .. })
                if *blend_mode == Default::default()
        )
    }) {
        return None;
    }
    if color[3] != 1.0
        || *opacity != 100.0
        || *blend_mode != Default::default()
        || *dashes != Default::default()
        || *animations != VectorPaintAnimations::default()
        || geometry_keys.has_parametric_tracks()
    {
        return None;
    }
    let slots = split_path_track(geometry_keys.path.as_ref()?).ok()?;
    let strokes: Vec<_> = slots
        .into_iter()
        .enumerate()
        .map(|(index, track)| {
            let opacity = visibility(&track, false).ok()?;
            let path = track.keyframes.first()?.path.clone();
            Some(VectorContent::AnimatedGroup(
                VectorGroupSpec {
                    name: format!("Path contour {} visibility", index + 1),
                    transform: VectorGroupTransform::default(),
                    blend_mode: Default::default(),
                    contents: vec![
                        VectorContent::Geometry {
                            geometry: VectorGeometry::Path(path),
                            animations: GeometryAnimations {
                                path: Some(track),
                                ..Default::default()
                            },
                        },
                        VectorContent::Paint(stroke.clone()),
                    ],
                },
                VectorGroupAnimations {
                    opacity,
                    ..Default::default()
                },
            ))
        })
        .collect::<Option<_>>()?;
    let mut result = Vec::with_capacity(strokes.len() + usize::from(!fills.is_empty()));
    if !fills.is_empty() {
        // Keep compound winding and Fill paint ordering in one scope. Drawing
        // this group first also prevents outer Fill inheritance into the slots.
        result.push(VectorContent::Group(VectorGroupSpec {
            name: "Compound Path Fill".into(),
            transform: VectorGroupTransform::default(),
            blend_mode: Default::default(),
            contents: std::iter::once(geometry.clone())
                .chain(fills.iter().cloned())
                .collect(),
        }));
    }
    result.extend(strokes);
    Some(result)
}

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
    may_paint_empty_slots: bool,
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
    if may_paint_empty_slots
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
