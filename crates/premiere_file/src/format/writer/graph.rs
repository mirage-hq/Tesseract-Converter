//! Native graph construction, identity allocation, and record-family ordering.

use super::{
    media, project, sequence,
    tracks::{audio, graphic, nested, video},
    BoundMedia,
};
use crate::{
    format::Result,
    schema::{
        native::*,
        records::CHANNEL_VOLUME_EXTRA_PARAMS,
        records::CHANNEL_VOLUME_NAMES,
        text::{PrGraphicObject, SHAPE_PARAM_COUNT, TEXT_PARAM_COUNT, VECTOR_MOTION_PARAM_COUNT},
        AudioChannels, MediaId, PrAnimatedProperty, PrAudioOccurrence, PrBlendMode, PrEffect,
        PrGraphic, PrLinearWipe, PrMask, PrMedia, PrNestOccurrence, PrPropertyAnimation,
        PrSequence, PrStaticCrop, PrStaticTransform, PrTrackMatte, PrVideoItem, PrVideoOccurrence,
        CROP_PARAM_COUNT, MASK_FORM_V7, MOTION_PARAM_COUNT, OPACITY_PARAM_COUNT, TRACK_MATTE_KEY,
    },
};
use std::collections::BTreeMap;

/// The channel layout of each media with sound.
type AudioLayouts<'a> = BTreeMap<&'a MediaId, AudioChannels>;

pub(super) fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Debug)]
pub(super) struct ProjectIds {
    pub(super) shell: ShellIds,
    pub(super) main: SequenceGraphIds,
    pub(super) media: Vec<MediaIds>,
}

/// Identities of one sequence and of the placements on its tracks.
#[derive(Debug)]
pub(super) struct SequenceGraphIds {
    pub(super) sequence: SequenceIds,
    pub(super) mixer: MixerIds,
    /// Item identities per track, in the same order as the track's items.
    pub(super) placements: Vec<Vec<ItemIds>>,
    /// One per `PrSequence::audio` entry, on the audio track of the same index.
    pub(super) audio_placements: Vec<AudioPlacementIds>,
    /// Nested-sequence placements, per track like `placements`.
    pub(super) nests: Vec<Vec<NestIds>>,
    pub(super) video_transitions: Vec<Vec<ObjectId<VideoTransitionTrackItem>>>,
}

/// One nested-sequence placement and the sequence it plays.
#[derive(Debug)]
pub(super) struct NestIds {
    pub(super) placement: PlacementIds,
    pub(super) inner: SequenceGraphIds,
    /// The audio item of a placement whose sequence has sound.
    pub(super) sound: Option<NestSoundIds>,
}

/// The audio item that plays a nested sequence's stereo mix beside its video
/// item, and the link between the two.
#[derive(Debug)]
pub(super) struct NestSoundIds {
    pub(super) clip: ObjectId<AudioClip>,
    pub(super) subclip: ObjectId<SubClip>,
    pub(super) components: ObjectId<AudioComponentChain>,
    pub(super) track_item: ObjectId<AudioClipTrackItem>,
    pub(super) secondary: [ObjectId<SecondaryContent>; 2],
    pub(super) link: ObjectId<Link>,
    pub(super) clip_uid: String,
}

impl NestSoundIds {
    fn allocate(ids: &mut ObjectIdAllocator) -> Self {
        Self {
            clip: ids.take(),
            subclip: ids.take(),
            components: ids.take(),
            track_item: ids.take(),
            secondary: std::array::from_fn(|_| ids.take()),
            link: ids.take(),
            clip_uid: uuid(),
        }
    }
}

impl SequenceGraphIds {
    /// The audio track items in track order: the sequence's sounds, then the
    /// audio items of its nests, each on its own track.
    pub(super) fn audio_items(&self) -> Vec<ObjectId<AudioClipTrackItem>> {
        self.audio_placements
            .iter()
            .map(|placement| placement.track_item)
            .chain(
                self.nests
                    .iter()
                    .flatten()
                    .filter_map(|nest| nest.sound.as_ref().map(|sound| sound.track_item)),
            )
            .collect()
    }
}

/// Audio tracks of `sequence`: one per sound and per nest with sound, at
/// least Premiere's default four.
fn audio_track_count(sequence: &PrSequence) -> usize {
    let sounds = sequence
        .nest_occurrences()
        .filter(|nest| nest.sequence.has_sound())
        .count();
    (sequence.audio.len() + sounds).max(4)
}

#[derive(Debug)]
pub(super) struct ShellIds {
    pub(super) project: ObjectId<Project>,
    pub(super) document: Uid<RootProjectItem>,
    pub(super) project_guid: String,
    pub(super) view_state: String,
    pub(super) project_settings: ObjectId<ProjectSettings>,
    pub(super) scratch_disk_settings: ObjectId<ScratchDiskSettings>,
    pub(super) ingest_settings: ObjectId<IngestSettings>,
    pub(super) workspace_settings: ObjectId<WorkspaceSettings>,
    pub(super) video_settings: ObjectId<VideoSettings>,
    pub(super) audio_settings: ObjectId<AudioSettings>,
    pub(super) video_compile_settings: ObjectId<VideoCompileSettings>,
    pub(super) audio_compile_settings: ObjectId<AudioCompileSettings>,
    pub(super) compile_video_settings: ObjectId<VideoSettings>,
    pub(super) compile_audio_settings: ObjectId<AudioSettings>,
    pub(super) dummy_capture_settings: ObjectId<DummyCaptureSettings>,
    pub(super) default_sequence_settings: ObjectId<DefaultSequenceSettings>,
}

