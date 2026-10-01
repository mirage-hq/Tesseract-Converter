//! Shared native XML records used by both Premiere conversion directions.

mod animation;
mod audio;
mod caption;
mod clip;
mod media;
mod project;
mod reference;
mod sequence;
mod track;

pub(crate) use animation::*;
pub(crate) use audio::*;
pub(crate) use caption::*;
pub(crate) use clip::*;
pub(crate) use media::*;
pub(crate) use project::*;
pub(crate) use reference::{Reference, ReferenceList};
pub(crate) use sequence::*;
pub(crate) use track::*;

use serde::{de::IgnoredAny, Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, marker::PhantomData};

/// A writer-owned XML subtree that the reader does not inspect. Its presence is
/// decoded without retaining a second shape; encoding only emits constructed data.
#[derive(Debug, Default)]
pub(crate) enum RetainedOrSkipped<T> {
    Retained(T),
    #[default]
    Skipped,
}

impl<'de, T> Deserialize<'de> for RetainedOrSkipped<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        IgnoredAny::deserialize(deserializer)?;
        Ok(Self::Skipped)
    }
}

impl<T: Serialize> Serialize for RetainedOrSkipped<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Retained(value) => value.serialize(serializer),
            Self::Skipped => Err(serde::ser::Error::custom(
                "cannot encode skipped XML subtree",
            )),
        }
    }
}

impl<T> RetainedOrSkipped<T> {
    pub(crate) fn is_skipped(&self) -> bool {
        matches!(self, Self::Skipped)
    }
}

impl<T> From<T> for RetainedOrSkipped<T> {
    fn from(value: T) -> Self {
        Self::Retained(value)
    }
}
use uuid::Uuid;

#[derive(Deserialize, Serialize)]
#[serde(transparent, bound = "")]
pub(crate) struct ObjectId<T> {
    value: u32,
    #[serde(skip)]
    owner: PhantomData<fn() -> T>,
}

impl<T> ObjectId<T> {
    pub(crate) const fn new(value: u32) -> Self {
        Self {
            value,
            owner: PhantomData,
        }
    }

    pub(crate) fn as_native_string(self) -> String {
        self.value.to_string()
    }
}

impl<T> Clone for ObjectId<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for ObjectId<T> {}

impl<T> fmt::Debug for ObjectId<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value.fmt(formatter)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(transparent, bound = "")]
pub(crate) struct Uid<T> {
    value: Uuid,
    #[serde(skip)]
    owner: PhantomData<fn() -> T>,
}

impl<T> Uid<T> {
    pub(crate) fn random() -> Self {
        Self {
            value: Uuid::new_v4(),
            owner: PhantomData,
        }
    }

    pub(crate) fn as_native_string(self) -> String {
        self.value.to_string()
    }
}

impl<T> Clone for Uid<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Uid<T> {}

impl<T> fmt::Debug for Uid<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value.fmt(formatter)
    }
}

#[derive(Debug, Serialize)]
#[serde(bound = "")]
pub(crate) struct Ref<T> {
    #[serde(rename = "@ObjectRef")]
    id: ObjectId<T>,
}

impl<T> From<ObjectId<T>> for Ref<T> {
    fn from(id: ObjectId<T>) -> Self {
        Self { id }
    }
}

#[derive(Debug, Serialize)]
#[serde(bound = "")]
pub(crate) struct URef<T> {
    #[serde(rename = "@ObjectURef")]
    id: Uid<T>,
}

impl<T> From<Uid<T>> for URef<T> {
    fn from(id: Uid<T>) -> Self {
        Self { id }
    }
}

#[derive(Debug, Serialize)]
#[serde(bound = "")]
pub(crate) struct IndexedRef<T> {
    #[serde(rename = "@Index")]
    index: usize,
    #[serde(rename = "@ObjectRef")]
    id: ObjectId<T>,
}

impl<T> IndexedRef<T> {
    pub(crate) fn new(index: usize, id: ObjectId<T>) -> Self {
        Self { index, id }
    }

    pub(crate) fn into_reference(self) -> Reference {
        Reference::indexed_object(self.index, self.id)
    }

