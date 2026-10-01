//! Premiere caption tracks: timed caption items in a sequence's data track
//! group.
//!
//! Each `CaptionDataClipTrackItem` carries its timeline range, a synthetic
//! caption source chain, and one `Block` whose `FormattedTextData` is the
//! Source Text payload with caption document markers (`text_payload`). Every
//! cue stores a full style copy, and so does the track's
//! `CaptionDataTemplateStyle`. That the template is the style for new captions
//! and that Premiere renders each cue from its own copy is inferred: the
//! corpus template and cues carry the same style, so it cannot show which one
//! wins. Until Adobe evidence exists, a cue converts only when its style (text
//! aside) equals its track's template. Captions convert to timed text graphics
//! and never export as caption tracks.
//!
//! Record shapes, class IDs and versions are those Premiere 25.5 wrote (project
//! Version 43) in the `practice_files_transcription_magic` corpus project
//! (sequence `Practice-Sequence_CAPTIONS-ONLY`, 42 cues). No Premiere 26
//! caption sample exists. The reader dispatches on the tag only.

use super::records::XmlRecordDefinition;

pub(crate) const CAPTION_DATA_CLIP_TRACK: XmlRecordDefinition = XmlRecordDefinition::new(
    "CaptionDataClipTrack",
    "b9d20db2-f229-482d-87fd-10f1fa157107",
    "1",
);
pub(crate) const CAPTION_DATA_CLIP_TRACK_ITEM: XmlRecordDefinition = XmlRecordDefinition::new(
    "CaptionDataClipTrackItem",
    "541ba122-fe61-4350-abe5-040ed395006e",
    "3",
);
pub(crate) const DATA_COMPONENT_CHAIN: XmlRecordDefinition = XmlRecordDefinition::new(
    "DataComponentChain",
    "1d83b349-453e-4099-801d-6b23edae1724",
    "1",
);
pub(crate) const TRANSCRIPT_CLIP: XmlRecordDefinition = XmlRecordDefinition::new(
    "TranscriptClip",
    "9e0179bb-153c-4884-b34b-eb7082f34384",
    "2",
);
pub(crate) const DATA_MEDIA_SOURCE: XmlRecordDefinition = XmlRecordDefinition::new(
    "DataMediaSource",
    "ff36343e-4ece-4d37-ab61-e99b758f9d30",
    "1",
);
pub(crate) const DATA_STREAM: XmlRecordDefinition =
    XmlRecordDefinition::new("DataStream", "9e4e76eb-b72f-4b9b-9ea8-2887b1cc24fd", "1");
pub(crate) const BLOCK: XmlRecordDefinition =
    XmlRecordDefinition::new("Block", "d3782b80-516f-47e3-a7e8-e83779f0ed01", "1");

/// The generator token a caption's synthetic media stores where a file path
/// would go. Its implementation ID is the graphic generator's.
pub(crate) const CAPTION_MEDIA_TOKEN: &str = "1396920390";