#[derive(Debug)]
pub(super) struct SequenceIds {
    pub(super) item: Uid<ClipProjectItem>,
    pub(super) master: Uid<MasterClipId>,
    pub(super) audio_clip_uid: String,
    pub(super) video_clip_uid: String,
    pub(super) sequence: Uid<SequenceId>,
    pub(super) logging: ObjectId<ClipLoggingInfo>,
    pub(super) audio_component_chain: ObjectId<AudioComponentChain>,
    pub(super) audio_clip: ObjectId<AudioClip>,
    pub(super) video_clip: ObjectId<VideoClipId>,
    pub(super) channel_groups: ObjectId<ClipChannelGroupVectorSerializer>,
    pub(super) audio_source: ObjectId<AudioSequenceSource>,
    pub(super) secondary_content: [ObjectId<SecondaryContent>; 2],
    pub(super) video_source: ObjectId<VideoSequenceSource>,
    pub(super) channel_vector: ObjectId<ClipChannelVectorSerializer>,
    pub(super) channels: [ObjectId<ClipChannelSerializer>; 2],
    pub(super) video_group: ObjectId<VideoTrackGroup>,
    pub(super) audio_group: ObjectId<AudioTrackGroup>,
    pub(super) data_group: ObjectId<DataTrackGroup>,
    pub(super) video_component_chain: ObjectId<VideoComponentChainId>,
    pub(super) video_tracks: Vec<Uid<VideoClipTrackId>>,
    pub(super) audio_tracks: Vec<Uid<AudioClipTrack>>,
    pub(super) audio_group_uid: String,
    pub(super) audio_track_uids: Vec<String>,
    pub(super) mix_track_uid: String,
}

#[derive(Debug)]
pub(super) struct MixerStripIds {
    pub(super) chain: ObjectId<AudioComponentChain>,
    pub(super) panner: ObjectId<StereoToStereoPanProcessor>,
    pub(super) balance: ObjectId<AudioComponentParam>,
    pub(super) components: MixerComponentIds,
}

#[derive(Debug)]
pub(super) struct MixerMasterIds {
    pub(super) chain: ObjectId<AudioComponentChain>,
    pub(super) panner: ObjectId<DefaultPanProcessor>,
    pub(super) components: MixerComponentIds,
}

#[derive(Debug)]
pub(super) struct MixerComponentIds {
    pub(super) fader: ObjectId<AudioFader>,
    pub(super) meter: ObjectId<AudioMeter>,

    pub(super) volume: ObjectId<AudioComponentParam>,
    pub(super) mute: ObjectId<AudioComponentParam>,
}

#[derive(Debug)]
pub(super) struct MixerIds {
    pub(super) mix_track: ObjectId<AudioMixTrack>,
    pub(super) strips: Vec<MixerStripIds>,
    pub(super) master: MixerMasterIds,
    pub(super) inlet: ObjectId<AudioTrackInlet>,
}

#[derive(Debug)]
pub(super) struct MediaIds {
    pub(super) audio: Option<AudioMediaIds>,
    pub(super) stream: ObjectId<VideoStream>,
    pub(super) source: ObjectId<VideoMediaSource>,
    pub(super) markers: ObjectId<Markers>,
    pub(super) logging: ObjectId<ClipLoggingInfo>,
    pub(super) template_clip: ObjectId<VideoClipId>,
    pub(super) channels: ObjectId<ClipChannelGroupVectorSerializer>,
    pub(super) media: Uid<Media>,
    pub(super) media_state: uuid::Uuid,
    pub(super) media_binary_hash: String,
    pub(super) media_file_key: String,
    pub(super) template_clip_uid: String,
    pub(super) master: Uid<MasterClipId>,
    pub(super) item: Uid<ClipProjectItem>,
}

#[derive(Debug)]
pub(super) struct AudioMediaIds {
    pub(super) stream: ObjectId<AudioStream>,
    pub(super) source: ObjectId<AudioMediaSource>,
    pub(super) template_clip: ObjectId<AudioClip>,
    pub(super) components: ObjectId<AudioComponentChain>,
    pub(super) channel_vector: ObjectId<ClipChannelVectorSerializer>,
    pub(super) secondary: Vec<ObjectId<SecondaryContent>>,
    pub(super) channels: Vec<ObjectId<ClipChannelSerializer>>,
}

