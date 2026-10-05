use super::{
    AudioClipTrack, ComponentOwner, IndexedRef, Node, ObjectId, Reference, ReferenceList,
    RetainedOrSkipped, URef, Uid, VideoClipTrackId,
};
use crate::schema::records;
use serde::{de::IgnoredAny, Deserialize, Deserializer, Serialize};
use std::marker::PhantomData;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct Content {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    /// Premiere CS6 to CC 2015 save content boundaries on every media source
    /// (256 corpus sources, all unset: both boundaries hold
    /// [`UNSET_CONTENT_BOUNDARY`]). A real boundary would trim the usable
    /// media, so [`Self::require_unbounded`] rejects any other present pair.
    /// `BoundariesAreHard`, `ProxyEnabled` and the empty `Node` do not change
    /// conversion; the writer writes none of these fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) start_boundary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) end_boundary: Option<String>,
    #[serde(rename = "BoundariesAreHard", skip_serializing)]
    pub(crate) _boundaries_are_hard: Option<IgnoredAny>,
    #[serde(rename = "ProxyEnabled", skip_serializing)]
    pub(crate) _proxy_enabled: Option<IgnoredAny>,
    /// NativeMediaScope validates this alternate edge; playback still requires
    /// MediaSource/Media. Admission here must not select or export proxy content.
    #[serde(rename = "ProxyMedia", skip_serializing)]
    pub(crate) _proxy_media: Option<Reference>,
    /// Like ProxyMedia, these are alternate edges, not the original channel
    /// selectors. NativeMediaScope resolves their AudioProxy/ProxyMedia targets.
    #[serde(skip_serializing)]
    pub(crate) audio_proxies: Option<AudioProxies>,
    #[serde(rename = "Node", skip_serializing)]
    pub(crate) _node: Option<IgnoredAny>,
}

/// Saved audio preview attachments. Their indices do not select primary sound.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AudioProxies {
    #[serde(rename = "@Version")]
    pub(crate) _version: Option<String>,
    #[serde(rename = "AudioProxyItem", default)]
    pub(crate) items: Vec<Reference>,
}

impl Content {
    /// The writer's content record: a version and nothing else.
    pub(crate) fn versioned(version: String) -> Self {
        Self {
            version: Some(version),
            start_boundary: None,
            end_boundary: None,
            _boundaries_are_hard: None,
            _proxy_enabled: None,
            _proxy_media: None,
            audio_proxies: None,
            _node: None,
        }
    }

    /// Rejects a legacy content boundary that would trim the media: only the
    /// unset sentinel pair, or no boundaries at all, is accepted.
    pub(crate) fn require_unbounded(&self, identity: &str) -> crate::error::Result<()> {
        let unset = Some(UNSET_CONTENT_BOUNDARY);
        let boundaries = (self.start_boundary.as_deref(), self.end_boundary.as_deref());
        crate::error::ensure!(
            boundaries == (None, None) || boundaries == (unset, unset),
            "{identity}: media content boundaries are not converted"
        );
        Ok(())
    }
}

/// The value Premiere CS6 to CC 2015 write for an unset content boundary:
/// -400000 seconds at 254016000000 ticks per second.
pub(crate) const UNSET_CONTENT_BOUNDARY: &str = "-101606400000000000";

#[derive(Debug)]
pub(crate) enum SequenceId {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct SequenceSourceBody {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    pub(crate) content: Content,
    pub(crate) sequence: URef<SequenceId>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AudioSequenceSource {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<AudioSequenceSource>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "SequenceSource")]
    pub(crate) source: SequenceSourceBody,
    pub(crate) original_duration: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct VideoSequenceSource {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<VideoSequenceSource>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "SequenceSource")]
    pub(crate) source: SequenceSourceBody,
    pub(crate) original_duration: i64,
}

