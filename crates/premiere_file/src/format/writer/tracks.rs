pub(super) mod animation;
pub(super) mod audio;
pub(super) mod effects;
pub(super) mod graphic;
pub(super) mod mask;
pub(super) mod nested;
pub(super) mod video;

use crate::schema::{
    native::{
        ClipItems, ClipTrack, IndexedRef, Node, Track, TrackItems, TrackProperties, TransitionItems,
    },
    records,
};

/// The `StartKeyframe` of a scalar parameter whose static value is `value`.
fn scalar_start_keyframe(value: impl std::fmt::Display) -> String {
    format!("{},{value},0,0,0,0,0,0", records::STATIC_KEYFRAME_TIME)
}

/// The `StartKeyframe` of a point parameter whose static value is `value`,
/// written `x:y`.
fn point_start_keyframe(value: impl std::fmt::Display) -> String {
    format!(
        "{},{value},0,0,0,0,0,0,5,4,0,0,0,0",
        records::STATIC_KEYFRAME_TIME
    )
}

#[derive(Clone, Copy)]
enum MediaKind {
    Video,
    Audio,
}

impl MediaKind {
    const fn id(self) -> &'static str {
        match self {
            Self::Video => records::VIDEO_MEDIA,
            Self::Audio => records::AUDIO_MEDIA,
        }
    }
}

/// The persistent `Track/ID` that the writer gives the video track at `index`,
/// which a written Track Matte Key's Matte names.
pub(super) fn video_track_id(index: usize) -> usize {
    index + 1
}

fn clip_track<T>(
    kind: MediaKind,
    track_id: usize,
    index: usize,
    placements: Vec<IndexedRef<T>>,
) -> ClipTrack {
    let audio = matches!(kind, MediaKind::Audio);
    let media_type = kind.id().to_owned();
    let index_text = index.to_string();
    ClipTrack {
        version: Some("2".to_owned()),
        track: Some(Track {
            version: Some(records::TRACK_VERSION.to_owned()),
            node: Node {
                version: records::NODE_VERSION,
                properties: TrackProperties {
                    version: records::PROPERTIES_VERSION,
                    expanded: records::TL_SQ_TRACK_EXPANDED,
                    expanded_height: records::TL_SQ_TRACK_EXPANDED_HEIGHT,
                    source_track_state: "0",
                    source_track_number: index,
                    targeted: "1",
                    shy: (index < records::INITIAL_VISIBLE_TRACK_COUNT).then_some("0"),
                    audio_keyframe_style: (audio && index < records::INITIAL_VISIBLE_TRACK_COUNT)
                        .then_some("0"),
                    keyframe_mode: audio.then_some("true"),
                },
                id: None,
            }
            .into(),
            id: Some(track_id.to_string()),
            media_type: Some(media_type.clone()),
            index: Some(index_text.clone()),
            is_muted: None,
            _name: None,
        }),
        clip_items: Some(ClipItems {
            version: Some("3".to_owned()),
            track_items: (!placements.is_empty()).then(|| TrackItems::from_indexed(placements)),
            media_type: Some(media_type.clone()),
            index: Some(index_text.clone()),
        }),
        transition_items: Some(TransitionItems {
            version: Some("3".to_owned()),
            track_items: None,
            media_type: Some(media_type),
            index: Some(index_text),
        }),
    }
}