#[derive(Debug)]
pub(super) struct AudioPlacementIds {
    pub(super) clip: ObjectId<AudioClip>,
    pub(super) subclip: ObjectId<SubClip>,
    pub(super) components: ObjectId<AudioComponentChain>,
    pub(super) track_item: ObjectId<AudioClipTrackItem>,
    pub(super) secondary: Vec<ObjectId<SecondaryContent>>,
    /// Present unless the placement plays at a static unity level.
    pub(super) volume: Option<ClipVolumeIds>,
    /// The one-sided transitions of the placement's fades.
    pub(super) fade_in: Option<ObjectId<AudioTransitionTrackItem>>,
    pub(super) fade_out: Option<ObjectId<AudioTransitionTrackItem>>,
}

/// Premiere's intrinsic clip Volume, and the Channel Volume of a stereo clip.
#[derive(Debug)]
pub(super) struct ClipVolumeIds {
    pub(super) component: ObjectId<AudioFilterComponent>,
    pub(super) mute: ObjectId<AudioComponentParam>,
    pub(super) level: ObjectId<AudioComponentParam>,
    pub(super) channel_volume: Option<ChannelVolumeIds>,
}

#[derive(Debug)]
pub(super) struct ChannelVolumeIds {
    pub(super) component: ObjectId<AudioFilterComponent>,
    pub(super) bypass: ObjectId<AudioComponentParam>,
    /// Left, Right, then the unnamed parameters.
    pub(super) levels: Vec<ObjectId<AudioComponentParam>>,
}

#[derive(Debug)]
pub(super) struct PlacementIds {
    pub(super) placed_clip: ObjectId<VideoClipId>,
    pub(super) subclip: ObjectId<SubClip>,
    pub(super) components: ObjectId<VideoComponentChainId>,
    pub(super) track_item: ObjectId<VideoClipTrackItemId>,
    pub(super) ramp: Option<TimeRemapIds>,
    pub(super) motion: Option<MotionIds>,
    pub(super) opacity: Option<OpacityIds>,
    pub(super) crop: Option<CropIds>,
    pub(super) linear_wipe: Option<LinearWipeIds>,
    pub(super) track_matte: Option<TrackMatteIds>,
    /// One entry per standard effect, in stack order.
    pub(super) effects: Vec<EffectIds>,
    pub(super) clip_uid: String,
}

/// The edits of one media or nest placement that decide its components.
#[derive(Debug, Clone, Copy)]
pub(super) struct PlacementEdits<'a> {
    pub(super) ramp: bool,
    pub(super) transform: PrStaticTransform,
    pub(super) opacity: f64,
    pub(super) blend_mode: PrBlendMode,
    pub(super) animations: &'a [PrPropertyAnimation],
    pub(super) crop: PrStaticCrop,
    pub(super) linear_wipe: Option<&'a PrLinearWipe>,
    pub(super) opacity_mask: Option<&'a PrMask>,
    pub(super) track_matte: Option<PrTrackMatte>,
    /// Standard effects in stack order.
    pub(super) effects: &'a [PrEffect],
    /// How many of `effects` apply before the Crop, Linear Wipe or Track
    /// Matte Key.
    pub(super) effects_above_mask: usize,
}

impl<'a> From<&'a PrVideoOccurrence> for PlacementEdits<'a> {
    fn from(clip: &'a PrVideoOccurrence) -> Self {
        Self {
            ramp: clip.time_remap.is_some() && clip.held_source_ticks().is_none(),
            transform: clip.transform,
            opacity: clip.opacity,
            blend_mode: clip.blend_mode,
            animations: &clip.animations,
            crop: clip.crop,
            linear_wipe: clip.linear_wipe.as_ref(),
            opacity_mask: clip.opacity_mask.as_ref(),
            track_matte: clip.track_matte,
            effects: &clip.effects,
            effects_above_mask: clip.effects_above_mask,
        }
    }
}

impl<'a> From<&'a PrNestOccurrence> for PlacementEdits<'a> {
    fn from(nest: &'a PrNestOccurrence) -> Self {
        Self {
            ramp: false,
            transform: nest.transform,
            opacity: nest.opacity,
            blend_mode: nest.blend_mode,
            animations: &nest.animations,
            crop: nest.crop,
            linear_wipe: nest.linear_wipe.as_ref(),
            opacity_mask: nest.opacity_mask.as_ref(),
            track_matte: nest.track_matte,
            effects: &nest.effects,
            effects_above_mask: nest.effects_above_mask,
        }
    }
}

/// Identities for one video track item, matching its item kind.
#[derive(Debug)]
pub(super) enum ItemIds {
    Media(PlacementIds),
    Graphic(GraphicIds),
}

impl ItemIds {
    pub(super) fn track_item(&self) -> ObjectId<VideoClipTrackItemId> {
        match self {
            Self::Media(ids) => ids.track_item,
            Self::Graphic(ids) => ids.track_item,
        }
    }
}

