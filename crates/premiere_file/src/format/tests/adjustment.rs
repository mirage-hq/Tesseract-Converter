use super::{
    animation::animation_fixture::SOURCE,
    effects::{blur, film_impact_20_parameter_fragment},
};
use crate::{
    format::{inspect_project_with_omissions, FrameRate, PrProjectFile},
    schema::{
        adjustment::retains_edit, EditKind, OccurrenceEdit, PrAnimatedProperty, PrEffectParams,
        PrGaussianBlur, PrMediaKind, PrVideoOccurrence, TICKS,
    },
    tests::support::project_document_with_media,
    Omission, OmissionKind, OmissionScope,
};
use serde_json::json;

/// An adjustment placement's source in-point on the 30 fps test sequence.
const IN_TICKS: i64 = FrameRate::Fps30.generator_in_ticks();
const DEFAULT_FLAGS: &str =
    "<DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity>";
const BLACK_VIDEO_MEDIA: &str = r#"<Media ObjectUID="adjustment-media"><VideoStream ObjectRef="45"/><FilePath>1112293707</FilePath><Infinite>true</Infinite><ImplementationID>42008e7a-de6f-4270-96de-7e287abb9b4b</ImplementationID><Title>Black Video</Title><ActualMediaFilePath>1112293707</ActualMediaFilePath><ContentAndMetadataState>00000000-0000-0000-0000-000000000000</ContentAndMetadataState><ConformedAudioRate>9223372036854775807</ConformedAudioRate></Media>
<VideoStream ObjectID="45"><IsStill>true</IsStill><IsContinuousTime>true</IsContinuousTime><FrameRate>8467200000</FrameRate><FrameRect>0,0,1920,1080</FrameRect><Duration>10973491200000000</Duration><CodecType>1380013856</CodecType><FieldTypeIsUncertain>true</FieldTypeIsUncertain></VideoStream>"#;
const CLIP_FLAG: &str = "<AdjustmentLayer>true</AdjustmentLayer>";
const MASTER_FLAG: &str = "<IsAdjustmentLayer>true</IsAdjustmentLayer>";

/// The 26.3-layout clip Opacity records (`feature_opacity_screen_strict`
/// form) with the static `value` and, when `keys` is nonempty, keyed Opacity.
pub(super) fn opacity(value: &str, keys: &str) -> String {
    let (time_varying, keyframes) = if keys.is_empty() {
        ("false", String::new())
    } else {
        ("true", format!("<Keyframes>{keys}</Keyframes>"))
    };
    format!(
        r#"<VideoFilterComponent ObjectID="50" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="7"><Component Version="5"><Params Version="1"><Param Index="0" ObjectRef="51"/><Param Index="1" ObjectRef="52"/><Param Index="2" ObjectRef="53"/></Params><ID>2</ID><DisplayName>Opacity</DisplayName><Bypass>false</Bypass><Intrinsic>true</Intrinsic></Component><MatchName>AE.ADBE Opacity</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>
<VideoComponentParam ObjectID="51" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="9"><Name>Opacity</Name><IsTimeVarying>{time_varying}</IsTimeVarying><ParameterControlType>2</ParameterControlType><StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0</StartKeyframe>{keyframes}<LowerBound>0</LowerBound><UpperBound>100</UpperBound><ParameterID>1</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID="52" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="9"><Name>Blend Mode</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>10</ParameterControlType><StartKeyframe>-91445760000000000,18,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>26</UpperBound><ParameterID>2</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID="53" ClassID="6e02e8bb-2569-46b2-8ab1-4ab11c43e9c8" Version="9"><Name>Blend Mode</Name><IsTimeVarying>false</IsTimeVarying><DiscontinuousInterpolate>true</DiscontinuousInterpolate><ParameterControlType>7</ParameterControlType><StartKeyframe>-91445760000000000,0,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>31</UpperBound><ParameterID>3</ParameterID></VideoComponentParam>"#
    )
}

/// The Motion component of `animated_xml`, keyed on Rotation when `keys` is
/// nonempty, with `scale` as the static Scale.
fn motion(scale: &str, keys: &str) -> String {
    let mut params = String::new();
    for (id, name, value, point) in [
        (1, "Position", "0.5:0.5", true),
        (2, "Scale", scale, false),
        (3, "Scale Width", scale, false),
        (4, " ", "true", false),
        (5, "Rotation", "0.", false),
        (6, "Anchor Point", "0.5:0.5", true),
        (7, "Anti-flicker Filter", "0.", false),
    ] {
        let tag = if point {
            "PointComponentParam"
        } else {
            "VideoComponentParam"
        };
        let initial = if point {
            format!("-91445760000000000,{value},0,0,0,0,0,0,5,4,0,0,0,0")
        } else {
            format!("-91445760000000000,{value},0,0,0,0,0,0")
        };
        let animation = if id == 5 && !keys.is_empty() {
            format!("<Keyframes>{keys}</Keyframes>")
        } else {
            String::new()
        };
        params.push_str(&format!("<{tag} ObjectID=\"{}\"><Name>{name}</Name><ParameterID>{id}</ParameterID><StartKeyframe>{initial}</StartKeyframe>{animation}</{tag}>", id + 70));
    }
    let refs = (1..=7)
        .map(|id| format!("<Param Index=\"{}\" ObjectRef=\"{}\"/>", id - 1, id + 70))
        .collect::<String>();
    format!("<VideoFilterComponent ObjectID=\"70\"><Component><Params>{refs}</Params><DisplayName>Motion</DisplayName><Bypass>false</Bypass><Intrinsic>true</Intrinsic></Component><MatchName>AE.ADBE Motion</MatchName></VideoFilterComponent>{params}")
}

