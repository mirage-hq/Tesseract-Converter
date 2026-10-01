use super::{
    AudioComponentChain, ClipTrackItem, IndexedRef, Node, ObjectId, ProjectItem, Ref, Reference,
    ReferenceList, RetainedOrSkipped, URef, Uid,
};
use crate::schema::records;
use serde::{de::IgnoredAny, Deserialize, Deserializer, Serialize};

#[derive(Debug, Serialize)]
pub(crate) struct ClipProjectItemProperties {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(
        rename = "project.icon.view.grid.order",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) icon_view_grid_order: Option<&'static str>,
    #[serde(rename = "Column.PropertyText.Label")]
    pub(crate) label: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ClipProjectItem {
    #[serde(rename = "@ObjectUID")]
    pub(crate) object_uid: Uid<ClipProjectItem>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    pub(crate) project_item: ProjectItem<ClipProjectItemProperties>,
    pub(crate) master_clip: URef<MasterClipId>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct AudioComponentChains {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(rename = "AudioComponentChain", default)]
    pub(crate) chains: Vec<Reference>,
}

impl AudioComponentChains {
    pub(crate) fn single(chain: ObjectId<AudioComponentChain>) -> Self {
        Self {
            version: Some("1".into()),
            chains: vec![Reference::indexed_object(0, chain)],
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Clips {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(rename = "Clip")]
    pub(crate) items: Vec<Reference>,
}

impl<'de> Deserialize<'de> for Clips {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let references = ReferenceList::deserialize(deserializer)?;
        Ok(Self {
            version: None,
            items: references.items,
        })
    }
}

impl Clips {
    /// Premiere lists the audio clip before the video clip.
    pub(crate) fn from_ids(
        audio: Option<ObjectId<AudioClip>>,
        video: Option<ObjectId<VideoClipId>>,
    ) -> Self {
        let items = audio
            .map(Reference::object)
            .into_iter()
            .chain(video.map(Reference::object))
            .enumerate()
            .map(|(index, reference)| Reference {
                index: Some(index.to_string()),
                ..reference
            })
            .collect();
        Self {
            version: Some(records::CLIPS_VERSION.into()),
            items,
        }
    }
}

#[derive(Debug)]
pub(crate) enum MasterClipId {}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct MasterClip {
    #[serde(rename = "@ObjectUID", skip_serializing_if = "Option::is_none")]
    pub(crate) object_uid: Option<String>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) node: Option<MasterNode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) logging_info: Option<RetainedOrSkipped<Ref<ClipLoggingInfo>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) audio_component_chains: Option<AudioComponentChains>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) clips: Option<Clips>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) audio_clip_channel_groups:
        Option<RetainedOrSkipped<Ref<ClipChannelGroupVectorSerializer>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<RetainedOrSkipped<String>>,
    /// `true` on the project item of an adjustment layer, whose placed clips
    /// carry [`VideoClip::adjustment_layer`]; absent on every other item.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) is_adjustment_layer: Option<String>,
    #[serde(
        rename = "MasterClipChangeVersion",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) change_version: Option<RetainedOrSkipped<String>>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct MasterNode {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    /// Bin item number of a master clip; not conversion data. Premiere 26.5.1
    /// writes a file-media master clip's `Node` with this ID alone.
    #[serde(rename = "ID", skip_serializing)]
    pub(crate) _id: Option<IgnoredAny>,
    /// Source Monitor state of the item; not conversion data, and absent on a
    /// master clip that has never been opened in the monitor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) properties: Option<MonitorProperties>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MonitorProperties {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(rename = "AMM.CurrentSolo", skip_serializing)]
    pub(crate) _current_solo: Option<EmptySolo>,
    #[serde(rename = "monitor.edit.time", skip_serializing_if = "Option::is_none")]
    pub(crate) edit_time: Option<RetainedOrSkipped<String>>,
    #[serde(rename = "monitor.looping", skip_serializing_if = "Option::is_none")]
    pub(crate) looping: Option<RetainedOrSkipped<String>>,
    #[serde(
        rename = "monitor.show.audio.waveform",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) show_audio_waveform: Option<RetainedOrSkipped<String>>,
    #[serde(rename = "monitor.take.audio", skip_serializing_if = "Option::is_none")]
    pub(crate) take_audio: Option<RetainedOrSkipped<String>>,
    #[serde(
        rename = "monitor.take.audio.linked",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) take_audio_linked: Option<RetainedOrSkipped<String>>,
    #[serde(rename = "monitor.take.video", skip_serializing_if = "Option::is_none")]
    pub(crate) take_video: Option<RetainedOrSkipped<String>>,
    #[serde(
        rename = "monitor.take.video.linked",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) take_video_linked: Option<RetainedOrSkipped<String>>,
    #[serde(
        rename = "monitor.zoom.in.time",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) zoom_in_time: Option<RetainedOrSkipped<String>>,
    #[serde(
        rename = "monitor.zoom.out.time",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) zoom_out_time: Option<RetainedOrSkipped<String>>,
}