/// Every identity of one graphic: its private generator media, master clip,
/// placement, object components, and any Vector Motion or clip Opacity
/// component.
#[derive(Debug)]
pub(super) struct GraphicIds {
    pub(super) media: Uid<Media>,
    pub(super) stream: ObjectId<VideoStream>,
    pub(super) source: ObjectId<VideoMediaSource>,
    pub(super) master: Uid<MasterClipId>,
    pub(super) logging: ObjectId<ClipLoggingInfo>,
    pub(super) template_clip: ObjectId<VideoClipId>,
    pub(super) channels: ObjectId<ClipChannelGroupVectorSerializer>,
    pub(super) placed_clip: ObjectId<VideoClipId>,
    pub(super) subclip: ObjectId<SubClip>,
    pub(super) components: ObjectId<VideoComponentChainId>,
    /// One entry per object, in chain order.
    pub(super) objects: Vec<GraphicObjectIds>,
    pub(super) group_map: Option<GroupMapIds>,
    pub(super) vector_motion: Option<VectorMotionIds>,
    pub(super) opacity: Option<OpacityIds>,
    pub(super) track_item: ObjectId<VideoClipTrackItemId>,
    pub(super) template_clip_uid: String,
    pub(super) clip_uid: String,
}

/// One graphic object's component and parameters, and its binary hashes.
#[derive(Debug)]
pub(super) enum GraphicObjectIds {
    Text(TextIds),
    Shape(ShapeIds),
    Group(GroupIds),
}

/// A SubGroup's component, its parameters in the Vector Motion layout, and
/// its members.
#[derive(Debug)]
pub(super) struct GroupIds {
    pub(super) component: ObjectId<VideoFilterComponent>,
    pub(super) params: [ObjectId<MotionParamId>; VECTOR_MOTION_PARAM_COUNT],
    pub(super) objects: Vec<GraphicObjectIds>,
}

/// A chain's `ComponentGroupMap` and its pins, one per SubGroup member in
/// chain order.
#[derive(Debug)]
pub(super) struct GroupMapIds {
    pub(super) vector: ObjectId<ComponentPinVectorSerializer>,
    pub(super) pins: Vec<ObjectId<ComponentPinSerializer>>,
}

#[derive(Debug)]
pub(super) struct TextIds {
    pub(super) component: ObjectId<VideoFilterComponent>,
    pub(super) source_text: ObjectId<MotionParamId>,
    pub(super) params: [ObjectId<MotionParamId>; TEXT_PARAM_COUNT],
    pub(super) source_text_hash: String,
}

#[derive(Debug)]
pub(super) struct ShapeIds {
    pub(super) component: ObjectId<VideoFilterComponent>,
    pub(super) path: ObjectId<MotionParamId>,
    pub(super) appearance: ObjectId<MotionParamId>,
    pub(super) params: [ObjectId<MotionParamId>; SHAPE_PARAM_COUNT],
    pub(super) path_hash: String,
    pub(super) appearance_hash: String,
    pub(super) mask: Option<MaskIds>,
}

impl GraphicObjectIds {
    pub(super) fn component(&self) -> ObjectId<VideoFilterComponent> {
        match self {
            Self::Text(ids) => ids.component,
            Self::Shape(ids) => ids.component,
            Self::Group(ids) => ids.component,
        }
    }

    /// Every identity of `objects`, in chain order: a SubGroup, then its
    /// members.
    fn allocate(ids: &mut ObjectIdAllocator, objects: &[PrGraphicObject]) -> Vec<Self> {
        objects
            .iter()
            .map(|object| match object {
                PrGraphicObject::Text(_) | PrGraphicObject::TextLines(_) => Self::Text(TextIds {
                    component: ids.take(),
                    source_text: ids.take(),
                    params: std::array::from_fn(|_| ids.take()),
                    source_text_hash: uuid(),
                }),
                PrGraphicObject::Shape(shape) => Self::Shape(ShapeIds {
                    component: ids.take(),
                    path: ids.take(),
                    appearance: ids.take(),
                    params: std::array::from_fn(|_| ids.take()),
                    path_hash: uuid(),
                    appearance_hash: uuid(),
                    mask: shape.mask.as_ref().map(|_| MaskIds {
                        component: ids.take(),
                        params: std::array::from_fn(|_| ids.take()),
                        path_hash: uuid(),
                        private_data_hash: uuid(),
                    }),
                }),
                PrGraphicObject::Group(group) => Self::Group(GroupIds {
                    component: ids.take(),
                    params: std::array::from_fn(|_| ids.take()),
                    objects: Self::allocate(ids, &group.objects),
                }),
            })
            .collect()
    }
}

/// The number of SubGroup members among `objects`, at every depth.
fn group_members(objects: &[PrGraphicObject]) -> usize {
    objects
        .iter()
        .map(|object| match object {
            PrGraphicObject::Group(group) => group.objects.len() + group_members(&group.objects),
            PrGraphicObject::Text(_)
            | PrGraphicObject::TextLines(_)
            | PrGraphicObject::Shape(_) => 0,
        })
        .sum()
}

/// A keyed graphic's Vector Motion component and its parameters.
#[derive(Debug)]
pub(super) struct VectorMotionIds {
    pub(super) component: ObjectId<VideoFilterComponent>,
    pub(super) params: [ObjectId<MotionParamId>; VECTOR_MOTION_PARAM_COUNT],
}

#[derive(Debug)]
pub(super) struct TimeRemapIds {
    pub(super) mapping: ObjectId<TimeRemapping>,
    pub(super) parameter: ObjectId<TimeParamId>,
}

