use super::{ClipProjectItem, IndexedURef, Node, ObjectId, Ref, URef, Uid};
use serde::Serialize;

#[derive(Debug)]
pub(crate) enum VideoSettings {}
#[derive(Debug)]
pub(crate) enum AudioSettings {}
#[derive(Debug)]
pub(crate) enum VideoCompileSettings {}
#[derive(Debug)]
pub(crate) enum AudioCompileSettings {}

#[derive(Debug, Serialize)]
pub(crate) struct ProjectProperties {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "MZ.Project.WorkspaceName")]
    pub(crate) workspace_name: &'static str,
    #[serde(rename = "MZ.BuildVersion.Created")]
    pub(crate) build_version_created: &'static str,
    #[serde(rename = "MZ.BuildVersion.Modified")]
    pub(crate) build_version_modified: &'static str,
    #[serde(rename = "MZ.Project.ApplicationID")]
    pub(crate) application_id: &'static str,
    #[serde(rename = "MZ.Project.GUID")]
    pub(crate) project_guid: String,
    #[serde(rename = "TL.PJSnappingState")]
    pub(crate) snapping_state: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct Project {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<Project>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    pub(crate) node: Node<ProjectProperties>,
    pub(crate) root_project_item: URef<RootProjectItem>,
    pub(crate) project_settings: Ref<ProjectSettings>,
    pub(crate) scratch_disk_settings: Ref<ScratchDiskSettings>,
    pub(crate) ingest_settings: Ref<IngestSettings>,
    pub(crate) project_workspace: Ref<WorkspaceSettings>,
    #[serde(rename = "NextID")]
    pub(crate) next_id: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct RootProjectItemProperties {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "ProjectViewState.ID")]
    pub(crate) project_view_state_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ProjectItem<P> {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    pub(crate) node: Node<P>,
    pub(crate) name: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct Items {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "Item")]
    pub(crate) items: Vec<IndexedURef<ClipProjectItem>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ProjectItemContainer {
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    pub(crate) items: Items,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct RootProjectItem {
    #[serde(rename = "@ObjectUID")]
    pub(crate) object_uid: Uid<RootProjectItem>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    pub(crate) project_item: ProjectItem<RootProjectItemProperties>,
    #[serde(rename = "ProjectItemContainer")]
    pub(crate) container: ProjectItemContainer,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ProjectSettings {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<ProjectSettings>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    pub(crate) video_settings: Ref<VideoSettings>,
    pub(crate) audio_settings: Ref<AudioSettings>,
    pub(crate) video_compile_settings: Ref<VideoCompileSettings>,
    pub(crate) audio_compile_settings: Ref<AudioCompileSettings>,
    pub(crate) capture_settings: Ref<DummyCaptureSettings>,
    pub(crate) default_sequence_settings: Ref<DefaultSequenceSettings>,
    pub(crate) color_management_settings: &'static str,
    pub(crate) video_time_display: &'static str,
    pub(crate) audio_time_display: &'static str,
    pub(crate) color_aware_effects_enabled: &'static str,
    pub(crate) video_time_display_initial: &'static str,
    pub(crate) action_safe_width: &'static str,
    pub(crate) action_safe_height: &'static str,
    pub(crate) title_safe_width: &'static str,
    pub(crate) title_safe_height: &'static str,
    pub(crate) should_scale_media: &'static str,
    #[serde(rename = "EditingModeID")]
    pub(crate) editing_mode_id: &'static str,
    #[serde(rename = "PreviewFileFormatID")]
    pub(crate) preview_file_format_id: &'static str,
    pub(crate) use_preview_cache: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct ScratchDiskSettings {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<ScratchDiskSettings>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    #[serde(rename = "AudioPreviewLocation0")]
    pub(crate) audio_preview_location: &'static str,
    #[serde(rename = "VideoPreviewLocation0")]
    pub(crate) video_preview_location: &'static str,
    #[serde(rename = "AutoSaveLocation0")]
    pub(crate) auto_save_location: &'static str,
    #[serde(rename = "DVDEncodingLocation0")]
    pub(crate) dvd_encoding_location: &'static str,
    #[serde(rename = "TransferMediaLocation0")]
    pub(crate) transfer_media_location: &'static str,
    #[serde(rename = "CCLibrariesLocation0")]
    pub(crate) cc_libraries_location: &'static str,
    #[serde(rename = "CapturedVideoLocation0")]
    pub(crate) captured_video_location: &'static str,
    #[serde(rename = "CapsuleMediaLocation0")]
    pub(crate) capsule_media_location: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct IngestSettings {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<IngestSettings>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    pub(crate) action: &'static str,
    pub(crate) enabled: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct WorkspaceSettings {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<WorkspaceSettings>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct DummyCaptureSettings {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<DummyCaptureSettings>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DefaultSequenceSettings {
    #[serde(rename = "@ObjectID")]
    pub(crate) object_id: ObjectId<DefaultSequenceSettings>,
    #[serde(rename = "@ClassID")]
    pub(crate) class_id: &'static str,
    #[serde(rename = "@Version")]
    pub(crate) version: &'static str,
    pub(crate) total_video_tracks: &'static str,
    #[serde(rename = "DefaultAudioStandardMonoTracks")]
    pub(crate) audio_standard_mono_tracks: &'static str,
    #[serde(rename = "DefaultAudioStandardStereoTracks")]
    pub(crate) audio_standard_stereo_tracks: &'static str,
    #[serde(rename = "DefaultAudioStandard51Tracks")]
    pub(crate) audio_standard_51_tracks: &'static str,
    #[serde(rename = "DefaultAudioSubmixMonoTracks")]
    pub(crate) audio_submix_mono_tracks: &'static str,
    #[serde(rename = "DefaultAudioSubmixStereoTracks")]
    pub(crate) audio_submix_stereo_tracks: &'static str,
    #[serde(rename = "DefaultAudioSubmix51Tracks")]
    pub(crate) audio_submix_51_tracks: &'static str,
}
