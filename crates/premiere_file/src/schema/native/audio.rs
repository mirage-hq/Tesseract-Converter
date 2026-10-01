use super::{AudioClipTrack, IndexedURef, ObjectId, Reference};
use serde::{Deserialize, Serialize};

/// Chain components in processing order. The reader accepts any component
/// records; the writer emits a fader followed by a meter.
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Components {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(rename = "Component", default)]
    pub(crate) items: Vec<Reference>,
}

impl Components {
    pub(crate) fn fader_meter(fader: ObjectId<AudioFader>, meter: ObjectId<AudioMeter>) -> Self {
        Self {
            version: Some("1".into()),
            items: vec![
                Reference::indexed_object(0, fader),
                Reference::indexed_object(1, meter),
            ],
        }
    }

    /// A placement's intrinsic Volume, then its Channel Volume when stereo.
    pub(crate) fn clip_volume(
        components: impl IntoIterator<Item = ObjectId<AudioFilterComponent>>,
    ) -> Self {
        Self {
            version: Some("1".into()),
            items: components
                .into_iter()
                .enumerate()
                .map(|(index, component)| Reference::indexed_object(index, component))
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AudioChain {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) components: Option<Components>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AudioComponentChain {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<AudioComponentChain>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(rename = "DefaultVol", skip_serializing_if = "Option::is_none")]
    pub(crate) default_volume: Option<String>,
    #[serde(
        rename = "DefaultVolumeComponentID",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) default_volume_component_id: Option<String>,
    #[serde(
        rename = "DefaultChannelVolumeComponentID",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) default_channel_volume_component_id: Option<String>,
    pub(crate) component_chain: AudioChain,
    /// Premiere 26.5.1 omits the layout from mono chains.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) audio_channel_layout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) channel_type: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Params {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(rename = "Param", default)]
    pub(crate) params: Vec<Reference>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct Component {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) params: Option<Params>,
    #[serde(rename = "ID")]
    pub(crate) id: String,
    /// Written by projects up to Premiere 13 only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bypass: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) intrinsic: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AudioComponent {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    pub(crate) component: Component,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) frame_rate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) audio_channel_layout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) channel_type: Option<String>,
    #[serde(rename = "AudioComponentType", skip_serializing_if = "Option::is_none")]
    pub(crate) component_type: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PanProcessor {
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    pub(crate) audio_component: AudioComponent,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct StereoToStereoPanProcessor {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<StereoToStereoPanProcessor>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    pub(crate) pan_processor: PanProcessor,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DefaultPanProcessor {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<DefaultPanProcessor>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    pub(crate) pan_processor: PanProcessor,
    #[serde(rename = "DefaultPannerInputChannelType")]
    pub(crate) input_channel_type: &'static str,
    #[serde(rename = "DefaultPannerOutputChannelType")]
    pub(crate) output_channel_type: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct Sources {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "Source")]
    pub(crate) sources: Vec<IndexedURef<AudioClipTrack>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AudioTrackInlet {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<AudioTrackInlet>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    pub(crate) sources: Sources,
    pub(crate) audio_channel_layout: &'static str,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AudioFader {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<AudioFader>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    pub(crate) audio_component: AudioComponent,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AudioMeter {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<AudioMeter>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    pub(crate) audio_component: AudioComponent,
}

/// An intrinsic clip filter: Premiere's clip Volume and Channel Volume.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AudioFilterComponent {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<AudioFilterComponent>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    pub(crate) audio_component: AudioComponent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) filter_preset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) channel_config_data: Option<String>,
    pub(crate) filter_match_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) filter_index: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct AudioComponentParam {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<AudioComponentParam>,
    #[serde(rename = "@ClassID", skip_serializing_if = "Option::is_none")]
    pub(crate) class_id: Option<String>,
    #[serde(rename = "@Version", skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) start_keyframe: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) current_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) keyframes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) is_time_varying: Option<String>,
    /// Premiere 26.5.1 leaves the extra Channel Volume parameters unnamed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) is_inverted: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) upper_bound: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) range_locked: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) units_string: Option<String>,
}
