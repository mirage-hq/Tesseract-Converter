/// Timing fields of `one-clip.xml` in Adobe ticks. `Default` is the stored file:
/// 30 fps sequence and media, a 10 s source, and a 5 s clip at the start.
pub(crate) struct OneClip {
    pub(crate) sequence_frame: i64,
    pub(crate) media_frame: i64,
    pub(crate) media_duration: i64,
    pub(crate) start: i64,
    pub(crate) end: i64,
    pub(crate) in_point: i64,
    pub(crate) out_point: i64,
}

impl Default for OneClip {
    fn default() -> Self {
        Self {
            sequence_frame: TICKS / 30,
            media_frame: TICKS / 30,
            media_duration: 10 * TICKS,
            start: 0,
            end: 5 * TICKS,
            in_point: 0,
            out_point: 5 * TICKS,
        }
    }
}

/// Renders `one-clip.xml` with new timing. Each field has one XML location, so
/// a changed fixture fails here instead of leaving a stale value in place.
pub(crate) fn one_clip_xml(clip: OneClip) -> String {
    let stored = OneClip::default();
    let start = match clip.start {
        0 => String::new(),
        ticks => format!("<Start>{ticks}</Start>"),
    };
    [
        (
            format!("</Tracks><FrameRate>{}</FrameRate>", stored.sequence_frame),
            format!("</Tracks><FrameRate>{}</FrameRate>", clip.sequence_frame),
        ),
        (
            format!("<TrackItem><End>{}</End>", stored.end),
            format!("<TrackItem>{start}<End>{}</End>", clip.end),
        ),
        (
            format!(
                "<InPoint>0</InPoint><OutPoint>{}</OutPoint>",
                stored.out_point
            ),
            format!(
                "<InPoint>{}</InPoint><OutPoint>{}</OutPoint>",
                clip.in_point, clip.out_point
            ),
        ),
        (
            format!(
                "<OriginalDuration>{}</OriginalDuration>",
                stored.media_duration
            ),
            format!("<OriginalDuration>{}</OriginalDuration>", clip.media_duration),
        ),
        (
            format!(
                "<Duration>{}</Duration><FrameRate>{}</FrameRate>",
                stored.media_duration, stored.media_frame
            ),
            format!(
                "<Duration>{}</Duration><FrameRate>{}</FrameRate>",
                clip.media_duration, clip.media_frame
            ),
        ),
    ]
    .into_iter()
    .fold(
        include_str!("../fixtures/one-clip.xml").to_owned(),
        |xml, (stored, new)| {
            assert_eq!(xml.matches(&stored).count(), 1, "one-clip.xml: {stored}");
            xml.replacen(&stored, &new, 1)
        },
    )
}

/// An editable remap on its authored parent clock, with an independent window.
pub(crate) fn remapped_playback(window: Value, property: Value) -> Value {
    json!({
        "type": "windowed", "inputRange": window,
        "mapping": {"type": "timeRemap", "property": property}, "inputOffsetMs": 0
    })
}