#[derive(Debug)]
pub(super) struct MotionIds {
    pub(super) component: ObjectId<VideoFilterComponent>,
    pub(super) params: [ObjectId<MotionParamId>; MOTION_PARAM_COUNT],
}

#[derive(Debug)]
pub(super) struct OpacityIds {
    pub(super) component: ObjectId<VideoFilterComponent>,
    pub(super) params: [ObjectId<MotionParamId>; OPACITY_PARAM_COUNT],
    /// The mask that the Opacity names in `SubComponents`.
    pub(super) mask: Option<MaskIds>,
}

/// Identities of one mask record in the written form ([`MASK_FORM_V7`]):
/// its parameters in `ParameterID` order, the Mask Path among them.
#[derive(Debug)]
pub(super) struct MaskIds {
    pub(super) component: ObjectId<VideoFilterComponent>,
    pub(super) params: [ObjectId<MotionParamId>; MASK_FORM_V7.param_count],
    /// The `BinaryHash` of the Mask Path value and of the private data; two
    /// binaries under one hash would read as a conflict.
    pub(super) path_hash: String,
    pub(super) private_data_hash: String,
}

#[derive(Debug)]
pub(super) struct CropIds {
    pub(super) component: ObjectId<VideoFilterComponent>,
    pub(super) params: [ObjectId<MotionParamId>; CROP_PARAM_COUNT],
}

#[derive(Debug)]
pub(super) struct LinearWipeIds {
    pub(super) component: ObjectId<VideoFilterComponent>,
    pub(super) params: [ObjectId<MotionParamId>; 3],
}

/// Identities of one Track Matte Key record: its parameters in the native
/// `Params` order of [`TRACK_MATTE_KEY`].
#[derive(Debug)]
pub(super) struct TrackMatteIds {
    pub(super) component: ObjectId<VideoFilterComponent>,
    pub(super) params: [ObjectId<MotionParamId>; TRACK_MATTE_KEY.params.len()],
}

#[derive(Debug)]
pub(super) struct EffectIds {
    pub(super) component: ObjectId<VideoFilterComponent>,
    /// Parameter records in the effect's native `Params` order.
    pub(super) params: Vec<ObjectId<MotionParamId>>,
    pub(super) mask: Option<MaskIds>,
}

struct ObjectIdAllocator {
    next: u32,
}

impl ObjectIdAllocator {
    const fn new() -> Self {
        Self { next: 1 }
    }

    fn take<T>(&mut self) -> ObjectId<T> {
        let id = ObjectId::new(self.next);
        self.next += 1;
        id
    }

    fn skip(&mut self, count: u32) {
        self.next += count;
    }
}

impl ProjectIds {
    fn new(media_specs: &[&PrMedia], sequence: &PrSequence) -> Self {
        let media_count = media_specs.len();
        let audio_track_count = audio_track_count(sequence);
        let mut ids = ObjectIdAllocator::new();
        let project = ids.take();
        ids.skip(1); // ObjectID 2 is absent in the native scaffold.
        let project_settings = ids.take();
        ids.skip(5); // ObjectIDs 4–8 are absent in the native scaffold.
        let scratch_disk_settings = ids.take();
        let ingest_settings = ids.take();
        let workspace_settings = ids.take();
        // Preserve the existing project settings and placement numbering.
        let video_settings = ids.take();
        let audio_settings = ids.take();
        let video_compile_settings = ids.take();
        let audio_compile_settings = ids.take();
        let dummy_capture_settings = ids.take();
        let default_sequence_settings = ids.take();
        let sequence_ids =
            SequenceIds::allocate(&mut ids, sequence.video_tracks.len(), audio_track_count);
        let mixer = MixerIds::allocate(&mut ids, audio_track_count);
        debug_assert_eq!(ids.next, 70 + 7 * (audio_track_count as u32 - 4));
        let mut media = Vec::with_capacity(media_count);
        let mut flat_placements = Vec::with_capacity(sequence.video_items().count());
        let mut block = 0;
        for item in sequence.video_items() {
            let occurrence = match item {
                PrVideoItem::Media(occurrence) => occurrence,
                PrVideoItem::Graphic(graphic) => {
                    flat_placements.push(ItemIds::Graphic(GraphicIds::allocate(&mut ids, graphic)));
                    continue;
                }
            };
            if block < media_count {
                media.push(MediaIds::allocate(&mut ids));
            } else {
                ids.skip(6);
            }
            block += 1;
            flat_placements.push(ItemIds::Media(PlacementIds::allocate(
                &mut ids,
                occurrence.into(),
            )));
        }
        // Media placed only inside nested sequences follows every top-level block.
        while media.len() < media_count {
            media.push(MediaIds::allocate(&mut ids));
        }
        for (spec, media) in media_specs.iter().zip(&mut media) {
            if let Some(audio) = &spec.audio {
                let channels = audio.channels.count();
                media.audio = Some(AudioMediaIds {
                    stream: ids.take(),
                    source: ids.take(),
                    template_clip: ids.take(),
                    components: ids.take(),
                    channel_vector: ids.take(),
                    secondary: (0..channels).map(|_| ids.take()).collect(),
                    channels: (0..channels).map(|_| ids.take()).collect(),
                });
            }
        }
        let layouts: AudioLayouts<'_> = sequence
            .media_in_order()
            .into_iter()
            .zip(media_specs)
            .filter_map(|(id, spec)| Some((id, spec.audio.as_ref()?.channels)))
            .collect();
        let audio_placements = sequence
            .audio
            .iter()
            .map(|clip| AudioPlacementIds::allocate(&mut ids, clip, layouts[&clip.media]))
            .collect();
        let mut flat_placements = flat_placements.into_iter();
        let placements = sequence
            .video_tracks()
            .map(|track| {
                track
                    .iter()
                    .map(|_| flat_placements.next().expect("allocated every track item"))
                    .collect()
            })
            .collect();
        debug_assert!(flat_placements.next().is_none());
        let nests = NestIds::allocate(&mut ids, sequence, &layouts);
        // Compile settings own separate empty settings records in native saves.
        // Allocate them last so existing sequence/media/placement IDs stay fixed.
        let shell = ShellIds {
            project,
            document: Uid::random(),
            project_guid: uuid(),
            view_state: uuid(),
            project_settings,
            scratch_disk_settings,
            ingest_settings,
            workspace_settings,
            video_settings,
            audio_settings,
            video_compile_settings,
            audio_compile_settings,
            compile_video_settings: ids.take(),
            compile_audio_settings: ids.take(),
            dummy_capture_settings,
            default_sequence_settings,
        };

        Self {
            shell,
            main: SequenceGraphIds {
                sequence: sequence_ids,
                mixer,
                placements,
                audio_placements,
                nests,
                video_transitions: sequence
                    .video_tracks
                    .iter()
                    .map(|track| track.transitions.iter().map(|_| ids.take()).collect())
                    .collect(),
            },
            media,
        }
    }
}

