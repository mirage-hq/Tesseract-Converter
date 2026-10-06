//! Caption track records. Conversion reads them and exports captions as
//! graphics, so no writer constructs these types. Records whose unknown
//! children could carry caption content deny them; declared attributes that
//! conversion does not use are ignored.

use super::{
    Clip, ClipTrack, ClipTrackItem, EncodedValue, MediaSource, MotionComponents, Reference,
    ReferenceList,
};
use serde::{de::IgnoredAny, Deserialize, Deserializer};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct DataClipTrack {
    #[serde(rename = "@Version")]
    _version: Option<IgnoredAny>,
    pub(crate) clip_track: Option<ClipTrack>,
}

/// One timed caption: its placement on the track and its text blocks.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct CaptionDataClipTrackItem {
    pub(crate) data_clip_track_item: Option<DataClipTrackItem>,
    pub(crate) block_vector: Option<BlockVector>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct DataClipTrackItem {
    #[serde(rename = "@Version")]
    _version: Option<IgnoredAny>,
    pub(crate) clip_track_item: Option<ClipTrackItem>,
}

/// A caption's text block references (`BlockVectorItem` children).
#[derive(Debug)]
pub(crate) struct BlockVector {
    pub(crate) items: Vec<Reference>,
}

impl<'de> Deserialize<'de> for BlockVector {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self {
            items: ReferenceList::deserialize(deserializer)?.items,
        })
    }
}

/// A caption item's effect chain; Premiere writes it without components.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DataComponentChain {
    pub(crate) component_chain: Option<DataChain>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct DataChain {
    #[serde(rename = "@Version")]
    _version: Option<IgnoredAny>,
    pub(crate) components: Option<MotionComponents>,
}

/// The clip that places a caption's synthetic source.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct TranscriptClip {
    #[serde(rename = "@ObjectID")]
    _object_id: Option<IgnoredAny>,
    #[serde(rename = "@ClassID")]
    _class_id: Option<IgnoredAny>,
    #[serde(rename = "@Version")]
    _version: Option<IgnoredAny>,
    pub(crate) data_clip: Option<DataClip>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct DataClip {
    #[serde(rename = "@Version")]
    _version: Option<IgnoredAny>,
    pub(crate) clip: Option<Clip>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DataMediaSource {
    pub(crate) media_source: Option<MediaSource>,
    pub(crate) original_duration: Option<String>,
}

/// The stream of a caption's synthetic media.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DataStream {
    pub(crate) frame_rate: Option<String>,
    pub(crate) duration: Option<String>,
}

/// One caption text block.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct Block {
    #[serde(rename = "@ObjectID")]
    _object_id: Option<IgnoredAny>,
    #[serde(rename = "@ClassID")]
    _class_id: Option<IgnoredAny>,
    #[serde(rename = "@Version")]
    _version: Option<IgnoredAny>,
    pub(crate) formatted_text_data: Option<EncodedValue>,
}
