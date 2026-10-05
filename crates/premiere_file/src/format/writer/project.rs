use super::graph::ProjectIds;
use crate::schema::{native::*, records};

pub(super) fn records(ids: &ProjectIds) -> Vec<Record> {
    let shell = &ids.shell;
    let project = Project {
        object_id: shell.project,
        class_id: records::PROJECT.class_id,
        version: records::PROJECT.version,
        node: Node {
            version: records::NODE_VERSION,
            properties: ProjectProperties {
                version: records::PROPERTIES_VERSION,
                workspace_name: records::MZ_PROJECT_WORKSPACE_NAME,
                build_version_created: records::BUILD_VERSION,
                build_version_modified: records::BUILD_VERSION,
                application_id: records::MZ_PROJECT_APPLICATION_ID,
                project_guid: shell.project_guid.clone(),
                snapping_state: "1",
            },
            id: None,
        },
        root_project_item: shell.document.into(),
        project_settings: shell.project_settings.into(),
        scratch_disk_settings: shell.scratch_disk_settings.into(),
        ingest_settings: shell.ingest_settings.into(),
        project_workspace: shell.workspace_settings.into(),
        next_id: records::NEXT_ID,
    };

    let items = IndexedURef::list(
        std::iter::once(ids.main.sequence.item)
            .chain(ids.main.nested_items())
            .chain(ids.media.iter().map(|media| media.item)),
    );
    let root_project_item = RootProjectItem {
        object_uid: shell.document,
        class_id: records::ROOT_PROJECT_ITEM.class_id,
        version: records::ROOT_PROJECT_ITEM.version,
        project_item: ProjectItem {
            version: records::PROJECT_ITEM_VERSION,
            node: Node {
                version: records::NODE_VERSION,
                properties: RootProjectItemProperties {
                    version: records::PROPERTIES_VERSION,
                    project_view_state_id: shell.view_state.clone(),
                },
                id: Some(records::ROOT_BIN_ID),
            },
            name: records::ROOT_BIN_NAME.to_owned(),
        },
        container: ProjectItemContainer {
            version: "1",
            items: Items {
                version: "1",
                items,
            },
        },
    };

    let project_settings = ProjectSettings {
        object_id: shell.project_settings,
        class_id: records::PROJECT_SETTINGS.class_id,
        version: records::PROJECT_SETTINGS.version,
        video_settings: shell.video_settings.into(),
        audio_settings: shell.audio_settings.into(),
        video_compile_settings: shell.video_compile_settings.into(),
        audio_compile_settings: shell.audio_compile_settings.into(),
        capture_settings: shell.dummy_capture_settings.into(),
        default_sequence_settings: shell.default_sequence_settings.into(),
        color_management_settings: records::PROJECT_COLOR_MANAGEMENT_SETTINGS,
        video_time_display: "102",
        audio_time_display: "200",
        color_aware_effects_enabled: "0",
        video_time_display_initial: "102",
        action_safe_width: "10",
        action_safe_height: "10",
        title_safe_width: "20",
        title_safe_height: "20",
        should_scale_media: "false",
        editing_mode_id: records::ZERO_GUID,
        preview_file_format_id: records::ZERO_GUID,
        use_preview_cache: "false",
    };

    let scratch_disk_settings = ScratchDiskSettings {
        object_id: shell.scratch_disk_settings,
        class_id: records::SCRATCH_DISK_SETTINGS.class_id,
        version: records::SCRATCH_DISK_SETTINGS.version,
        audio_preview_location: records::SCRATCH_DISK_SAME_AS_PROJECT,
        video_preview_location: records::SCRATCH_DISK_SAME_AS_PROJECT,
        auto_save_location: records::SCRATCH_DISK_SAME_AS_PROJECT,
        dvd_encoding_location: records::SCRATCH_DISK_SAME_AS_PROJECT,
        transfer_media_location: records::SCRATCH_DISK_SAME_AS_PROJECT,
        cc_libraries_location: records::SCRATCH_DISK_SAME_AS_PROJECT,
        captured_video_location: records::SCRATCH_DISK_SAME_AS_PROJECT,
        capsule_media_location: records::SCRATCH_DISK_SAME_AS_PROJECT,
    };

    vec![
        Record::Project(project),
        Record::RootProjectItem(root_project_item),
        Record::ProjectSettings(project_settings),
        Record::VideoSettings(VideoSettings {
            object_id: shell.video_settings,
            class_id: records::VIDEO_SETTINGS.class_id,
            version: records::VIDEO_SETTINGS.version,
        }),
        Record::AudioSettings(AudioSettings {
            object_id: shell.audio_settings,
            class_id: records::AUDIO_SETTINGS.class_id,
            version: records::AUDIO_SETTINGS.version,
        }),
        // These are the project defaults saved by Premiere 26.5.1 in
        // feature_track_matte_key_26_5_strict.prproj, not sequence settings.
        Record::VideoCompileSettings(VideoCompileSettings {
            object_id: shell.video_compile_settings,
            class_id: records::VIDEO_COMPILE_SETTINGS.class_id,
            version: records::VIDEO_COMPILE_SETTINGS.version,
            video_settings: shell.compile_video_settings.into(),
            compressor: "1685480224",
            video_compiler_class_id_four_cc: "1061109567",
            video_file_type_four_cc: "1299148630",
            depth: "24",
            render_depth: "0",
            aspect_43: "false",
            quality: "100",
            use_data_rate: "false",
            data_rate: "3500",
            force_recompress: "true",
            force_recompress_value: "2",
            deinterlace: "false",
            ignore_video_filters: "false",
            optimize_stills: "false",
            frames_at_markers: "false",
            real_time_preview: "true",
            video_field_type: "0",
            do_keyframe_every_n_frames: "false",
            do_keyframe_every_n_frames_value: "0",
            add_keyframes_at_markers: "false",
            add_keyframes_at_edits: "false",
            relative_frame_size: "1",
        }),
        Record::AudioCompileSettings(AudioCompileSettings {
            object_id: shell.audio_compile_settings,
            class_id: records::AUDIO_COMPILE_SETTINGS.class_id,
            version: records::AUDIO_COMPILE_SETTINGS.version,
            audio_settings: shell.compile_audio_settings.into(),
            interleave: "1",
            sample_type: "3",
            compressor: "1380013856",
        }),
        Record::VideoSettings(VideoSettings {
            object_id: shell.compile_video_settings,
            class_id: records::VIDEO_SETTINGS.class_id,
            version: records::VIDEO_SETTINGS.version,
        }),
        Record::AudioSettings(AudioSettings {
            object_id: shell.compile_audio_settings,
            class_id: records::AUDIO_SETTINGS.class_id,
            version: records::AUDIO_SETTINGS.version,
        }),
        Record::ScratchDiskSettings(scratch_disk_settings),
        Record::IngestSettings(IngestSettings {
            object_id: shell.ingest_settings,
            class_id: records::INGEST_SETTINGS.class_id,
            version: records::INGEST_SETTINGS.version,
            action: records::ACTION,
            enabled: "false",
        }),
        Record::WorkspaceSettings(WorkspaceSettings {
            object_id: shell.workspace_settings,
            class_id: records::WORKSPACE_SETTINGS.class_id,
            version: records::WORKSPACE_SETTINGS.version,
        }),
        Record::DummyCaptureSettings(DummyCaptureSettings {
            object_id: shell.dummy_capture_settings,
            class_id: records::DUMMY_CAPTURE_SETTINGS.class_id,
            version: records::DUMMY_CAPTURE_SETTINGS.version,
        }),
        Record::DefaultSequenceSettings(DefaultSequenceSettings {
            object_id: shell.default_sequence_settings,
            class_id: records::DEFAULT_SEQUENCE_SETTINGS.class_id,
            version: records::DEFAULT_SEQUENCE_SETTINGS.version,
            total_video_tracks: "1",
            audio_standard_mono_tracks: "0",
            audio_standard_stereo_tracks: "1",
            audio_standard_51_tracks: "0",
            audio_submix_mono_tracks: "0",
            audio_submix_stereo_tracks: "0",
            audio_submix_51_tracks: "0",
        }),
    ]
}