impl SequenceIds {
    fn allocate(ids: &mut ObjectIdAllocator, track_count: usize, audio_track_count: usize) -> Self {
        Self {
            item: Uid::random(),
            master: Uid::random(),
            audio_clip_uid: uuid(),
            video_clip_uid: uuid(),
            sequence: Uid::random(),
            logging: ids.take(),
            audio_component_chain: ids.take(),
            audio_clip: ids.take(),
            video_clip: ids.take(),
            channel_groups: ids.take(),
            audio_source: ids.take(),
            secondary_content: std::array::from_fn(|_| ids.take()),
            video_source: ids.take(),
            channel_vector: ids.take(),
            channels: std::array::from_fn(|_| ids.take()),
            video_group: ids.take(),
            audio_group: ids.take(),
            data_group: ids.take(),
            video_component_chain: ids.take(),
            video_tracks: (0..track_count).map(|_| Uid::random()).collect(),
            audio_tracks: (0..audio_track_count).map(|_| Uid::random()).collect(),
            audio_group_uid: uuid(),
            audio_track_uids: (0..audio_track_count).map(|_| uuid()).collect(),
            mix_track_uid: uuid(),
        }
    }
}

impl MixerIds {
    fn allocate(ids: &mut ObjectIdAllocator, audio_track_count: usize) -> Self {
        // Native numbering allocates chains first, then components, then parameters.
        // Keep each strip together through those passes without storing parallel arrays.
        let mix_track = ids.take();
        let strips: Vec<_> = (0..audio_track_count)
            .map(|_| (ids.take(), ids.take()))
            .collect();
        let master_chain = ids.take();
        let master_panner = ids.take();
        let inlet = ids.take();
        let strips: Vec<_> = strips
            .into_iter()
            .map(|(chain, panner)| (chain, panner, ids.take(), ids.take(), ids.take()))
            .collect();
        let master_fader = ids.take();
        let master_meter = ids.take();
        let strips = strips
            .into_iter()
            .map(|(chain, panner, fader, meter, balance)| MixerStripIds {
                chain,
                panner,
                balance,
                components: MixerComponentIds {
                    fader,
                    meter,
                    volume: ids.take(),
                    mute: ids.take(),
                },
            })
            .collect();
        Self {
            mix_track,
            strips,
            master: MixerMasterIds {
                chain: master_chain,
                panner: master_panner,
                components: MixerComponentIds {
                    fader: master_fader,
                    meter: master_meter,
                    volume: ids.take(),
                    mute: ids.take(),
                },
            },
            inlet,
        }
    }
}

impl MediaIds {
    fn allocate(ids: &mut ObjectIdAllocator) -> Self {
        Self {
            audio: None,
            stream: ids.take(),
            source: ids.take(),
            markers: ids.take(),
            logging: ids.take(),
            template_clip: ids.take(),
            channels: ids.take(),
            media: Uid::random(),
            media_state: uuid::Uuid::new_v4(),
            media_binary_hash: uuid(),
            media_file_key: uuid(),
            template_clip_uid: uuid(),
            master: Uid::random(),
            item: Uid::random(),
        }
    }
}