/// The one-clip project plus an adjustment layer over 1 to 4 s on a second
/// track (`VideoClipTrackItem:40`), in the corpus record form: a flagged
/// `VideoClip:43` of Black Video media under a flagged `MasterClip`. Its
/// chain has `flags` and holds `components` in chain order; `clip_flag` and
/// `master_flag` replace the two flags; `media` replaces the Black Video
/// media and stream.
fn adjustment_xml(
    flags: &str,
    components: &[(u32, String)],
    clip_flag: &str,
    master_flag: &str,
    media: &str,
) -> String {
    let references: String = components
        .iter()
        .enumerate()
        .map(|(index, (id, _))| format!("<Component Index=\"{index}\" ObjectRef=\"{id}\"/>"))
        .collect();
    let records: String = components
        .iter()
        .map(|(_, records)| records.as_str())
        .collect();
    let track = format!(
        r#"<VideoClipTrack ObjectUID="track-2"><ClipTrack><Track><ID>2</ID><Index>1</Index></Track><ClipItems><TrackItems><TrackItem ObjectRef="40"/></TrackItems><Index>1</Index></ClipItems></ClipTrack></VideoClipTrack>
<VideoClipTrackItem ObjectID="40"><ClipTrackItem><ComponentOwner><Components ObjectRef="41"/></ComponentOwner><TrackItem><Start>{start}</Start><End>{end}</End></TrackItem><SubClip ObjectRef="42"/></ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>1,1</PixelAspectRatio></VideoClipTrackItem>
<VideoComponentChain ObjectID="41">{flags}<ComponentChain><Components>{references}</Components></ComponentChain></VideoComponentChain>
<SubClip ObjectID="42"><Clip ObjectRef="43"/><MasterClip ObjectURef="adjustment-master"/><Name>Adjustment Layer</Name></SubClip>
<VideoClip ObjectID="43"><Clip><Node Version="1"><Properties Version="1"><BE.Prefs.SyntheticMedia.DefaultIsDropFrame>false</BE.Prefs.SyntheticMedia.DefaultIsDropFrame></Properties></Node><Source ObjectRef="44"/><ClipID>placed-adjustment</ClipID><InPoint>{IN_TICKS}</InPoint><OutPoint>{out}</OutPoint></Clip>{clip_flag}</VideoClip>
<VideoMediaSource ObjectID="44"><MediaSource Version="4"><Content Version="10"></Content><Media ObjectURef="adjustment-media"/></MediaSource><OriginalDuration>10973491200000000</OriginalDuration></VideoMediaSource>
{media}
<MasterClip ObjectUID="adjustment-master"><Clips Version="1"><Clip Index="0" ObjectRef="46"/></Clips><Name>Adjustment Layer</Name>{master_flag}<MasterClipChangeVersion>4</MasterClipChangeVersion></MasterClip>
<VideoClip ObjectID="46"><Clip><Source ObjectRef="44"/><ClipID>template-adjustment</ClipID><InPoint>0</InPoint><OutPoint>1270080000000</OutPoint><InUse>false</InUse></Clip></VideoClip>
{records}"#,
        start = TICKS,
        end = 4 * TICKS,
        out = IN_TICKS + 3 * TICKS,
    );
    SOURCE
        .replace(
            r#"<Track ObjectURef="track-1"/>"#,
            r#"<Track ObjectURef="track-1"/><Track ObjectURef="track-2" Index="1"/>"#,
        )
        .replace("</PremiereData>", &format!("{track}</PremiereData>"))
}

/// The corpus form: default Motion and Opacity, both flags, Black Video media.
fn corpus_adjustment_xml(components: &[(u32, String)]) -> String {
    adjustment_xml(
        DEFAULT_FLAGS,
        components,
        CLIP_FLAG,
        MASTER_FLAG,
        BLACK_VIDEO_MEDIA,
    )
}

/// The kept occurrences of `xml` in track order, with their media kinds.
fn read(
    xml: &str,
) -> (
    PrProjectFile,
    Vec<PrVideoOccurrence>,
    Vec<PrMediaKind>,
    Vec<Omission>,
) {
    let (project, omissions) = inspect_project_with_omissions(xml, Some("sequence-1")).unwrap();
    let clips: Vec<_> = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .cloned()
        .collect();
    let kinds = clips
        .iter()
        .map(|clip| project.media(clip).unwrap().video.as_ref().unwrap().kind)
        .collect();
    (project, clips, kinds, omissions)
}