#[derive(Debug, Serialize)]
pub(crate) struct SequenceProperties {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "AMM.CurrentSolo")]
    pub(crate) current_solo: &'static str,
    #[serde(rename = "TL.SQTimePerPixel")]
    pub(crate) time_per_pixel: &'static str,
    #[serde(rename = "Monitor.ProgramZoomIn")]
    pub(crate) monitor_zoom_in: &'static str,
    #[serde(rename = "Monitor.ProgramZoomOut")]
    pub(crate) monitor_zoom_out: &'static str,
    #[serde(rename = "TL.SQHeaderWidth")]
    pub(crate) header_width: &'static str,
    #[serde(rename = "TL.SQVisibleBaseTime")]
    pub(crate) visible_base_time: &'static str,
    #[serde(rename = "TL.SQVideoVisibleBase")]
    pub(crate) video_visible_base: &'static str,
    #[serde(rename = "TL.SQAudioVisibleBase")]
    pub(crate) audio_visible_base: &'static str,
    #[serde(rename = "TL.SQDataVisibleBase")]
    pub(crate) data_visible_base: &'static str,
    #[serde(rename = "TL.SQHideShyTracks")]
    pub(crate) hide_shy_tracks: &'static str,
    #[serde(rename = "TL.SQAVDividerPosition")]
    pub(crate) av_divider_position: &'static str,
    #[serde(rename = "MZ.WorkInPoint")]
    pub(crate) work_in_point: &'static str,
    #[serde(rename = "MZ.WorkOutPoint")]
    pub(crate) work_out_point: i64,
    #[serde(rename = "MZ.EditLine")]
    pub(crate) edit_line: &'static str,
    #[serde(rename = "MZ.Sequence.VideoTimeDisplayFormat")]
    pub(crate) video_time_display_format: &'static str,
    #[serde(rename = "MZ.Sequence.AudioTimeDisplayFormat")]
    pub(crate) audio_time_display_format: &'static str,
    #[serde(rename = "MZ.Sequence.EditingModeGUID")]
    pub(crate) editing_mode_guid: &'static str,
    #[serde(rename = "MZ.Sequence.PreviewUseMaxBitDepth")]
    pub(crate) preview_use_max_bit_depth: &'static str,
    #[serde(rename = "MZ.Sequence.PreviewUseMaxRenderQuality")]
    pub(crate) preview_use_max_render_quality: &'static str,
    #[serde(rename = "MZ.Sequence.PreviewRenderingPresetPath")]
    pub(crate) preview_rendering_preset_path: &'static str,
    #[serde(rename = "MZ.Sequence.PreviewRenderingPresetCodec")]
    pub(crate) preview_rendering_preset_codec: &'static str,
    #[serde(rename = "MZ.Sequence.PreviewRenderingClassID")]
    pub(crate) preview_rendering_class_id: &'static str,
    #[serde(rename = "MZ.Sequence.PreviewFrameSizeWidth")]
    pub(crate) preview_width: u32,
    #[serde(rename = "MZ.Sequence.PreviewFrameSizeHeight")]
    pub(crate) preview_height: u32,
}

#[derive(Debug, Serialize)]
pub(crate) struct LinkContainer {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "Links", skip_serializing_if = "Option::is_none")]
    pub(crate) links: Option<Links>,
}

/// The links of a sequence's track items.
#[derive(Debug, Serialize)]
pub(crate) struct Links {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "Link")]
    pub(crate) links: Vec<IndexedRef<Link>>,
}

/// Track items that Premiere moves and selects together: the video and audio
/// item of a nested sequence, as Premiere 26.5.1 saves them
/// (`premiere_isolated_images_nests_26_5`, Link 107). The reader pairs them
/// by their ranges instead (`nested::pair_sounds`).
#[derive(Debug, Serialize)]
pub(crate) struct Link {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<Link>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "TrackItemGroup")]
    pub(crate) group: LinkedItems,
}

#[derive(Debug, Serialize)]
pub(crate) struct LinkedItems {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "TrackItems")]
    pub(crate) items: LinkedItemList,
}

#[derive(Debug, Serialize)]
pub(crate) struct LinkedItemList {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "TrackItem")]
    pub(crate) items: Vec<Reference>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PersistentGroupContainer {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    pub(crate) link_container: LinkContainer,
}

/// A positional group edge. Writer constructors accept only the matching typed ID.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename = "TrackGroup")]
pub(crate) struct TrackGroupLink {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(rename = "@Index", skip_serializing_if = "Option::is_none")]
    pub(crate) index: Option<usize>,
    #[serde(rename = "First", skip_serializing_if = "Option::is_none")]
    pub(crate) media: Option<String>,
    #[serde(rename = "Second")]
    pub(crate) target: Reference,
}