/// A master clip's `AMM.CurrentSolo` monitor state when it solos nothing: the
/// empty list [`records::AMM_CURRENT_SOLO`], the only value that the master
/// clips of the local corpus and of a real Premiere 26.3 project's music hold.
/// It is not conversion data. Another value is unobserved and rejects the
/// master clip, as an unknown key does.
#[derive(Debug)]
pub(crate) struct EmptySolo;

impl<'de> Deserialize<'de> for EmptySolo {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value == records::AMM_CURRENT_SOLO {
            Ok(Self)
        } else {
            Err(serde::de::Error::custom(format!(
                "unsupported AMM.CurrentSolo {value:?}: only an empty solo list is read"
            )))
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ClipLoggingInfo {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<ClipLoggingInfo>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) capture_mode: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) clip_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) timecode_format: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) media_in_point: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) media_out_point: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) media_frame_rate: Option<i64>,
}

/// Clip node properties: media clips carry a label; graphic clips carry the
/// synthetic generator's timecode preference.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum ClipProperties {
    Media(MediaClipProperties),
    Graphic(GraphicClipProperties),
}

#[derive(Debug, Serialize)]
pub(crate) struct MediaClipProperties {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    /// Written before the labels, as Adobe does, on generator clips only.
    #[serde(
        rename = "BE.Prefs.SyntheticMedia.DefaultIsDropFrame",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) default_is_drop_frame: Option<&'static str>,
    #[serde(rename = "asl.clip.label.color")]
    pub(crate) label_color: &'static str,
    #[serde(rename = "asl.clip.label.name")]
    pub(crate) label_name: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct GraphicClipProperties {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "BE.Prefs.SyntheticMedia.DefaultIsDropFrame")]
    pub(crate) default_is_drop_frame: &'static str,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct MarkerOwner {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) markers: Option<Reference>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct Clip {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(default, skip_serializing_if = "RetainedOrSkipped::is_skipped")]
    pub(crate) node: RetainedOrSkipped<Node<ClipProperties>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) marker_owner: Option<MarkerOwner>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) time_remapping: Option<Reference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) playback_speed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) play_backwards: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<Reference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) out_point: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) in_point: Option<String>,
    #[serde(rename = "ClipID", skip_serializing_if = "Option::is_none")]
    pub(crate) clip_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) in_use: Option<RetainedOrSkipped<&'static str>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SecondaryContents {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(rename = "SecondaryContentItem")]
    pub(crate) items: Vec<Reference>,
}

impl<'de> Deserialize<'de> for SecondaryContents {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let references = ReferenceList::deserialize(deserializer)?;
        Ok(Self {
            version: None,
            items: references.items,
        })
    }
}