#[test]
fn a_flagged_black_video_clip_reads_as_an_adjustment_with_its_effects() {
    let (project, clips, kinds, omissions) = read(&corpus_adjustment_xml(&[(60, blur(60))]));
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        kinds,
        [
            PrMediaKind::Video {
                codec: None,
                hdr_profile: None
            },
            PrMediaKind::Adjustment
        ]
    );
    let adjustment = &clips[1];
    assert_eq!(adjustment.id.as_deref(), Some("VideoClipTrackItem:40"));
    assert_eq!(adjustment.timeline_ticks(), TICKS..4 * TICKS);
    assert_eq!(adjustment.source_ticks(), IN_TICKS..IN_TICKS + 3 * TICKS);
    assert_eq!(adjustment.opacity, 100.0);
    assert!(adjustment.animations.is_empty());
    assert_eq!(adjustment.effects.len(), 1);
    assert_eq!(
        adjustment.effects[0].params,
        PrEffectParams::GaussianBlur(PrGaussianBlur {
            blurriness: 25.0,
            repeat_edge_pixels: false,
        })
    );
    let media = project.media(adjustment).unwrap();
    assert_eq!(media.name(), "Adjustment Layer");
    assert!(media.is_adjustment() && media.is_generator() && !media.is_still());
    assert_eq!(
        media.video.as_ref().unwrap().intrinsic_ticks,
        10973491200000000
    );
}

#[test]
fn adjustment_wipe_does_not_feather_the_composite_outside_its_window() {
    let mut clip = crate::tests::support::clip_of("source", 0..TICKS, 0);
    clip.linear_wipe = Some(crate::schema::PrLinearWipe {
        initial_completion: 50.0,
        completion: Vec::new(),
        angle_degrees: 90,
        feather: 0.0,
    });
    assert!(crate::schema::adjustment::supports_wipe_coverage(&clip));
    clip.linear_wipe.as_mut().unwrap().feather = 1.0;
    assert!(!crate::schema::adjustment::supports_wipe_coverage(&clip));
}

#[test]
fn retained_adjustment_edits_are_opacity_and_its_keys_only() {
    // Every edit that `occurrence_edits` names, through the policy table.
    let properties: Vec<_> = {
        let mut every = crate::tests::support::clip_of("source", 0..TICKS, 0);
        every.linear_wipe = Some(crate::schema::PrLinearWipe {
            initial_completion: 50.0,
            completion: Vec::new(),
            angle_degrees: 90,
            feather: 0.0,
        });
        every.animations = vec![
            crate::schema::PrPropertyAnimation::Rotation(Vec::new()),
            crate::schema::PrPropertyAnimation::Opacity(Vec::new()),
        ];
        every.transform.position = [0.25, 0.5];
        every.transform.anchor_point = [0.5, 0.25];
        every.transform.scale = [150.0; 2];
        every.transform.rotation = 30.0;
        every.crop.left = 10.0;
        every.opacity = 50.0;
        every.playback_rate = 2.0;
        every.time_remap = Some(crate::schema::PrTimeRemap { keys: Vec::new() });
        every.edits()
    };
    assert_eq!(properties.len(), 11);
    let retained: Vec<_> = properties
        .iter()
        .filter(|edit| retains_edit(**edit))
        .map(|edit| edit.label())
        .collect();
    assert_eq!(retained, ["Opacity keyframes", "nondefault Opacity"]);
    assert!(!retains_edit(OccurrenceEdit::MotionKeys));
    assert_eq!(
        properties
            .iter()
            .filter(|edit| edit.kind() == EditKind::Clock)
            .count(),
        2,
        "clock edits omit an adjustment, unlike a still or matte"
    );
}

