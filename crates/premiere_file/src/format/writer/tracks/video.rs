use super::{animation, clip_track, effects, mask, nested, video_track_id, MediaKind};
use crate::format::{
    writer::{
        graph::{MediaIds, PlacementEdits, PlacementIds, SequenceGraphIds},
        media::media_clip,
    },
    Result,
};
use crate::schema::{
    chain_render_order, native::*, records, ColorSpace, PrMedia, PrSequence, PrVideoOccurrence,
    ToneMapSettings,
};

pub(in crate::format::writer) fn group_record(spec: &PrSequence, ids: &SequenceGraphIds) -> Record {
    let output_color = serde_json::to_string(&ColorSpace::sequence_sdr())
        .expect("native sequence color fields serialize");
    Record::VideoTrackGroup(VideoTrackGroup {
        object_id: ids.sequence.video_group,
        class_id: Some(records::VIDEO_TRACK_GROUP.class_id.into()),
        version: Some(records::VIDEO_TRACK_GROUP.version.into()),
        track_group: Some(TrackGroup {
            version: Some(records::TRACK_GROUP_VERSION.into()),
            tracks: Some(Tracks::from_uids(ids.sequence.video_tracks.iter().copied())),
            frame_rate: Some(spec.frame_rate.ticks_per_frame().to_string()),
            next_track_id: (ids.sequence.video_tracks.len() + 1).into(),
        }),
        color_management_settings: Some(records::COLOR_MANAGEMENT_SETTINGS.into()),
        immersive_video_vr_configuration: Some(records::IMMERSIVE_VIDEO_VR_CONFIGURATION.into()),
        output_color_space: Some(output_color),
        auto_input_gamut_compression_enabled: Some("true".into()),
        is_graphics_white_same_as_project: Some("false".into()),
        is_color_aware_effects_enabled_same_as_project: Some("false".into()),
        frame_rect: Some(format!("0,0,{},{}", spec.width, spec.height)),
        pixel_aspect_ratio: None,
        component_owner: Some(ComponentOwner::video(ids.sequence.video_component_chain)),
    })
}

pub(in crate::format::writer) fn track_records(
    spec: &PrSequence,
    ids: &SequenceGraphIds,
) -> Vec<Record> {
    let mut records_out = Vec::with_capacity(ids.sequence.video_tracks.len() + 1);
    for (index, (placements, uid)) in ids
        .placements
        .iter()
        .zip(&ids.sequence.video_tracks)
        .enumerate()
    {
        let placements = IndexedRef::list(nested::track_items(
            &spec.video_tracks[index],
            placements,
            &ids.nests[index],
        ));
        records_out.push(Record::VideoClipTrack(VideoClipTrack {
            object_uid: Some(uid.as_native_string()),
            class_id: Some(records::VIDEO_CLIP_TRACK.class_id.to_owned()),
            version: Some(records::VIDEO_CLIP_TRACK.version.to_owned()),
            clip_track: Some(clip_track(
                MediaKind::Video,
                video_track_id(index),
                index,
                placements,
            )),
        }));
    }
    records_out.push(Record::VideoComponentChain(VideoComponentChain {
        object_id: Some(ids.sequence.video_component_chain.as_native_string()),
        class_id: Some(records::VIDEO_COMPONENT_CHAIN.class_id.into()),
        version: Some(records::VIDEO_COMPONENT_CHAIN.version.into()),
        default_motion: None,
        default_opacity: None,
        default_motion_component_id: None,
        default_opacity_component_id: None,
        component_chain: Some(VideoChain {
            version: Some(records::COMPONENT_CHAIN_VERSION.into()),
            node: None,
            components: None,
        }),
    }));
    records_out
}

