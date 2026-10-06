//! The sequence end comes from the last track item, converted or not.
use crate::{
    format::{inspect_project, inspect_project_with_omissions, FrameRate},
    schema::TICKS,
    Omission, OmissionScope,
};

const SOURCE: &str = include_str!("../../../tests/fixtures/one-clip.xml");
const FRAME_30: i64 = FrameRate::Fps30.ticks_per_frame();

/// Adds a second track whose only item is a graphics-style track item: its
/// `TrackItem` carries a `Node`, which the occurrence reader reports, as Adobe
/// writes for titles (`type_title.prproj`, item 75). Its source remains 5 s, so a
/// longer `range` is omitted for a mismatched source span. `range` is the item's
/// `Start`/`End` markup.
fn with_omitted_tail_range(range: &str) -> String {
    SOURCE
        .replace(
            "<Track ObjectURef=\"track-1\"/>",
            "<Track ObjectURef=\"track-1\"/><Track ObjectURef=\"track-2\"/>",
        )
        .replace(
            "</PremiereData>",
            &format!(
                r#"
  <VideoClipTrack ObjectUID="track-2"><ClipTrack><Track><ID>2</ID><Index>1</Index></Track><ClipItems><Index>1</Index><TrackItems><TrackItem ObjectRef="9"/></TrackItems></ClipItems></ClipTrack></VideoClipTrack>
  <VideoClipTrackItem ObjectID="9"><ClipTrackItem><ComponentOwner><Components ObjectRef="4"/></ComponentOwner><TrackItem><Node><Properties><MZ.SequenceActions.NextShapeComponentNumber>2</MZ.SequenceActions.NextShapeComponentNumber></Properties></Node>{range}</TrackItem><SubClip ObjectRef="5"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio></VideoClipTrackItem>
</PremiereData>"#
            ),
        )
}

fn with_omitted_tail_item(end_ticks: i64) -> String {
    with_omitted_tail_range(&format!("<Start>0</Start><End>{end_ticks}</End>"))
}

/// Whether the occurrence reader reported `record` at `scope`.
fn reported(omissions: &[Omission], scope: OmissionScope, record: &str) -> bool {
    omissions
        .iter()
        .any(|item| item.scope == scope && item.record == record)
}

#[test]
fn recovered_tail_item_keeps_selection_and_ends_the_sequence() {
    let (project, omissions) =
        inspect_project_with_omissions(&with_omitted_tail_item(10 * TICKS), None).unwrap();
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.video_occurrences().count(), 2);
    assert_eq!(sequence.occurrence_end_ticks(), 10 * TICKS);
    assert_eq!(sequence.timeline_end_ticks, 10 * TICKS);
    assert!(sequence.gaps(&project.media).is_empty());
    let tail = sequence.video_tracks[1].clip(0);
    assert_eq!(tail.source_ticks(), 0..5 * TICKS);
    assert_eq!(tail.playback_rate, 0.5);
    assert!(
        omissions
            .iter()
            .any(|item| item.scope == OmissionScope::Feature
                && item.record == "VideoClipTrackItem:9"
                && item.reason.contains("constant speed")),
        "the recovered tail is reported, not silently replaced by black: {omissions:?}"
    );
    assert!(reported(
        &omissions,
        OmissionScope::Feature,
        "VideoClipTrackItem:9"
    ));
    // Adobe omits a zero `Start`; it reads as 0, as in the typed occurrence reader.
    let source = with_omitted_tail_range(&format!("<End>{}</End>", 10 * TICKS));
    assert_eq!(
        inspect_project(&source, None).unwrap().timeline_end_ticks,
        10 * TICKS
    );
}

#[test]
fn off_grid_item_ends_snap_to_the_nearest_sequence_frame_ties_forward() {
    // Exported clip boundaries snap the same way. Whether AME renders a
    // mid-frame item end to the nearest or to the next frame is unverified.
    let half_frame = FRAME_30 / 2;
    for (end, expected) in [
        (10 * TICKS + 1, 10 * TICKS),
        (10 * TICKS + half_frame - 1, 10 * TICKS),
        (10 * TICKS + half_frame, 10 * TICKS + FRAME_30),
        (10 * TICKS + FRAME_30 - 1, 10 * TICKS + FRAME_30),
    ] {
        let (project, omissions) =
            inspect_project_with_omissions(&with_omitted_tail_item(end), None).unwrap();
        let sequence = project.single_sequence().unwrap();
        assert_eq!(sequence.timeline_end_ticks, expected, "End {end}");
        assert!(
            reported(&omissions, OmissionScope::Occurrence, "9"),
            "{omissions:?}"
        );
    }
}

#[test]
fn unreadable_or_invalid_item_ranges_fall_back_to_the_remaining_items() {
    let range = |start: i64, end: i64| format!("<Start>{start}</Start><End>{end}</End>");
    for (case, source, omitted) in [
        (
            "unreadable Start",
            with_omitted_tail_range(&format!("<Start>earlier</Start><End>{}</End>", 10 * TICKS)),
            "9",
        ),
        (
            "unreadable End",
            with_omitted_tail_range("<Start>0</Start><End>later</End>"),
            "9",
        ),
        (
            "End overflows frame snapping",
            with_omitted_tail_item(i64::MAX),
            "9",
        ),
        (
            "missing End",
            with_omitted_tail_range("<Start>0</Start>"),
            "9",
        ),
        (
            "Start after End",
            with_omitted_tail_range(&range(12 * TICKS, 10 * TICKS)),
            "9",
        ),
        (
            "Start at End",
            with_omitted_tail_range(&range(10 * TICKS, 10 * TICKS)),
            "9",
        ),
        (
            "negative Start",
            with_omitted_tail_range(&range(-FRAME_30, 10 * TICKS)),
            "9",
        ),
        (
            "unresolved item reference",
            with_omitted_tail_item(10 * TICKS).replace(
                "<TrackItem ObjectRef=\"9\"/>",
                "<TrackItem ObjectRef=\"999\"/>",
            ),
            "999",
        ),
    ] {
        let (project, omissions) = inspect_project_with_omissions(&source, None).unwrap();
        let sequence = project.single_sequence().unwrap();
        assert_eq!(sequence.timeline_end_ticks, 5 * TICKS, "{case}");
        assert_eq!(sequence.occurrence_end_ticks(), 5 * TICKS, "{case}");
        assert!(
            reported(&omissions, OmissionScope::Occurrence, omitted),
            "{case}: {omissions:?}"
        );
    }
}