#[test]
fn adjustment_opacity_converts_and_other_edits_omit_the_occurrence() {
    let opacity_keys = format!(
        "{IN_TICKS},100.,0,0,0,0,0,0;{},20.,4,0,0,0,0,0;",
        IN_TICKS + TICKS
    );
    let rotation_keys = format!(
        "{IN_TICKS},0.,0,0,0,0,0,0;{},90.,0,0,0,0,0,0;",
        IN_TICKS + TICKS
    );
    let motion_flags = "<DefaultOpacity>true</DefaultOpacity>";
    let default_motion = "<DefaultMotion>true</DefaultMotion>";
    let (_, clips, kinds, omissions) = read(&adjustment_xml(
        default_motion,
        &[(50, opacity("100.", &opacity_keys)), (60, blur(60))],
        CLIP_FLAG,
        MASTER_FLAG,
        BLACK_VIDEO_MEDIA,
    ));
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(kinds[1], PrMediaKind::Adjustment);
    assert_eq!(clips[1].animations.len(), 1);
    assert_eq!(
        clips[1].animations[0].property(),
        PrAnimatedProperty::Opacity
    );
    assert_eq!(clips[1].effects.len(), 1);

    let (_, clips, _, omissions) = read(&adjustment_xml(
        default_motion,
        &[(50, opacity("50.", ""))],
        CLIP_FLAG,
        MASTER_FLAG,
        BLACK_VIDEO_MEDIA,
    ));
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(clips[1].opacity, 50.0);

    let context = "track 1, range 254016000000..1016064000000 ticks";
    for (case, xml, reason) in [
        (
            "Motion keys",
            adjustment_xml(
                motion_flags,
                &[(70, motion("100.", &rotation_keys))],
                CLIP_FLAG,
                MASTER_FLAG,
                BLACK_VIDEO_MEDIA,
            ),
            "Motion keyframes on an adjustment layer is unsupported",
        ),
        (
            "static Rotation",
            adjustment_xml(
                motion_flags,
                &[(
                    70,
                    motion("100.", "").replace(
                        "<ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,0.",
                        "<ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,30.",
                    ),
                )],
                CLIP_FLAG,
                MASTER_FLAG,
                BLACK_VIDEO_MEDIA,
            ),
            "nondefault Motion Rotation on an adjustment layer is unsupported",
        ),
        (
            "playback rate",
            corpus_adjustment_xml(&[])
                .replace(
                    "<ClipID>placed-adjustment</ClipID>",
                    "<ClipID>placed-adjustment</ClipID><PlaybackSpeed>2</PlaybackSpeed>",
                )
                .replace(
                    &format!("<OutPoint>{}</OutPoint>", IN_TICKS + 3 * TICKS),
                    &format!("<OutPoint>{}</OutPoint>", IN_TICKS + 6 * TICKS),
                ),
            "playback rate on an adjustment layer is unsupported",
        ),
    ] {
        let (_, clips, _, omissions) = read(&xml);
        assert_eq!(clips.len(), 1, "{case}: {omissions:?}");
        assert_eq!(
            omissions,
            [Omission {
                scope: OmissionScope::Occurrence,
                kind: OmissionKind::Omitted,
                record: "VideoClipTrackItem:40".into(),
                reason: format!("{context}: {reason}; occurrence omitted"),
            }],
            "{case}"
        );
    }
}