impl PlacementIds {
    fn allocate(ids: &mut ObjectIdAllocator, edits: PlacementEdits<'_>) -> Self {
        Self {
            placed_clip: ids.take(),
            subclip: ids.take(),
            components: ids.take(),
            track_item: ids.take(),
            ramp: edits.ramp.then(|| TimeRemapIds {
                mapping: ids.take(),
                parameter: ids.take(),
            }),
            opacity: (edits.opacity != 100.0
                || edits.blend_mode != PrBlendMode::Normal
                || edits.opacity_mask.is_some()
                || edits
                    .animations
                    .iter()
                    .any(|animation| animation.property() == PrAnimatedProperty::Opacity))
            .then(|| OpacityIds {
                component: ids.take(),
                params: std::array::from_fn(|_| ids.take()),
                mask: edits.opacity_mask.map(|_| MaskIds {
                    component: ids.take(),
                    params: std::array::from_fn(|_| ids.take()),
                    path_hash: uuid(),
                    private_data_hash: uuid(),
                }),
            }),
            motion: (edits.transform != Default::default()
                || edits
                    .animations
                    .iter()
                    .any(|animation| animation.property() != PrAnimatedProperty::Opacity))
            .then(|| MotionIds {
                component: ids.take(),
                params: std::array::from_fn(|_| ids.take()),
            }),
            crop: (!edits.crop.is_default()).then(|| CropIds {
                component: ids.take(),
                params: std::array::from_fn(|_| ids.take()),
            }),
            linear_wipe: edits.linear_wipe.map(|_| LinearWipeIds {
                component: ids.take(),
                params: std::array::from_fn(|_| ids.take()),
            }),
            track_matte: edits.track_matte.map(|_| TrackMatteIds {
                component: ids.take(),
                params: std::array::from_fn(|_| ids.take()),
            }),
            effects: edits
                .effects
                .iter()
                .map(|effect| EffectIds {
                    component: ids.take(),
                    params: effect.spec().params.iter().map(|_| ids.take()).collect(),
                    mask: effect.mask.as_ref().map(|_| MaskIds {
                        component: ids.take(),
                        params: std::array::from_fn(|_| ids.take()),
                        path_hash: uuid(),
                        private_data_hash: uuid(),
                    }),
                })
                .collect(),
            clip_uid: uuid(),
        }
    }
}

impl SequenceGraphIds {
    /// Allocates a nested sequence, the placements on its tracks, its sounds
    /// and its own nests.
    fn nested(
        ids: &mut ObjectIdAllocator,
        sequence: &PrSequence,
        layouts: &AudioLayouts<'_>,
    ) -> Self {
        let audio_track_count = audio_track_count(sequence);
        let sequence_ids =
            SequenceIds::allocate(ids, sequence.video_tracks.len(), audio_track_count);
        let mixer = MixerIds::allocate(ids, audio_track_count);
        let mut placements = Vec::with_capacity(sequence.video_tracks.len());
        for track in sequence.video_tracks() {
            let mut track_ids = Vec::with_capacity(track.len());
            for item in track {
                track_ids.push(match item {
                    PrVideoItem::Media(occurrence) => {
                        ItemIds::Media(PlacementIds::allocate(ids, occurrence.into()))
                    }
                    PrVideoItem::Graphic(graphic) => {
                        ItemIds::Graphic(GraphicIds::allocate(ids, graphic))
                    }
                });
            }
            placements.push(track_ids);
        }
        let audio_placements = sequence
            .audio
            .iter()
            .map(|clip| AudioPlacementIds::allocate(ids, clip, layouts[&clip.media]))
            .collect();
        Self {
            sequence: sequence_ids,
            mixer,
            placements,
            audio_placements,
            nests: NestIds::allocate(ids, sequence, layouts),
            video_transitions: sequence
                .video_tracks
                .iter()
                .map(|track| track.transitions.iter().map(|_| ids.take()).collect())
                .collect(),
        }
    }

    /// Project items of every sequence nested below this one, outermost first.
    pub(super) fn nested_items(&self) -> Vec<Uid<ClipProjectItem>> {
        self.nests
            .iter()
            .flatten()
            .flat_map(|nest| {
                std::iter::once(nest.inner.sequence.item).chain(nest.inner.nested_items())
            })
            .collect()
    }
}

impl NestIds {
    /// Allocates the nested placements of `sequence`, track by track.
    fn allocate(
        ids: &mut ObjectIdAllocator,
        sequence: &PrSequence,
        layouts: &AudioLayouts<'_>,
    ) -> Vec<Vec<Self>> {
        let mut tracks = Vec::with_capacity(sequence.video_tracks.len());
        for track in &sequence.video_tracks {
            let mut nests = Vec::with_capacity(track.nests.len());
            for nest in &track.nests {
                nests.push(Self {
                    placement: PlacementIds::allocate(ids, nest.into()),
                    inner: SequenceGraphIds::nested(ids, &nest.sequence, layouts),
                    sound: nest
                        .sequence
                        .has_sound()
                        .then(|| NestSoundIds::allocate(ids)),
                });
            }
            tracks.push(nests);
        }
        tracks
    }
}