pub(in crate::format::writer) fn placement_records(
    project: &PrSequence,
    spec: &PrVideoOccurrence,
    media: &PrMedia,
    ids: &MediaIds,
    placement: &PlacementIds,
) -> Result<Vec<Record>> {
    let tone_map =
        serde_json::to_string(&ToneMapSettings::DEFAULT).expect("native tone-map fields serialize");
    let mut output = vec![
        Record::VideoClip(VideoClip {
            object_id: Some(placement.placed_clip.as_native_string()),
            class_id: Some(records::VIDEO_CLIP.class_id.into()),
            version: Some(records::VIDEO_CLIP.version.into()),
            clip: Some(media_clip(
                media,
                Some(spec),
                ids,
                placement.clip_uid.clone(),
            )),
            adjustment_layer: media.is_adjustment().then(|| "true".to_owned()),
            time_interpolation_type: spec.frame_blending.map(|mode| match mode {
                fx_schema::FrameBlendingMode::Simple => "1".to_owned(),
                fx_schema::FrameBlendingMode::OpticalFlow => "2".to_owned(),
            }),
            scale_to_frame_policy: None,
            _poster_frame: None,
            frame_blend: None,
            field_processing: None,
            hold_filters: None,
            deinterlace_on_hold: None,
            reverse_field_dominance: None,
            scale_to_frame_size: None,
            frame_hold: None,
            frame_hold_start: None,
        }),
        Record::SubClip(SubClip {
            object_id: placement.subclip,
            class_id: Some(records::SUB_CLIP.class_id.to_owned()),
            version: Some(records::SUB_CLIP.version.to_owned()),
            clip: Reference::object(placement.placed_clip),
            master_clip: Some(Reference::uid(ids.master)),
            name: Some(media.name.clone()),
            original_channel_group: Some("0".to_owned()),
        }),
        component_chain(spec.into(), placement),
        Record::VideoClipTrackItem(VideoClipTrackItem {
            object_id: Some(placement.track_item.as_native_string()),
            class_id: Some(records::VIDEO_CLIP_TRACK_ITEM.class_id.into()),
            version: Some(records::VIDEO_CLIP_TRACK_ITEM.version.into()),
            clip_track_item: Some(ClipTrackItem {
                version: Some("8".into()),
                component_owner: Some(ComponentOwner::video(placement.components)),
                track_item: Some(TrackItemRange {
                    version: Some("4".into()),
                    _node: None,
                    _item_type: None,
                    _media_type: None,
                    _track_index: None,
                    _track_ref_count: None,
                    start: (spec.start_ticks != 0).then(|| spec.start_ticks.to_string()),
                    end: spec.end_ticks.to_string(),
                }),
                sub_clip: Some(Reference::object(placement.subclip)),
                head_transition: None,
                tail_transition: None,
                is_muted: (!spec.enabled).then(|| "true".to_owned()),
                original_sub_clip_time_offset: None,
            }),
            pixel_aspect_ratio: Some(records::PIXEL_ASPECT_RATIO.into()),
            tone_map_settings: Some(tone_map),
            frame_rect: Some(format!("0,0,{},{}", project.width, project.height)),
        }),
    ];
    output.extend(component_records(spec.into(), placement)?);
    Ok(output)
}

/// The component chain of a media or nest placement.
pub(super) fn component_chain(edits: PlacementEdits<'_>, placement: &PlacementIds) -> Record {
    // The order in which the components apply: the effects applied before the
    // Crop, Linear Wipe or Track Matte Key, the mask, the effects applied
    // after it, then Motion and Opacity, which commute. The chain lists that
    // order reversed, so the component that applies last gets Index 0.
    let effects = placement.effects.iter().map(|ids| ids.component);
    let applied = effects
        .clone()
        .take(edits.effects_above_mask)
        .chain(placement.crop.iter().map(|crop| crop.component))
        .chain(placement.linear_wipe.iter().map(|wipe| wipe.component))
        .chain(placement.track_matte.iter().map(|matte| matte.component))
        .chain(effects.skip(edits.effects_above_mask))
        .chain(placement.motion.iter().map(|motion| motion.component))
        .chain(placement.opacity.iter().map(|opacity| opacity.component))
        .collect::<Vec<_>>();
    let component_ids = chain_render_order(applied).collect::<Vec<_>>();
    Record::VideoComponentChain(VideoComponentChain {
        object_id: Some(placement.components.as_native_string()),
        class_id: Some(records::VIDEO_COMPONENT_CHAIN.class_id.into()),
        version: Some(records::VIDEO_COMPONENT_CHAIN.version.into()),
        default_motion: placement.motion.is_none().then(|| "true".into()),
        default_opacity: placement.opacity.is_none().then(|| "true".into()),
        default_motion_component_id: placement.motion.is_none().then_some("1".into()),
        default_opacity_component_id: placement.opacity.is_none().then_some("2".into()),
        component_chain: Some(VideoChain {
            version: Some(records::COMPONENT_CHAIN_VERSION.into()),
            node: (!component_ids.is_empty()).then(|| {
                RetainedOrSkipped::from(Node {
                    version: "1",
                    properties: MotionChainProperties {
                        version: "1",
                        active_component_id: "2",
                        active_component_param_index: "4294967295",
                    },
                    id: None,
                })
            }),
            components: (!component_ids.is_empty())
                .then(|| MotionComponents::from_ids(component_ids)),
        }),
    })
}

/// The records of the components that `placement` allocated for `edits`.
pub(super) fn component_records(
    edits: PlacementEdits<'_>,
    placement: &PlacementIds,
) -> Result<Vec<Record>> {
    let mut output = Vec::new();
    if let Some(opacity) = &placement.opacity {
        output.extend(animation::opacity_records(
            edits.opacity,
            edits.blend_mode,
            edits.animations,
            opacity,
        )?);
        if let (Some(ids), Some(mask)) = (&opacity.mask, edits.opacity_mask) {
            output.extend(mask::records(mask, ids)?);
        }
    }
    if let Some(motion) = &placement.motion {
        output.extend(animation::records(
            edits.transform,
            edits.animations,
            motion,
        )?);
    }
    if let Some(crop) = &placement.crop {
        output.extend(animation::crop_records(edits.crop, crop));
    }
    if let (Some(ids), Some(wipe)) = (&placement.linear_wipe, edits.linear_wipe) {
        output.extend(animation::linear_wipe_records(wipe, ids)?);
    }
    if let (Some(ids), Some(matte)) = (&placement.track_matte, edits.track_matte) {
        output.extend(animation::track_matte_records(matte, ids));
    }
    output.extend(effects::records(edits.effects, &placement.effects)?);
    Ok(output)
}
