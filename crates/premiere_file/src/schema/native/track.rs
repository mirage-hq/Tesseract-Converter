use super::{
    AudioComponentChain, IndexedRef, MotionComponents, Node, ObjectId, Reference, ReferenceList,
    RetainedOrSkipped,
};
use serde::{de::IgnoredAny, Deserialize, Deserializer, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ComponentOwner {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) components: Option<Reference>,
}

impl ComponentOwner {
    pub(crate) fn video(id: ObjectId<VideoComponentChainId>) -> Self {
        Self {
            version: Some(crate::schema::records::COMPONENT_OWNER_VERSION.into()),
            components: Some(Reference::object(id)),
        }
    }

    pub(crate) fn audio(id: ObjectId<AudioComponentChain>) -> Self {
        Self {
            version: Some(crate::schema::records::COMPONENT_OWNER_VERSION.into()),
            components: Some(Reference::object(id)),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct TrackProperties {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "TL.SQTrackExpanded")]
    pub(crate) expanded: &'static str,
    #[serde(rename = "TL.SQTrackExpandedHeight")]
    pub(crate) expanded_height: &'static str,
    #[serde(rename = "MZ.SourceTrackState")]
    pub(crate) source_track_state: &'static str,
    #[serde(rename = "MZ.SourceTrackNumber")]
    pub(crate) source_track_number: usize,
    #[serde(rename = "MZ.TrackTargeted")]
    pub(crate) targeted: &'static str,
    #[serde(rename = "TL.SQTrackShy", skip_serializing_if = "Option::is_none")]
    pub(crate) shy: Option<&'static str>,
    #[serde(
        rename = "TL.SQTrackAudioKeyframeStyle",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) audio_keyframe_style: Option<&'static str>,
    #[serde(rename = "CM.KeyframeMode", skip_serializing_if = "Option::is_none")]
    pub(crate) keyframe_mode: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MixTrackProperties {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "TL.SQTrackExpanded")]
    pub(crate) expanded: &'static str,
    #[serde(rename = "TL.SQTrackExpandedHeight")]
    pub(crate) expanded_height: &'static str,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", bound(serialize = "N: Serialize"))]
pub(crate) struct Track<N = RetainedOrSkipped<Node<TrackProperties>>> {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(default)]
    pub(crate) node: N,
    /// The track's persistent identity, which a Track Matte Key's Matte names;
    /// it survives track deletions, unlike `index` (`horror_title` carries IDs
    /// 1, 2, 3, 4, 6 and 7 on `Index` 0 to 5). The writer writes `index` + 1.
    #[serde(rename = "ID", skip_serializing_if = "Option::is_none")]
    pub(crate) id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) index: Option<String>,
    /// Native `IsMuted`, shared by video, audio and audio-mix tracks. The
    /// video reader reads it as track video output off when `true` (picture
    /// verified by the AME render of `premiere_isolated_clip_disabled`; field
    /// semantics inferred, as the corpus has it only on empty tracks). Track v3
    /// writes an explicit `false`; the v4 fixtures omit it.
    #[serde(rename = "IsMuted", skip_serializing_if = "Option::is_none")]
    pub(crate) is_muted: Option<String>,
    #[serde(rename = "Name", skip_serializing)]
    pub(crate) _name: Option<IgnoredAny>,
}

#[derive(Debug, Serialize)]
pub(crate) struct TrackItems {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(rename = "TrackItem")]
    pub(crate) items: Vec<Reference>,
}

impl<'de> Deserialize<'de> for TrackItems {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let references = ReferenceList::deserialize(deserializer)?;
        Ok(Self {
            version: None,
            items: references.items,
        })
    }
}

impl TrackItems {
    pub(crate) fn from_indexed<T>(items: Vec<IndexedRef<T>>) -> Self {
        Self {
            version: Some("1".into()),
            items: items.into_iter().map(IndexedRef::into_reference).collect(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct ClipItems {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) track_items: Option<TrackItems>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) index: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct TransitionItems {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) track_items: Option<TrackItems>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) index: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct ClipTrack {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) track: Option<Track>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) clip_items: Option<ClipItems>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) transition_items: Option<TransitionItems>,
}