impl SecondaryContents {
    /// One channel item per source channel, numbered from zero.
    pub(crate) fn from_ids(ids: impl IntoIterator<Item = ObjectId<SecondaryContent>>) -> Self {
        Self {
            version: Some("1".into()),
            items: IndexedRef::list(ids)
                .into_iter()
                .map(IndexedRef::into_reference)
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AudioClip {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<AudioClip>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    pub(crate) clip: Clip,
    pub(crate) secondary_contents: SecondaryContents,
    pub(crate) audio_channel_layout: String,
    /// Clip gain, linear; it multiplies the clip Volume.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) gain: Option<String>,
}

#[derive(Debug)]
pub(crate) enum VideoClipId {}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct VideoClip {
    #[serde(rename = "@ObjectID", skip_serializing_if = "Option::is_none")]
    pub(crate) object_id: Option<String>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) clip: Option<Clip>,
    /// `true` on every placed clip of an adjustment layer, after `Clip` in
    /// each corpus save; absent on every other clip, template clips included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) adjustment_layer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) time_interpolation_type: Option<String>,
    /// The measured explicit hold mode `4`, with a source-tick FrameHoldStart.
    /// Other native hold modes have no observed mapping.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) frame_hold: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) frame_hold_start: Option<String>,
    /// Scale to Frame Size: `1` when on, absent when off. Premiere also writes
    /// it on a master clip's `VideoClip`, where it does not render; the readers
    /// check only a placement's clip (`reader::video::scale_to_frame_size`),
    /// and the writer never writes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) scale_to_frame_policy: Option<String>,
    /// The source-tick thumbnail frame that Premiere CS6 to CC 2015 save on
    /// every clip. It selects a Project panel icon, not conversion data, so the
    /// reader ignores it and the writer never writes it.
    #[serde(rename = "PosterFrame", skip_serializing)]
    pub(crate) _poster_frame: Option<IgnoredAny>,
    /// Legacy clip settings that Premiere CS6 to CC 2015 save on every clip
    /// (427 corpus clips in 12 packages). `FrameBlend` is the older form of
    /// `TimeInterpolationType` 1 and is read like it. The other four are
    /// interlace options that the corpus only saves in their inert form
    /// (`0` / `false`); `reader::video::legacy_clip_settings` rejects any other
    /// value. The writer never writes them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) frame_blend: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) field_processing: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) hold_filters: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) deinterlace_on_hold: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reverse_field_dominance: Option<String>,
    /// The Version 7 form of `ScaleToFramePolicy`: `true` is policy `1`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) scale_to_frame_size: Option<String>,
}

impl VideoClip {
    /// Whether the clip declares a frame hold. Premiere CS6 to CC 2015 save
    /// `FrameHold` mode `0` (no hold) on every clip, usually with a placeholder
    /// `FrameHoldStart`; modern saves omit both fields when the hold is off.
    pub(crate) fn declares_frame_hold(&self) -> bool {
        match self.frame_hold.as_deref() {
            None => self.frame_hold_start.is_some(),
            Some(mode) => mode != "0",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MasterNode, VideoClip};

    #[test]
    fn legacy_clip_fields_are_read_and_the_poster_frame_is_never_written() {
        // Field order of the corpus `5 Ink Transitions.prproj` (Premiere CC 2015)
        // plus the Version 7 `ScaleToFrameSize` of the CS3 corpus projects.
        let clip: VideoClip = quick_xml::de::from_str(concat!(
            r#"<VideoClip ObjectID="1" ClassID="c" Version="11">"#,
            "<PosterFrame>0</PosterFrame><FrameHold>0</FrameHold>",
            "<HoldFilters>false</HoldFilters><DeinterlaceOnHold>false</DeinterlaceOnHold>",
            "<ReverseFieldDominance>false</ReverseFieldDominance>",
            "<FieldProcessing>0</FieldProcessing><FrameBlend>true</FrameBlend>",
            "<ScaleToFrameSize>false</ScaleToFrameSize>",
            "</VideoClip>",
        ))
        .unwrap();
        assert_eq!(clip.frame_hold.as_deref(), Some("0"));
        assert_eq!(clip.frame_blend.as_deref(), Some("true"));
        assert_eq!(clip.field_processing.as_deref(), Some("0"));
        assert_eq!(clip.scale_to_frame_size.as_deref(), Some("false"));
        assert!(!quick_xml::se::to_string(&clip)
            .unwrap()
            .contains("PosterFrame"));
    }

    #[test]
    fn adobe_native_optical_flow_field_is_a_video_clip_sibling() {
        // Structural-only evidence extracted byte-for-byte from the pinned
        // `abstract_slideshow` source project (SHA-256
        // bf44012a58895c7901b431adb80f369ba5c41b7ec51bcd692b037786ddfa0ad9).
        // Field placement alone does not prove frame-interpolation visual parity
        // or that Premiere UI values were inspected.
        let source = include_str!(
            "../../../tests/fixtures/adobe-native-time-interpolation-optical-flow.xml"
        );
        let clip: VideoClip = quick_xml::de::from_str(source).unwrap();
        assert_eq!(clip.time_interpolation_type.as_deref(), Some("2"));
        assert_eq!(clip.clip.unwrap().playback_speed.as_deref(), Some("0.88"));
    }

    #[test]
    fn source_monitor_ui_keys_do_not_reject_a_master_clip() {
        // The master clip Node of the corpus prepared/adobe-improve-audio/native-26.5.1
        // project (Premiere 26.5.1); 76 of the 164 corpus projects write
        // `monitor.show.audio.waveform` there. `monitor.looping` and both
        // `monitor.take.*.linked` keys are added by hand: the corpus writes them
        // only in track item and sequence nodes.
        let node = concat!(
            r#"<Node Version="1"><Properties Version="1">"#,
            "<monitor.edit.time>281292784722</monitor.edit.time>",
            "<monitor.looping>false</monitor.looping>",
            "<monitor.show.audio.waveform>true</monitor.show.audio.waveform>",
            "<monitor.take.audio>true</monitor.take.audio>",
            "<monitor.take.audio.linked>true</monitor.take.audio.linked>",
            "<monitor.take.video>false</monitor.take.video>",
            "<monitor.take.video.linked>true</monitor.take.video.linked>",
            "<monitor.zoom.in.time>0</monitor.zoom.in.time>",
            "<monitor.zoom.out.time>6302729664000</monitor.zoom.out.time>",
            "</Properties></Node>",
        );
        // The music master clip Node of a real Premiere 26.3 project (SHA-256
        // 3f47fe84ac2c285da95da40aa53189400f963a5fd87e68be57e26ece47d93174),
        // whitespace removed.
        let music = concat!(
            r#"<Node Version="1"><Properties Version="1">"#,
            "<AMM.CurrentSolo>[]</AMM.CurrentSolo>",
            "<monitor.edit.time>34224744153600</monitor.edit.time>",
            "<monitor.zoom.in.time>0</monitor.zoom.in.time>",
            "<monitor.zoom.out.time>56720396880000</monitor.zoom.out.time>",
            "<monitor.take.video>false</monitor.take.video>",
            "<monitor.take.audio>true</monitor.take.audio>",
            "<monitor.show.audio.waveform>true</monitor.show.audio.waveform>",
            "</Properties></Node>",
        );
        for node in [node, music] {
            let master: MasterNode = quick_xml::de::from_str(node).unwrap();
            assert!(master.properties.is_some());
        }

        // A solo list that is not empty, and other keys, such as the
        // render-and-replace offset of the corpus adobe-pro-audio master clip
        // `cf41ac53-6d62-417e-85f4-e97030c49738`, still reject.
        for (from, to, reason) in [
            ("[]", "[0]", r#"unsupported AMM.CurrentSolo "[0]""#),
            (
                "AMM.CurrentSolo>[]</AMM.CurrentSolo",
                "BE.MasterClip.Rendered.OffsetToOriginal>-805188384000</BE.MasterClip.Rendered.OffsetToOriginal",
                "unknown field `BE.MasterClip.Rendered.OffsetToOriginal`",
            ),
        ] {
            let error = quick_xml::de::from_str::<MasterNode>(&music.replace(from, to)).unwrap_err();
            assert!(error.to_string().contains(reason), "{error}");
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ClipChannelVectors {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "ClipChannelVectorItem")]
    pub(crate) items: Vec<IndexedRef<ClipChannelVectorSerializer>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ClipChannelGroupVectorSerializer {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<ClipChannelGroupVectorSerializer>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "ClipChannelVectors", skip_serializing_if = "Option::is_none")]
    pub(crate) vectors: Option<ClipChannelVectors>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct SecondaryContent {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<SecondaryContent>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    pub(crate) content: Reference,
    pub(crate) channel_index: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct ClipChannels {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "ClipChannelItem")]
    pub(crate) items: Vec<IndexedRef<ClipChannelSerializer>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ClipChannelVectorSerializer {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<ClipChannelVectorSerializer>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "ClipChannels")]
    pub(crate) channels: ClipChannels,
    pub(crate) channel_type: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ClipChannelSerializer {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<ClipChannelSerializer>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    pub(crate) source_clip_index: &'static str,
    #[serde(rename = "mSourceChannelIndex")]
    pub(crate) source_channel_index: usize,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct Markers {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<Markers>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(rename = "ByGUID", skip_serializing_if = "Option::is_none")]
    pub(crate) by_guid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_metadata_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_content_state: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub(crate) struct SubClip {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<SubClip>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    pub(crate) clip: Reference,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) master_clip: Option<Reference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    #[serde(rename = "OrigChGrp", skip_serializing_if = "Option::is_none")]
    pub(crate) original_channel_group: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AudioClipTrackItem {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<AudioClipTrackItem>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    pub(crate) clip_track_item: ClipTrackItem,
}