impl TrackGroupLink {
    fn for_group<T>(index: usize, media: &'static str, id: ObjectId<T>) -> Self {
        Self {
            version: Some(records::TRACK_GROUP_VERSION.to_owned()),
            index: Some(index),
            media: Some(media.to_owned()),
            target: Reference::object(id),
        }
    }

    pub(crate) fn video(id: ObjectId<VideoTrackGroup>) -> Self {
        Self::for_group(0, records::VIDEO_MEDIA, id)
    }

    pub(crate) fn audio(id: ObjectId<AudioTrackGroup>) -> Self {
        Self::for_group(1, records::AUDIO_MEDIA, id)
    }

    pub(crate) fn data(id: ObjectId<DataTrackGroup>) -> Self {
        Self::for_group(2, records::DATA_MEDIA, id)
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct TrackGroups {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(rename = "$value", default)]
    pub(crate) groups: Vec<TrackGroupLink>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct Sequence {
    #[serde(rename = "@ObjectUID", skip_serializing_if = "Option::is_none")]
    pub(crate) object_uid: Option<String>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(default, skip_serializing_if = "RetainedOrSkipped::is_skipped")]
    pub(crate) node: RetainedOrSkipped<Node<SequenceProperties>>,
    #[serde(default, skip_serializing_if = "RetainedOrSkipped::is_skipped")]
    pub(crate) persistent_group_container: RetainedOrSkipped<PersistentGroupContainer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) track_groups: Option<TrackGroups>,
    // Adobe's numeric local ID is not the ObjectUID used to select a sequence.
    #[serde(rename = "ID", default, skip_serializing_if = "Option::is_none")]
    pub(crate) local_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    #[serde(default, skip_serializing_if = "RetainedOrSkipped::is_skipped")]
    pub(crate) preview_format_identifier: RetainedOrSkipped<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(bound = "")]
pub(crate) struct Tracks<T> {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(rename = "Track")]
    pub(crate) tracks: Vec<Reference>,
    #[serde(skip)]
    owner: PhantomData<fn() -> T>,
}

impl<'de, T> Deserialize<'de> for Tracks<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let references = ReferenceList::deserialize(deserializer)?;
        Ok(Self {
            version: None,
            tracks: references.items,
            owner: PhantomData,
        })
    }
}

impl<T> Tracks<T> {
    pub(crate) fn from_uids(ids: impl IntoIterator<Item = Uid<T>>) -> Self {
        Self {
            version: Some(records::TRACKS_VERSION.into()),
            tracks: ids
                .into_iter()
                .enumerate()
                .map(|(index, id)| Reference::indexed_uid(index, id))
                .collect(),
            owner: PhantomData,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(
    rename_all = "PascalCase",
    bound(deserialize = "T: Deserialize<'de>", serialize = "T: Serialize")
)]
pub(crate) struct TrackGroup<T = Tracks<VideoClipTrackId>> {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tracks: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) frame_rate: Option<String>,
    #[serde(
        rename = "NextTrackID",
        default,
        skip_serializing_if = "RetainedOrSkipped::is_skipped"
    )]
    pub(crate) next_track_id: RetainedOrSkipped<usize>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct VideoTrackGroup {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<VideoTrackGroup>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<RetainedOrSkipped<&'static str>>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<RetainedOrSkipped<&'static str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) track_group: Option<TrackGroup>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) color_management_settings: Option<String>,
    #[serde(
        rename = "ImmersiveVideoVRConfiguration",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) immersive_video_vr_configuration: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) output_color_space: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) auto_input_gamut_compression_enabled: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) is_graphics_white_same_as_project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) is_color_aware_effects_enabled_same_as_project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) frame_rect: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) pixel_aspect_ratio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) component_owner: Option<ComponentOwner>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AudioTrackGroup {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<AudioTrackGroup>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) track_group: Option<TrackGroup<Tracks<AudioClipTrack>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) master_track: Option<Reference>,
    #[serde(rename = "ID", skip_serializing_if = "Option::is_none")]
    pub(crate) id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) automation_safe_flags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) num_adaptive_channels: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DataTrackGroup {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<DataTrackGroup>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    pub(crate) track_group: EmptyTrackGroup,
}

/// The supported data group has no tracks.
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct EmptyTrackGroup {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    pub(crate) frame_rate: String,
    #[serde(rename = "NextTrackID")]
    pub(crate) next_track_id: usize,
}
