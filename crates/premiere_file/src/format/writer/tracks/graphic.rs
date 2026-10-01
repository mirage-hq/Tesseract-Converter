//! Type-tool graphic records: private generator media, a master clip outside the
//! project panel, and one placement whose chain holds any nondefault clip Opacity,
//! any keyed Vector Motion, then the Text. Premiere 26.5.1 saves show
//! [Opacity, Text] and [Vector Motion, Text]; the order of all three is inferred.

use super::{
    animation::{opacity_records, point_keyframes, scalar_keyframes},
    point_start_keyframe, scalar_start_keyframe,
};
use crate::format::{
    invalid, shape_payload, text_payload,
    writer::{
        graph::{GraphicIds, GraphicObjectIds, ShapeIds, TextIds},
        media::video_media_source,
    },
};
use crate::schema::{
    native::*,
    records,
    text::{
        self, GraphicParamRole, GraphicParamSpec, PrGraphicObject, PrShape, PrTextTransform,
        SHAPE_PARAMS, TEXT_PARAMS, VECTOR_MOTION_PARAMS,
    },
    ColorSpace, PrGraphic, PrPropertyAnimation, PrSequence, PrText, ToneMapSettings,
};
use base64::{engine::general_purpose::STANDARD, Engine};

pub(in crate::format::writer) fn records(
    sequence: &PrSequence,
    graphic: &PrGraphic,
    ids: &GraphicIds,
) -> crate::format::Result<Vec<Record>> {
    let frame_rect = format!("0,0,{},{}", sequence.width, sequence.height);
    let duration = graphic.end_ticks - graphic.start_ticks;
    // Keys use this generator clock. Premiere places graphic clips one hour
    // into their generator, and export does the same.
    let placed_in = graphic.in_ticks;
    let placed_out = placed_in.checked_add(duration).ok_or_else(|| {
        invalid(format!(
            "sequence {:?}, {} ({}..{} ticks): source out-point {} ticks into the graphic generator exceeds Premiere's tick range",
            sequence.name,
            graphic.id.as_deref().unwrap_or("graphic"),
            graphic.start_ticks,
            graphic.end_ticks,
            placed_in
        ))
    })?;
    let color_space = serde_json::to_string(&ColorSpace::sequence_sdr())
        .expect("native graphic color fields serialize");
    let tone_map =
        serde_json::to_string(&ToneMapSettings::DEFAULT).expect("native tone-map fields serialize");
    let clip = |in_point: i64, out_point: i64, clip_id: &str, in_use: Option<&'static str>| Clip {
        version: Some(records::CLIP_VERSION.to_owned()),
        node: Node {
            version: records::NODE_VERSION,
            properties: ClipProperties::Graphic(GraphicClipProperties {
                version: records::PROPERTIES_VERSION,
                default_is_drop_frame: "false",
            }),
            id: None,
        }
        .into(),
        marker_owner: None,
        time_remapping: None,
        playback_speed: None,
        play_backwards: None,
        source: Some(Reference::object(ids.source)),
        out_point: Some(out_point.to_string()),
        in_point: Some(in_point.to_string()),
        clip_id: Some(clip_id.to_owned()),
        in_use: in_use.map(Into::into),
    };
    let mut output = vec![
        Record::Media(Media {
            importer_prefs: None,
            conformed_audio_rate: None,
            object_id: None,
            object_uid: Some(ids.media.as_native_string()),
            class_id: Some(records::MEDIA.class_id.to_owned()),
            version: Some(records::MEDIA.version.to_owned()),
            video_stream: Some(Reference::object(ids.stream)),
            modification_state: None,
            relative_paths: Vec::new(),
            file_path: Some(text::GRAPHIC_MEDIA_TOKEN.to_owned()),
            implementation_id: Some(text::GRAPHIC_IMPLEMENTATION_ID.to_owned()),
            title: Some(text::GRAPHIC_NAME.to_owned()),
            infinite: Some("true".to_owned()),
            file_key: None,
            content_and_metadata_state: None,
            actual_media_file_path: Some(text::GRAPHIC_MEDIA_TOKEN.to_owned()),
            audio_stream: None,
        }),
        Record::VideoStream(VideoStream {
            object_id: ids.stream,
            class_id: Some(records::VIDEO_STREAM.class_id.to_owned()),
            version: Some(records::VIDEO_STREAM.version.to_owned()),
            frame_rate: Some(sequence.frame_rate.ticks_per_frame().to_string()),
            is_frame_rate_overridden: None,
            overidden_frame_rate: None,
            duration: Some(text::GRAPHIC_MEDIA_TICKS.to_string()),
            ignore_alpha: None,
            frame_rect: Some(frame_rect.clone()),
            pixel_aspect_ratio: None,
            codec_type: Some(text::GRAPHIC_CODEC_TYPE.to_owned()),
            is_still: Some("true".to_owned()),
            is_overriden_image_orientation_type: None,
            is_continuous_time: Some("true".to_owned()),
            original_color_space: Some(color_space),
            alpha_type: Some("1".to_owned()),
            alpha_info_is_uncertain: Some("true".to_owned()),
            field_type_is_uncertain: Some("true".to_owned()),
            original_field_type: None,
            original_image_orientation_type: None,
        }),
        video_media_source(ids.source, ids.media, text::GRAPHIC_MEDIA_TICKS),
        Record::MasterClip(MasterClip {
            object_uid: Some(ids.master.as_native_string()),
            class_id: Some(records::MASTER_CLIP.class_id.into()),
            version: Some(records::MASTER_CLIP.version.into()),
            node: None,
            logging_info: Some(Ref::from(ids.logging).into()),
            audio_component_chains: None,
            clips: Some(Clips::from_ids(None, Some(ids.template_clip))),
            audio_clip_channel_groups: Some(Ref::from(ids.channels).into()),
            name: Some(text::GRAPHIC_NAME.to_owned().into()),
            is_adjustment_layer: None,
            change_version: Some("0".to_owned().into()),
        }),
        Record::ClipLoggingInfo(ClipLoggingInfo {
            object_id: ids.logging,
            class_id: records::CLIP_LOGGING_INFO.class_id,
            version: records::CLIP_LOGGING_INFO.version,
            capture_mode: Some("2"),
            clip_name: Some(text::GRAPHIC_NAME.to_owned()),
            timecode_format: Some("104"),
            media_in_point: None,
            media_out_point: None,
            media_frame_rate: Some(sequence.frame_rate.ticks_per_frame()),
        }),
        Record::VideoClip(VideoClip {
            object_id: Some(ids.template_clip.as_native_string()),
            class_id: Some(records::VIDEO_CLIP.class_id.into()),
            version: Some(records::VIDEO_CLIP.version.into()),
            clip: Some(clip(
                0,
                duration,
                &ids.template_clip_uid,
                Some(records::IN_USE),
            )),
            adjustment_layer: None,
            time_interpolation_type: None,
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
        Record::ClipChannelGroupVectorSerializer(ClipChannelGroupVectorSerializer {
            object_id: ids.channels,
            class_id: records::CLIP_CHANNEL_GROUP_VECTOR_SERIALIZER.class_id,
            version: records::CLIP_CHANNEL_GROUP_VECTOR_SERIALIZER.version,
            vectors: None,
        }),
        Record::VideoClip(VideoClip {
            object_id: Some(ids.placed_clip.as_native_string()),
            class_id: Some(records::VIDEO_CLIP.class_id.into()),
            version: Some(records::VIDEO_CLIP.version.into()),
            clip: Some(clip(placed_in, placed_out, &ids.clip_uid, None)),
            adjustment_layer: None,
            time_interpolation_type: None,
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
            object_id: ids.subclip,
            class_id: Some(records::SUB_CLIP.class_id.to_owned()),
            version: Some(records::SUB_CLIP.version.to_owned()),
            clip: Reference::object(ids.placed_clip),
            master_clip: Some(Reference::uid(ids.master)),
            name: Some(text::GRAPHIC_NAME.to_owned()),
            original_channel_group: Some("0".to_owned()),
        }),
        Record::VideoComponentChain(VideoComponentChain {
            object_id: Some(ids.components.as_native_string()),
            class_id: Some(records::VIDEO_COMPONENT_CHAIN.class_id.into()),
            version: Some(records::VIDEO_COMPONENT_CHAIN.version.into()),
            default_motion: Some("true".into()),
            // Premiere 26.5.1 drops `DefaultOpacity` from a graphic clip
            // whose Opacity it keeps, as for a media clip.
            default_opacity: ids.opacity.is_none().then(|| "true".into()),
            default_motion_component_id: Some("1".into()),
            default_opacity_component_id: ids.opacity.is_none().then(|| "2".into()),
            component_chain: Some(VideoChain {
                version: Some(records::COMPONENT_CHAIN_VERSION.into()),
                node: Some(
                    Node {
                        version: records::NODE_VERSION,
                        properties: MotionChainProperties {
                            version: records::PROPERTIES_VERSION,
                            active_component_id: "2",
                            active_component_param_index: "4294967295",
                        },
                        id: None,
                    }
                    .into(),
                ),
                components: Some(MotionComponents::from_ids(
                    ids.opacity // first, as saved; the order with a Vector Motion is inferred
                        .as_ref()
                        .map(|opacity| opacity.component)
                        .into_iter()
                        .chain(ids.vector_motion.as_ref().map(|motion| motion.component))
                        .chain(ids.objects.iter().map(GraphicObjectIds::component)),
                )),
            }),
        }),
    ];
    let frame = [f64::from(sequence.width), f64::from(sequence.height)];
    // Component IDs count from the first object's 4, as in Premiere 26.5.1
    // saves of Type-tool graphics.
    for (index, (object, object_ids)) in graphic.objects.iter().zip(&ids.objects).enumerate() {
        let id = (4 + index).to_string();
        match (object, object_ids) {
            (PrGraphicObject::Text(text), GraphicObjectIds::Text(text_ids)) => {
                output.extend(text_records(text, text_ids, id, frame)?);
            }
            (PrGraphicObject::Shape(shape), GraphicObjectIds::Shape(shape_ids)) => {
                output.extend(shape_records(shape, shape_ids, id, frame)?);
            }
            _ => {
                return Err(crate::format::invalid(
                    "graphic object identities do not follow its objects",
                ))
            }
        }
    }
    if let (Some(motion), Some(motion_ids)) = (&graphic.vector_motion, &ids.vector_motion) {
        // The Premiere 26.5.1 layout of keyed Vector Motion: the record that
        // keying creates, without private data, and the static parameters of
        // Adobe-saved Type-tool graphics. Its ID follows the objects' (5
        // after one Text's 4). A static Vector Motion of several objects is
        // written the same way.
        output.push(Record::VideoFilterComponent(VideoFilterComponent {
            object_id: motion_ids.component,
            class_id: Some(text::TEXT_FILTER_COMPONENT.class_id.to_owned()),
            version: Some(text::TEXT_FILTER_COMPONENT.version.to_owned()),
            component: Some(MotionBody {
                version: Some(text::TEXT_FILTER_BODY_VERSION.to_owned()),
                params: Some(MotionParams::from_ids(motion_ids.params)),
                id: Some((4 + ids.objects.len()).to_string()),
                display_name: Some("Vector Motion".to_owned()),
                instance_name: None,
                bypass: None,
                intrinsic: Some("true".to_owned()),
            }),
            premiere_filter_private_data: None,
            sub_components: None,
            match_name: Some(text::VECTOR_MOTION_MATCH_NAME.to_owned()),
            video_filter_type: Some("2".to_owned()),
        }));
        for (&object_id, spec) in motion_ids.params.iter().zip(&VECTOR_MOTION_PARAMS) {
            let value = match spec.role {
                GraphicParamRole::Position => normalized(motion.position, frame),
                GraphicParamRole::Anchor => normalized(motion.anchor, frame),
                GraphicParamRole::Scale => motion.scale.to_string(),
                GraphicParamRole::Rotation => motion.rotation.to_string(),
                GraphicParamRole::Opacity
                | GraphicParamRole::HorizontalScale
                | GraphicParamRole::Uniform
                | GraphicParamRole::Selection
                | GraphicParamRole::Fixed => spec.initial.to_owned(),
            };
            output.push(param_record(spec, object_id, value, &motion.animations)?);
        }
    }
    if let Some(opacity_ids) = &ids.opacity {
        output.extend(opacity_records(
            graphic.opacity,
            graphic.blend_mode,
            &graphic.animations,
            opacity_ids,
        )?);
    }
    output.push(Record::VideoClipTrackItem(VideoClipTrackItem {
        object_id: Some(ids.track_item.as_native_string()),
        class_id: Some(records::VIDEO_CLIP_TRACK_ITEM.class_id.into()),
        version: Some(records::VIDEO_CLIP_TRACK_ITEM.version.into()),
        clip_track_item: Some(ClipTrackItem {
            version: Some("8".into()),
            component_owner: Some(ComponentOwner::video(ids.components)),
            track_item: Some(TrackItemRange {
                version: Some("4".into()),
                _node: None,
                _item_type: None,
                _media_type: None,
                _track_index: None,
                _track_ref_count: None,
                start: (graphic.start_ticks != 0).then(|| graphic.start_ticks.to_string()),
                end: graphic.end_ticks.to_string(),
            }),
            sub_clip: Some(Reference::object(ids.subclip)),
            head_transition: None,
            tail_transition: None,
            is_muted: (!graphic.enabled).then(|| "true".to_owned()),
            original_sub_clip_time_offset: None,
        }),
        pixel_aspect_ratio: Some(records::PIXEL_ASPECT_RATIO.into()),
        tone_map_settings: Some(tone_map),
        frame_rect: Some(frame_rect),
    }));
    Ok(output)
}

/// The Text component of one text object, its Source Text and its
/// parameters.
fn text_records(
    text: &PrText,
    ids: &TextIds,
    id: String,
    frame: [f64; 2],
) -> crate::format::Result<Vec<Record>> {
    let TextIds {
        component,
        source_text,
        params,
        source_text_hash,
    } = ids;
    let node = Node {
        version: records::NODE_VERSION,
        properties: GraphicsProperties {
            version: records::PROPERTIES_VERSION,
            expanded: "true",
        },
        id: None,
    };
    let mut output = vec![
        object_component(
            *component,
            std::iter::once(*source_text).chain(*params),
            id,
            ("Text", text::TEXT_MATCH_NAME, text::TEXT_PRIVATE_DATA),
            &text.name,
        ),
        source_text_param(*source_text, node, text, source_text_hash)?,
    ];
    for (&object_id, spec) in params.iter().zip(&TEXT_PARAMS) {
        let value = text_value(&text.transform, spec, frame);
        output.push(param_record(spec, object_id, value, &text.animations)?);
    }
    Ok(output)
}

/// The Shape component of one shape object, in the layout that Premiere
/// 26.5.1 saves: its Path, its Appearance and static parameters, and the
/// calibration projects' private data.
fn shape_records(
    shape: &PrShape,
    ids: &ShapeIds,
    id: String,
    frame: [f64; 2],
) -> crate::format::Result<Vec<Record>> {
    let ShapeIds {
        component,
        path,
        appearance,
        params,
        path_hash,
        appearance_hash,
    } = ids;
    let mut output = vec![
        object_component(
            *component,
            [*path, *appearance].into_iter().chain(*params),
            id,
            ("Shape", text::SHAPE_MATCH_NAME, text::SHAPE_PRIVATE_DATA),
            &shape.name,
        ),
        Record::ArbVideoComponentParam(binary_param(
            *path,
            RetainedOrSkipped::Skipped,
            ("1", "Path", "22"),
            path_hash,
            &shape_payload::encode_path(&shape.path)?,
        )),
        Record::ArbVideoComponentParam(binary_param(
            *appearance,
            RetainedOrSkipped::Skipped,
            ("2", "Appearance", "9"),
            appearance_hash,
            &shape_payload::encode_appearance(&shape.appearance)?,
        )),
    ];
    for (&object_id, spec) in params.iter().zip(&SHAPE_PARAMS) {
        let value = match (spec.role, shape.horizontal_scale) {
            (GraphicParamRole::HorizontalScale, Some(scale)) => scale.to_string(),
            (GraphicParamRole::Uniform, Some(_)) => "false".to_owned(),
            _ => text_value(&shape.transform, spec, frame),
        };
        output.push(param_record(spec, object_id, value, &[])?);
    }
    Ok(output)
}

/// The component record of one graphic object: the filter `(DisplayName,
/// MatchName, PremiereFilterPrivateData)` with its parameters.
fn object_component(
    object_id: ObjectId<VideoFilterComponent>,
    params: impl IntoIterator<Item = ObjectId<MotionParamId>>,
    id: String,
    (display_name, match_name, (binary_hash, value)): (&str, &str, (&'static str, &'static str)),
    instance_name: &str,
) -> Record {
    Record::VideoFilterComponent(VideoFilterComponent {
        object_id,
        class_id: Some(text::TEXT_FILTER_COMPONENT.class_id.to_owned()),
        version: Some(text::TEXT_FILTER_COMPONENT.version.to_owned()),
        component: Some(MotionBody {
            version: Some(text::TEXT_FILTER_BODY_VERSION.to_owned()),
            params: Some(MotionParams::from_ids(params)),
            id: Some(id),
            display_name: Some(display_name.to_owned()),
            instance_name: Some(instance_name.to_owned()),
            bypass: None,
            intrinsic: None,
        }),
        premiere_filter_private_data: Some(
            MotionPrivateData {
                encoding: records::ENCODING,
                binary_hash: binary_hash.to_owned(),
                value: value.to_owned(),
            }
            .into(),
        ),
        sub_components: None,
        match_name: Some(match_name.to_owned()),
        video_filter_type: Some("2".to_owned()),
    })
}

/// A Text's Source Text: its document, and its keys in the form Premiere
/// 26.5.1 saves them (`IsTimeVarying` true; `ticks,base64;` per key, each a
/// complete document, no `BinaryHash`). Premiere renders the first key's
/// document before the first key and ignores a distinct `StartKeyframeValue`
/// (measured on the Source Text keys fixture), so the document written there
/// is the first key's, which `PrText::validate` makes the model's document.
fn source_text_param(
    object_id: ObjectId<MotionParamId>,
    node: Node<GraphicsProperties>,
    text: &PrText,
    hash: &str,
) -> crate::format::Result<Record> {
    let mut param = binary_param(
        object_id,
        node.into(),
        ("1", "Source Text", "9"),
        hash,
        &text_payload::encode(&text.document)?,
    );
    if !text.source_text_keys.is_empty() {
        let mut wire = String::new();
        for key in &text.source_text_keys {
            let payload = text_payload::encode(&key.document)?;
            wire.push_str(&format!(
                "{},{};",
                key.source_ticks,
                STANDARD.encode(payload)
            ));
        }
        param.is_time_varying = Some("true".to_owned());
        param.keyframes = Some(wire);
    }
    Ok(Record::ArbVideoComponentParam(param))
}

/// A static binary parameter of a graphic object, stored as Source Text is:
/// `(ParameterID, Name, ParameterControlType)`, its `BinaryHash` and payload,
/// and the Node that Premiere saves with it (a Text's, not a Shape's).
fn binary_param(
    object_id: ObjectId<MotionParamId>,
    node: RetainedOrSkipped<Node<GraphicsProperties>>,
    (parameter_id, name, control): (&str, &str, &str),
    hash: &str,
    payload: &[u8],
) -> ArbVideoComponentParam {
    ArbVideoComponentParam {
        object_id,
        class_id: Some(text::SOURCE_TEXT_PARAM.class_id.to_owned()),
        version: Some(text::SOURCE_TEXT_PARAM.version.to_owned()),
        node,
        name: Some(name.to_owned()),
        is_time_varying: None,
        parameter_control_type: Some(control.to_owned()),
        parameter_id: parameter_id.to_owned(),
        start_keyframe_position: Some(records::STATIC_KEYFRAME_TIME.to_owned()),
        start_keyframe_value: Some(EncodedValue {
            encoding: records::ENCODING.to_owned(),
            binary_hash: Some(hash.to_owned()),
            value: STANDARD.encode(payload),
        }),
        keyframes: None,
    }
}

/// The static value of one object parameter from the current transform.
fn text_value(transform: &PrTextTransform, spec: &GraphicParamSpec, frame: [f64; 2]) -> String {
    match spec.role {
        GraphicParamRole::Position => normalized(transform.position, frame),
        GraphicParamRole::Anchor => normalized(transform.anchor, frame),
        GraphicParamRole::Scale => transform.scale.to_string(),
        GraphicParamRole::Rotation => transform.rotation.to_string(),
        GraphicParamRole::Opacity => transform.opacity.to_string(),
        GraphicParamRole::HorizontalScale
        | GraphicParamRole::Uniform
        | GraphicParamRole::Selection
        | GraphicParamRole::Fixed => spec.initial.to_owned(),
    }
}

/// Positions and anchors are stored normalized to the sequence frame.
fn normalized(point: [f64; 2], frame: [f64; 2]) -> String {
    format!("{}:{}", point[0] / frame[0], point[1] / frame[1])
}

/// One graphic component parameter with its static value and, when the
/// parameter keys one of `animations`, those keys. Premiere 26.5.1 marks a
/// keyed graphic parameter `IsTimeVarying` and omits the flag otherwise.
fn param_record(
    spec: &GraphicParamSpec,
    object_id: ObjectId<MotionParamId>,
    value: String,
    animations: &[PrPropertyAnimation],
) -> crate::format::Result<Record> {
    let animation = spec.role.animation().and_then(|property| {
        animations
            .iter()
            .find(|animation| animation.property() == property)
    });
    if spec.is_point() {
        return Ok(Record::PointComponentParam(PointComponentParam {
            object_id,
            class_id: spec.record.class_id,
            version: spec.record.version,
            name: spec.name.expect("point parameters are named"),
            is_time_varying: animation.map(|_| "true"),
            parameter_control_type: spec.control,
            start_keyframe: point_start_keyframe(&value),
            keyframes: animation
                .and_then(PrPropertyAnimation::point_keys)
                .map(point_keyframes)
                .transpose()?,
            parameter_id: spec.id,
        }));
    }
    Ok(Record::VideoComponentParam(VideoComponentParam {
        object_id,
        class_id: Some(spec.record.class_id.to_owned()),
        version: Some(spec.record.version.to_owned()),
        name: spec.name.map(str::to_owned),
        is_time_varying: animation.map(|_| "true".to_owned()),
        discontinuous_interpolate: None,
        parameter_control_type: spec.control.map(str::to_owned),
        start_keyframe: scalar_start_keyframe(&value),
        keyframes: animation
            .and_then(PrPropertyAnimation::scalar_keys)
            .map(scalar_keyframes)
            .transpose()?,
        current_value: None,
        lower_bound: spec.lower.map(str::to_owned),
        upper_bound: spec.upper.map(str::to_owned),
        parameter_id: spec.id.to_string(),
        lower_ui_bound: None,
        upper_ui_bound: spec.upper_ui.map(str::to_owned),
        bypass: None,
    }))
}
