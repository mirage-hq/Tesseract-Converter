//! The sequence end: the last track item end of the selected sequence, converted or not.
//!
//! Inferred from Adobe Media Encoder's `exportSequence` probes, including items
//! this reader omits: the work area (`MZ.WorkInPoint`/`MZ.WorkOutPoint`) and
//! sequence-source `OriginalDuration` do not set the rendered length. See
//! `tests/README.md`, "Sequence end evidence".
//!
//! The lookup is lenient on purpose. An item that the typed occurrence reader
//! omits still extends the timeline, so its `ClipTrackItem/TrackItem` range is
//! read from the element tree without decoding the record. An item whose range
//! cannot be read, or is not `0 <= Start < End`, does not fail conversion; the
//! occurrence reader already reports it, and the end comes from the remaining
//! items. An end off the sequence frame grid snaps to the nearest frame, ties
//! forward, like an exported clip boundary. Whether AME renders such an end to
//! the nearest or to the next frame is unverified.

use crate::{
    format::{graph::Element, Graph, Record},
    schema::FrameRate,
};

/// The last track item end in the video track `group`, snapped to the sequence
/// frame grid, or 0 when no item range can be read.
///
/// Converted audio extends this end exactly in `PrSequence::end_ticks`; omitted
/// audio does not extend it.
pub(super) fn read_sequence_end(
    graph: &Graph<'_>,
    group: Record<'_>,
    frame_rate: FrameRate,
) -> i64 {
    let frame = frame_rate.ticks_per_frame();
    group
        .track_references()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|reference| graph.locate(&reference, "track group").ok())
        .flat_map(|track| clip_item_references(graph, track))
        .filter_map(|item| item_end(item.element()))
        .filter_map(|end| nearest_frame_boundary(end, frame))
        .max()
        .unwrap_or(0)
}

/// Located `ClipTrack/ClipItems/TrackItems/*` records of one track.
fn clip_item_references<'a>(
    graph: &'a Graph<'_>,
    track: Record<'a>,
) -> impl Iterator<Item = Record<'a>> + 'a {
    track
        .element()
        .child("ClipTrack")
        .and_then(|clip_track| clip_track.child("ClipItems"))
        .and_then(|items| items.child("TrackItems"))
        .into_iter()
        .flat_map(Element::children)
        .filter_map(move |item| graph.locate(&item.reference(), "track items").ok())
}

/// The `End` of an item whose `ClipTrackItem/TrackItem` range satisfies
/// `0 <= Start < End`. A missing `Start` is 0, as in the typed occurrence reader.
fn item_end(item: Element<'_>) -> Option<i64> {
    let range = item.child("ClipTrackItem")?.child("TrackItem")?;
    let start = match range.child("Start") {
        Some(start) => ticks(start)?,
        None => 0,
    };
    let end = ticks(range.child("End")?)?;
    (0 <= start && start < end).then_some(end)
}

fn ticks(element: Element<'_>) -> Option<i64> {
    element.text()?.trim().parse().ok()
}

/// The nearest multiple of `frame` to a positive tick count, ties forward.
fn nearest_frame_boundary(ticks: i64, frame: i64) -> Option<i64> {
    (ticks.checked_add(frame / 2)? / frame).checked_mul(frame)
}
