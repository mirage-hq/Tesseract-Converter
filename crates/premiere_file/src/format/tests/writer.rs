use crate::format::{
    inspect_project,
    writer::{project_xml, PremiereProjectXml},
    FormatError, FrameRate, Graph,
};
use crate::schema::{
    records::MediaPathField, text::PrGraphicObject, MediaId, PrGraphic, PrMedia, PrProjectFile,
    PrSequence, PrVideoItem, PrVideoOccurrence, PrVideoStream, PrVideoTrack, VideoCodec,
};
use crate::tests::support::{shape_graphic, text_graphic};
use std::path::Path;
use tempfile::tempdir;

mod after_effects;

const TICKS_PER_SECOND: i64 = crate::schema::TICKS;
const THIRTY_FPS_TICKS: i64 = FrameRate::Fps30.ticks_per_frame();

fn occurrence(path: &Path) -> PrVideoOccurrence {
    PrVideoOccurrence {
        id: None,
        media: MediaId(path.to_string_lossy().into_owned()),
        start_ticks: TICKS_PER_SECOND,
        end_ticks: 4 * TICKS_PER_SECOND,
        in_ticks: 2 * TICKS_PER_SECOND,
        out_ticks: 5 * TICKS_PER_SECOND,
        playback_rate: 1.0,
        frame_blending: None,
        opacity: 100.0,
        blend_mode: crate::schema::PrBlendMode::Normal,
        transform: Default::default(),
        crop: Default::default(),
        animations: Vec::new(),
        time_remap: None,
        linear_wipe: None,
        opacity_mask: None,
        track_matte: None,
        enabled: true,
        effects: Vec::new(),
        effects_above_mask: 0,
        stroke: None,
        active_transforms: 0,
        source_effects: None,
    }
}

fn video_stream(media: &mut PrMedia) -> &mut PrVideoStream {
    media.video.as_mut().expect("video media")
}

fn project(occurrences: Vec<PrVideoOccurrence>) -> PrProjectFile {
    let media = occurrences
        .iter()
        .map(|clip| {
            let path = std::path::PathBuf::from(clip.media.as_str());
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let relative = format!("./media/{name}");
            (
                clip.media.clone(),
                PrMedia {
                    name,
                    relative_path: Some(relative.clone()),
                    relative_paths: vec![relative],
                    absolute_paths: vec![
                        (MediaPathField::ActualMediaFilePath, path.clone()),
                        (MediaPathField::FilePath, path),
                    ],
                    video: Some(PrVideoStream {
                        pixel_aspect: Default::default(),
                        interpretation: Default::default(),
                        orientation: crate::schema::VideoOrientation::Identity,
                        kind: crate::schema::PrMediaKind::Video {
                            codec: Some(VideoCodec::H264),
                            hdr_profile: None,
                        },
                        intrinsic_ticks: 10 * TICKS_PER_SECOND,
                        frame_rate: (FrameRate::Fps30).into(),
                        width: 1920,
                        height: 1080,
                    }),
                    audio: None,
                },
            )
        })
        .collect();
    let timeline_end_ticks = occurrences
        .iter()
        .map(|clip| clip.end_ticks)
        .max()
        .unwrap_or(0);
    PrProjectFile::from_sequences(
        vec![PrSequence {
            native_frame_ticks: None,
            id: None,
            name: "Fresh & editable".into(),
            top_level: Some(true),
            video_tracks: vec![PrVideoTrack::media(occurrences)],
            audio: Vec::new(),
            frame_rate: FrameRate::Fps30,
            width: 1920,
            height: 1080,
            timeline_end_ticks,
        }],
        media,
    )
}