    /// Numbers the references from zero in iteration order.
    pub(crate) fn list(ids: impl IntoIterator<Item = ObjectId<T>>) -> Vec<Self> {
        ids.into_iter()
            .enumerate()
            .map(|(index, id)| Self::new(index, id))
            .collect()
    }
}

#[derive(Debug, Serialize)]
#[serde(bound = "")]
pub(crate) struct IndexedURef<T> {
    #[serde(rename = "@Index")]
    index: usize,
    #[serde(rename = "@ObjectURef")]
    id: Uid<T>,
}

impl<T> IndexedURef<T> {
    pub(crate) fn new(index: usize, id: Uid<T>) -> Self {
        Self { index, id }
    }

    /// Numbers the references from zero in iteration order.
    pub(crate) fn list(ids: impl IntoIterator<Item = Uid<T>>) -> Vec<Self> {
        ids.into_iter()
            .enumerate()
            .map(|(index, id)| Self::new(index, id))
            .collect()
    }
}

#[derive(Debug, Serialize)]
#[serde(bound(serialize = "P: Serialize"))]
#[serde(rename_all = "PascalCase")]
pub(crate) struct Node<P> {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    pub(crate) properties: P,
    #[serde(rename = "ID", skip_serializing_if = "Option::is_none")]
    pub(crate) id: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename = "PremiereData")]
pub(crate) struct PremiereData {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "Project")]
    pub(crate) root: Ref<Project>,
    #[serde(rename = "$value")]
    pub(crate) records: Vec<Record>,
}

#[derive(Debug, Serialize)]
pub(crate) enum Record {
    Project(Project),
    RootProjectItem(RootProjectItem),
    ProjectSettings(ProjectSettings),
    ScratchDiskSettings(ScratchDiskSettings),
    IngestSettings(IngestSettings),
    WorkspaceSettings(WorkspaceSettings),
    DummyCaptureSettings(DummyCaptureSettings),
    DefaultSequenceSettings(DefaultSequenceSettings),
    ClipProjectItem(ClipProjectItem),
    MasterClip(MasterClip),
    ClipLoggingInfo(ClipLoggingInfo),
    AudioComponentChain(AudioComponentChain),
    AudioClip(AudioClip),
    VideoClip(VideoClip),
    ClipChannelGroupVectorSerializer(ClipChannelGroupVectorSerializer),
    AudioSequenceSource(AudioSequenceSource),
    SecondaryContent(SecondaryContent),
    VideoSequenceSource(VideoSequenceSource),
    ClipChannelVectorSerializer(ClipChannelVectorSerializer),
    Sequence(Sequence),
    ClipChannelSerializer(ClipChannelSerializer),
    VideoTrackGroup(VideoTrackGroup),
    AudioTrackGroup(AudioTrackGroup),
    DataTrackGroup(DataTrackGroup),
    VideoClipTrack(VideoClipTrack),
    VideoComponentChain(VideoComponentChain),
    VideoFilterComponent(VideoFilterComponent),
    VideoComponentParam(VideoComponentParam),
    PointComponentParam(PointComponentParam),
    ArbVideoComponentParam(ArbVideoComponentParam),
    AudioClipTrack(Box<AudioClipTrack>),
    AudioMixTrack(AudioMixTrack),
    StereoToStereoPanProcessor(StereoToStereoPanProcessor),
    DefaultPanProcessor(DefaultPanProcessor),
    AudioTrackInlet(AudioTrackInlet),
    AudioFader(AudioFader),
    AudioMeter(AudioMeter),
    AudioFilterComponent(AudioFilterComponent),
    AudioComponentParam(AudioComponentParam),
    VideoStream(VideoStream),
    AudioStream(AudioStream),
    Media(Media),
    VideoMediaSource(VideoMediaSource),
    AudioMediaSource(AudioMediaSource),
    Markers(Markers),
    SubClip(SubClip),
    VideoClipTrackItem(VideoClipTrackItem),
    AudioClipTrackItem(AudioClipTrackItem),
    Link(Link),
}