#[derive(Debug)]
pub(crate) enum VideoClipTrackId {}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct VideoClipTrack {
    #[serde(rename = "@ObjectUID", skip_serializing_if = "Option::is_none")]
    pub(crate) object_uid: Option<String>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) clip_track: Option<ClipTrack>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AudioTrack {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    pub(crate) component_owner: ComponentOwner,
    pub(crate) panner: Reference,
    #[serde(rename = "ID", skip_serializing_if = "Option::is_none")]
    pub(crate) id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sub_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) assign: Option<String>,
    #[serde(rename = "NextPannerID", skip_serializing_if = "Option::is_none")]
    pub(crate) next_panner_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) solo: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AudioClipTrack {
    #[serde(rename = "@ObjectUID", skip_serializing_if = "Option::is_none")]
    pub(crate) object_uid: Option<String>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    pub(crate) clip_track: ClipTrack,
    pub(crate) audio_track: AudioTrack,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AudioMixTrack {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<AudioMixTrack>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    pub(crate) audio_track: AudioTrack,
    pub(crate) track: Track<RetainedOrSkipped<Node<MixTrackProperties>>>,
    pub(crate) inlet: Reference,
}

#[derive(Debug)]
pub(crate) enum VideoComponentChainId {}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct VideoComponentChain {
    #[serde(rename = "@ObjectID", skip_serializing_if = "Option::is_none")]
    pub(crate) object_id: Option<String>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) default_motion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) default_opacity: Option<String>,
    #[serde(
        rename = "DefaultMotionComponentID",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) default_motion_component_id: Option<RetainedOrSkipped<&'static str>>,
    #[serde(
        rename = "DefaultOpacityComponentID",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) default_opacity_component_id: Option<RetainedOrSkipped<&'static str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) component_chain: Option<VideoChain>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct VideoChain {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) node: Option<RetainedOrSkipped<Node<MotionChainProperties>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) components: Option<MotionComponents>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MotionChainProperties {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "MZ.ComponentChain.ActiveComponentID")]
    pub(crate) active_component_id: &'static str,
    #[serde(rename = "MZ.ComponentChain.ActiveComponentParamIndex")]
    pub(crate) active_component_param_index: &'static str,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct TrackItemRange {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    /// Essential Sound tags on audio placements; not conversion data.
    #[serde(rename = "Node", skip_serializing)]
    pub(crate) _node: Option<IgnoredAny>,
    /// Version 3 items, which Premiere CS6 to CC 2015 save, also carry `Type`
    /// (1 clip, 2 transition, 3 preview item), the `MediaType` GUID,
    /// `TrackIndex` and `TrackRefCount`. The owning track-item class and the
    /// track structure already say the same, so the reader ignores these four
    /// fields and the writer does not write them.
    #[serde(rename = "Type", skip_serializing)]
    pub(crate) _item_type: Option<IgnoredAny>,
    #[serde(rename = "MediaType", skip_serializing)]
    pub(crate) _media_type: Option<IgnoredAny>,
    #[serde(rename = "TrackIndex", skip_serializing)]
    pub(crate) _track_index: Option<IgnoredAny>,
    #[serde(rename = "TrackRefCount", skip_serializing)]
    pub(crate) _track_ref_count: Option<IgnoredAny>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) start: Option<String>,
    pub(crate) end: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ClipTrackItem {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) component_owner: Option<ComponentOwner>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) track_item: Option<TrackItemRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sub_clip: Option<Reference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) head_transition: Option<Reference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tail_transition: Option<Reference>,
    /// Native `IsMuted`; `true` is read as clip Enable off. The picture is
    /// verified by the AME render of `premiere_isolated_clip_disabled`; the field
    /// semantics are inferred from one production project, and no corpus case
    /// has the field.
    #[serde(rename = "IsMuted", skip_serializing_if = "Option::is_none")]
    pub(crate) is_muted: Option<String>,
    /// Native `OriginalSubClipTimeOffset`, written last by Premiere 9.x/10.x
    /// saves. The reader accepts only an absent or zero value; the writer
    /// never sets it.
    #[serde(
        rename = "OriginalSubClipTimeOffset",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) original_sub_clip_time_offset: Option<String>,
}

#[derive(Debug)]
pub(crate) enum VideoClipTrackItemId {}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct VideoClipTrackItem {
    #[serde(rename = "@ObjectID", skip_serializing_if = "Option::is_none")]
    pub(crate) object_id: Option<String>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) clip_track_item: Option<ClipTrackItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) pixel_aspect_ratio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tone_map_settings: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) frame_rect: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct TransitionTrackItem {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) track_item: Option<TrackItemRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) alignment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) match_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) has_outgoing_clip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) has_incoming_clip: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct VideoTransitionTrackItem {
    #[serde(rename = "@ObjectID", skip_serializing_if = "Option::is_none")]
    pub(crate) object_id: Option<String>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) transition_track_item: Option<TransitionTrackItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) video_filter_component: Option<Reference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) start_percent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) end_percent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) switch_sources: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reverse: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{ClipTrackItem, TrackItemRange, TransitionTrackItem};

    /// Version 3 clip and transition items as Premiere CS6 to CC 2015 save
    /// them, in two element orders, one with Essential Sound tags.
    const LEGACY_CLIP: &str = r#"<ClipTrackItem Version="6"><TrackItem Version="3"><Node Version="1"><Properties Version="1"><monitor.show.audio.waveform>false</monitor.show.audio.waveform></Properties></Node><Type>1</Type><MediaType>80b8e3d5-6dca-4195-aefb-cb5f407ab009</MediaType><TrackIndex>0</TrackIndex><TrackRefCount>1</TrackRefCount><Start>0</Start><End>254016000000</End></TrackItem></ClipTrackItem>"#;
    const LEGACY_TRANSITION: &str = r#"<TransitionTrackItem Version="4"><TrackItem Version="3"><Node Version="1"></Node><TrackRefCount>1</TrackRefCount><TrackIndex>0</TrackIndex><Type>2</Type><End>254016000000</End><Start>0</Start><MediaType>228cda18-3625-4d2d-951e-348879e4ed93</MediaType></TrackItem><Alignment>0</Alignment></TransitionTrackItem>"#;

    #[test]
    fn legacy_track_item_fields_are_read_and_never_written() {
        let clip: ClipTrackItem = quick_xml::de::from_str(LEGACY_CLIP).unwrap();
        let transition: TransitionTrackItem = quick_xml::de::from_str(LEGACY_TRANSITION).unwrap();
        for range in [clip.track_item, transition.track_item] {
            let range = range.expect("the item keeps its range");
            assert_eq!(
                (range.start.as_deref(), range.end.as_str()),
                (Some("0"), "254016000000")
            );
            assert_eq!(
                quick_xml::se::to_string_with_root("TrackItem", &range).unwrap(),
                r#"<TrackItem Version="3"><Start>0</Start><End>254016000000</End></TrackItem>"#
            );
        }
        // Other unknown fields still reject.
        let error = quick_xml::de::from_str::<TrackItemRange>(
            r#"<TrackItem Version="3"><TrackKind>1</TrackKind><End>1</End></TrackItem>"#,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("unknown field `TrackKind`"),
            "{error}"
        );
    }
}
