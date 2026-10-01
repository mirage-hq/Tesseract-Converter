//! AE26 parallel Path keys: 64-byte timing records and one `shap` per key.
//! The trailing native pointer slot is process-local and is written as zero.

#[cfg(test)]
mod tests;

use super::*;
use crate::writer::{KeyframeEasing, keyframes};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PathKeyframe {
    pub time_millis: i64,
    pub path: ShapePath,
    /// Incoming segment easing, matching the FX track convention.
    pub easing: KeyframeEasing,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PathTrack {
    pub keyframes: Vec<PathKeyframe>,
}

/// Split compound keyed geometry into independent native paths while retaining
/// each contour's original key times and shared paint scope.
pub(crate) fn split_path_track(track: &PathTrack) -> Result<Vec<PathTrack>, AepWriteError> {
    if track.keyframes.is_empty() || u16::try_from(track.keyframes.len()).is_err() {
        return Err(AepWriteError::Invalid(
            "native Path key count exceeds the native field",
        ));
    }
    let contours = track
        .keyframes
        .iter()
        .map(|key| super::contours(&key.path))
        .collect::<Result<Vec<_>, _>>()?;
    for (keys, paths) in track.keyframes.windows(2).zip(contours.windows(2)) {
        if paths[0].len() != paths[1].len() && keys[1].easing != KeyframeEasing::Hold {
            return Err(AepWriteError::Invalid(
                "animated compound Path changes contour count across a non-Hold segment",
            ));
        }
    }
    let count = contours.iter().map(Vec::len).max().unwrap_or(1);
    let mut tracks: Vec<_> = (0..count)
        .map(|_| PathTrack {
            keyframes: Vec::with_capacity(track.keyframes.len()),
        })
        .collect();
    for (key, mut contours) in track.keyframes.iter().zip(contours) {
        // Ordinal slots are only reused across held discontinuities. There is
        // no morph or claimed particle correspondence across births/deaths.
        contours.resize_with(count, super::empty_path);
        for (track, contour) in tracks.iter_mut().zip(contours) {
            track.keyframes.push(PathKeyframe {
                time_millis: key.time_millis,
                path: contour,
                easing: key.easing,
            });
        }
    }
    for track in &tracks {
        validate_path_track(track)?;
    }
    Ok(tracks)
}

pub(crate) fn validate_path_track(track: &PathTrack) -> Result<(), AepWriteError> {
    if track.keyframes.is_empty() || u16::try_from(track.keyframes.len()).is_err() {
        return Err(AepWriteError::Invalid(
            "native Path key count exceeds the native field",
        ));
    }
    for key in &track.keyframes {
        keyframes::time_units(key.time_millis)?;
        shape(&key.path)?;
    }
    for pair in track.keyframes.windows(2) {
        if pair[0].time_millis >= pair[1].time_millis {
            return Err(AepWriteError::Invalid(
                "native Path keys must increase in time",
            ));
        }
        if pair[1].easing != KeyframeEasing::Hold
            && (pair[0].path.commands.len() != pair[1].path.commands.len()
                || pair[0]
                    .path
                    .commands
                    .iter()
                    .zip(&pair[1].path.commands)
                    .any(|(a, b)| std::mem::discriminant(a) != std::mem::discriminant(b)))
        {
            return Err(AepWriteError::Invalid(
                "native Path morph requires matching command topology",
            ));
        }
        let side = sides(pair[1].easing)?;
        if side[0].0 != side[1].0 && pair[1].time_millis - pair[0].time_millis != 2000 {
            return Err(AepWriteError::Invalid(
                "mixed Path easing outside the pinned two-second segment is unproved",
            ));
        }
        if pair[1].easing != KeyframeEasing::Hold
            && folded_seam(&pair[0].path) != folded_seam(&pair[1].path)
        {
            return Err(AepWriteError::Invalid(
                "Path keys change native seam topology",
            ));
        }
    }
    Ok(())
}

// The static native encoder folds a terminal segment back to the MoveTo.
// A contour which only coincides at some keys would otherwise change the
// native vertex count despite a stable FX command layout.
fn folded_seam(path: &ShapePath) -> bool {
    if !matches!(path.commands.last(), Some(ShapePathCommand::Close)) {
        return false;
    }
    let Some(ShapePathCommand::MoveTo { x, y, .. }) = path.commands.first() else {
        return false;
    };
    matches!(path.commands.get(path.commands.len().saturating_sub(2)),
        Some(ShapePathCommand::LineTo {x: end_x,y: end_y,..} | ShapePathCommand::CubicTo {x: end_x,y: end_y,..})
        if x == end_x && y == end_y)
}

// Native nonnumeric Path ease has no established nonzero speed scale. Only
// independently pinned zero-speed Bezier handles and Linear/Hold are emitted.
fn sides(easing: KeyframeEasing) -> Result<[(u8, f64); 2], AepWriteError> {
    match easing {
        KeyframeEasing::Hold => Ok([(3, 0.0); 2]),
        KeyframeEasing::Linear => Ok([(1, 0.0); 2]),
        KeyframeEasing::CubicBezier { x1, y1, x2, y2 }
            if [x1, y1, x2, y2].iter().all(|v| v.is_finite())
                && (0.0..=1.0).contains(&x1)
                && (0.0..=1.0).contains(&x2)
                && (y1 == 0.0 || (y1 == x1 && x1 == 1.0 / 6.0))
                && (y2 == 1.0 || (y2 == x2 && x2 == 1.0 - 1.0 / 6.0)) =>
        {
            let linear_out = y1 != 0.0;
            let linear_in = y2 != 1.0;
            Ok([
                (
                    if linear_out { 1 } else { 2 },
                    if linear_out { 0.0 } else { x1 },
                ),
                (
                    if linear_in { 1 } else { 2 },
                    if linear_in { 0.0 } else { 1.0 - x2 },
                ),
            ])
        }
        _ => Err(AepWriteError::Invalid(
            "native Path nonzero-speed temporal easing is not established",
        )),
    }
}

pub(in crate::writer) fn animated_property(track: &PathTrack) -> Result<Chunk, AepWriteError> {
    validate_path_track(track)?;
    if track.keyframes.len() == 1 {
        return property(&track.keyframes[0].path);
    }
    let mut records = vec![vec![0u8; 64]; track.keyframes.len()];
    for (record, key) in records.iter_mut().zip(&track.keyframes) {
        record[..4].copy_from_slice(&keyframes::time_units(key.time_millis)?.to_be_bytes());
        record[4..8].copy_from_slice(&[1, 1, 0, 1]);
    }
    for (index, pair) in track.keyframes.windows(2).enumerate() {
        let [(out_kind, out_influence), (in_kind, in_influence)] = sides(pair[1].easing)?;
        records[index][5] = out_kind;
        records[index + 1][4] = in_kind;
        records[index][48..56].copy_from_slice(&out_influence.to_be_bytes());
        records[index + 1][32..40].copy_from_slice(&in_influence.to_be_bytes());
        let duration = (pair[1].time_millis - pair[0].time_millis) as f64 / 1000.0;
        records[index][16..24].copy_from_slice(&duration.to_be_bytes());
    }
    let mut descriptor = StaticPropertyRecord::new(1, 6, 1, 0x20007, 0x10008, 0, false).encode();
    descriptor[68] = 1;
    Ok(Chunk::list(
        *b"om-s",
        vec![
            Chunk::list(
                *b"tdbs",
                vec![
                    Chunk::data(*b"tdsb", 1_u32.to_be_bytes())?,
                    views::name_payload("-_0_/-")?,
                    Chunk::data(*b"tdb4", descriptor)?,
                    keyframes::keyframe_list(records.len(), 64, records.into_iter())?,
                ],
            ),
            Chunk::list(
                *b"omks",
                track
                    .keyframes
                    .iter()
                    .map(|key| shape(&key.path))
                    .collect::<Result<_, _>>()?,
            ),
        ],
    ))
}