#[test]
fn writer_digest_covers_complete_gzip_without_changing_legacy_bytes() {
    use flate2::{read::GzDecoder, Compression, GzBuilder};
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};

    let root = tempdir().unwrap();
    let project = project(vec![occurrence(Path::new("/tmp/source.mp4"))]);
    let encoded = PremiereProjectXml::new(&project).unwrap();
    let path = root.path().join("project.prproj");
    let digest = encoded.write_new(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let expected: [u8; 32] = Sha256::digest(&bytes).into();
    assert_eq!(digest, expected);

    // Same serialized graph, pre-digest compression path; includes header/footer.
    let mut xml = Vec::new();
    GzDecoder::new(bytes.as_slice())
        .read_to_end(&mut xml)
        .unwrap();
    let mut legacy = GzBuilder::new()
        .mtime(0)
        .operating_system(10)
        .write(Vec::new(), Compression::new(6));
    legacy.write_all(&xml).unwrap();
    assert_eq!(bytes, legacy.finish().unwrap());
    assert!(encoded.write_new(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn animated_nonuniform_scale_is_rejected_before_writing() {
    let mut clip = occurrence(Path::new("/media/source.mp4"));
    clip.transform.scale = [1.0, 2.0];
    clip.animations
        .push(crate::schema::PrPropertyAnimation::UniformScale(vec![
            crate::schema::PrScalarKeyframe {
                source_ticks: 2 * TICKS_PER_SECOND,
                value: 1.0,
                easing: crate::schema::PrKeyframeEasing::Linear,
            },
        ]));
    let error = project_xml(&project(vec![clip])).unwrap_err();
    assert!(
        error.to_string().contains("animated nonuniform Scale"),
        "{error}"
    );
}

#[test]
fn written_media_uses_its_own_frame_rate_and_exact_duration() {
    // The 23.976 fps source keeps its own rate in a sequence of each rate. The
    // sequence writes its frame ticks and the display code Premiere saves for it.
    for (sequence_rate, sequence_ticks, display_format) in [
        (FrameRate::Fps30, "8467200000", "104"),
        (FrameRate::Fps50, "5080320000", "105"),
        (FrameRate::Fps60, "4233600000", "108"),
    ] {
        let clip = occurrence(Path::new("/media/source.mp4"));
        let mut project = project(vec![clip.clone()]);
        project.sequences[0].frame_rate = sequence_rate;
        let source = video_stream(project.media.get_mut(&clip.media).unwrap());
        source.frame_rate = FrameRate::Fps24000Over1001.into();
        source.intrinsic_ticks = 240 * 10_594_584_000;
        let xml = project_xml(&project).unwrap();
        let document = roxmltree::Document::parse(&xml).unwrap();
        let stream = document
            .descendants()
            .find(|node| node.has_tag_name("VideoStream"))
            .unwrap();
        assert_eq!(xml_at(stream, "FrameRate").text(), Some("10594584000"));
        assert_eq!(xml_at(stream, "Duration").text(), Some("2542700160000"));
        let group_rates: Vec<_> = document
            .descendants()
            .filter(|node| {
                node.has_tag_name("TrackGroup")
                    && node.parent().is_some_and(|parent| {
                        parent.has_tag_name("VideoTrackGroup")
                            || parent.has_tag_name("DataTrackGroup")
                    })
            })
            .map(|node| xml_at(node, "FrameRate").text())
            .collect();
        assert_eq!(group_rates, [Some(sequence_ticks); 2]);
        let display_formats: Vec<_> = document
            .descendants()
            .filter(|node| node.has_tag_name("MZ.Sequence.VideoTimeDisplayFormat"))
            .map(|node| node.text())
            .collect();
        assert_eq!(display_formats, [Some(display_format)]);
        assert_eq!(
            document
                .descendants()
                .find(|node| node.has_tag_name("MediaFrameRate"))
                .unwrap()
                .text(),
            Some("10594584000")
        );
        let reloaded = crate::format::inspect_project_with_media(&xml, None).unwrap();
        let sequence = reloaded.sequences().next().unwrap();
        assert_eq!(sequence.frame_rate, sequence_rate);
        let occurrence = sequence.video_occurrences().next().unwrap();
        let video = reloaded.media(occurrence).unwrap().video.as_ref().unwrap();
        assert_eq!(video.frame_rate, FrameRate::Fps24000Over1001.into());
    }
}

/// A video master's `VideoStream` carries the code Premiere saves for its
/// codec family and, for an alpha master, readable straight alpha: the
/// corpus `ap4h` masters (Podcast Opener) save `CodecType` 1634743400 and
/// `AlphaType` 1 without `IgnoreAlpha`; an opaque master keeps `AlphaType` 3
/// and `IgnoreAlpha` true.
#[test]
fn written_video_masters_carry_their_codec_code_and_alpha_declaration() {
    use crate::schema::video_codec::ProResProfile;
    let child = |stream: roxmltree::Node<'_, '_>, tag: &str| {
        stream
            .children()
            .find(|node| node.has_tag_name(tag))
            .and_then(|node| node.text())
            .map(str::to_owned)
    };
    for (codec, codec_type, alpha_type, ignore_alpha) in [
        (VideoCodec::H264, "1635148593", "3", Some("true")),
        (VideoCodec::HevcMain, "1212503619", "3", Some("true")),
        (
            VideoCodec::ProRes {
                profile: ProResProfile::Hq,
                alpha: false,
            },
            "1634755432",
            "3",
            Some("true"),
        ),
        (
            VideoCodec::ProRes {
                profile: ProResProfile::P4444,
                alpha: true,
            },
            "1634743400",
            "1",
            None,
        ),
    ] {
        let clip = occurrence(Path::new("/media/source.mov"));
        let mut project = project(vec![clip.clone()]);
        video_stream(project.media.get_mut(&clip.media).unwrap()).kind =
            crate::schema::PrMediaKind::Video {
                codec: Some(codec),
                hdr_profile: None,
            };
        let xml = project_xml(&project).unwrap();
        let document = roxmltree::Document::parse(&xml).unwrap();
        let stream = document
            .descendants()
            .find(|node| node.has_tag_name("VideoStream"))
            .unwrap();
        assert_eq!(
            child(stream, "CodecType").as_deref(),
            Some(codec_type),
            "{codec:?}"
        );
        assert_eq!(
            child(stream, "AlphaType").as_deref(),
            Some(alpha_type),
            "{codec:?}"
        );
        assert_eq!(
            child(stream, "IgnoreAlpha").as_deref(),
            ignore_alpha,
            "{codec:?}"
        );
        // The reader takes the codec from the file, not the record, so the
        // written project reads back as a video master of the same frame.
        let reloaded = crate::format::inspect_project_with_media(&xml, None).unwrap();
        let sequence = reloaded.sequences().next().unwrap();
        let occurrence = sequence.video_occurrences().next().unwrap();
        let video = reloaded.media(occurrence).unwrap().video.as_ref().unwrap();
        assert_eq!((video.width, video.height), (1920, 1080));
    }
}

#[test]
fn fresh_graph_has_new_ids_and_exact_semantics() {
    let path = Path::new("/tmp/fresh package/media/source.mp4");
    let mut project = project(vec![occurrence(path)]);
    project.sequences[0].width = 192;
    project.sequences[0].height = 108;
    let media = video_stream(project.media.values_mut().next().unwrap());
    media.width = 192;
    media.height = 108;
    let first = project_xml(&project).unwrap();
    let second = project_xml(&project).unwrap();
    assert_ne!(first, second);
    Graph::parse(&first).unwrap();
    assert!(first.contains("<Name>Fresh &amp; editable</Name>"));
    assert!(first.contains("<FrameRect>0,0,192,108</FrameRect>"));
    assert!(first.contains("<Start>254016000000</Start>"));
    assert!(first.contains("<End>1016064000000</End>"));
    assert!(first.contains("<InPoint>508032000000</InPoint>"));
    assert!(first.contains("<OutPoint>1270080000000</OutPoint>"));
    assert!(first.contains("<RelativePath>./media/source.mp4</RelativePath>"));
    assert!(first.contains(r#"<OriginalColorSpace>{"baseColorProfile":{"colorProfileData":"AQAAAP////8=","colorProfileName":"BT.709,32f,Display-Referred"},"baseProfileType":1}</OriginalColorSpace>"#));
    assert!(first.contains(r#"<OutputColorSpace>{"baseColorProfile":{"colorProfileName":"BT.709 RGB Full"},"baseProfileType":1}</OutputColorSpace>"#));
    assert!(first.contains(r#"<ToneMapSettings>{"peak":-1,"version":3}</ToneMapSettings>"#));
    assert_eq!(first.matches("<Sequence ObjectUID=").count(), 1);
    assert!(!first.contains("premiere-eval-corpus"));
    assert!(!first.contains("feed0000-"));
}

#[test]
fn writer_emits_native_speed_and_reverse_without_changing_source_bounds() {
    let mut clip = occurrence(Path::new("/tmp/media/source.mp4"));
    clip.out_ticks = 7 * TICKS_PER_SECOND / 2;
    clip.playback_rate = -0.5;

    let xml = project_xml(&project(vec![clip])).unwrap();
    assert!(xml.contains("<PlaybackSpeed>0.5</PlaybackSpeed>"));
    assert!(xml.contains("<PlayBackwards>true</PlayBackwards>"));
    assert!(xml.contains("<InPoint>508032000000</InPoint>"));
    assert!(xml.contains("<OutPoint>889056000000</OutPoint>"));

    let read = crate::format::inspect_project(&xml, None).unwrap();
    let imported = read.video_occurrences().next().unwrap();
    assert_eq!(imported.playback_rate, -0.5);
    assert_eq!(
        imported.source_ticks(),
        2 * TICKS_PER_SECOND..7 * TICKS_PER_SECOND / 2
    );
}

#[test]
fn writer_preserves_known_time_interpolation_modes() {
    let path = Path::new("/tmp/frame blending/media/source.mp4");
    for (mode, native) in [
        (fx_schema::FrameBlendingMode::Simple, "1"),
        (fx_schema::FrameBlendingMode::OpticalFlow, "2"),
    ] {
        let mut clip = occurrence(path);
        clip.frame_blending = Some(mode);
        let xml = project_xml(&project(vec![clip])).unwrap();
        assert_eq!(
            xml.matches(&format!(
                "<TimeInterpolationType>{native}</TimeInterpolationType>"
            ))
            .count(),
            1
        );
        let imported = inspect_project(&xml, None).unwrap();
        assert_eq!(
            imported.video_occurrences().next().unwrap().frame_blending,
            Some(mode)
        );
    }

    let xml = project_xml(&project(vec![occurrence(path)])).unwrap();
    assert!(!xml.contains("<TimeInterpolationType>"));
}

#[test]
fn static_motion_writer_round_trip_preserves_editable_values() {
    let path = Path::new("/tmp/media/source.mp4");
    let mut clip = occurrence(path);
    clip.transform.position = [0.75, 0.25];
    clip.transform.anchor_point = [0.125, 0.75];
    clip.transform.scale = [80.0, 125.0];
    clip.transform.rotation = -15.0;
    let xml = project_xml(&project(vec![clip])).unwrap();
    assert!(xml.contains("<Name>Scale Height</Name>"));
    assert!(xml.contains("-91445760000000000,false,0,0,0,0,0,0"));

    let imported = inspect_project(&xml, None).unwrap();
    let transform = imported.video_occurrences().next().unwrap().transform;
    assert_eq!(transform.position, [0.75, 0.25]);
    assert_eq!(transform.anchor_point, [0.125, 0.75]);
    assert_eq!(transform.scale, [80.0, 125.0]);
    assert_eq!(transform.rotation, -15.0);
}

#[test]
fn writer_round_trips_position_timing_and_explicit_spatial_tangents() {
    use crate::schema::{PrKeyframeEasing, PrPointKeyframe, PrPropertyAnimation};

    let mut clip = occurrence(Path::new("/tmp/media/source.mp4"));
    clip.animations.push(PrPropertyAnimation::Position(vec![
        PrPointKeyframe {
            source_ticks: 2 * TICKS_PER_SECOND,
            value: [0.5, 0.5],
            easing: PrKeyframeEasing::Linear,
            spatial_in_tangent: Some([0.0, 0.0]),
            spatial_out_tangent: Some([0.1, -0.05]),
        },
        PrPointKeyframe {
            source_ticks: 3 * TICKS_PER_SECOND,
            value: [0.75, 0.75],
            easing: PrKeyframeEasing::CubicBezier {
                x1: 0.3,
                y1: 0.2,
                x2: 0.7,
                y2: 0.8,
            },
            spatial_in_tangent: Some([-0.1, 0.05]),
            spatial_out_tangent: Some([0.0, 0.0]),
        },
    ]));
    let xml = project_xml(&project(vec![clip])).unwrap();
    assert!(xml.contains("<Name>Position</Name>"));
    assert!(xml.contains(",5,0,0,0,0.1,-0.05;"));
    assert!(xml.contains(",5,0,-0.1,0.05,0,0;"));

    let imported = inspect_project(&xml, None).unwrap();
    let position = imported.video_occurrences().next().unwrap().animations[0]
        .point_keys()
        .unwrap();
    assert_eq!(position.len(), 2);
    assert_eq!(position[0].source_ticks, 2 * TICKS_PER_SECOND);
    assert_eq!(position[1].source_ticks, 3 * TICKS_PER_SECOND);
    assert_eq!(position[0].value, [0.5, 0.5]);
    assert_eq!(position[1].value, [0.75, 0.75]);
    assert_eq!(position[0].spatial_out_tangent, Some([0.1, -0.05]));
    assert_eq!(position[1].spatial_in_tangent, Some([-0.1, 0.05]));
    let PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = position[1].easing else {
        panic!("expected cubic temporal easing");
    };
    assert!((x1 - 0.3).abs() < 1e-12);
    assert!((y1 - 0.2).abs() < 1e-12);
    assert!((x2 - 0.7).abs() < 1e-12);
    assert!((y2 - 0.8).abs() < 1e-12);
}

#[test]
fn writer_reader_inverse_preserves_closed_curved_position_timing() {
    use crate::schema::{PrKeyframeEasing, PrPointKeyframe, PrPropertyAnimation};

    let expected = PrKeyframeEasing::CubicBezier {
        x1: 0.25,
        y1: 0.4,
        x2: 0.75,
        y2: 0.6,
    };
    let mut clip = occurrence(Path::new("/tmp/media/source.mp4"));
    clip.animations.push(PrPropertyAnimation::Position(vec![
        PrPointKeyframe {
            source_ticks: 2 * TICKS_PER_SECOND,
            value: [0.5, 0.5],
            easing: PrKeyframeEasing::Linear,
            spatial_in_tangent: None,
            spatial_out_tangent: Some([0.25, -0.2]),
        },
        PrPointKeyframe {
            source_ticks: 3 * TICKS_PER_SECOND,
            value: [0.5, 0.5],
            easing: expected,
            spatial_in_tangent: Some([-0.25, -0.2]),
            spatial_out_tangent: None,
        },
    ]));

    let xml = project_xml(&project(vec![clip])).unwrap();
    let imported = inspect_project(&xml, None).unwrap();
    let keys = imported.video_occurrences().next().unwrap().animations[0]
        .point_keys()
        .unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].value, keys[1].value);
    let PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = keys[1].easing else {
        panic!("expected cubic temporal easing");
    };
    assert!((x1 - 0.25).abs() < 1e-12);
    assert!((y1 - 0.4).abs() < 1e-12);
    assert!((x2 - 0.75).abs() < 1e-12);
    assert!((y2 - 0.6).abs() < 1e-12);
}

#[test]
fn writer_preserves_hold_linear_and_off_trim_rotation_keys() {
    use crate::schema::{PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe};
    let mut clip = occurrence(Path::new("/tmp/media/source.mp4"));
    let keys = vec![
        PrScalarKeyframe {
            source_ticks: TICKS_PER_SECOND,
            value: 0.0,
            easing: PrKeyframeEasing::Linear,
        },
        PrScalarKeyframe {
            source_ticks: 3 * TICKS_PER_SECOND,
            value: 30.0,
            easing: PrKeyframeEasing::Hold,
        },
        PrScalarKeyframe {
            source_ticks: 7 * TICKS_PER_SECOND,
            value: 60.0,
            easing: PrKeyframeEasing::Linear,
        },
    ];
    clip.animations
        .push(PrPropertyAnimation::Rotation(keys.clone()));
    let xml = project_xml(&project(vec![clip])).unwrap();
    assert!(xml.contains("<VideoFilterComponent "));
    assert!(xml.contains(",0,0,0,0,0,0;"));
    assert!(xml.contains(",4,0,0,0,0,0;"));
    let read = crate::format::inspect_project(&xml, None).unwrap();
    let imported = &read.video_occurrences().next().unwrap().animations;
    assert_eq!(imported.len(), 1);
    assert_eq!(imported[0].keys().len(), keys.len());
    for (actual, expected) in imported[0].keys().iter().zip(keys.iter()) {
        assert_eq!(actual.source_ticks, expected.source_ticks);
        assert_eq!(actual.value, expected.value);
        assert_eq!(actual.easing, expected.easing);
    }
}

#[test]
fn writer_round_trips_both_bezier_handles_with_negative_and_extreme_source_ticks() {
    use crate::schema::{PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe};
    for (start_ticks, end_ticks, start_value, end_value) in [
        (0, TICKS_PER_SECOND, 0.0, 90.0),
        (0, TICKS_PER_SECOND, 180.0, 90.0),
        (i64::MIN, i64::MAX, 0.0, 90.0),
    ] {
        let mut clip = occurrence(Path::new("/tmp/media/source.mp4"));
        let keys = vec![
            PrScalarKeyframe {
                source_ticks: start_ticks,
                value: start_value,
                easing: PrKeyframeEasing::Linear,
            },
            PrScalarKeyframe {
                source_ticks: end_ticks,
                value: end_value,
                easing: PrKeyframeEasing::CubicBezier {
                    x1: 0.4,
                    y1: 0.2,
                    x2: 0.8,
                    y2: 0.85,
                },
            },
        ];
        clip.animations.push(PrPropertyAnimation::Rotation(keys));
        let xml = project_xml(&project(vec![clip])).unwrap();
        let imported = crate::format::inspect_project(&xml, None).unwrap();
        let actual = imported.video_occurrences().next().unwrap().animations[0].keys();
        assert_eq!(
            (actual[0].source_ticks, actual[1].source_ticks),
            (start_ticks, end_ticks)
        );
        assert_eq!((actual[0].value, actual[1].value), (start_value, end_value));
        let PrKeyframeEasing::CubicBezier { x1, y1, x2, y2 } = actual[1].easing else {
            panic!("expected cubic on re-import");
        };
        for (actual, expected) in [(x1, 0.4), (y1, 0.2), (x2, 0.8), (y2, 0.85)] {
            assert!((actual - expected).abs() < 1e-10, "{actual} != {expected}");
        }
    }
}

#[test]
fn writer_rejects_nonfinite_native_bezier_velocities_before_serializing() {
    use crate::schema::{PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe};
    for (y1, y2) in [(f64::MAX, 0.5), (0.5, -f64::MAX)] {
        let mut clip = occurrence(Path::new("/tmp/media/source.mp4"));
        clip.animations.push(PrPropertyAnimation::Rotation(vec![
            PrScalarKeyframe {
                source_ticks: 0,
                value: 0.0,
                easing: PrKeyframeEasing::Linear,
            },
            PrScalarKeyframe {
                source_ticks: TICKS_PER_SECOND,
                value: 90.0,
                easing: PrKeyframeEasing::CubicBezier {
                    x1: 0.5,
                    y1,
                    x2: 0.5,
                    y2,
                },
            },
        ]));
        let error = project_xml(&project(vec![clip])).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("nonfinite native Bezier velocity"),
            "{error}"
        );
    }

    use crate::schema::PrPointKeyframe;
    let mut clip = occurrence(Path::new("/tmp/media/source.mp4"));
    clip.animations.push(PrPropertyAnimation::Position(vec![
        PrPointKeyframe {
            source_ticks: 2 * TICKS_PER_SECOND,
            value: [f64::MAX, 0.0],
            easing: PrKeyframeEasing::Linear,
            spatial_in_tangent: None,
            spatial_out_tangent: None,
        },
        PrPointKeyframe {
            source_ticks: 3 * TICKS_PER_SECOND,
            value: [-f64::MAX, 0.0],
            easing: PrKeyframeEasing::CubicBezier {
                x1: 0.25,
                y1: 0.4,
                x2: 0.75,
                y2: 0.6,
            },
            spatial_in_tangent: None,
            spatial_out_tangent: None,
        },
    ]));
    let error = project_xml(&project(vec![clip])).unwrap_err();
    assert!(
        error.to_string().contains("nonfinite spatial curve length"),
        "{error}"
    );
}

#[test]
fn edited_uniform_scale_and_rotation_export_independently() {
    use crate::schema::{
        PrAnimatedProperty, PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe,
    };
    let mut clip = occurrence(Path::new("/tmp/media/source.mp4"));
    for (property, initial, later) in [
        (PrAnimatedProperty::UniformScale, 100.0, 150.0),
        (PrAnimatedProperty::Rotation, 0.0, 30.0),
    ] {
        let keys = vec![
            PrScalarKeyframe {
                source_ticks: 2 * TICKS_PER_SECOND,
                value: initial,
                easing: PrKeyframeEasing::Linear,
            },
            PrScalarKeyframe {
                source_ticks: 3 * TICKS_PER_SECOND,
                value: later,
                easing: PrKeyframeEasing::Hold,
            },
        ];
        clip.animations.push(match property {
            PrAnimatedProperty::Opacity => PrPropertyAnimation::Opacity(keys),
            PrAnimatedProperty::UniformScale => PrPropertyAnimation::UniformScale(keys),
            PrAnimatedProperty::Rotation => PrPropertyAnimation::Rotation(keys),
            PrAnimatedProperty::Position
            | PrAnimatedProperty::AnchorPoint
            | PrAnimatedProperty::ScaleWidth => {
                panic!("test only constructs uniform Scale and Rotation")
            }
        });
    }
    let native = project(vec![clip]);
    let imported =
        crate::format::inspect_project_with_media(&project_xml(&native).unwrap(), None).unwrap();
    let mut document = crate::tests::support::project_document_with_media(
        imported.single_sequence().unwrap(),
        &imported.media,
    );
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap();
    assert_eq!(entries.len(), 3);
    for entry in entries {
        let property = entry["target"]["propertyType"].as_str().unwrap();
        if matches!(property, "scaleX" | "scaleY") {
            entry["animator"]["keyframes"][1]["value"]["value"] = serde_json::json!(175.0);
        }
    }
    let editable = fx_schema::EditableFxCompositionDocument::from_json_value(document).unwrap();
    let original_media = native.media.values().next().unwrap();
    let original_video = original_media.video.as_ref().unwrap();
    // The native record describes the source file in this in-memory round trip.
    let facts = std::collections::BTreeMap::from([(
        "premiere-video-1".to_owned(),
        crate::media::MediaFacts::Video(crate::media::VideoMedia {
            pixel_aspect: Default::default(),
            orientation: crate::schema::VideoOrientation::Identity,
            codec: VideoCodec::H264,
            bit_depth: 8,
            colour: None,
            width: original_video.width,
            height: original_video.height,
            timing: crate::media::VideoTiming::for_test(
                original_video.frame_rate.supported().unwrap(),
                original_video.intrinsic_ticks,
            ),
        }),
    )]);
    let mut omissions = Vec::new();
    let mut converted = crate::convert::tesseract_to_premiere(
        &editable,
        &facts,
        &Default::default(),
        &Default::default(),
        crate::format::FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    for media in converted.media.values_mut() {
        *media = original_media.clone();
    }
    let reopened = inspect_project(&project_xml(&converted).unwrap(), None).unwrap();
    let clip = reopened.video_occurrences().next().unwrap();
    assert_eq!(clip.animations.len(), 2);
    let scale = clip
        .animations
        .iter()
        .find(|animation| animation.property() == PrAnimatedProperty::UniformScale)
        .unwrap();
    let rotation = clip
        .animations
        .iter()
        .find(|animation| animation.property() == PrAnimatedProperty::Rotation)
        .unwrap();
    assert_eq!(
        scale.keys().iter().map(|key| key.value).collect::<Vec<_>>(),
        [100.0, 175.0]
    );
    assert_eq!(scale.keys()[1].easing, PrKeyframeEasing::Hold);
    assert_eq!(
        rotation
            .keys()
            .iter()
            .map(|key| key.value)
            .collect::<Vec<_>>(),
        [0.0, 30.0]
    );
}

#[test]
fn duplicate_motion_property_cannot_be_silently_overwritten() {
    use crate::schema::{PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe};
    let mut clip = occurrence(Path::new("/tmp/media/source.mp4"));
    let keys = vec![PrScalarKeyframe {
        source_ticks: 2 * TICKS_PER_SECOND,
        value: 0.0,
        easing: PrKeyframeEasing::Linear,
    }];
    clip.animations = vec![
        PrPropertyAnimation::Rotation(keys.clone()),
        PrPropertyAnimation::Rotation(keys),
    ];
    assert!(project_xml(&project(vec![clip]))
        .unwrap_err()
        .to_string()
        .contains("duplicate Rotation animation"));
}

#[test]
fn import_edit_export_and_reimport_use_the_edited_key() {
    use crate::schema::{PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe};
    let mut native = project(vec![occurrence(Path::new("/tmp/media/source.mp4"))]);
    native.sequences[0].video_tracks[0]
        .clip_mut(0)
        .animations
        .push(PrPropertyAnimation::Rotation(vec![
            PrScalarKeyframe {
                source_ticks: TICKS_PER_SECOND,
                value: 0.0,
                easing: PrKeyframeEasing::Linear,
            },
            PrScalarKeyframe {
                source_ticks: 3 * TICKS_PER_SECOND,
                value: 30.0,
                easing: PrKeyframeEasing::Hold,
            },
        ]));
    let imported =
        crate::format::inspect_project_with_media(&project_xml(&native).unwrap(), None).unwrap();
    let mut document = crate::tests::support::project_document_with_media(
        imported.single_sequence().unwrap(),
        &imported.media,
    );
    document["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"][1]["value"]
        ["value"] = serde_json::json!(75.0);
    let editable = fx_schema::EditableFxCompositionDocument::from_json_value(document).unwrap();
    let original_media = native.media.values().next().unwrap();
    let original_video = original_media.video.as_ref().unwrap();
    // The native record describes the source file in this in-memory round trip.
    let facts = std::collections::BTreeMap::from([(
        "premiere-video-1".to_owned(),
        crate::media::MediaFacts::Video(crate::media::VideoMedia {
            pixel_aspect: Default::default(),
            orientation: crate::schema::VideoOrientation::Identity,
            codec: VideoCodec::H264,
            bit_depth: 8,
            colour: None,
            width: original_video.width,
            height: original_video.height,
            timing: crate::media::VideoTiming::for_test(
                original_video.frame_rate.supported().unwrap(),
                original_video.intrinsic_ticks,
            ),
        }),
    )]);
    let mut omissions = Vec::new();
    let mut converted = crate::convert::tesseract_to_premiere(
        &editable,
        &facts,
        &Default::default(),
        &Default::default(),
        crate::format::FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    for media in converted.media.values_mut() {
        *media = original_media.clone();
    }
    let exported_xml = project_xml(&converted).unwrap();
    let reopened = inspect_project(&exported_xml, None).unwrap();
    let keys = reopened.video_occurrences().next().unwrap().animations[0].keys();
    assert_eq!(
        keys.iter()
            .map(|key| (key.source_ticks, key.value))
            .collect::<Vec<_>>(),
        [(TICKS_PER_SECOND, 0.0), (3 * TICKS_PER_SECOND, 75.0)]
    );
    assert_eq!(keys[1].easing, PrKeyframeEasing::Hold);
}

#[test]
fn independent_occurrences_do_not_share_motion_keyframes() {
    use crate::schema::{PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe};
    let path = Path::new("/tmp/media/source.mp4");
    let mut first = occurrence(path);
    let mut second = occurrence(path);
    second.start_ticks = 5 * TICKS_PER_SECOND;
    second.end_ticks = 8 * TICKS_PER_SECOND;
    for (clip, value) in [(&mut first, 10.0), (&mut second, 75.0)] {
        clip.animations
            .push(PrPropertyAnimation::Rotation(vec![PrScalarKeyframe {
                source_ticks: 0,
                value,
                easing: PrKeyframeEasing::Linear,
            }]));
    }
    let xml = project_xml(&project(vec![first, second])).unwrap();
    let parsed = crate::format::inspect_project(&xml, None).unwrap();
    assert_eq!(
        parsed
            .video_occurrences()
            .map(|clip| clip.animations[0].keys()[0].value)
            .collect::<Vec<_>>(),
        [10.0, 75.0]
    );
}

#[test]
fn opacity_animation_and_normal_blend_round_trip_through_native_records() {
    use crate::schema::{
        PrAnimatedProperty, PrBlendMode, PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe,
    };

    let mut clip = occurrence(Path::new("/tmp/media/source.mp4"));
    clip.opacity = 50.0;
    clip.blend_mode = PrBlendMode::Normal;
    clip.animations.push(PrPropertyAnimation::Opacity(vec![
        PrScalarKeyframe {
            source_ticks: 2 * TICKS_PER_SECOND,
            value: 50.0,
            easing: PrKeyframeEasing::Linear,
        },
        PrScalarKeyframe {
            source_ticks: 3 * TICKS_PER_SECOND,
            value: 25.0,
            easing: PrKeyframeEasing::Hold,
        },
    ]));

    let native = project(vec![clip]);
    let imported =
        crate::format::inspect_project_with_media(&project_xml(&native).unwrap(), None).unwrap();
    let actual = imported
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!(actual.opacity, 50.0);
    assert_eq!(actual.blend_mode, PrBlendMode::Normal);
    let animation = actual
        .animations
        .iter()
        .find(|animation| animation.property() == PrAnimatedProperty::Opacity)
        .unwrap();
    assert_eq!(animation.keys().len(), 2);
    assert_eq!(animation.keys()[0].source_ticks, 2 * TICKS_PER_SECOND);
    assert_eq!(animation.keys()[0].value, 50.0);
    assert_eq!(animation.keys()[0].easing, PrKeyframeEasing::Linear);
    assert_eq!(animation.keys()[1].source_ticks, 3 * TICKS_PER_SECOND);
    assert_eq!(animation.keys()[1].value, 25.0);
    assert_eq!(animation.keys()[1].easing, PrKeyframeEasing::Hold);
}

#[test]
fn nondefault_opacity_writes_the_normal_blend_pair_18_0() {
    let mut clip = occurrence(Path::new("/tmp/media/source.mp4"));
    clip.opacity = 50.0;
    let xml = project_xml(&project(vec![clip])).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let text = |param: roxmltree::Node, tag: &str| {
        param
            .children()
            .find(|child| child.has_tag_name(tag))
            .and_then(|child| child.text())
            .unwrap()
            .to_owned()
    };
    let blend: Vec<_> = document
        .descendants()
        .filter(|node| node.has_tag_name("VideoComponentParam"))
        .filter(|param| text(*param, "Name") == "Blend Mode")
        .map(|param| (text(param, "ParameterID"), text(param, "StartKeyframe")))
        .collect();
    assert_eq!(
        blend,
        [
            ("2".into(), "-91445760000000000,18,0,0,0,0,0,0".into()),
            ("3".into(), "-91445760000000000,0,0,0,0,0,0,0".into()),
        ]
    );
}

#[test]
fn written_project_preserves_model_semantics() {
    let first = Path::new("/tmp/media/a & <one>\r.mov");
    let second = Path::new("/tmp/media/second.mp4");
    let cases = [
        vec![occurrence(first)],
        vec![
            PrVideoOccurrence {
                start_ticks: 0,
                end_ticks: TICKS_PER_SECOND,
                in_ticks: 0,
                out_ticks: TICKS_PER_SECOND,
                ..occurrence(first)
            },
            PrVideoOccurrence {
                start_ticks: 2 * TICKS_PER_SECOND,
                end_ticks: 3 * TICKS_PER_SECOND,
                in_ticks: 4 * TICKS_PER_SECOND,
                out_ticks: 5 * TICKS_PER_SECOND,
                ..occurrence(second)
            },
            PrVideoOccurrence {
                start_ticks: 3 * TICKS_PER_SECOND,
                end_ticks: 4 * TICKS_PER_SECOND,
                in_ticks: 2 * TICKS_PER_SECOND,
                out_ticks: 3 * TICKS_PER_SECOND,
                ..occurrence(first)
            },
        ],
    ];
    for occurrences in cases {
        let project = project(occurrences);
        let directory = tempdir().unwrap();
        let path = directory.path().join("project.prproj");
        PremiereProjectXml::new(&project)
            .unwrap()
            .write_new(&path)
            .unwrap();
        let (loaded, omissions) = PrProjectFile::load(&path).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let expected = project.single_sequence().unwrap();
        let actual = loaded.single_sequence().unwrap();
        assert_eq!(actual.name, expected.name);
        assert_eq!(actual.dimensions(), expected.dimensions());
        assert_eq!(actual.frame_rate, expected.frame_rate);
        assert_eq!(
            actual.video_tracks().map(<[_]>::len).collect::<Vec<_>>(),
            expected.video_tracks().map(<[_]>::len).collect::<Vec<_>>()
        );
        for (actual, expected) in actual.video_occurrences().zip(expected.video_occurrences()) {
            assert_eq!(actual.timeline_ticks(), expected.timeline_ticks());
            assert_eq!(actual.source_ticks(), expected.source_ticks());
            let actual = loaded.media(actual).unwrap();
            let expected = project.media(expected).unwrap();
            let actual_video = actual.video.as_ref().unwrap();
            let expected_video = expected.video.as_ref().unwrap();
            assert_eq!(actual_video.intrinsic_ticks, expected_video.intrinsic_ticks);
            assert_eq!(actual_video.frame_rate, expected_video.frame_rate);
            assert_eq!(actual_video.width, expected_video.width);
            assert_eq!(actual_video.height, expected_video.height);
            assert_eq!(actual.name, expected.name);
            assert_eq!(actual.relative_path, expected.relative_path);
            assert_eq!(actual.relative_paths, expected.relative_paths);
            assert_eq!(actual.absolute_paths, expected.absolute_paths);
        }
    }
}

#[test]
fn missing_media_paths_return_errors_before_encoding() {
    let mut project = crate::format::inspect_project_with_media(
        include_str!("../../../tests/fixtures/one-clip.xml"),
        None,
    )
    .unwrap();
    assert!(PremiereProjectXml::new(&project)
        .unwrap_err()
        .to_string()
        .contains("absolute media path"));

    project.media.values_mut().next().unwrap().relative_path = None;
    assert!(PremiereProjectXml::new(&project)
        .unwrap_err()
        .to_string()
        .contains("relative media path"));
}

#[test]
fn carriage_returns_survive_xml_text_normalization() {
    let path = Path::new("/tmp/carriage\rreturn/media/source.mp4");
    let xml = project_xml(&project(vec![occurrence(path)])).unwrap();
    assert!(xml.contains("carriage&#13;return"));
    Graph::parse(&xml).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let media = document
        .root_element()
        .children()
        .find(|node| node.has_tag_name("Media"))
        .unwrap();
    for field in ["FilePath", "ActualMediaFilePath"] {
        assert_eq!(xml_at(media, field).text().unwrap(), path.to_str().unwrap());
    }
}

#[test]
fn oversized_media_text_rejects_before_writing() {
    let dir = tempdir().unwrap();
    let output = dir.path().join("oversized.prproj");
    let long_name = format!("{}.mp4", "é".repeat(126));
    let absolute = format!("/tmp/{long_name}");
    let oversized = project(vec![occurrence(Path::new(&absolute))]);
    assert!(PremiereProjectXml::new(&oversized)
        .and_then(|xml| xml.write_new(&output))
        .unwrap_err()
        .to_string()
        .contains("255 bytes"));
    assert!(!output.exists());

    let long_path = format!("/{}/source.mp4", "a".repeat(4096));
    let oversized = project(vec![occurrence(Path::new(&long_path))]);
    assert!(PremiereProjectXml::new(&oversized)
        .and_then(|xml| xml.write_new(&output))
        .unwrap_err()
        .to_string()
        .contains("4096 bytes"));
    assert!(!output.exists());
}

#[test]
fn media_name_is_preserved_inside_new_paths() {
    let path = Path::new("/tmp/fresh package/media/clip-a.mp4");
    let xml = project_xml(&project(vec![occurrence(path)])).unwrap();
    Graph::parse(&xml).unwrap();
    assert!(xml.contains("<RelativePath>./media/clip-a.mp4</RelativePath>"));
    assert!(xml.contains("<ActualMediaFilePath>/tmp/fresh package/media/clip-a.mp4"));
}

#[test]
fn rejects_xml_invalid_names_and_paths_before_serialization() {
    let path = Path::new("/tmp/media/source.mp4");
    let mut invalid = project(vec![occurrence(path)]);
    invalid.sequences[0].name = "invalid\u{1}name".into();
    assert!(project_xml(&invalid)
        .unwrap_err()
        .to_string()
        .contains("XML"));
    let invalid = project(vec![occurrence(Path::new(
        "/tmp/invalid\u{0}path/source.mp4",
    ))]);
    assert!(project_xml(&invalid)
        .unwrap_err()
        .to_string()
        .contains("XML"));
    let invalid = project(vec![occurrence(Path::new(
        "/tmp/media/invalid\u{ffff}.mp4",
    ))]);
    assert!(project_xml(&invalid).is_err());
    #[cfg(unix)]
    {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt, path::PathBuf};
        let invalid_utf8 = PathBuf::from(OsString::from_vec(b"/tmp/\xff/source.mp4".to_vec()));
        let mut invalid = project(vec![occurrence(&invalid_utf8)]);
        invalid.media.values_mut().next().unwrap().absolute_paths[0].1 = invalid_utf8.clone();
        invalid.media.values_mut().next().unwrap().absolute_paths[1].1 = invalid_utf8;
        assert!(project_xml(&invalid)
            .unwrap_err()
            .to_string()
            .contains("UTF-8"));
    }
    let mut valid = project(vec![occurrence(path)]);
    valid.sequences[0].name = "valid & < > \" 😀".into();
    Graph::parse(&project_xml(&valid).unwrap()).unwrap();
}

#[test]
fn repeated_source_has_one_project_item_and_independent_placements() {
    let path = Path::new("/tmp/media/source.mov");
    let occurrences = (0..3)
        .map(|index| PrVideoOccurrence {
            start_ticks: index * TICKS_PER_SECOND,
            end_ticks: (index + 1) * TICKS_PER_SECOND,
            in_ticks: index * TICKS_PER_SECOND,
            out_ticks: (index + 1) * TICKS_PER_SECOND,
            ..occurrence(path)
        })
        .collect();
    let mut project = project(occurrences);
    project.sequences[0].name = "Repeated".into();
    let xml = project_xml(&project).unwrap();
    Graph::parse(&xml).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let root = document.root_element();
    for (tag, count) in [
        ("Media", 1),
        ("MasterClip", 2),
        ("ClipProjectItem", 2),
        ("SubClip", 3),
        ("VideoClipTrackItem", 3),
    ] {
        assert_eq!(
            root.children()
                .filter(|node| node.has_tag_name(tag))
                .count(),
            count,
            "{tag}"
        );
    }
    let masters: std::collections::BTreeSet<_> = root
        .children()
        .filter(|node| node.has_tag_name("SubClip"))
        .map(|node| xml_at(node, "MasterClip").attribute("ObjectURef").unwrap())
        .collect();
    assert_eq!(masters.len(), 1);
    let placed_ids: std::collections::BTreeSet<_> = root
        .children()
        .filter(|node| node.has_tag_name("SubClip"))
        .map(|node| {
            let clip = referenced_record(root, xml_at(node, "Clip"));
            xml_at(clip, "Clip/ClipID").text().unwrap()
        })
        .collect();
    assert_eq!(placed_ids.len(), 3);
    let track = root
        .children()
        .find(|node| node.has_tag_name("VideoClipTrack"))
        .unwrap();
    let placement_refs: std::collections::BTreeSet<_> = track
        .descendants()
        .filter(|node| node.has_tag_name("TrackItem"))
        .map(|reference| {
            let placement = referenced_record(root, reference);
            assert!(placement.has_tag_name("VideoClipTrackItem"));
            reference.attribute("ObjectRef").unwrap()
        })
        .collect();
    assert_eq!(placement_refs.len(), 3);
    let bin = root
        .children()
        .find(|node| node.has_tag_name("RootProjectItem"))
        .unwrap();
    assert_eq!(
        bin.descendants()
            .filter(|node| node.has_tag_name("Item"))
            .count(),
        2
    );

    video_stream(project.media.values_mut().next().unwrap()).intrinsic_ticks = 2 * TICKS_PER_SECOND;
    assert!(project_xml(&project)
        .unwrap_err()
        .to_string()
        .contains("invalid timeline/source ranges"));
}

#[test]
fn distinct_media_requires_distinct_relative_paths() {
    let first = occurrence(Path::new("/tmp/first/source.mov"));
    let mut second = occurrence(Path::new("/tmp/second/source.mov"));
    second.start_ticks = 4 * TICKS_PER_SECOND;
    second.end_ticks = 7 * TICKS_PER_SECOND;
    let project = project(vec![first, second]);

    assert!(project_xml(&project)
        .unwrap_err()
        .to_string()
        .contains("media relative path must be unique"));
}

#[test]
fn media_dimensions_may_differ_from_canvas_but_must_be_positive() {
    let mut project = project(vec![occurrence(Path::new("/tmp/media/source.mp4"))]);
    video_stream(project.media.values_mut().next().unwrap()).width = 3840;
    let xml = project_xml(&project).unwrap();
    let imported = crate::format::inspect_project_with_media(&xml, None).unwrap();
    assert_eq!(
        imported
            .media
            .values()
            .next()
            .unwrap()
            .video
            .as_ref()
            .unwrap()
            .width,
        3840
    );

    video_stream(project.media.values_mut().next().unwrap()).width = 0;
    assert!(project_xml(&project)
        .unwrap_err()
        .to_string()
        .contains("media dimensions must be positive"));
}

#[test]
fn rejects_rate_mismatch_and_never_replaces_output() {
    let path = Path::new("/tmp/media/source.mp4");
    let mut invalid = project(vec![occurrence(path)]);
    invalid.sequences[0].video_tracks[0].clip_mut(0).out_ticks += TICKS_PER_SECOND;
    assert!(project_xml(&invalid)
        .unwrap_err()
        .to_string()
        .contains("source span"));

    let directory = tempdir().unwrap();
    let output = directory.path().join("existing.prproj");
    std::fs::write(&output, b"keep").unwrap();
    let error = PremiereProjectXml::new(&project(vec![occurrence(path)]))
        .unwrap()
        .write_new(&output)
        .unwrap_err();
    assert!(matches!(error, FormatError::Io(_)));
    assert_eq!(std::fs::read(output).unwrap(), b"keep");
}

#[test]
fn requires_frame_aligned_ranges() {
    let path = Path::new("/tmp/media/source.mp4");
    let mut project = project(vec![occurrence(path)]);
    assert!(project_xml(&project).is_ok());

    let off_frame = project.sequences[0].video_tracks[0].clip_mut(0);
    off_frame.start_ticks += TICKS_PER_SECOND / 100;
    off_frame.end_ticks += TICKS_PER_SECOND / 100;
    let error = project_xml(&project).unwrap_err();
    assert!(error.to_string().contains("timeline start"));
    assert!(error.to_string().contains("frame boundary"));
}

#[test]
fn two_occurrences_keep_independent_adjacent_ranges() {
    let first_path = Path::new("/tmp/fresh package/media/clip-a.mp4");
    let second_path = Path::new("/tmp/fresh package/media/clip-b.mp4");
    let mut project = project(vec![
        PrVideoOccurrence {
            start_ticks: 0,
            end_ticks: 60 * THIRTY_FPS_TICKS,
            in_ticks: 0,
            out_ticks: 60 * THIRTY_FPS_TICKS,
            ..occurrence(first_path)
        },
        PrVideoOccurrence {
            start_ticks: 60 * THIRTY_FPS_TICKS,
            end_ticks: 151 * THIRTY_FPS_TICKS,
            in_ticks: 90 * THIRTY_FPS_TICKS,
            out_ticks: 181 * THIRTY_FPS_TICKS,
            ..occurrence(second_path)
        },
    ]);
    project.sequences[0].name = "Cut and trim".into();

    let xml = project_xml(&project).unwrap();
    Graph::parse(&xml).unwrap();
    assert_eq!(xml.matches("<VideoClipTrackItem ObjectID=").count(), 2);
    assert!(xml.contains("<End>508032000000</End>"));
    assert!(xml.contains("<Start>508032000000</Start>"));
    assert!(xml.contains("<End>1278547200000</End>"));
    assert!(xml.contains("<InPoint>762048000000</InPoint>"));
    assert!(xml.contains("<OutPoint>1532563200000</OutPoint>"));
    assert!(xml.contains("<RelativePath>./media/clip-a.mp4</RelativePath>"));
    assert!(xml.contains("<RelativePath>./media/clip-b.mp4</RelativePath>"));
}

#[test]
fn many_media_names_remain_readable() {
    let occurrences = (0..257)
        .map(|index| {
            let name = format!("{index:03}{}.mp4", "&".repeat(248));
            let absolute = format!("/{}/{name}", "&".repeat(4096 - 257));
            PrVideoOccurrence {
                start_ticks: index as i64 * THIRTY_FPS_TICKS,
                end_ticks: (index as i64 + 1) * THIRTY_FPS_TICKS,
                in_ticks: 0,
                out_ticks: THIRTY_FPS_TICKS,
                ..occurrence(Path::new(&absolute))
            }
        })
        .collect();
    let mut project = project(occurrences);
    for media in project.media.values_mut() {
        video_stream(media).intrinsic_ticks = THIRTY_FPS_TICKS;
    }
    project.sequences[0].name = "&".repeat(255);
    let xml = project_xml(&project).unwrap();
    Graph::parse(&xml).unwrap();
}

#[test]
fn occurrence_count_above_former_limit_survives_round_trip() {
    assert!(project_xml(&project(Vec::new()))
        .unwrap_err()
        .to_string()
        .contains("at least one media occurrence"));
    let mut project = project(
        (0..257)
            .map(|index| PrVideoOccurrence {
                start_ticks: index * TICKS_PER_SECOND,
                end_ticks: (index + 1) * TICKS_PER_SECOND,
                in_ticks: 0,
                out_ticks: TICKS_PER_SECOND,
                ..occurrence(Path::new("/tmp/media/source.mp4"))
            })
            .collect(),
    );
    video_stream(project.media.values_mut().next().unwrap()).intrinsic_ticks = TICKS_PER_SECOND;
    project.sequences[0].name = "Many clips".into();
    let xml = project_xml(&project).unwrap();
    assert_eq!(
        inspect_project(&xml, None)
            .unwrap()
            .video_occurrences()
            .count(),
        257
    );
    let clips = std::mem::take(&mut project.sequences[0].video_tracks[0].items);
    project.sequences[0].video_tracks = clips
        .into_iter()
        .map(|clip| PrVideoTrack {
            items: vec![clip],
            transitions: Vec::new(),
            nests: Vec::new(),
        })
        .collect();
    let xml = project_xml(&project).unwrap();
    let sequence = inspect_project(&xml, None).unwrap();
    assert_eq!(sequence.video_tracks().len(), 257);
    assert_eq!(sequence.video_occurrences().count(), 257);
}

#[test]
fn placement_references_preserve_shared_media_across_empty_and_unequal_tracks() {
    let sources = [
        occurrence(Path::new("/tmp/media/a.mp4")),
        occurrence(Path::new("/tmp/media/b.mp4")),
    ];
    let mut project = project(sources.to_vec());
    project.sequences[0].video_tracks[0].items.clear();
    for entries in [
        vec![(0, 0, 2), (1, 1, 3)],
        vec![],
        vec![(1, 0, 4), (0, 1, 5), (1, 2, 6)],
    ] {
        project.sequences[0]
            .video_tracks
            .push(PrVideoTrack::media(entries.into_iter().map(
                |(media, start, source)| PrVideoOccurrence {
                    start_ticks: start * TICKS_PER_SECOND,
                    end_ticks: (start + 1) * TICKS_PER_SECOND,
                    in_ticks: source * TICKS_PER_SECOND,
                    out_ticks: (source + 1) * TICKS_PER_SECOND,
                    ..sources[media].clone()
                },
            )));
    }
    let xml = project_xml(&project).unwrap();
    assert_eq!(xml.matches("<Media ObjectUID=").count(), 2);
    assert_eq!(xml.matches("<VideoClipTrackItem ObjectID=").count(), 5);
    // Check native metadata directly: a matching reader/writer mistake can survive self-reading.
    let document = roxmltree::Document::parse(&xml).unwrap();
    let root = document.root_element();
    let group = root
        .children()
        .find(|node| node.has_tag_name("VideoTrackGroup"))
        .unwrap();
    assert_eq!(xml_at(group, "TrackGroup/NextTrackID").text(), Some("5"));
    let references: Vec<_> = xml_at(group, "TrackGroup/Tracks")
        .children()
        .filter(roxmltree::Node::is_element)
        .collect();
    assert_eq!(references.len(), 4);
    for (index, reference) in references.into_iter().enumerate() {
        let expected = index.to_string();
        assert_eq!(reference.attribute("Index"), Some(expected.as_str()));
        let track = referenced_record(root, reference);
        assert!(track.has_tag_name("VideoClipTrack"));
        for holder in ["Track", "ClipItems", "TransitionItems"] {
            assert_eq!(
                xml_at(track, &format!("ClipTrack/{holder}/Index")).text(),
                Some(expected.as_str())
            );
        }
        assert_eq!(
            xml_at(track, "ClipTrack/Track/ID").text(),
            Some((index + 1).to_string().as_str())
        );
    }
    let restored = crate::format::inspect_project_with_media(&xml, None).unwrap();
    let restored_sequence = restored.single_sequence().unwrap();
    assert_eq!(
        restored_sequence
            .video_tracks()
            .map(<[_]>::len)
            .collect::<Vec<_>>(),
        [0, 2, 0, 3]
    );
    for (expected, actual) in project.sequences[0]
        .video_occurrences()
        .zip(restored_sequence.video_occurrences())
    {
        assert_eq!(
            restored.media(actual).unwrap().name,
            project.media(expected).unwrap().name
        );
        assert_eq!(actual.timeline_ticks(), expected.timeline_ticks());
        assert_eq!(actual.source_ticks(), expected.source_ticks());
    }
}

#[test]
fn nested_ranges_do_not_create_false_gaps_and_empty_tracks_round_trip() {
    let clip = occurrence(Path::new("/tmp/media/source.mp4"));
    let mut project = project(vec![PrVideoOccurrence {
        start_ticks: 0,
        end_ticks: 5 * TICKS_PER_SECOND,
        in_ticks: 0,
        out_ticks: 5 * TICKS_PER_SECOND,
        ..clip.clone()
    }]);
    project.sequences[0]
        .video_tracks
        .push(PrVideoTrack::media([1, 3].into_iter().map(|start| {
            PrVideoOccurrence {
                start_ticks: start * TICKS_PER_SECOND,
                end_ticks: (start + 1) * TICKS_PER_SECOND,
                in_ticks: 0,
                out_ticks: TICKS_PER_SECOND,
                ..clip.clone()
            }
        })));
    project.sequences[0]
        .video_tracks
        .push(PrVideoTrack::media([]));
    let xml = project_xml(&project).unwrap();
    assert!(xml.ends_with("</PremiereData>\n\n"));
    let reread = crate::format::inspect_project_with_media(&xml, None).unwrap();
    let sequence = reread.single_sequence().unwrap();
    assert_eq!(
        sequence.video_tracks().map(<[_]>::len).collect::<Vec<_>>(),
        [1, 2, 0]
    );
    assert_eq!(sequence.end_ticks(), 5 * TICKS_PER_SECOND);
    assert!(sequence.gaps(&reread.media).is_empty());
    let lower = project.sequences[0].video_tracks[0].clip_mut(0);
    lower.start_ticks = TICKS_PER_SECOND;
    lower.out_ticks = 4 * TICKS_PER_SECOND;
    project.sequences[0]
        .validate_timeline(&project.media)
        .unwrap();
    assert_eq!(
        project.sequences[0].gaps(&project.media),
        vec![0..TICKS_PER_SECOND]
    );
}

/// Inspect writer output without the production graph or reader helpers.
fn xml_at<'a>(node: roxmltree::Node<'a, 'a>, path: &str) -> roxmltree::Node<'a, 'a> {
    path.split('/').fold(node, |parent, tag| {
        parent
            .children()
            .find(|child| child.has_tag_name(tag))
            .unwrap()
    })
}

/// Follow `reference` to a `tag` record that no earlier call has claimed.
fn owned_record<'a>(
    root: roxmltree::Node<'a, 'a>,
    owned: &mut std::collections::HashSet<String>,
    reference: roxmltree::Node<'a, 'a>,
    tag: &str,
) -> roxmltree::Node<'a, 'a> {
    let record = referenced_record(root, reference);
    assert!(record.has_tag_name(tag), "expected {tag}: {record:?}");
    assert!(
        owned.insert(record.attribute("ObjectID").unwrap().to_owned()),
        "{tag} is shared between strips"
    );
    record
}

fn referenced_record<'a>(
    root: roxmltree::Node<'a, 'a>,
    reference: roxmltree::Node<'a, 'a>,
) -> roxmltree::Node<'a, 'a> {
    let (attribute, value) = match (
        reference.attribute("ObjectRef"),
        reference.attribute("ObjectURef"),
    ) {
        (Some(value), None) => ("ObjectID", value),
        (None, Some(value)) => ("ObjectUID", value),
        _ => panic!("expected one native reference"),
    };
    root.children()
        .find(|record| record.attribute(attribute) == Some(value))
        .unwrap()
}

#[test]
fn native_scaffold_preserves_property_names_and_nesting() {
    let xml = project_xml(&project(vec![occurrence(Path::new(
        "/tmp/media/source.mp4",
    ))]))
    .unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let root = document.root_element();
    let project = root
        .children()
        .find(|node| node.attribute("ObjectID") == Some("1"))
        .unwrap();
    assert_eq!(
        xml_at(project, "Node/Properties/TL.PJSnappingState").text(),
        Some("1")
    );
    let bin = xml_at(root, "RootProjectItem/ProjectItem");
    assert_eq!(xml_at(bin, "Node/ID").text(), Some("1000000"));
    assert!(!bin.children().any(|node| node.has_tag_name("ID")));
    assert!(xml_at(bin, "Node/Properties/ProjectViewState.ID")
        .text()
        .is_some());
    let item = xml_at(root, "ClipProjectItem/ProjectItem");
    assert_eq!(
        xml_at(item, "Node/Properties/project.icon.view.grid.order").text(),
        Some("0")
    );
    let clip = xml_at(root, "VideoClip/Clip");
    assert_eq!(
        xml_at(clip, "Node/Properties/asl.clip.label.color").text(),
        Some("19005")
    );
    assert_eq!(
        xml_at(clip, "Node/Properties/asl.clip.label.name").text(),
        Some("BE.Prefs.LabelColors.5")
    );
    let data = xml_at(root, "DataTrackGroup/TrackGroup");
    assert_eq!(
        data.children()
            .filter(roxmltree::Node::is_element)
            .map(|node| node.tag_name().name())
            .collect::<Vec<_>>(),
        ["FrameRate", "NextTrackID"]
    );
    assert_eq!(data.attribute("Version"), Some("1"));
    assert_eq!(xml_at(data, "FrameRate").text(), Some("8467200000"));
    assert_eq!(xml_at(data, "NextTrackID").text(), Some("1"));
}

#[test]
fn native_audio_scaffold_keeps_each_strip_and_master_wired_independently() {
    let xml = project_xml(&project(vec![occurrence(Path::new(
        "/tmp/media/source.mp4",
    ))]))
    .unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let root = document.root_element();
    let tracks: Vec<_> = root
        .children()
        .filter(|node| node.has_tag_name("AudioClipTrack"))
        .collect();
    assert_eq!(tracks.len(), 4);
    let master = xml_at(root, "AudioMixTrack");
    let group = xml_at(root, "AudioTrackGroup");
    assert_eq!(
        xml_at(group, "MasterTrack").attribute("ObjectRef"),
        master.attribute("ObjectID")
    );
    let inlet = xml_at(root, "AudioTrackInlet");
    assert_eq!(
        xml_at(master, "Inlet").attribute("ObjectRef"),
        inlet.attribute("ObjectID")
    );

    // ObjectIDs are allocation output, not a format requirement. Follow each
    // strip's references and require that no strip shares a record with another.
    let mut owned = std::collections::HashSet::new();
    for (index, track) in tracks.iter().copied().chain([master]).enumerate() {
        let is_master = index == tracks.len();
        let mut own = |reference, tag| owned_record(root, &mut owned, reference, tag);
        let audio = xml_at(track, "AudioTrack");
        let chain = own(
            xml_at(audio, "ComponentOwner/Components"),
            "AudioComponentChain",
        );
        let components: Vec<_> = xml_at(chain, "ComponentChain/Components")
            .children()
            .filter(roxmltree::Node::is_element)
            .collect();
        assert_eq!(components.len(), 2);
        let fader = own(components[0], "AudioFader");
        own(components[1], "AudioMeter");
        let params: Vec<_> = xml_at(fader, "AudioComponent/Component/Params")
            .children()
            .filter(roxmltree::Node::is_element)
            .collect();
        assert_eq!(params.len(), 2);
        for (param, name) in params.into_iter().zip(["Volume", "Mute"]) {
            let param = own(param, "AudioComponentParam");
            assert_eq!(xml_at(param, "Name").text(), Some(name));
        }
        let panner_tag = if is_master {
            "DefaultPanProcessor"
        } else {
            "StereoToStereoPanProcessor"
        };
        let panner = own(xml_at(audio, "Panner"), panner_tag);
        let pan = xml_at(panner, "PanProcessor/AudioComponent/Component");
        if is_master {
            assert!(!pan.children().any(|node| node.has_tag_name("Params")));
            assert_eq!(xml_at(audio, "SubType").text(), Some("3"));
            continue;
        }
        let balance = own(xml_at(pan, "Params/Param"), "AudioComponentParam");
        assert_eq!(xml_at(balance, "Name").text(), Some("Balance"));
        let props = xml_at(track, "ClipTrack/Track/Node/Properties");
        assert_eq!(xml_at(props, "CM.KeyframeMode").text(), Some("true"));
        assert_eq!(
            props
                .children()
                .any(|node| node.has_tag_name("TL.SQTrackAudioKeyframeStyle")),
            index < 2
        );
        for references in [xml_at(group, "TrackGroup/Tracks"), xml_at(inlet, "Sources")] {
            let reference = references
                .children()
                .filter(roxmltree::Node::is_element)
                .nth(index)
                .unwrap();
            assert_eq!(
                reference.attribute("ObjectURef"),
                track.attribute("ObjectUID")
            );
        }
    }
}

#[test]
fn typed_encoder_preserves_supported_text() {
    let cases = [
        "A & B <clip> \"quoted\" 'apostrophe'",
        "café 中文",
        "line\n\ttab",
        "carriage\rreturn",
        "\r\n",
        "literal &#13;",
    ];
    for value in cases {
        let mut input = project(vec![occurrence(Path::new("/tmp/media/source.mp4"))]);
        input.sequences[0].name = value.to_owned();
        let xml = project_xml(&input).unwrap();
        let document = roxmltree::Document::parse(&xml).unwrap();
        let sequence = document
            .root_element()
            .children()
            .find(|node| node.has_tag_name("Sequence") && node.attribute("ObjectUID").is_some())
            .unwrap();
        assert_eq!(
            sequence
                .children()
                .find(|node| node.has_tag_name("Name"))
                .and_then(|node| node.text()),
            Some(value)
        );
        if value.contains('\r') {
            assert!(xml.contains("&#13;"));
            assert!(!xml.contains('\r'));
        }
    }
}

#[test]
fn typed_writer_preserves_native_track_shape_and_reference_types() {
    let mut placed = occurrence(Path::new("/tmp/media/source.mp4"));
    placed.start_ticks = 0;
    placed.end_ticks = placed.out_ticks - placed.in_ticks;
    let mut input = project(vec![placed]);
    input.sequences[0]
        .video_tracks
        .push(PrVideoTrack::media([]));

    let xml = project_xml(&input).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let root = document.root_element();
    assert_eq!(root.tag_name().name(), "PremiereData");
    assert_eq!(root.attribute("Version"), Some("3"));
    let root_children: Vec<_> = root
        .children()
        .filter(roxmltree::Node::is_element)
        .collect();
    assert_eq!(root_children[0].tag_name().name(), "Project");
    assert_eq!(root_children[0].attribute("ObjectRef"), Some("1"));
    assert!(root_children[0].attribute("ObjectID").is_none());

    let tracks: Vec<_> = root_children
        .iter()
        .copied()
        .filter(|node| node.has_tag_name("VideoClipTrack"))
        .collect();
    assert_eq!(tracks.len(), 2);
    for track in &tracks {
        assert!(track.attribute("ObjectUID").is_some());
        assert_eq!(track.attribute("Version"), Some("1"));
        assert_eq!(
            track
                .children()
                .filter(roxmltree::Node::is_element)
                .map(|node| node.tag_name().name())
                .collect::<Vec<_>>(),
            ["ClipTrack"]
        );
        let clip_track = track.children().find(roxmltree::Node::is_element).unwrap();
        assert_eq!(
            clip_track
                .children()
                .filter(roxmltree::Node::is_element)
                .map(|node| node.tag_name().name())
                .collect::<Vec<_>>(),
            ["Track", "ClipItems", "TransitionItems"]
        );
    }

    let populated_clip_items = tracks[0]
        .descendants()
        .find(|node| node.has_tag_name("ClipItems"))
        .unwrap();
    let populated_children: Vec<_> = populated_clip_items
        .children()
        .filter(roxmltree::Node::is_element)
        .map(|node| node.tag_name().name())
        .collect();
    assert_eq!(populated_children, ["TrackItems", "MediaType", "Index"]);
    let empty_clip_items = tracks[1]
        .descendants()
        .find(|node| node.has_tag_name("ClipItems"))
        .unwrap();
    let empty_children: Vec<_> = empty_clip_items
        .children()
        .filter(roxmltree::Node::is_element)
        .map(|node| node.tag_name().name())
        .collect();
    assert_eq!(empty_children, ["MediaType", "Index"]);

    let placement_ref = populated_clip_items
        .descendants()
        .find(|node| node.has_tag_name("TrackItem"))
        .unwrap()
        .attribute("ObjectRef")
        .unwrap();
    let placement = root_children
        .iter()
        .copied()
        .find(|node| node.attribute("ObjectID") == Some(placement_ref))
        .unwrap();
    assert!(placement.has_tag_name("VideoClipTrackItem"));
    assert!(placement
        .descendants()
        .find(|node| node.has_tag_name("TrackItem"))
        .unwrap()
        .children()
        .all(|node| !node.has_tag_name("Start")));
}

#[test]
fn graphic_text_is_written_as_an_editable_native_graphic() {
    let mut project = project(vec![occurrence(Path::new("/tmp/media/source.mp4"))]);
    let graphic = text_graphic();
    project.sequences[0].video_tracks.push(PrVideoTrack {
        items: vec![PrVideoItem::Graphic(graphic.clone())],
        transitions: Vec::new(),
        nests: Vec::new(),
    });
    let xml = project_xml(&project).unwrap();
    // Check native records directly: a matching reader/writer mistake can survive self-reading.
    assert_eq!(
        xml.matches("<MatchName>AE.ADBE Text</MatchName>").count(),
        1
    );
    assert_eq!(
        xml.matches("<ImplementationID>42008e7a-de6f-4270-96de-7e287abb9b4b</ImplementationID>")
            .count(),
        1
    );
    // The sequence and the one video source; graphics stay out of the project panel.
    assert_eq!(xml.matches("<ClipProjectItem ").count(), 2);
    assert!(xml.contains("<InstanceName>Title</InstanceName>"));
    // Position and anchor are normalized to the 1920x1080 frame.
    assert!(xml.contains(",0.25:0.5,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe>"));
    assert!(xml.contains(",0.05:0.05,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe>"));
    assert!(xml.contains("<StartKeyframe>-91445760000000000,-15,0,0,0,0,0,0</StartKeyframe>"));

    let reopened = inspect_project(&xml, None).unwrap();
    let read: Vec<_> = reopened
        .video_items()
        .filter_map(PrVideoItem::graphic)
        .collect();
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].timeline_ticks(), graphic.timeline_ticks());
    assert_eq!(read[0].text(), graphic.text());
}

#[test]
fn keyed_text_parameters_are_written_in_the_premiere_26_5_1_form() {
    use crate::schema::{PrKeyframeEasing, PrPointKeyframe, PrPropertyAnimation, PrScalarKeyframe};
    let mut project = project(vec![occurrence(Path::new("/tmp/media/source.mp4"))]);
    let mut graphic = text_graphic();
    let start = graphic.in_ticks;
    let scalar = |seconds: i64, value: f64, easing| PrScalarKeyframe {
        source_ticks: start + seconds * TICKS_PER_SECOND,
        value,
        easing,
    };
    // Keys before, inside and after the two-second placement.
    graphic.text_mut().animations = vec![
        PrPropertyAnimation::Position(vec![
            PrPointKeyframe {
                source_ticks: start - TICKS_PER_SECOND,
                value: [0.25, 0.5],
                easing: PrKeyframeEasing::Linear,
                spatial_in_tangent: Some([0.0, 0.0]),
                spatial_out_tangent: Some([0.05, 0.0]),
            },
            PrPointKeyframe {
                source_ticks: start + 3 * TICKS_PER_SECOND,
                value: [0.3, 0.55],
                easing: PrKeyframeEasing::Linear,
                spatial_in_tangent: Some([-0.05, 0.0]),
                spatial_out_tangent: Some([0.0, 0.0]),
            },
        ]),
        PrPropertyAnimation::UniformScale(vec![
            scalar(0, 80.0, PrKeyframeEasing::Linear),
            scalar(1, 96.0, PrKeyframeEasing::Hold),
        ]),
        PrPropertyAnimation::Rotation(vec![
            scalar(0, -15.0, PrKeyframeEasing::Linear),
            scalar(1, 20.0, PrKeyframeEasing::Linear),
        ]),
        PrPropertyAnimation::Opacity(vec![
            scalar(0, 75.0, PrKeyframeEasing::Linear),
            scalar(2, 40.0, PrKeyframeEasing::Linear),
        ]),
    ];
    project.sequences[0].video_tracks.push(PrVideoTrack {
        items: vec![PrVideoItem::Graphic(graphic.clone())],
        transitions: Vec::new(),
        nests: Vec::new(),
    });
    let xml = project_xml(&project).unwrap();
    // Keyed graphic parameters carry IsTimeVarying; static ones omit it.
    assert_eq!(
        xml.matches("<IsTimeVarying>true</IsTimeVarying>").count(),
        4
    );
    assert!(!xml.contains("<IsTimeVarying>false</IsTimeVarying>"));
    // A Hold key (mode 4) holds from the key before; the curved path keeps
    // its tangents with spatial mode 5.
    assert!(xml.contains(&format!(
        "<Keyframes>{start},80,4,0,0,0,0,0;{},96,0,0,0,0,0,0;</Keyframes>",
        start + TICKS_PER_SECOND
    )));
    assert!(xml.contains(&format!(
        "<Keyframes>{},0.25:0.5,0,0,0,0,0,0,5,0,0,0,0.05,0;",
        start - TICKS_PER_SECOND
    )));
    let reopened = inspect_project(&xml, None).unwrap();
    let read: Vec<_> = reopened
        .video_items()
        .filter_map(PrVideoItem::graphic)
        .collect();
    assert_eq!(read[0].in_ticks, start);
    assert_eq!(read[0].text(), graphic.text());
}

#[test]
fn keyed_source_text_is_written_in_the_premiere_26_5_1_form() {
    use crate::format::text_payload;
    use crate::schema::text::PrSourceTextKey;
    use base64::{engine::general_purpose::STANDARD, Engine};
    let mut project = project(vec![occurrence(Path::new("/tmp/media/source.mp4"))]);
    let mut graphic = text_graphic();
    let start = graphic.in_ticks;
    let text = graphic.text_mut();
    let document = |body: &str, size: f32| {
        let mut document = text.document.clone();
        document.text = body.to_owned();
        document.size = size;
        document
    };
    let keys = [
        (start + TICKS_PER_SECOND / 2, document("TWO", 80.0)),
        (start + 5 * TICKS_PER_SECOND / 2, document("THREE", 140.0)),
    ];
    // Premiere renders the first key's document before the first key, so
    // the model's document is the first key's.
    text.document = keys[0].1.clone();
    text.source_text_keys = keys
        .iter()
        .map(|(source_ticks, document)| PrSourceTextKey {
            source_ticks: *source_ticks,
            document: document.clone(),
        })
        .collect();
    project.sequences[0].video_tracks.push(PrVideoTrack {
        items: vec![PrVideoItem::Graphic(graphic.clone())],
        transitions: Vec::new(),
        nests: Vec::new(),
    });
    let xml = project_xml(&project).unwrap();
    // The record as Premiere 26.5.1 saves it: version 3, IsTimeVarying
    // after the control type, `ticks,base64;` per key with no BinaryHash,
    // the keys before the StartKeyframeValue, which is the first key's
    // document.
    let payload = |document| STANDARD.encode(text_payload::encode(document).unwrap());
    let record = xml
        .split("<Name>Source Text</Name>")
        .nth(1)
        .and_then(|rest| rest.split("</ArbVideoComponentParam>").next())
        .unwrap();
    assert_eq!(
        xml.matches("<Name>Source Text</Name>").count(),
        1,
        "one Source Text record"
    );
    assert!(xml.contains(&format!(
        "Version=\"3\">\n\t\t<Node Version=\"1\">\n\t\t\t<Properties Version=\"1\">\n\t\t\t\t<ECP.Graphics.Expanded>true</ECP.Graphics.Expanded>\n\t\t\t</Properties>\n\t\t</Node>\n\t\t<Name>Source Text</Name>\n\t\t<ParameterControlType>9</ParameterControlType>\n\t\t<IsTimeVarying>true</IsTimeVarying>\n\t\t<ParameterID>1</ParameterID>\n\t\t<StartKeyframePosition>-91445760000000000</StartKeyframePosition>\n\t\t<Keyframes>{},{};{},{};</Keyframes>\n\t\t<StartKeyframeValue Encoding=\"base64\" BinaryHash=\"",
        keys[0].0,
        payload(&keys[0].1),
        keys[1].0,
        payload(&keys[1].1),
    )), "{record}");
    assert!(record.contains(&format!("\">{}</StartKeyframeValue>", payload(&keys[0].1))));
    let reopened = inspect_project(&xml, None).unwrap();
    let read: Vec<_> = reopened
        .video_items()
        .filter_map(PrVideoItem::graphic)
        .collect();
    assert_eq!(read[0].text(), graphic.text());
}

#[test]
fn keyed_vector_motion_is_written_before_the_text_in_the_premiere_26_5_1_layout() {
    use crate::schema::{
        text::PrVectorMotion, PrKeyframeEasing, PrPointKeyframe, PrPropertyAnimation,
        PrScalarKeyframe,
    };
    let mut project = project(Vec::new());
    let mut graphic = text_graphic();
    let start = graphic.in_ticks;
    graphic.vector_motion = Some(PrVectorMotion {
        position: [960.0, 540.0],
        anchor: [768.0, 486.0],
        scale: 100.0,
        rotation: 0.0,
        animations: vec![
            PrPropertyAnimation::Position(vec![
                PrPointKeyframe {
                    source_ticks: start,
                    value: [0.5, 0.5],
                    easing: PrKeyframeEasing::Linear,
                    spatial_in_tangent: None,
                    spatial_out_tangent: None,
                },
                PrPointKeyframe {
                    source_ticks: start + TICKS_PER_SECOND,
                    value: [0.55, 0.45],
                    easing: PrKeyframeEasing::Linear,
                    spatial_in_tangent: None,
                    spatial_out_tangent: None,
                },
            ]),
            PrPropertyAnimation::UniformScale(vec![
                PrScalarKeyframe {
                    source_ticks: start,
                    value: 100.0,
                    easing: PrKeyframeEasing::Linear,
                },
                PrScalarKeyframe {
                    source_ticks: start + TICKS_PER_SECOND,
                    value: 70.0,
                    easing: PrKeyframeEasing::Linear,
                },
            ]),
        ],
    });
    project.sequences[0].timeline_end_ticks = graphic.end_ticks;
    project.sequences[0].video_tracks[0].items = vec![PrVideoItem::Graphic(graphic.clone())];
    let xml = project_xml(&project).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let components: Vec<_> = document
        .root_element()
        .children()
        .filter(|node| node.has_tag_name("VideoFilterComponent"))
        .collect();
    // The graphic's chain lists Vector Motion first, as Adobe-saved graphics do.
    let item = document
        .root_element()
        .children()
        .find(|node| node.has_tag_name("VideoClipTrackItem"))
        .unwrap();
    let chain = referenced_record(
        document.root_element(),
        xml_at(item, "ClipTrackItem/ComponentOwner/Components"),
    );
    let order: Vec<_> = chain
        .descendants()
        .filter(|node| node.has_tag_name("Component") && node.attribute("ObjectRef").is_some())
        .map(|node| {
            let id = node.attribute("ObjectRef").unwrap();
            let component = components
                .iter()
                .find(|component| component.attribute("ObjectID") == Some(id))
                .unwrap();
            xml_at(*component, "MatchName").text().unwrap()
        })
        .collect();
    assert_eq!(order, ["AE.ADBE Graphic Group", "AE.ADBE Text"]);
    let motion = components
        .iter()
        .find(|component| xml_at(**component, "MatchName").text() == Some("AE.ADBE Graphic Group"))
        .unwrap();
    assert_eq!(
        (
            motion.attribute("Version"),
            xml_at(*motion, "Component/ID").text(),
            xml_at(*motion, "Component/Intrinsic").text(),
            xml_at(*motion, "Component/DisplayName").text(),
        ),
        (Some("9"), Some("5"), Some("true"), Some("Vector Motion"))
    );
    assert!(motion
        .children()
        .all(|node| !node.has_tag_name("PremiereFilterPrivateData")));
    // Scale keeps the stored slider range; keyed parameters are time-varying.
    assert_eq!(xml.matches("<UpperUIBound>200</UpperUIBound>").count(), 2);
    assert_eq!(
        xml.matches("<IsTimeVarying>true</IsTimeVarying>").count(),
        2
    );
    assert!(xml.contains(",0.4:0.45,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe>"));
    let reopened = inspect_project(&xml, None).unwrap();
    let read = reopened
        .video_items()
        .find_map(PrVideoItem::graphic)
        .unwrap();
    assert_eq!(read.vector_motion, graphic.vector_motion);
    assert_eq!(read.text(), graphic.text());
}

#[test]
fn a_graphic_clip_opacity_is_written_first_in_the_graphic_chain() {
    use crate::schema::{
        text::PrVectorMotion, PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe,
    };
    let mut project = project(Vec::new());
    let mut graphic = text_graphic();
    let start = graphic.in_ticks;
    let key = |seconds: i64, value: f64, easing| PrScalarKeyframe {
        source_ticks: start + seconds * TICKS_PER_SECOND,
        value,
        easing,
    };
    graphic.opacity = 80.0;
    graphic.animations = vec![PrPropertyAnimation::Opacity(vec![
        key(0, 80.0, PrKeyframeEasing::Linear),
        key(1, 0.0, PrKeyframeEasing::Hold),
    ])];
    graphic.vector_motion = Some(PrVectorMotion {
        position: [960.0, 540.0],
        anchor: [960.0, 540.0],
        scale: 100.0,
        rotation: 0.0,
        animations: vec![PrPropertyAnimation::Rotation(vec![
            key(0, 0.0, PrKeyframeEasing::Linear),
            key(1, 20.0, PrKeyframeEasing::Linear),
        ])],
    });
    project.sequences[0].timeline_end_ticks = graphic.end_ticks;
    project.sequences[0].video_tracks[0].items = vec![PrVideoItem::Graphic(graphic.clone())];
    let xml = project_xml(&project).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let item = document
        .root_element()
        .children()
        .find(|node| node.has_tag_name("VideoClipTrackItem"))
        .unwrap();
    let chain = referenced_record(
        document.root_element(),
        xml_at(item, "ClipTrackItem/ComponentOwner/Components"),
    );
    // As Premiere 26.5.1 saved a keyed clip Opacity ([Opacity, Text]): no
    // `DefaultOpacity`, and the Opacity first. The intrinsic Vector Motion
    // between it and the Text is an inferred order: no Adobe save has both.
    let defaults: Vec<_> = chain
        .children()
        .filter(|node| node.tag_name().name().starts_with("Default"))
        .map(|node| (node.tag_name().name(), node.text().unwrap()))
        .collect();
    assert_eq!(
        defaults,
        [("DefaultMotion", "true"), ("DefaultMotionComponentID", "1")]
    );
    let order: Vec<_> = chain
        .descendants()
        .filter(|node| node.has_tag_name("Component") && node.attribute("ObjectRef").is_some())
        .map(|node| {
            let component = referenced_record(document.root_element(), node);
            xml_at(component, "MatchName").text().unwrap()
        })
        .collect();
    assert_eq!(
        order,
        ["AE.ADBE Opacity", "AE.ADBE Graphic Group", "AE.ADBE Text"]
    );
    // The Opacity records are the media clip's, with the Normal pair (18, 0).
    assert!(xml.contains(&format!(
        "<Keyframes>{start},80,4,0,0,0,0,0;{},0,0,0,0,0,0,0;</Keyframes>",
        start + TICKS_PER_SECOND
    )));
    assert!(xml.contains("<StartKeyframe>-91445760000000000,18,0,0,0,0,0,0</StartKeyframe>"));
    let reopened = inspect_project(&xml, None).unwrap();
    let read = reopened
        .video_items()
        .find_map(PrVideoItem::graphic)
        .unwrap();
    assert_eq!(
        (read.opacity, &read.animations, read.blend_mode),
        (80.0, &graphic.animations, graphic.blend_mode)
    );
    assert_eq!(read.vector_motion, graphic.vector_motion);
    assert_eq!(read.text(), graphic.text());
}

#[test]
fn a_graphic_clip_keeps_its_motion_and_an_opacity_that_premiere_holds() {
    use crate::schema::{PrKeyframeEasing, PrPropertyAnimation, PrScalarKeyframe};
    let key = |value| PrScalarKeyframe {
        source_ticks: 0,
        value,
        easing: PrKeyframeEasing::Linear,
    };
    let mut nondefault = text_graphic();
    nondefault.opacity = 120.0;
    let mut motion_keys = text_graphic();
    motion_keys.animations = vec![PrPropertyAnimation::Rotation(vec![key(10.0)])];
    let mut opacity_keys = text_graphic();
    opacity_keys.animations = vec![PrPropertyAnimation::Opacity(vec![key(120.0)])];
    for (graphic, message) in [
        (
            nondefault,
            "graphic clip opacity must be finite and between 0 and 100",
        ),
        (motion_keys, "graphic clip Motion keys are unsupported"),
        (
            opacity_keys,
            "graphic clip Opacity keys must be between 0 and 100",
        ),
    ] {
        let mut project = project(Vec::new());
        project.sequences[0].timeline_end_ticks = graphic.end_ticks;
        project.sequences[0].video_tracks[0].items = vec![PrVideoItem::Graphic(graphic)];
        let error = project_xml(&project).unwrap_err().to_string();
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn text_payload_past_the_former_limit_survives_native_xml_round_trip() {
    let mut project = project(Vec::new());
    let mut graphic = text_graphic();
    graphic.text_mut().document.text = "x".repeat(1 << 20);
    project.sequences[0].timeline_end_ticks = graphic.end_ticks;
    project.sequences[0].video_tracks[0].items = vec![PrVideoItem::Graphic(graphic)];
    let xml = project_xml(&project).unwrap();
    let reopened = inspect_project(&xml, None).unwrap();
    let text = reopened
        .video_items()
        .find_map(PrVideoItem::graphic)
        .unwrap();
    assert_eq!(text.text().document.text, "x".repeat(1 << 20));
}

#[test]
fn text_only_projects_need_no_media() {
    let mut project = project(vec![occurrence(Path::new("/tmp/media/source.mp4"))]);
    project.media.clear();
    project.sequences[0].video_tracks = vec![PrVideoTrack {
        items: vec![PrVideoItem::Graphic(text_graphic())],
        transitions: Vec::new(),
        nests: Vec::new(),
    }];
    let xml = project_xml(&project).unwrap();
    assert!(!xml.contains("<RelativePath>"));
    let reopened = inspect_project(&xml, None).unwrap();
    let graphics = reopened.video_items().filter_map(PrVideoItem::graphic);
    assert_eq!(graphics.count(), 1);
    assert_eq!(reopened.video_occurrences().count(), 0);
}

/// A media-free sequence that ends with its one `graphic`.
fn text_only(graphic: PrGraphic) -> PrProjectFile {
    let mut project = project(Vec::new());
    project.sequences[0].timeline_end_ticks = graphic.end_ticks;
    project.sequences[0].video_tracks[0].items = vec![PrVideoItem::Graphic(graphic)];
    project
}

#[test]
fn graphic_source_out_point_must_fit_the_tick_range() {
    // A 30 fps placement plays its generator from 3600 s in, so its source
    // out-point is that offset plus the graphic duration.
    let source_in = 3600 * TICKS_PER_SECOND;
    let longest = (i64::MAX - source_in) / THIRTY_FPS_TICKS * THIRTY_FPS_TICKS;
    let graphic = |end_ticks| PrGraphic {
        start_ticks: 0,
        end_ticks,
        ..text_graphic()
    };

    let xml = project_xml(&text_only(graphic(longest))).unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    let source_ranges: Vec<_> = document
        .root_element()
        .children()
        .filter(|node| node.has_tag_name("VideoClip"))
        .map(|record| {
            let clip = xml_at(record, "Clip");
            ["InPoint", "OutPoint"].map(|field| {
                clip.children()
                    .find(|child| child.has_tag_name(field))
                    .and_then(|node| node.text()?.parse::<i64>().ok())
            })
        })
        .collect();
    assert!(
        source_ranges.contains(&[Some(source_in), Some(source_in + longest)]),
        "{source_ranges:?}"
    );
    let reopened = inspect_project(&xml, None).unwrap();
    let read: Vec<_> = reopened
        .video_items()
        .filter_map(PrVideoItem::graphic)
        .collect();
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].timeline_ticks(), 0..longest);

    // One frame longer passes i64::MAX and must return an error, not overflow.
    let error = project_xml(&text_only(graphic(longest + THIRTY_FPS_TICKS)))
        .unwrap_err()
        .to_string();
    assert!(error.contains("tick range"), "{error}");
}

#[test]
fn graphic_names_must_be_xml_text() {
    for name in ["bad\u{0}name", "bad\u{1}name", "bad\u{ffff}name"] {
        let mut text = text_graphic();
        text.text_mut().name = name.to_owned();
        let mut shape = shape_graphic();
        if let [PrGraphicObject::Shape(object)] = shape.objects.as_mut_slice() {
            object.name = name.to_owned();
        }
        for (graphic, kind) in [(text, "text"), (shape, "shape")] {
            let error = project_xml(&text_only(graphic)).unwrap_err().to_string();
            assert!(
                error.contains(&format!("{kind} layer name")),
                "{name:?}: {error}"
            );
        }
    }
}

#[test]
fn graphic_names_keep_escaped_delimiters_unicode_and_line_breaks() {
    for name in [
        "",
        "A & B <title> \"quoted\" 'apostrophe'",
        "café 中文 😀 \u{fffd}",
        "line\n\ttab",
        "carriage\rreturn",
        "\r\n",
        "literal &#13;",
    ] {
        let mut graphic = text_graphic();
        graphic.text_mut().name = name.to_owned();
        let xml = project_xml(&text_only(graphic)).unwrap();
        // An independent XML 1.0 parser must read back the exact name.
        let document = roxmltree::Document::parse(&xml).unwrap();
        let instance_names: Vec<_> = document
            .descendants()
            .filter(|node| node.has_tag_name("InstanceName"))
            .map(|node| node.text().unwrap_or_default())
            .collect();
        assert_eq!(instance_names, [name]);
        let reopened = inspect_project(&xml, None).unwrap();
        let read: Vec<_> = reopened
            .video_items()
            .filter_map(PrVideoItem::graphic)
            .map(|graphic| graphic.text().name.as_str())
            .collect();
        assert_eq!(read, [name]);
    }
}
