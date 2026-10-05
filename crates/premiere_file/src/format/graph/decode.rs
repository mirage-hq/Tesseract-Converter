//! Selected native records decoded during graph traversal (not a second schema).

use super::InputRecord;
use crate::schema::{caption, native::*, records, text};

macro_rules! input_record {
    ($ty:ty, $tag:expr) => {
        impl InputRecord for $ty {
            const TAG: &'static str = $tag.tag;
        }
    };
}

input_record!(MasterClip, records::MASTER_CLIP);
input_record!(Media, records::MEDIA);
input_record!(VideoMediaSource, records::VIDEO_MEDIA_SOURCE);
input_record!(VideoStream, records::VIDEO_STREAM);
input_record!(Sequence, records::SEQUENCE);
input_record!(VideoTrackGroup, records::VIDEO_TRACK_GROUP);
input_record!(VideoClipTrack, records::VIDEO_CLIP_TRACK);
input_record!(VideoClipTrackItem, records::VIDEO_CLIP_TRACK_ITEM);
input_record!(
    VideoTransitionTrackItem,
    records::VIDEO_TRANSITION_TRACK_ITEM
);
input_record!(SubClip, records::SUB_CLIP);
input_record!(VideoClip, records::VIDEO_CLIP);
input_record!(Markers, records::MARKERS);
input_record!(VideoComponentChain, records::VIDEO_COMPONENT_CHAIN);
input_record!(VideoFilterComponent, records::VIDEO_FILTER_COMPONENT);
input_record!(TimeRemapping, records::TIME_REMAPPING);
input_record!(TimeComponentParam, records::TIME_COMPONENT_PARAM);
input_record!(AudioTrackGroup, records::AUDIO_TRACK_GROUP);
input_record!(AudioClipTrack, records::AUDIO_CLIP_TRACK);
input_record!(AudioMixTrack, records::AUDIO_MIX_TRACK);
input_record!(AudioClipTrackItem, records::AUDIO_CLIP_TRACK_ITEM);
input_record!(
    AudioTransitionTrackItem,
    records::AUDIO_TRANSITION_TRACK_ITEM
);
input_record!(AudioClip, records::AUDIO_CLIP);
input_record!(AudioMediaSource, records::AUDIO_MEDIA_SOURCE);
input_record!(AudioStream, records::AUDIO_STREAM);
input_record!(AudioComponentChain, records::AUDIO_COMPONENT_CHAIN);
input_record!(AudioFader, records::AUDIO_FADER);
input_record!(AudioComponentParam, records::SCALAR_PARAM);
input_record!(
    StereoToStereoPanProcessor,
    records::STEREO_TO_STEREO_PAN_PROCESSOR
);
input_record!(SecondaryContent, records::SECONDARY_CONTENT);
input_record!(ArbVideoComponentParam, text::SOURCE_TEXT_PARAM);
input_record!(CaptionDataClipTrack, caption::CAPTION_DATA_CLIP_TRACK);
input_record!(
    CaptionDataClipTrackItem,
    caption::CAPTION_DATA_CLIP_TRACK_ITEM
);
input_record!(DataComponentChain, caption::DATA_COMPONENT_CHAIN);
input_record!(TranscriptClip, caption::TRANSCRIPT_CLIP);
input_record!(DataMediaSource, caption::DATA_MEDIA_SOURCE);
input_record!(DataStream, caption::DATA_STREAM);
input_record!(Block, caption::BLOCK);