impl GraphicIds {
    /// Every identity of `graphic`, in the order the native scaffold numbers them.
    fn allocate(ids: &mut ObjectIdAllocator, graphic: &PrGraphic) -> Self {
        Self {
            media: Uid::random(),
            stream: ids.take(),
            source: ids.take(),
            master: Uid::random(),
            logging: ids.take(),
            template_clip: ids.take(),
            channels: ids.take(),
            placed_clip: ids.take(),
            subclip: ids.take(),
            components: ids.take(),
            objects: GraphicObjectIds::allocate(ids, &graphic.objects),
            group_map: match group_members(&graphic.objects) {
                0 => None,
                members => Some(GroupMapIds {
                    vector: ids.take(),
                    pins: (0..members).map(|_| ids.take()).collect(),
                }),
            },
            vector_motion: graphic.vector_motion.as_ref().map(|_| VectorMotionIds {
                component: ids.take(),
                params: std::array::from_fn(|_| ids.take()),
            }),
            // As for a media placement: only a nondefault or masked clip
            // Opacity has its own component.
            opacity: (graphic.opacity != 100.0
                || graphic.blend_mode != PrBlendMode::Normal
                || !graphic.animations.is_empty()
                || graphic.opacity_mask.is_some())
            .then(|| OpacityIds {
                component: ids.take(),
                params: std::array::from_fn(|_| ids.take()),
                mask: graphic.opacity_mask.as_ref().map(|_| MaskIds {
                    component: ids.take(),
                    params: std::array::from_fn(|_| ids.take()),
                    path_hash: uuid(),
                    private_data_hash: uuid(),
                }),
            }),
            track_item: ids.take(),
            template_clip_uid: uuid(),
            clip_uid: uuid(),
        }
    }
}

impl AudioPlacementIds {
    /// The placement's transitions, as its track lists them.
    pub(super) fn transitions(&self) -> Vec<ObjectId<AudioTransitionTrackItem>> {
        self.fade_in.into_iter().chain(self.fade_out).collect()
    }

    /// The identities of one sound placement whose source has `channels`.
    fn allocate(
        ids: &mut ObjectIdAllocator,
        clip: &PrAudioOccurrence,
        channels: AudioChannels,
    ) -> Self {
        Self {
            clip: ids.take(),
            subclip: ids.take(),
            components: ids.take(),
            track_item: ids.take(),
            secondary: (0..channels.count()).map(|_| ids.take()).collect(),
            volume: (clip.volume.as_f64() != channels.centered_stereo_gain()
                || clip.volume_keys.is_some())
            .then(|| ClipVolumeIds {
                component: ids.take(),
                mute: ids.take(),
                level: ids.take(),
                channel_volume: (channels == AudioChannels::Stereo).then(|| ChannelVolumeIds {
                    component: ids.take(),
                    bypass: ids.take(),
                    levels: (0..CHANNEL_VOLUME_NAMES.len() + CHANNEL_VOLUME_EXTRA_PARAMS)
                        .map(|_| ids.take())
                        .collect(),
                }),
            }),
            fade_in: clip.fade_in.as_ref().map(|_| ids.take()),
            fade_out: clip.fade_out.as_ref().map(|_| ids.take()),
        }
    }
}

pub(super) fn build(
    spec: &PrSequence,
    bound_media: &std::collections::BTreeMap<&crate::format::MediaId, BoundMedia<'_>>,
) -> Result<PremiereData> {
    let order = spec.media_in_order();
    let media_specs: Vec<_> = order.iter().map(|id| bound_media[id].media()).collect();
    let ids = ProjectIds::new(&media_specs, spec);
    let media_ids: std::collections::BTreeMap<_, _> = order
        .iter()
        .zip(&ids.media)
        .map(|(id, ids)| (*id, ids))
        .collect();
    let mut records = project::records(&ids);
    records.extend(sequence::records(spec, &ids.main)?);
    for id in order {
        records.extend(media::records(&bound_media[id], media_ids[id]));
    }
    for (item, item_ids) in spec.video_items().zip(ids.main.placements.iter().flatten()) {
        match (item, item_ids) {
            (PrVideoItem::Media(occurrence), ItemIds::Media(placement)) => {
                records.extend(video::placement_records(
                    spec,
                    occurrence,
                    bound_media[&occurrence.media].media(),
                    media_ids[&occurrence.media],
                    placement,
                )?);
            }
            (PrVideoItem::Graphic(graphic), ItemIds::Graphic(graphic_ids)) => {
                records.extend(graphic::records(spec, graphic, graphic_ids)?);
            }
            _ => unreachable!("ProjectIds allocates identities of each item's own kind"),
        }
    }
    for (occurrence, placement) in spec.audio.iter().zip(&ids.main.audio_placements) {
        records.extend(audio::placement_records(
            occurrence,
            bound_media[&occurrence.media].media(),
            media_ids[&occurrence.media],
            placement,
        )?);
    }
    records.extend(nested::records(spec, &ids.main, bound_media, &media_ids)?);
    super::tracks::transitions::attach(spec, &ids.main, &mut records)?;
    Ok(PremiereData {
        version: "3",
        root: ids.shell.project.into(),
        records,
    })
}