#[test]
fn adjustment_flags_and_media_that_disagree_with_the_corpus_form_fail_closed() {
    const GRAPHIC: &str = "1196574294";
    let file_media = r#"<Media ObjectUID="adjustment-media"><VideoStream ObjectRef="45"/><RelativePath>media/black.mp4</RelativePath></Media>
<VideoStream ObjectID="45"><Duration>10973491200000000</Duration><FrameRate>8467200000</FrameRate><FrameRect>0,0,1920,1080</FrameRect></VideoStream>"#;
    for (case, xml, reason) in [
        (
            "flag on file media",
            adjustment_xml(DEFAULT_FLAGS, &[], CLIP_FLAG, MASTER_FLAG, file_media),
            "Media:adjustment-media: an AdjustmentLayer clip must play Black Video generator media",
        ),
        (
            "flag on the video clip's own media",
            adjustment_xml(DEFAULT_FLAGS, &[], CLIP_FLAG, MASTER_FLAG, "")
                .replace(
                    r#"<Media ObjectURef="adjustment-media"/>"#,
                    r#"<Media ObjectURef="media-1"/>"#,
                )
                .replace(
                    "<OriginalDuration>10973491200000000</OriginalDuration>",
                    "<OriginalDuration>2540160000000</OriginalDuration>",
                ),
            "VideoMediaSource:44: media placed both by an AdjustmentLayer clip and by another clip",
        ),
        (
            "flag on another generator",
            adjustment_xml(
                DEFAULT_FLAGS,
                &[],
                CLIP_FLAG,
                MASTER_FLAG,
                &BLACK_VIDEO_MEDIA.replace("1112293707", GRAPHIC),
            ),
            "Media:adjustment-media: an AdjustmentLayer clip must play Black Video generator media",
        ),
        (
            "clip flag without the master flag",
            adjustment_xml(DEFAULT_FLAGS, &[], CLIP_FLAG, "", BLACK_VIDEO_MEDIA),
            "MasterClip:adjustment-master: IsAdjustmentLayer disagrees with the placed clip's AdjustmentLayer flag",
        ),
        (
            "clip flag without a master reference",
            adjustment_xml(DEFAULT_FLAGS, &[], CLIP_FLAG, MASTER_FLAG, BLACK_VIDEO_MEDIA)
                .replace(r#"<MasterClip ObjectURef="adjustment-master"/>"#, ""),
            "SubClip:42: an AdjustmentLayer clip requires its MasterClip",
        ),
        (
            "clip flag with a dangling master reference",
            adjustment_xml(DEFAULT_FLAGS, &[], CLIP_FLAG, MASTER_FLAG, BLACK_VIDEO_MEDIA)
                .replace(
                    r#"<MasterClip ObjectURef="adjustment-master"/>"#,
                    r#"<MasterClip ObjectURef="missing-master"/>"#,
                ),
            "missing reference at MasterClip",
        ),
        (
            "clip flag false under the master flag",
            adjustment_xml(
                DEFAULT_FLAGS,
                &[],
                "<AdjustmentLayer>false</AdjustmentLayer>",
                MASTER_FLAG,
                BLACK_VIDEO_MEDIA,
            ),
            "MasterClip:adjustment-master: IsAdjustmentLayer disagrees with the placed clip's AdjustmentLayer flag",
        ),
        (
            "master flag without the clip flag",
            adjustment_xml(DEFAULT_FLAGS, &[], "", MASTER_FLAG, BLACK_VIDEO_MEDIA),
            "MasterClip:adjustment-master: IsAdjustmentLayer disagrees with the placed clip's AdjustmentLayer flag",
        ),
        (
            "invalid flag",
            adjustment_xml(
                DEFAULT_FLAGS,
                &[],
                "<AdjustmentLayer>yes</AdjustmentLayer>",
                MASTER_FLAG,
                BLACK_VIDEO_MEDIA,
            ),
            "VideoClip:43: invalid AdjustmentLayer",
        ),
        (
            "smaller frame",
            adjustment_xml(
                DEFAULT_FLAGS,
                &[],
                CLIP_FLAG,
                MASTER_FLAG,
                &BLACK_VIDEO_MEDIA.replace(
                    "<FrameRect>0,0,1920,1080</FrameRect>",
                    "<FrameRect>0,0,1280,720</FrameRect>",
                ),
            ),
            "VideoClipTrackItem:40: an adjustment layer of 1280x720 on a 1920x1080 sequence is not converted",
        ),
        (
            "frame off the origin",
            adjustment_xml(
                DEFAULT_FLAGS,
                &[],
                CLIP_FLAG,
                MASTER_FLAG,
                &BLACK_VIDEO_MEDIA.replace(
                    "<FrameRect>0,0,1920,1080</FrameRect>",
                    "<FrameRect>0,8,1920,1088</FrameRect>",
                ),
            ),
            "VideoStream:45: invalid FrameRect",
        ),
        (
            "non-square pixels",
            adjustment_xml(
                DEFAULT_FLAGS,
                &[],
                CLIP_FLAG,
                MASTER_FLAG,
                &BLACK_VIDEO_MEDIA.replace(
                    "<FrameRect>0,0,1920,1080</FrameRect>",
                    "<FrameRect>0,0,1920,1080</FrameRect><PixelAspectRatio>2,1</PixelAspectRatio>",
                ),
            ),
            "VideoStream:45: non-square source pixels",
        ),
        (
            "video stream",
            adjustment_xml(
                DEFAULT_FLAGS,
                &[],
                CLIP_FLAG,
                MASTER_FLAG,
                &BLACK_VIDEO_MEDIA.replace("<IsStill>true</IsStill>", ""),
            ),
            "VideoStream:45: adjustment layer media must be an IsStill stream",
        ),
    ] {
        let (_, clips, _, omissions) = read(&xml);
        assert_eq!(clips.len(), 1, "{case}: {omissions:?}");
        assert_eq!(omissions.len(), 1, "{case}: {omissions:?}");
        assert_eq!(omissions[0].scope, OmissionScope::Occurrence, "{case}");
        assert_eq!(omissions[0].record, "40", "{case}");
        assert!(
            omissions[0].reason.contains(reason),
            "{case}: {}",
            omissions[0].reason
        );
    }
}

#[test]
fn a_sequence_whose_only_kept_items_are_adjustments_has_no_convertible_content() {
    // The video clip on V1 is omitted (empty source range); the effect-less
    // adjustment reads, but a document of adjustments over the canvas alone is
    // not a conversion.
    let xml = corpus_adjustment_xml(&[(60, blur(60))]).replace(
        "<InPoint>0</InPoint><OutPoint>1270080000000</OutPoint>",
        "<InPoint>0</InPoint><OutPoint>0</OutPoint>",
    );
    let mut omissions = Vec::new();
    let error = crate::format::reader::read_sequence(
        &crate::format::Graph::parse(&xml).unwrap(),
        Some("sequence-1"),
        &std::collections::BTreeSet::new(),
        &mut std::collections::BTreeMap::new(),
        &mut omissions,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("no convertible video or audio occurrences"),
        "{error}"
    );
    let records: Vec<_> = omissions
        .iter()
        .map(|omission| (omission.scope, omission.record.as_str()))
        .collect();
    assert_eq!(
        records,
        [(OmissionScope::Occurrence, "3")],
        "the adjustment itself reads: {omissions:?}"
    );
}

#[test]
fn an_adjustment_imports_as_an_fx_adjustment_layer_with_its_blur() {
    let blur = blur(60).replace(
        "<StartKeyframe>-91445760000000000,false,0,0,0,0,0,0</StartKeyframe>",
        "<StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe>",
    );
    let xml = corpus_adjustment_xml(&[(60, blur)]);
    assert_eq!(
        xml.matches("<FrameRect>0,0,1920,1080</FrameRect>").count(),
        5
    );
    // On a portrait canvas every frame, the sequence's, both placements' and
    // both streams', is portrait, so the adjustment still covers the canvas.
    for [width, height] in [[1920, 1080], [1080, 1920]] {
        let xml = xml.replace(
            "<FrameRect>0,0,1920,1080</FrameRect>",
            &format!("<FrameRect>0,0,{width},{height}</FrameRect>"),
        );
        let (project, omissions) =
            inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let document =
            project_document_with_media(project.single_sequence().unwrap(), &project.media);
        assert_eq!(
            document["dimensions"],
            json!({"width": width, "height": height})
        );
        let layers = document["composition"]["layers"].as_array().unwrap();
        assert_eq!(
            layers
                .iter()
                .map(|layer| layer["type"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["Adjustment", "Video", "Rect"]
        );
        let adjustment = &layers[0];
        assert_eq!(adjustment["name"], "Premiere adjustment 2");
        assert_eq!(
            adjustment["activeRange"],
            json!({"start": 1000, "duration": 3000})
        );
        assert_eq!(adjustment["effects"][0]["effect"]["type"], "gaussianBlur");
        assert_eq!(adjustment["effects"][0]["effect"]["blurriness"], 25.0);
        assert_eq!(adjustment["transform"]["opacity"], 100.0);
        assert_eq!(adjustment["transform"]["scale"], json!([100.0, 100.0]));
    }
}

#[test]
fn an_adjustment_imports_the_20_parameter_film_impact_ramp_as_editable_blurriness_keys() {
    // Synthetic ramp: the Amount keys at the adjustment's source In and In +
    // 1 s fall at layer times 0 and 1000 ms, Amount 0 and 2 at 5.7 Blurriness
    // per Amount.
    let xml = corpus_adjustment_xml(&[(128, film_impact_20_parameter_fragment(IN_TICKS))]);
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let document = project_document_with_media(project.single_sequence().unwrap(), &project.media);
    let adjustment = &document["composition"]["layers"][0];
    assert_eq!(adjustment["type"], "Adjustment");
    assert_eq!(
        adjustment["effects"][0]["effect"],
        json!({"type": "gaussianBlur", "blurriness": 0.0, "repeatEdgePixels": true})
    );
    let [entry] = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .as_slice()
    else {
        panic!(
            "one animated property: {}",
            document["composition"]["dynamics"]
        );
    };
    let keys: Vec<_> = entry["animator"]["keyframes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| {
            (
                key["layerTime"].as_i64().unwrap(),
                key["value"]["value"].as_f64().unwrap(),
                key["easing"]["type"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        keys,
        [
            (0, 0.0, "linear".to_owned()),
            (1000, 11.4, "linear".to_owned())
        ]
    );
}

#[test]
fn adjustment_stream_accepts_only_the_measured_missing_rate_override() {
    let explicit = "<FrameRate>8467200000</FrameRate>";
    let overridden = "<IsFrameRateOverridden>true</IsFrameRateOverridden><OveriddenFrameRate>8467200000</OveriddenFrameRate>";
    for (rate, expected) in [
        (overridden.to_owned(), FrameRate::Fps30),
        (
            format!("<FrameRate>8475667200</FrameRate>{overridden}"),
            FrameRate::Fps30000Over1001,
        ),
    ] {
        let (project, clips, _, omissions) = read(&adjustment_xml(
            DEFAULT_FLAGS,
            &[],
            CLIP_FLAG,
            MASTER_FLAG,
            &BLACK_VIDEO_MEDIA.replace(explicit, &rate),
        ));
        assert_eq!(clips.len(), 2, "{omissions:?}");
        assert!(omissions.is_empty(), "{omissions:?}");
        assert_eq!(
            project
                .media(&clips[1])
                .unwrap()
                .video
                .as_ref()
                .unwrap()
                .frame_rate,
            expected.into()
        );
    }
    for rate in [
        "",
        "<OveriddenFrameRate>8467200000</OveriddenFrameRate>",
        "<IsFrameRateOverridden>true</IsFrameRateOverridden>",
        "<IsFrameRateOverridden>false</IsFrameRateOverridden><OveriddenFrameRate>8467200000</OveriddenFrameRate>",
        "<IsFrameRateOverridden>yes</IsFrameRateOverridden><OveriddenFrameRate>8467200000</OveriddenFrameRate>",
        "<IsFrameRateOverridden>true</IsFrameRateOverridden><OveriddenFrameRate>8467200000.0</OveriddenFrameRate>",
        "<IsFrameRateOverridden>true</IsFrameRateOverridden><OveriddenFrameRate>10160640000</OveriddenFrameRate>",
        "<IsFrameRateOverridden>true</IsFrameRateOverridden><OveriddenFrameRate>-1</OveriddenFrameRate>",
    ] {
        let (_, clips, _, omissions) = read(&adjustment_xml(DEFAULT_FLAGS, &[], CLIP_FLAG, MASTER_FLAG, &BLACK_VIDEO_MEDIA.replace(explicit, rate)));
        assert_eq!(clips.len(), 1, "{rate}: {omissions:?}");
        assert!(omissions.iter().any(|omission| omission.scope == OmissionScope::Occurrence
            && (omission.reason.contains("FrameRate") || omission.reason.contains("frame rate") || omission.reason.contains("override"))), "{rate}: {omissions:?}");
    }
}

#[test]
fn adjustment_motion_coverage_keeps_only_static_axis_aligned_controls() {
    let scaled = motion("50.", "");
    let flags = "<DefaultOpacity>true</DefaultOpacity>";
    let moved = motion("100.", "").replace(
        "<ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,0.5:0.5",
        "<ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,0.6:0.55",
    );
    for motion in [scaled.clone(), moved] {
        let (_, clips, _, omissions) = read(&adjustment_xml(
            flags,
            &[(70, motion)],
            CLIP_FLAG,
            MASTER_FLAG,
            BLACK_VIDEO_MEDIA,
        ));
        assert_eq!(clips.len(), 2, "{omissions:?}");
        assert!(omissions.is_empty(), "{omissions:?}");
    }
    for (name, changed, opacity_component) in [
        ("zero Scale", motion("0.", ""), None),
        ("negative Scale", motion("-50.", ""), None),
        (
            "Rotation",
            scaled.replace(
                "<ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,0.",
                "<ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,30.",
            ),
            None,
        ),
        (
            "Anchor",
            scaled.replace(
                "<ParameterID>6</ParameterID><StartKeyframe>-91445760000000000,0.5:0.5",
                "<ParameterID>6</ParameterID><StartKeyframe>-91445760000000000,0.2:0.5",
            ),
            None,
        ),
        (
            "nonuniform Scale",
            scaled
                .replace(
                    "<ParameterID>4</ParameterID><StartKeyframe>-91445760000000000,true",
                    "<ParameterID>4</ParameterID><StartKeyframe>-91445760000000000,false",
                )
                .replace(
                    "<ParameterID>3</ParameterID><StartKeyframe>-91445760000000000,50.",
                    "<ParameterID>3</ParameterID><StartKeyframe>-91445760000000000,75.",
                ),
            None,
        ),
        ("Opacity", scaled.clone(), Some(opacity("50.", ""))),
        (
            "blend",
            scaled.clone(),
            Some(opacity("100.", "").replace("-91445760000000000,18,", "-91445760000000000,22,")),
        ),
    ] {
        let mut components = vec![(70, changed)];
        if let Some(opacity) = opacity_component {
            components.push((50, opacity));
        }
        let flags = if components.len() == 1 { flags } else { "" };
        let (_, clips, _, omissions) = read(&adjustment_xml(
            flags,
            &components,
            CLIP_FLAG,
            MASTER_FLAG,
            BLACK_VIDEO_MEDIA,
        ));
        assert_eq!(clips.len(), 1, "{name}: {omissions:?}");
        assert!(
            omissions
                .iter()
                .any(|omission| omission.scope == OmissionScope::Occurrence),
            "{name}: {omissions:?}"
        );
    }
}

#[test]
fn adjustment_coverage_checks_active_native_effects_before_they_can_be_omitted() {
    use super::effects::{invert, top_crop};
    let supported = invert(90);
    for (name, effect, keep) in [
        ("RGB Invert", supported.clone(), true),
        ("bypassed blur", blur(90).replace("<Bypass>false</Bypass>", "<Bypass>true</Bypass>"), true),
        ("blur", blur(90), false),
        ("Crop", top_crop(90), false),
        ("non-RGB Invert", supported.replace("-91445760000000000,0,", "-91445760000000000,1,"), false),
        ("mixed Invert", invert(90).replace("-91445760000000000,0.,", "-91445760000000000,25.,"), false),
        ("keyed Invert", invert(90).replace("<Name>Blend With Original</Name><IsTimeVarying>false</IsTimeVarying>", "<Name>Blend With Original</Name><IsTimeVarying>true</IsTimeVarying>").replace("<ParameterID>2</ParameterID>", &format!("<Keyframes>{IN_TICKS},0.,0,0,0,0,0,0;{},100.,0,0,0,0,0,0;</Keyframes><ParameterID>2</ParameterID>", IN_TICKS + TICKS)), false),
        ("unknown active effect", invert(90).replace("AE.ADBE Invert", "AE.Unknown"), false),
        ("invalid bypass", invert(90).replace("<Bypass>false</Bypass>", "<Bypass>maybe</Bypass>"), false),
    ] {
        let (_, clips, _, omissions) = read(&adjustment_xml("<DefaultOpacity>true</DefaultOpacity>", &[(70, motion("50.", "")), (90, effect)], CLIP_FLAG, MASTER_FLAG, BLACK_VIDEO_MEDIA));
        assert_eq!(clips.len(), if keep {2} else {1}, "{name}: {omissions:?}");
        assert_eq!(omissions.iter().any(|omission| omission.scope == OmissionScope::Occurrence), !keep, "{name}: {omissions:?}");
    }
}

#[test]
fn an_unmoved_adjustment_saved_at_a_moved_point_keeps_its_effects() {
    use crate::schema::PrStaticTransform;
    // Motion at Scale 100 with Position and Anchor Point at one point off the
    // centre, apart only by single-precision noise, as Premiere saves it.
    let at = |position: &str, anchor: &str| {
        motion("100.", "")
            .replace(
                "<ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,0.5:0.5",
                &format!(
                    "<ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,{position}"
                ),
            )
            .replace(
                "<ParameterID>6</ParameterID><StartKeyframe>-91445760000000000,0.5:0.5",
                &format!("<ParameterID>6</ParameterID><StartKeyframe>-91445760000000000,{anchor}"),
            )
    };
    let anchor = "0.50094517866770427:0.34285714891221786";
    let unmoved = at("0.50094517958412099:0.34285714285714303", anchor);
    let read_with = |motion: String| {
        read(&adjustment_xml(
            "<DefaultOpacity>true</DefaultOpacity>",
            &[(70, motion), (60, blur(60))],
            CLIP_FLAG,
            MASTER_FLAG,
            BLACK_VIDEO_MEDIA,
        ))
    };
    let (_, clips, kinds, omissions) = read_with(unmoved.clone());
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(kinds[1], PrMediaKind::Adjustment);
    assert_eq!(clips[1].transform, PrStaticTransform::default());
    assert!(matches!(
        clips[1].effects.as_slice(),
        [effect] if matches!(effect.params, PrEffectParams::GaussianBlur(_))
    ));
    // A real move of 1.8 px, a larger Scale or a Rotation still moves the
    // coverage, which is unmeasured with a blur.
    for (name, motion) in [
        ("moved", at("0.5:0.34259259700775146", anchor)),
        (
            "scaled",
            unmoved.replace(
                "<ParameterID>2</ParameterID><StartKeyframe>-91445760000000000,100.",
                "<ParameterID>2</ParameterID><StartKeyframe>-91445760000000000,101.",
            ),
        ),
        (
            "rotated",
            unmoved.replace(
                "<ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,0.",
                "<ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,1.",
            ),
        ),
    ] {
        let (_, clips, _, omissions) = read_with(motion);
        assert_eq!(clips.len(), 1, "{name}: {omissions:?}");
        assert!(
            omissions
                .iter()
                .any(|omission| omission.scope == OmissionScope::Occurrence
                    && omission.reason.contains("nondefault adjustment Motion")),
            "{name}: {omissions:?}"
        );
    }
}

#[test]
fn an_unmoved_adjustment_with_a_motion_crop_is_still_omitted_for_its_crop() {
    // Premiere 26.5.1 Motion (clip S1's records, Position 0.625:0.65) with its
    // Anchor Point on that Position: an adjustment saved unmoved at a moved
    // point keeps its blur under the default Motion. Its Motion Crop is no
    // part of that Motion: a Crop, which no adjustment converts, still omits
    // the layer.
    let unmoved = |crop_left: &str| {
        let motion = super::effects::motion_26_5(crop_left);
        let (head, anchor) = motion.split_at(motion.find("<Name>Anchor Point</Name>").unwrap());
        let edited =
            head.to_owned() + &anchor.replacen(",0.5:0.5,", ",0.625:0.65000000000000002,", 1);
        assert_ne!(edited, motion);
        edited
    };
    let read_with = |motion: String| {
        read(&adjustment_xml(
            "<DefaultOpacity>true</DefaultOpacity>",
            &[(199, motion), (60, blur(60))],
            CLIP_FLAG,
            MASTER_FLAG,
            BLACK_VIDEO_MEDIA,
        ))
    };
    let (_, clips, kinds, omissions) = read_with(unmoved("0."));
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(kinds[1], PrMediaKind::Adjustment);
    assert_eq!(
        clips[1].transform,
        crate::schema::PrStaticTransform::default()
    );
    let (_, clips, _, omissions) = read_with(unmoved("10."));
    assert_eq!(clips.len(), 1, "{omissions:?}");
    let omitted: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.scope == OmissionScope::Occurrence)
        .map(|omission| omission.reason.as_str())
        .collect();
    assert_eq!(
        omitted,
        ["track 1, range 254016000000..1016064000000 ticks: nondefault Crop on an adjustment layer is unsupported; occurrence omitted"]
    );
}

#[test]
fn adjustment_coverage_rejects_native_opacity_masks_and_opacity_keys() {
    let masked = format!(
        "{}{}",
        super::mask::masked_opacity(50, 100)
            .replace("-91445760000000000,50.,", "-91445760000000000,100.,"),
        super::mask::mask(100, true)
    );
    let keys = format!(
        "{IN_TICKS},100.,0,0,0,0,0,0;{},50.,0,0,0,0,0,0;",
        IN_TICKS + TICKS
    );
    for (name, component) in [
        ("native mask", masked),
        ("Opacity keys", opacity("100.", &keys)),
    ] {
        let (_, clips, _, omissions) = read(&adjustment_xml(
            "",
            &[(70, motion("50.", "")), (50, component)],
            CLIP_FLAG,
            MASTER_FLAG,
            BLACK_VIDEO_MEDIA,
        ));
        assert_eq!(clips.len(), 1, "{name}: {omissions:?}");
        assert!(
            omissions
                .iter()
                .any(|omission| omission.scope == OmissionScope::Occurrence),
            "{name}: {omissions:?}"
        );
    }
}
