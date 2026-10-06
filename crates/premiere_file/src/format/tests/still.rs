//! Native still-image records: reader acceptance, rejection, and writer output.

use super::{
    animation::animation_fixture::with_matte_track,
    effects::track_matte_key,
    nested::{placement_records, with_records, Placement},
};
use crate::format::{inspect_project, inspect_project_with_media, writer::project_xml, FrameRate};
use crate::schema::{
    records::MediaPathField, MediaId, PrMedia, PrMediaKind, PrProjectFile, PrSequence,
    PrVideoOccurrence, PrVideoTrack, STILL_INTRINSIC_TICKS, TICKS,
};

/// A still placement's source in-point on the 30 fps test sequences.
const STILL_SOURCE_IN_TICKS: i64 = FrameRate::Fps30.generator_in_ticks();

const SOURCE: &str = include_str!("../../../tests/fixtures/one-clip.xml");

/// The one-clip fixture as a Premiere still: `IsStill`, the twelve-hour
/// synthetic duration, a one-hour placed in-point, and a master clip that
/// keeps its own 0–5 s range (the `corporate_slideshow` shape).
pub(in crate::format) fn still_xml(alpha: bool) -> String {
    let alpha_type = if alpha {
        "<AlphaType>1</AlphaType>"
    } else {
        ""
    };
    SOURCE
        .replace(
            "<VideoStream ObjectID=\"8\"><Duration>2540160000000</Duration><FrameRate>8467200000</FrameRate><FrameRect>0,0,1920,1080</FrameRect></VideoStream>",
            &format!("<VideoStream ObjectID=\"8\"><IsStill>true</IsStill><FrameRate>8467200000</FrameRate><FrameRect>0,0,1920,1080</FrameRect><PARIsUncertain>true</PARIsUncertain><Duration>{STILL_INTRINSIC_TICKS}</Duration>{alpha_type}<CodecType>1380013856</CodecType></VideoStream>"),
        )
        .replace(
            "<RelativePath>media/source.mp4</RelativePath></Media>",
            "<RelativePath>media/source.png</RelativePath><Infinite>true</Infinite></Media>",
        )
        .replace(
            "<OriginalDuration>2540160000000</OriginalDuration>",
            &format!("<OriginalDuration>{STILL_INTRINSIC_TICKS}</OriginalDuration>"),
        )
        .replace(
            "<InPoint>0</InPoint><OutPoint>1270080000000</OutPoint>",
            &format!(
                "<InPoint>{STILL_SOURCE_IN_TICKS}</InPoint><OutPoint>{}</OutPoint>",
                STILL_SOURCE_IN_TICKS + 5 * TICKS
            ),
        )
        .replace(
            "<SubClip ObjectID=\"5\"><Clip ObjectRef=\"6\"/>",
            "<SubClip ObjectID=\"5\"><Clip ObjectRef=\"6\"/><MasterClip ObjectURef=\"master-still\"/>",
        )
        .replace(
            "</PremiereData>",
            &format!(
                r#"<MasterClip ObjectUID="master-still"><Clips><Clip ObjectRef="9"/></Clips><Name>source.png</Name></MasterClip>
  <VideoClip ObjectID="9"><Clip><Source ObjectRef="7"/><ClipID>master-still-clip</ClipID><InPoint>0</InPoint><OutPoint>{}</OutPoint><InUse>false</InUse></Clip></VideoClip>
</PremiereData>"#,
                5 * TICKS
            ),
        )
}

#[test]
fn still_media_keeps_its_placement_and_synthetic_source_clock() {
    // Premiere writes IsOverridenImageOrientationType=true on many stills; it is accepted.
    for (alpha, overridden) in [(false, false), (true, true)] {
        let mut xml = still_xml(alpha);
        if overridden {
            xml = xml.replace(
                "<IsStill>true</IsStill>",
                "<IsStill>true</IsStill><IsOverridenImageOrientationType>true</IsOverridenImageOrientationType>",
            );
        }
        let project = inspect_project_with_media(&xml, None).unwrap();
        let sequence = project.single_sequence().unwrap();
        let clip = sequence.video_occurrences().next().unwrap();
        assert_eq!(clip.timeline_ticks(), 0..5 * TICKS);
        assert_eq!(
            clip.source_ticks(),
            STILL_SOURCE_IN_TICKS..STILL_SOURCE_IN_TICKS + 5 * TICKS
        );
        let media = project.media(clip).unwrap();
        assert_eq!(
            media.video.as_ref().unwrap().kind,
            PrMediaKind::Still { alpha }
        );
        assert!(media.is_still());
        assert_eq!(
            media.video.as_ref().unwrap().intrinsic_ticks,
            STILL_INTRINSIC_TICKS
        );
        assert_eq!(
            (
                media.video.as_ref().unwrap().width,
                media.video.as_ref().unwrap().height
            ),
            (1920, 1080)
        );
        assert_eq!(media.name(), "source.png");
    }
}

#[test]
fn native_openexr_codec_and_black_matte_alpha_are_preserved() {
    for alpha in [false, true] {
        let mut xml = still_xml(alpha)
            .replace("source.png", "source.exr")
            .replace(
                "<CodecType>1380013856</CodecType>",
                "<CodecType>1281443650</CodecType>",
            );
        if alpha {
            xml = xml.replace("<AlphaType>1</AlphaType>", "<AlphaType>2</AlphaType>");
        }
        let project = inspect_project_with_media(&xml, None).unwrap();
        let clip = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .next()
            .unwrap();
        let media = project.media(clip).unwrap();
        assert_eq!(
            media.video.as_ref().unwrap().kind,
            PrMediaKind::OpenExr {
                alpha,
                numbered: false,
                channels: crate::schema::OpenExrChannels::Unspecified,
            }
        );
        assert!(media.is_still());
        assert_eq!(media.name(), "source.exr");
    }

    let wrong_alpha = still_xml(true).replace("source.png", "source.exr").replace(
        "<CodecType>1380013856</CodecType>",
        "<CodecType>1281443650</CodecType>",
    );
    let error = inspect_project_with_media(&wrong_alpha, None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("AlphaType \"1\" is unsupported"), "{error}");
}

#[test]
fn still_frame_rate_follows_the_still_preference_not_the_sequence_cadence() {
    // A 25 fps still preference (phone_title corpus) on a 30 fps sequence.
    let xml = still_xml(false).replace(
        "<IsStill>true</IsStill><FrameRate>8467200000</FrameRate>",
        "<IsStill>true</IsStill><FrameRate>10160640000</FrameRate>",
    );
    let project = inspect_project_with_media(&xml, None).unwrap();
    let clip = project
        .single_sequence()
        .unwrap()
        .video_occurrences()
        .next()
        .unwrap();
    assert_eq!(clip.timeline_ticks(), 0..5 * TICKS);
    assert_eq!(
        project
            .media(clip)
            .unwrap()
            .video
            .as_ref()
            .unwrap()
            .frame_rate,
        FrameRate::Fps25.into()
    );
}

#[test]
fn still_record_variants_reject_precisely() {
    // Corpus generator media: a decimal four-character code in FilePath and no RelativePath.
    let generator = |file_path: &str| {
        still_xml(false).replace(
            "<RelativePath>media/source.png</RelativePath>",
            &format!("<FilePath>{file_path}</FilePath>"),
        )
    };
    // The placed clip's `VideoClip`, where the six corpus clips with Scale to
    // Frame Size on keep it; the still's master clip follows.
    let scale_to_frame = |xml: &str, policy: &str| {
        xml.replacen(
            "</Clip></VideoClip>",
            &format!("</Clip><ScaleToFramePolicy>{policy}</ScaleToFramePolicy></VideoClip>"),
            1,
        )
    };
    let cases = [
        (
            still_xml(true).replace("<AlphaType>1</AlphaType>", "<AlphaType>2</AlphaType>"),
            "AlphaType \"2\" is unsupported",
        ),
        (
            still_xml(true).replace(
                "<AlphaType>1</AlphaType>",
                "<AlphaType>1</AlphaType><IgnoreAlpha>true</IgnoreAlpha>",
            ),
            "IgnoreAlpha would flatten",
        ),
        (
            still_xml(false).replace("<IsStill>true</IsStill>", "<IsStill>false</IsStill>"),
            "unsupported IsStill value",
        ),
        (
            still_xml(false).replace("<Infinite>true</Infinite>", "<Infinite>false</Infinite>"),
            "still media must be Infinite",
        ),
        (
            still_xml(false).replace(
                "<IsStill>true</IsStill>",
                "<IsStill>true</IsStill><IsOverridenImageOrientationType>yes</IsOverridenImageOrientationType>",
            ),
            "unsupported IsOverridenImageOrientationType value \"yes\"",
        ),
        (generator("1129270354"), "synthetic still media COLR "),
        (generator("1112293707"), "synthetic still media BLAK "),
        (generator("1414091852"), "synthetic still media TITL "),
        (generator("1196574294"), "synthetic still media GRFV "),
        (generator("1414680150"), "synthetic still media TRNV "),
        (generator("7"), "synthetic still media 7 "),
        (
            scale_to_frame(&still_xml(false), "1"),
            "VideoClip:6: Scale to Frame Size on a still is not converted",
        ),
        (
            scale_to_frame(SOURCE, "1"),
            "VideoClip:6: Scale to Frame Size on a video clip is not converted",
        ),
        (
            scale_to_frame(&still_xml(false), "2"),
            "VideoClip:6: unknown Scale to Frame Size policy \"2\"",
        ),
    ];
    for (xml, expected) in cases {
        // The only occurrence is omitted, so the sequence itself cannot convert.
        let error = inspect_project(&xml, None).unwrap_err().to_string();
        assert!(error.contains(expected), "expected {expected}: {error}");
    }
}

#[test]
fn legacy_clip_settings_map_or_reject_like_their_modern_fields() {
    // Premiere CS6 to CC 2015 clip settings, in the corpus element order.
    let legacy = |settings: &str| {
        SOURCE.replacen(
            "</Clip></VideoClip>",
            &format!("</Clip><PosterFrame>0</PosterFrame>{settings}</VideoClip>"),
            1,
        )
    };
    const INERT: &str = "<HoldFilters>false</HoldFilters><DeinterlaceOnHold>false</DeinterlaceOnHold><ReverseFieldDominance>false</ReverseFieldDominance><FieldProcessing>0</FieldProcessing>";

    // Explicit off states: policy 0, FrameHold mode 0 with the placeholder
    // FrameHoldStart that CC 2015 writes, and FrameBlend false.
    let frame_blend = |value: &str| {
        legacy(&format!(
            "{INERT}<FrameBlend>{value}</FrameBlend><ScaleToFramePolicy>0</ScaleToFramePolicy><FrameHold>0</FrameHold><FrameHoldStart>-10160000000000</FrameHoldStart>"
        ))
    };
    for (value, expected) in [
        ("false", None),
        ("true", Some(fx_schema::FrameBlendingMode::Simple)),
    ] {
        let parsed = inspect_project(&frame_blend(value), None).unwrap();
        let clip = parsed.video_occurrences().next().unwrap();
        assert_eq!(clip.frame_blending, expected, "FrameBlend {value}");
        assert!(clip.time_remap.is_none(), "FrameBlend {value}");
    }
    // Legacy media-source content: unset boundaries (one shared sentinel)
    // convert; a real boundary would trim the media and rejects.
    const SENTINEL: &str = "-101606400000000000";
    let content = |start: &str, end: &str| {
        SOURCE.replacen(
            "<MediaSource><Media",
            &format!(
                "<MediaSource Version=\"3\"><Content Version=\"9\"><StartBoundary>{start}</StartBoundary><EndBoundary>{end}</EndBoundary><BoundariesAreHard>true</BoundariesAreHard><ProxyEnabled>false</ProxyEnabled><Node Version=\"1\"></Node></Content><Media"
            ),
            1,
        )
    };
    assert!(inspect_project(&content(SENTINEL, SENTINEL), None).is_ok());
    // A finite pair is a real range even when it is empty.
    for (start, end) in [("0", "254016000000"), ("0", "0"), (SENTINEL, "0")] {
        let error = inspect_project(&content(start, end), None)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("VideoMediaSource:7: media content boundaries are not converted"),
            "{start}..{end}: {error}"
        );
    }
    for (settings, expected) in [
        (
            format!("{INERT}<ScaleToFrameSize>true</ScaleToFrameSize>"),
            "VideoClip:6: Scale to Frame Size on a video clip is not converted",
        ),
        (
            format!("{INERT}<ScaleToFrameSize>maybe</ScaleToFrameSize>"),
            "VideoClip:6: invalid ScaleToFrameSize \"maybe\"",
        ),
        (
            INERT.replace("<HoldFilters>false", "<HoldFilters>true"),
            "VideoClip:6: HoldFilters \"true\" is not converted",
        ),
        (
            INERT.replace("<FieldProcessing>0", "<FieldProcessing>1"),
            "VideoClip:6: FieldProcessing \"1\" is not converted",
        ),
        (
            format!("{INERT}<FrameBlend>1</FrameBlend>"),
            "VideoClip:6: invalid FrameBlend \"1\"",
        ),
    ] {
        let error = inspect_project(&legacy(&settings), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "expected {expected}: {error}");
    }
}

/// The `NYC Skyline-Pano.jpg` records of the corpus `stills_and_panorama`
/// "NYC Panorama" sequence (29.97 fps), verbatim apart from indentation: the
/// 4652x1080 still stream (`VideoStream:159`), its placement over 0..50.317 s
/// on the synthetic still clock (`VideoClip:324`, in-point 914982176908800)
/// and its Motion (`VideoFilterComponent:323`), which pans the still with two
/// Linear Position keys from 1.2010416984558105 to -0.2005208283662796 of
/// the canvas at Scale 100 and the default Anchor Point. Only the native
/// pixel size makes that a pan across the panorama.
const PANORAMA_STREAM: &str = r#"<VideoStream ObjectID="159" ClassID="a36e4719-3ec6-4a0c-ab11-8b4aab377aa5" Version="14"><IsStill>true</IsStill><FrameRate>8475667200</FrameRate><FrameRect>0,0,4652,1080</FrameRect><PARIsUncertain>true</PARIsUncertain><Duration>10973491200000000</Duration><CodecType>1380013856</CodecType><FieldTypeIsUncertain>true</FieldTypeIsUncertain></VideoStream>"#;
const PANORAMA_IN_TICKS: i64 = 914982176908800;
const PANORAMA_OUT_TICKS: i64 = 927763483046400;
const PANORAMA_END_TICKS: i64 = 12781306137600;
const PANORAMA_KEY_TICKS: [i64; 2] = [915041506579200, 927422648216064];
const PANORAMA_MOTION: &str = r#"<VideoFilterComponent ObjectID="323" ClassID="d10da199-beea-4dd1-b941-ed3a78766d50" Version="7"><Component Version="5"><Params Version="1"><Param Index="0" ObjectRef="365"/><Param Index="1" ObjectRef="366"/><Param Index="2" ObjectRef="367"/><Param Index="3" ObjectRef="368"/><Param Index="4" ObjectRef="369"/><Param Index="5" ObjectRef="370"/><Param Index="6" ObjectRef="371"/></Params><ID>1</ID><DisplayName>Motion</DisplayName><Bypass>false</Bypass><Intrinsic>true</Intrinsic></Component><PremiereFilterPrivateData Encoding="base64" BinaryHash="3805854c-243a-c4be-bd40-a8a40000000e">AQA=
</PremiereFilterPrivateData><MatchName>AE.ADBE Motion</MatchName><VideoFilterType>2</VideoFilterType></VideoFilterComponent>
<PointComponentParam ObjectID="365" ClassID="ca81d347-309b-44d2-acc7-1c572efb973c" Version="3"><Name>Position</Name><ParameterControlType>6</ParameterControlType><StartKeyframe>-91445760000000000,1.2010416984558105:0.5,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe><Keyframes>915041506579200,1.2010416984558105:0.5,0,0,0,0.16666666666666666,0.028754966000325587,0.16666666666666666,5,4,0,0,-0.23359375447034836,2.8606984371292482e-17;927422648216064,-0.2005208283662796:0.5,0,0,0.028754966000325587,0.16666666666666666,0,0.16666666666666666,5,4,0.23359375447034836,-2.8606984371292482e-17,0,0;</Keyframes><ParameterID>1</ParameterID></PointComponentParam>
<VideoComponentParam ObjectID="366" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="9"><Name>Scale</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>2</ParameterControlType><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>10000</UpperBound><ParameterID>2</ParameterID><UpperUIBound>100</UpperUIBound></VideoComponentParam>
<VideoComponentParam ObjectID="367" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="9"><Name>Scale Width</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>2</ParameterControlType><StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>10000</UpperBound><ParameterID>3</ParameterID><UpperUIBound>100</UpperUIBound></VideoComponentParam>
<VideoComponentParam ObjectID="368" ClassID="cc12343e-f113-4d3b-ae05-b287db77d461" Version="9"><Name> </Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>4</ParameterControlType><StartKeyframe>-91445760000000000,true,0,0,0,0,0,0</StartKeyframe><LowerBound>false</LowerBound><UpperBound>true</UpperBound><ParameterID>4</ParameterID></VideoComponentParam>
<VideoComponentParam ObjectID="369" ClassID="fe47129e-6c94-4fc0-95d5-c056a517aaf3" Version="9"><Name>Rotation</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>3</ParameterControlType><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>-32768</LowerBound><UpperBound>32767</UpperBound><ParameterID>5</ParameterID></VideoComponentParam>
<PointComponentParam ObjectID="370" ClassID="ca81d347-309b-44d2-acc7-1c572efb973c" Version="3"><Name>Anchor Point</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>6</ParameterControlType><StartKeyframe>-91445760000000000,0.5:0.5,0,0,0,0,0,0,5,4,0,0,0,0</StartKeyframe><ParameterID>6</ParameterID></PointComponentParam>
<VideoComponentParam ObjectID="371" ClassID="a4ff2d6e-7ac2-44f8-9d52-17d9ca50e542" Version="9"><Name>Anti-flicker Filter</Name><IsTimeVarying>false</IsTimeVarying><ParameterControlType>8</ParameterControlType><StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe><LowerBound>0</LowerBound><UpperBound>1</UpperBound><ParameterID>7</ParameterID></VideoComponentParam>"#;

/// The one-clip fixture playing the corpus panorama records above.
fn panorama_xml() -> String {
    let xml = SOURCE
        .replace(
            "<VideoStream ObjectID=\"8\"><Duration>2540160000000</Duration><FrameRate>8467200000</FrameRate><FrameRect>0,0,1920,1080</FrameRect></VideoStream>",
            PANORAMA_STREAM,
        )
        .replace("<VideoStream ObjectRef=\"8\"/>", "<VideoStream ObjectRef=\"159\"/>")
        .replace("<FrameRate>8467200000</FrameRate></TrackGroup>", "<FrameRate>8475667200</FrameRate></TrackGroup>")
        .replace(
            "<RelativePath>media/source.mp4</RelativePath></Media>",
            "<RelativePath>media/NYC Skyline-Pano.jpg</RelativePath><Infinite>true</Infinite></Media>",
        )
        .replace(
            "<OriginalDuration>2540160000000</OriginalDuration>",
            &format!("<OriginalDuration>{STILL_INTRINSIC_TICKS}</OriginalDuration>"),
        )
        .replace(
            "<TrackItem><End>1270080000000</End></TrackItem>",
            &format!("<TrackItem><Start>0</Start><End>{PANORAMA_END_TICKS}</End></TrackItem>"),
        )
        .replace(
            "<InPoint>0</InPoint><OutPoint>1270080000000</OutPoint>",
            &format!("<InPoint>{PANORAMA_IN_TICKS}</InPoint><OutPoint>{PANORAMA_OUT_TICKS}</OutPoint>"),
        )
        .replace(
            "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>",
            "<VideoComponentChain ObjectID=\"4\"><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"323\"/></Components></ComponentChain><DefaultOpacity>true</DefaultOpacity></VideoComponentChain>",
        )
        .replace("</PremiereData>", &format!("{PANORAMA_MOTION}</PremiereData>"));
    assert_eq!(
        xml.matches("ObjectRef=\"323\"").count(),
        1,
        "chain replaced"
    );
    xml
}

#[test]
fn corpus_panorama_still_pans_at_its_native_size() {
    let (project, omissions) =
        crate::format::inspect_project_with_omissions(&panorama_xml(), None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let clip = sequence.video_occurrences().next().unwrap();
    assert_eq!(clip.source_ticks(), PANORAMA_IN_TICKS..PANORAMA_OUT_TICKS);
    let stream = project.media(clip).unwrap().video.as_ref().unwrap();
    assert_eq!((stream.width, stream.height), (4652, 1080));
    let document = crate::tests::support::project_document_with_media(sequence, &project.media);
    let layer = &document["composition"]["layers"][0];
    assert_eq!(layer["type"], "Image");
    assert_eq!(
        layer["source"]["sourceRect"],
        serde_json::json!({"x": 0.0, "y": 0.0, "width": 4652.0, "height": 1080.0})
    );
    // Anchor Point 0.5:0.5 of the picture, Position of the canvas, Scale 100.
    let transform = &layer["transform"];
    assert_eq!(transform["anchorPoint"], serde_json::json!([2326.0, 540.0]));
    assert_eq!(
        transform["position"],
        serde_json::json!([1.2010416984558105 * 1920.0, 540.0])
    );
    assert_eq!(transform["scale"], serde_json::json!([100.0, 100.0]));
    // The Position keys, on the layer clock from the in-point, through the
    // shared Position reader: 234 ms and 48 976 ms after the still starts.
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    let key_ms = |ticks: i64| (ticks - PANORAMA_IN_TICKS) as f64 * 1000.0 / TICKS as f64;
    for (property, values) in [
        (
            "positionX",
            [1.2010416984558105 * 1920.0, -0.2005208283662796 * 1920.0],
        ),
        ("positionY", [540.0, 540.0]),
    ] {
        let entry = entries
            .iter()
            .find(|entry| {
                entry["target"]["layerId"] == layer["id"]
                    && entry["target"]["propertyType"] == property
            })
            .unwrap_or_else(|| panic!("missing {property}: {entries:#?}"));
        let keys = entry["animator"]["keyframes"].as_array().unwrap();
        assert_eq!(keys.len(), 2, "{property}");
        for (key, (ticks, value)) in keys.iter().zip(PANORAMA_KEY_TICKS.into_iter().zip(values)) {
            assert_eq!(
                key["layerTime"].as_f64().unwrap(),
                key_ms(ticks).round(),
                "{property}"
            );
            assert_eq!(key["value"]["value"].as_f64().unwrap(), value, "{property}");
            assert_eq!(key["easing"]["type"], "linear", "{property}");
        }
    }
    assert_eq!(entries.len(), 2, "{entries:#?}");
}

#[test]
fn still_motion_opacity_and_crop_import_as_a_video_clips_do() {
    // The pinned static-Motion fixture (Position 0.64:0.43, Scale 135 and
    // Scale Width 70 without Uniform Scale, Rotation 27, Anchor Point
    // 0.25:0.75), whose AME render pins the video mapping, imports the same
    // transform when its source is a still.
    let fixture = "feature_motion_static_transform_strict.prproj";
    let sequence = Some("c8acf9c1-34b2-4086-9f55-d528950a7059");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let video =
        inspect_project_with_media(&crate::format::read_xml(&path).unwrap(), sequence).unwrap();
    let still = inspect_project_with_media(&feature_fixture_as_stills(fixture), sequence).unwrap();
    let video_document = crate::tests::support::project_document_with_media(
        video.single_sequence().unwrap(),
        &video.media,
    );
    let still_document = crate::tests::support::project_document_with_media(
        still.single_sequence().unwrap(),
        &still.media,
    );
    let video_layer = &video_document["composition"]["layers"][0];
    let still_layer = &still_document["composition"]["layers"][0];
    assert_eq!(video_layer["type"], "Video");
    assert_eq!(still_layer["type"], "Image");
    assert_ne!(video_layer["transform"]["rotation"], 0.0);
    assert_eq!(still_layer["transform"], video_layer["transform"]);
    assert_eq!(
        still_layer["source"]["sourceRect"],
        video_layer["source"]["sourceRect"]
    );

    // A still keeps a static Crop effect as a video clip does. The one clip of
    // the derived `feature_media_fit_crop_strict` (26.3 Crop Left 12.5, Top
    // 7.5, Right 22.5 and Bottom 17.5, Edge Feather 24, on 1080x1920 media at
    // a moved Motion), flagged as a still, keeps the video's mask and Crop
    // guide: the same part of its own frame, in the same place.
    let fixture = "feature_media_fit_crop_strict.prproj";
    let path = path.with_file_name(fixture);
    let video =
        inspect_project_with_media(&crate::format::read_xml(&path).unwrap(), sequence).unwrap();
    let still = inspect_project_with_media(&feature_fixture_as_stills(fixture), sequence).unwrap();
    // The one `kind` layer and the guide of its mask.
    let cropped = |project: &PrProjectFile, kind: &str| {
        let document = crate::tests::support::project_document_with_media(
            project.single_sequence().unwrap(),
            &project.media,
        );
        let layers = document["composition"]["layers"].as_array().unwrap();
        let layer = layers
            .iter()
            .find(|layer| layer["type"] == kind)
            .unwrap_or_else(|| panic!("no {kind} layer: {layers:#?}"));
        let guide = layers
            .iter()
            .find(|guide| guide["id"] == layer["masks"][0]["layer"])
            .unwrap_or_else(|| panic!("{kind} without a mask guide: {layer}"));
        (layer.clone(), guide.clone())
    };
    let (video_layer, video_guide) = cropped(&video, "Video");
    let (image, guide) = cropped(&still, "Image");
    assert_eq!(guide["rect"]["position"], serde_json::json!([135.0, 144.0]));
    assert_eq!(guide["rect"]["size"], serde_json::json!([702.0, 1440.0]));
    assert_eq!(image["masks"], video_layer["masks"]);
    assert_eq!(image["transform"], video_layer["transform"]);
    assert_eq!(image["activeRange"], video_layer["playback"]["inputRange"]);
    for field in ["rect", "transform", "activeRange"] {
        assert_eq!(guide[field], video_guide[field], "{field}");
    }
}

#[test]
fn still_opacity_masks_import_as_a_video_clips_do() {
    // Premiere 26.5.1's masked clips A to C (`feature_opacity_masks_26_5_strict`,
    // the first at Mask Opacity 50, the second at Scale 50, the third a
    // feathered ellipse): as stills, each image keeps its clip's mask, whose
    // guide draws the same outline in the same frame as the video's guide.
    // D (inverted at Mask Opacity 50) and E (a masked effect) stay omitted as
    // stills. Video E has a separate effect scope outside this comparison.
    let fixture = "feature_opacity_masks_26_5_strict.prproj";
    let sequence = Some("5fe2e712-90a9-4044-b93a-7a75b79b1320");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let video =
        inspect_project_with_media(&crate::format::read_xml(&path).unwrap(), sequence).unwrap();
    let still = inspect_project_with_media(&feature_fixture_as_stills(fixture), sequence).unwrap();
    // Each masked layer's masks, transform and timeline range (a video's
    // playback input, an image's active range) beside its guide's.
    let masked = |project: &PrProjectFile, kind: &str, range: &str| {
        let document = crate::tests::support::project_document_with_media(
            project.single_sequence().unwrap(),
            &project.media,
        );
        let layers = document["composition"]["layers"]
            .as_array()
            .unwrap()
            .clone();
        layers
            .iter()
            .filter(|layer| {
                layer["type"] == kind
                    && layer["masks"]
                        .as_array()
                        .is_some_and(|masks| !masks.is_empty())
            })
            .map(|layer| {
                let guide = layers
                    .iter()
                    .find(|guide| guide["id"] == layer["masks"][0]["layer"])
                    .unwrap_or_else(|| panic!("{kind} without a mask guide: {layer}"));
                // Resolve the guide first, then compare controls rather than
                // IDs allocated beside each document's other occurrences.
                let mut masks = layer["masks"].clone();
                for mask in masks.as_array_mut().unwrap() {
                    let mask = mask.as_object_mut().unwrap();
                    mask.remove("id");
                    mask.remove("layer");
                }
                (
                    [
                        masks,
                        layer["transform"].clone(),
                        layer.pointer(range).unwrap().clone(),
                    ],
                    [
                        &guide["type"],
                        &guide["shape"],
                        &guide["transform"],
                        &guide["activeRange"],
                    ]
                    .map(Clone::clone),
                )
            })
            .collect::<Vec<_>>()
    };
    let videos = masked(&video, "Video", "/playback/inputRange");
    assert_eq!(videos.len(), 3);
    assert_eq!(masked(&still, "Image", "/activeRange"), videos);
}

fn still_project(alpha: bool, name: &str) -> PrProjectFile {
    let media_id = MediaId(format!("/tmp/stills/media/{name}"));
    let relative = format!("./media/{name}");
    let occurrence = |start: i64| PrVideoOccurrence {
        id: None,
        media: media_id.clone(),
        start_ticks: start,
        end_ticks: start + 3 * TICKS,
        in_ticks: STILL_SOURCE_IN_TICKS,
        out_ticks: STILL_SOURCE_IN_TICKS + 3 * TICKS,
        playback_rate: 1.0,
        frame_blending: None,
        time_remap: None,
        linear_wipe: None,
        opacity_mask: None,
        track_matte: None,
        opacity: 100.0,
        blend_mode: Default::default(),
        transform: Default::default(),
        crop: Default::default(),
        animations: Vec::new(),
        enabled: true,
        effects: Vec::new(),
        effects_above_mask: 0,
        stroke: None,
        active_transforms: 0,
        source_effects: None,
    };
    PrProjectFile::from_sequences(
        vec![PrSequence {
            native_frame_ticks: None,
            audio: Vec::new(),
            id: None,
            name: "Stills".into(),
            top_level: Some(true),
            video_tracks: vec![PrVideoTrack::media([occurrence(0), occurrence(4 * TICKS)])],
            timeline_end_ticks: 7 * TICKS,
            frame_rate: FrameRate::Fps30,
            width: 1920,
            height: 1080,
        }],
        [(
            media_id,
            PrMedia {
                name: name.to_owned(),
                relative_path: Some(relative.clone()),
                relative_paths: vec![relative],
                absolute_paths: vec![(
                    MediaPathField::FilePath,
                    format!("/tmp/stills/media/{name}").into(),
                )],
                video: Some(crate::schema::PrVideoStream {
                    pixel_aspect: Default::default(),
                    interpretation: Default::default(),
                    orientation: crate::schema::VideoOrientation::Identity,
                    intrinsic_ticks: STILL_INTRINSIC_TICKS,
                    frame_rate: (FrameRate::Fps30).into(),
                    width: 1920,
                    height: 1080,
                    kind: PrMediaKind::Still { alpha },
                }),
                audio: None,
            },
        )]
        .into_iter()
        .collect(),
    )
}

#[test]
fn written_still_records_declare_alpha_only_for_transparent_stills_and_read_back() {
    for (alpha, name) in [(false, "photo.jpg"), (true, "overlay.png")] {
        let xml = project_xml(&still_project(alpha, name)).unwrap();
        assert_eq!(xml.matches("<IsStill>true</IsStill>").count(), 1);
        assert_eq!(xml.matches("<Infinite>true</Infinite>").count(), 1);
        assert_eq!(
            xml.matches("<AlphaType>1</AlphaType>").count(),
            usize::from(alpha)
        );
        assert!(!xml.contains("<IgnoreAlpha>"));
        assert!(!xml.contains("<IsOverridenImageOrientationType>"));
        assert!(xml.contains("<CodecType>1380013856</CodecType>"));
        // The corporate_slideshow still clock, pinned independently of the constants.
        assert!(xml.contains("<Duration>10973491200000000</Duration>"));
        assert!(xml.contains("<InPoint>914457600000000</InPoint>"));
        // Premiere-authored still logging records carry no media range.
        assert!(xml.contains("<ClipLoggingInfo"));
        assert!(!xml.contains("<MediaInPoint>") && !xml.contains("<MediaOutPoint>"));
        let reread = inspect_project_with_media(&xml, None).unwrap();
        let sequence = reread.single_sequence().unwrap();
        let clips: Vec<_> = sequence.video_occurrences().collect();
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[1].timeline_ticks(), 4 * TICKS..7 * TICKS);
        assert_eq!(reread.media.len(), 1);
        let media = reread.media(clips[0]).unwrap();
        assert_eq!(
            media.video.as_ref().unwrap().kind,
            PrMediaKind::Still { alpha }
        );
        assert_eq!(media.name(), name);
    }
}

#[test]
fn writer_rejects_media_with_the_other_kind_of_extension() {
    for name in ["photo.mp4", "photo.exr"] {
        let error = project_xml(&still_project(false, name))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("PNG/JPEG/OpenEXR still media only"),
            "{error}"
        );
    }

    let mut project = still_project(false, "photo.png");
    project
        .media
        .values_mut()
        .next()
        .unwrap()
        .video
        .as_mut()
        .unwrap()
        .kind = PrMediaKind::OpenExr {
        alpha: false,
        numbered: false,
        channels: crate::schema::OpenExrChannels::Unspecified,
    };
    let error = project_xml(&project).unwrap_err().to_string();
    assert!(
        error.contains("PNG/JPEG/OpenEXR still media only"),
        "{error}"
    );

    let mut project = still_project(false, "photo.png");
    project
        .media
        .values_mut()
        .next()
        .unwrap()
        .video
        .as_mut()
        .unwrap()
        .kind = PrMediaKind::Video {
        codec: None,
        hdr_profile: None,
    };
    let error = project_xml(&project).unwrap_err().to_string();
    assert!(
        error.contains("MP4/MOV video and WAV/MP3/M4A audio"),
        "{error}"
    );
}

// Synthetic still flags on pinned main fixtures exercise feature interaction,
// not Adobe-authored retimed or wiped still evidence.
fn feature_fixture_as_stills(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    crate::format::read_xml(&path)
        .unwrap()
        .replace("</VideoStream>", "<IsStill>true</IsStill></VideoStream>")
        .replace("<AlphaType>3</AlphaType>", "")
}

#[test]
fn still_playback_rate_and_time_remap_keep_picture_and_report_lost_clock() {
    for (fixture, sequence, end_ticks, duration_ms, property) in [
        (
            "feature_constant_reverse_0_905_strict.prproj",
            "c8acf9c1-34b2-4086-9f55-d528950a7059",
            1_879_718_400_000,
            7400,
            "playback rate",
        ),
        (
            "feature_frame_blending_half_speed_strict.prproj",
            "c8acf9c1-34b2-4086-9f55-d528950a7059",
            508_032_000_000,
            2000,
            "playback rate",
        ),
        (
            "feature_time_remap_variable_speed_strict.prproj",
            "9a10a3b7-a83b-47d9-a68c-91d06d937738",
            508_032_000_000,
            2000,
            "time remap",
        ),
    ] {
        let xml = feature_fixture_as_stills(fixture);
        let (project, omissions) =
            crate::format::inspect_project_with_omissions(&xml, Some(sequence)).unwrap();
        let sequence = project.single_sequence().unwrap();
        assert_eq!(sequence.video_occurrences().count(), 1, "{fixture}");
        let clip = sequence.video_occurrences().next().unwrap();
        assert_eq!(clip.timeline_ticks(), 0..end_ticks, "{fixture}");
        let clock_omissions: Vec<_> = omissions
            .iter()
            .filter(|omission| {
                omission
                    .reason
                    .contains("a still has no time-varying picture")
            })
            .collect();
        assert_eq!(clock_omissions.len(), 1, "{omissions:#?}");
        assert_eq!(clock_omissions[0].scope, crate::OmissionScope::Feature);
        assert_eq!(clock_omissions[0].record, "VideoClipTrackItem:145");
        assert_eq!(
            clock_omissions[0].reason,
            format!("track 0, range 0..{end_ticks} ticks: {property} on a still image is not retained; a still has no time-varying picture")
        );
        let document = crate::tests::support::project_document_with_media(sequence, &project.media);
        let layer = &document["composition"]["layers"][0];
        assert_eq!(layer["type"], "Image", "{fixture}");
        assert_eq!(
            (*crate::test_support::layer_range(layer)),
            serde_json::json!({"start": 0, "duration": duration_ms})
        );
        assert!(layer["source"]["timeRemap"].is_null());
    }
}

#[test]
fn still_linear_wipe_omits_only_the_affected_occurrence() {
    let xml = feature_fixture_as_stills("feature_linear_wipe_strict.prproj");
    let (project, omissions) = crate::format::inspect_project_with_omissions(
        &xml,
        Some("49b892d1-dfc3-4be2-a83a-93789626be7c"),
    )
    .unwrap();
    let sequence = project.single_sequence().unwrap();
    let clips: Vec<_> = sequence.video_occurrences().collect();
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].id.as_deref(), Some("VideoClipTrackItem:145"));
    let wipe_omissions: Vec<_> = omissions
        .iter()
        .filter(|omission| omission.reason.contains("Linear Wipe on a still image"))
        .collect();
    assert_eq!(wipe_omissions.len(), 1, "{omissions:#?}");
    assert_eq!(wipe_omissions[0].scope, crate::OmissionScope::Occurrence);
    assert_eq!(wipe_omissions[0].record, "VideoClipTrackItem:153");
    assert_eq!(
        wipe_omissions[0].reason,
        "track 1, range 508032000000..1270080000000 ticks: Linear Wipe on a still image is unsupported; occurrence omitted"
    );
    let document = crate::tests::support::project_document_with_media(sequence, &project.media);
    assert_eq!(document["composition"]["layers"][0]["type"], "Image");
}

const IMAGES_NESTS_SEQUENCE: &str = "f3c651e6-0302-4499-b6f5-814b7b22c207";

/// `xml` with the first `from` inside the native record that starts `record`
/// replaced by `to`: a derived edit of one record.
fn edit_record(xml: &str, record: &str, from: &str, to: &str) -> String {
    let start = xml.find(record).unwrap_or_else(|| panic!("no {record}"));
    let offset = start
        + xml[start..]
            .find(from)
            .unwrap_or_else(|| panic!("{record}: no {from}"));
    assert!(
        !xml[start + record.len()..offset].contains("ObjectID="),
        "{from} is not in {record}"
    );
    format!("{}{to}{}", &xml[..offset], &xml[offset + from.len()..])
}

/// The Premiere 26.5.1 save `feature_images_nests_26_5.prproj` with each
/// [`edit_record`] edit. Its still E (`VideoClipTrackItem:145`, the 1080x1350
/// in_portrait.png at 8-10 s, `VideoClip:267` from InPoint 914449132800000,
/// one hour minus a frame) has Position and Scale keys 0.5 s and 1.5 s after
/// that InPoint; `VideoComponentParam` 287-290 are its Motion Crop Left, Top,
/// Right and Bottom.
fn images_nests_with(edits: &[(&str, &str, &str)]) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_images_nests_26_5.prproj");
    edits.iter().fold(
        crate::format::read_xml(&path).unwrap(),
        |xml, (record, from, to)| edit_record(&xml, record, from, to),
    )
}

const ZERO_EDGE: &str = "<StartKeyframe>-91445760000000000,0.,0,0,0,0,0,0</StartKeyframe>";

/// A static Motion Crop edge of `value` percent, as Premiere saves a nonzero
/// one (`CurrentValue` after the start key).
fn motion_crop_edge(value: &str) -> String {
    format!("<StartKeyframe>-91445760000000000,{value},0,0,0,0,0,0</StartKeyframe><CurrentValue>{value}</CurrentValue>")
}

#[test]
fn a_still_motion_crop_becomes_its_image_crop_guide() {
    // Still E with Motion Crop Left 10: the Motion Crop is part of the Motion
    // that a still converts, so the still keeps it as its Crop, and import
    // draws the flat video Crop guide in the image's own 1080x1350 frame.
    let left = motion_crop_edge("10.");
    let cropped = [(
        "<VideoComponentParam ObjectID=\"287\"",
        ZERO_EDGE,
        left.as_str(),
    )];
    let muted = [
        cropped[0],
        (
            "<VideoClipTrackItem ObjectID=\"145\"",
            "</ClipTrackItem>",
            "<IsMuted>true</IsMuted></ClipTrackItem>",
        ),
    ];
    for (case, edits, hidden) in [
        ("shown", &cropped[..], None),
        ("muted", &muted[..], Some(true)),
    ] {
        let (project, omissions) = crate::format::inspect_project_with_omissions(
            &images_nests_with(edits),
            Some(IMAGES_NESTS_SEQUENCE),
        )
        .unwrap();
        assert!(
            !omissions.iter().any(|omission| {
                omission.scope == crate::OmissionScope::Occurrence
                    && omission.record.ends_with("145")
            }),
            "{case}: {omissions:#?}"
        );
        let sequence = project.single_sequence().unwrap();
        let still = sequence
            .video_occurrences()
            .find(|clip| clip.id.as_deref() == Some("VideoClipTrackItem:145"))
            .expect("the still keeps its Motion Crop");
        assert_eq!(
            still.crop,
            crate::schema::PrStaticCrop {
                left: 10.0,
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
                edge_feather: 0.0,
            },
            "{case}"
        );
        let document = crate::tests::support::project_document_with_media(sequence, &project.media);
        let layers = document["composition"]["layers"].as_array().unwrap();
        let index = layers
            .iter()
            .position(|layer| layer["type"] == "Image" && layer["activeRange"]["start"] == 8000)
            .unwrap();
        let (image, guide) = (&layers[index], &layers[index + 1]);
        assert_eq!(
            image["activeRange"],
            serde_json::json!({"start": 8000, "duration": 2000}),
            "{case}"
        );
        assert_eq!(
            image.get("isHidden").and_then(serde_json::Value::as_bool),
            hidden,
            "{case}"
        );
        assert_eq!(
            image["transform"]["anchorPoint"],
            serde_json::json!([540.0, 675.0]),
            "{case}"
        );
        assert_eq!(
            image["masks"],
            serde_json::json!([{
                "id": image["masks"][0]["id"], "mode": "add", "inverted": false,
                "layer": guide["id"], "feather": [0.0, 0.0], "expansion": 0.0, "opacity": 1.0
            }]),
            "{case}"
        );
        // The guide crops the left 10 % of the image's own frame, beside it
        // with its range and transform; a hidden image keeps its Crop.
        assert_eq!(guide["type"], "Rect", "{case}");
        assert!(guide.get("isHidden").is_none(), "{case}");
        assert_eq!(
            guide["rect"]["position"],
            serde_json::json!([108.0, 0.0]),
            "{case}"
        );
        assert_eq!(
            guide["rect"]["size"],
            serde_json::json!([972.0, 1350.0]),
            "{case}"
        );
        assert_eq!(guide["activeRange"], image["activeRange"], "{case}");
        assert_eq!(guide["transform"], image["transform"], "{case}");
        // The guide repeats the image's Motion keys on the InPoint clock.
        let entries = document["composition"]["dynamics"]["entries"]
            .as_array()
            .unwrap();
        let keys = |layer: &serde_json::Value| -> Vec<(
            serde_json::Value,
            Vec<(serde_json::Value, serde_json::Value)>,
        )> {
            entries
                .iter()
                .filter(|entry| entry["target"]["layerId"] == layer["id"])
                .map(|entry| {
                    let keys = entry["animator"]["keyframes"].as_array().unwrap().iter();
                    (
                        entry["target"]["propertyType"].clone(),
                        keys.map(|key| (key["layerTime"].clone(), key["value"]["value"].clone()))
                            .collect(),
                    )
                })
                .collect()
        };
        let image_keys = keys(image);
        assert_eq!(
            image_keys,
            [
                ("positionX", [(500, 960.0), (1500, 960.0)]),
                ("positionY", [(500, 540.0), (1500, 324.0)]),
                ("scaleX", [(500, 100.0), (1500, 60.0)]),
                ("scaleY", [(500, 100.0), (1500, 60.0)]),
            ]
            .map(|(property, keys)| {
                (
                    serde_json::json!(property),
                    keys.map(|(time, value)| (serde_json::json!(time), serde_json::json!(value)))
                        .to_vec(),
                )
            }),
            "{case}"
        );
        assert_eq!(keys(guide), image_keys, "{case}");
    }
}

#[test]
fn a_still_keeps_its_placement_and_in_point_when_its_source_span_differs() {
    // A still has no media clock, so a source span that differs from its
    // placement shows the same picture: F of the unedited Premiere 26.5.1
    // save spans 5 s on its 2 s placement, and a still whose span is two
    // frames short or 3 s long keeps its placement and its InPoint, the
    // origin of its Motion keys. A video recovers a diagnosed constant-speed selection.
    // (A unit-speed Out at most one frame off its played end reads as that
    // end for any media, so these spans lie beyond that tolerance.)
    let frame = FrameRate::Fps30.ticks_per_frame();
    let respan = |xml: &str, in_ticks: i64, out_offset: i64| {
        let span = |out: i64| format!("<InPoint>{in_ticks}</InPoint><OutPoint>{out}</OutPoint>");
        let full = span(in_ticks + 5 * TICKS);
        assert_eq!(xml.matches(&full).count(), 1);
        xml.replace(&full, &span(in_ticks + 5 * TICKS + out_offset))
    };
    for out_offset in [-2 * frame, 3 * TICKS] {
        let (video, notes) =
            crate::format::inspect_project_with_omissions(&respan(SOURCE, 0, out_offset), None)
                .unwrap();
        let occurrence = video
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .next()
            .unwrap();
        assert_eq!(occurrence.timeline_ticks(), 0..5 * TICKS);
        assert_eq!(occurrence.source_ticks(), 0..5 * TICKS + out_offset);
        assert_eq!(
            occurrence.playback_rate,
            (5 * TICKS + out_offset) as f64 / (5 * TICKS) as f64
        );
        assert!(
            notes
                .iter()
                .any(|loss| loss.reason.contains("constant speed")),
            "{notes:?}"
        );
        let still = respan(&still_xml(false), STILL_SOURCE_IN_TICKS, out_offset);
        let (project, omissions) =
            crate::format::inspect_project_with_omissions(&still, None).unwrap();
        assert!(omissions.is_empty(), "{out_offset}: {omissions:#?}");
        let sequence = project.single_sequence().unwrap();
        let clip = sequence.video_occurrences().next().unwrap();
        assert_eq!(clip.timeline_ticks(), 0..5 * TICKS, "{out_offset}");
        assert_eq!(
            clip.source_ticks(),
            STILL_SOURCE_IN_TICKS..STILL_SOURCE_IN_TICKS + 5 * TICKS + out_offset,
            "{out_offset}"
        );
        let document = crate::tests::support::project_document_with_media(sequence, &project.media);
        assert_eq!(
            document["composition"]["layers"][0]["activeRange"],
            serde_json::json!({"start": 0, "duration": 5000}),
            "{out_offset}"
        );
    }

    // E two frames short (`VideoClip:267`), beside the unedited F.
    let xml = images_nests_with(&[(
        "<VideoClip ObjectID=\"267\"",
        "<OutPoint>914957164800000</OutPoint>",
        &format!("<OutPoint>{}</OutPoint>", 914_957_164_800_000 - 2 * frame),
    )]);
    let (project, omissions) =
        crate::format::inspect_project_with_omissions(&xml, Some(IMAGES_NESTS_SEQUENCE)).unwrap();
    assert!(
        !omissions.iter().any(|omission| {
            omission.scope == crate::OmissionScope::Occurrence
                && ["145", "146"]
                    .iter()
                    .any(|id| omission.record.ends_with(id))
        }),
        "{omissions:#?}"
    );
    let sequence = project.single_sequence().unwrap();
    let clip = |id: &str| {
        sequence
            .video_occurrences()
            .find(|clip| clip.id.as_deref() == Some(id))
            .unwrap_or_else(|| panic!("{id} is kept"))
    };
    let in_ticks = 914_449_132_800_000;
    for (id, timeline, source) in [
        (
            "VideoClipTrackItem:145",
            8..10,
            in_ticks..914_957_164_800_000 - 2 * frame,
        ),
        (
            "VideoClipTrackItem:146",
            0..2,
            in_ticks..in_ticks + 5 * TICKS,
        ),
    ] {
        assert_eq!(
            clip(id).timeline_ticks(),
            timeline.start * TICKS..timeline.end * TICKS,
            "{id}"
        );
        assert_eq!(clip(id).source_ticks(), source, "{id}");
    }
    let document = crate::tests::support::project_document_with_media(sequence, &project.media);
    let e = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Image" && layer["activeRange"]["start"] == 8000)
        .unwrap();
    assert_eq!(
        e["activeRange"],
        serde_json::json!({"start": 8000, "duration": 2000})
    );
    let key_times: Vec<_> = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["target"]["layerId"] == e["id"])
        .flat_map(|entry| entry["animator"]["keyframes"].as_array().unwrap())
        .map(|key| key["layerTime"].as_i64().unwrap())
        .collect();
    assert_eq!(key_times, [500, 1500, 500, 1500, 500, 1500, 500, 1500]);
}

#[test]
fn a_still_motion_crop_that_cannot_convert_still_omits_the_still() {
    // A still's Motion Crop follows the clip rules: a keyed or empty crop
    // omits the still; the Motion Crop admits none of a still's other masks,
    // and beside its Opacity mask the one-mask rule of every clip omits it.
    let keyed = format!(
        "<IsTimeVarying>true</IsTimeVarying>{}<Keyframes>914576140800000,0.,0,0,0,0.16666666666666666,10,0.16666666666666666;914830156800000,10.,0,0,10,0.16666666666666666,0,0.16666666666666666;</Keyframes>",
        ZERO_EDGE
    );
    let (sixty, forty) = (motion_crop_edge("60."), motion_crop_edge("40."));
    for (case, xml, sequence, record, reason) in [
        (
            "keyed Crop Left",
            images_nests_with(&[("<VideoComponentParam ObjectID=\"287\"", ZERO_EDGE, &keyed)]),
            Some(IMAGES_NESTS_SEQUENCE),
            "145",
            "unsupported conversion: VideoComponentParam:287: animated Motion Crop Left is unsupported",
        ),
        (
            "no visible area",
            images_nests_with(&[
                ("<VideoComponentParam ObjectID=\"287\"", ZERO_EDGE, &sixty),
                ("<VideoComponentParam ObjectID=\"289\"", ZERO_EDGE, &forty),
            ]),
            Some(IMAGES_NESTS_SEQUENCE),
            "145",
            "VideoFilterComponent:266: Motion Crop: invalid Premiere project: opposing Crop edges must leave a positive visible area",
        ),
        // Clip 87 of the Premiere 26.5.1 save `feature_opacity_masks_26_5_strict`
        // as a still, with Motion Crop Left 10 beside its Opacity mask.
        (
            "beside an Opacity mask",
            edit_record(
                &feature_fixture_as_stills("feature_opacity_masks_26_5_strict.prproj"),
                "<VideoComponentParam ObjectID=\"169\"",
                ZERO_EDGE,
                &motion_crop_edge("10."),
            ),
            None,
            "VideoClipTrackItem:87",
            "track 1, range 508032000000..1016064000000 ticks: an Opacity mask with a Crop or Linear Wipe on one clip is not converted; occurrence omitted",
        ),
    ] {
        let (project, omissions) =
            crate::format::inspect_project_with_omissions(&xml, sequence).unwrap();
        let omitted: Vec<_> = omissions
            .iter()
            .filter(|omission| {
                omission.scope == crate::OmissionScope::Occurrence && omission.record == record
            })
            .map(|omission| omission.reason.as_str())
            .collect();
        assert_eq!(omitted, [reason], "{case}: {omissions:#?}");
        assert!(
            project
                .single_sequence()
                .unwrap()
                .video_occurrences()
                .all(|clip| !clip.id.as_deref().unwrap_or_default().ends_with(record)),
            "{case}"
        );
    }
}

#[test]
fn still_track_matte_key_omits_the_still_and_its_matte_clip() {
    // Clip 3 (0-5 s) is keyed by Matte Alpha from `VideoClipTrackItem:93` on
    // the track above, the same source over the same range; item 95 plays
    // the source unkeyed at 5-10 s. Premiere draws a keyed clip only through
    // its matte and does not draw the matte clip (fixtures G1 and G1b). A
    // video keeps its key, so FX draws the matte only through it. Import
    // gives an image layer no key: a keyed canvas-sized still at unit speed
    // would draw unkeyed and its matte as content, so both are omitted and
    // the unkeyed still stays.
    let keyed = |xml: &str, sibling_in: i64| {
        let unkeyed = Placement {
            start: 5 * TICKS,
            end: 10 * TICKS,
            source_in: sibling_in,
        };
        let xml = with_records(
            &xml.replace(
                "<TrackItem ObjectRef=\"3\"/>",
                "<TrackItem ObjectRef=\"3\"/><TrackItem ObjectRef=\"95\"/>",
            ),
            &placement_records(95, 7, &unkeyed),
        );
        with_matte_track(&xml, &[(20, track_matte_key(20))])
    };
    let occurrence = crate::OmissionScope::Occurrence;
    let omitted_still = [
        (
            occurrence,
            "VideoClipTrackItem:3",
            "track 0, range 0..1270080000000 ticks: Track Matte Key on a still image is unsupported; occurrence omitted",
        ),
        (
            occurrence,
            "VideoClipTrackItem:93",
            "matte source of the omitted clip VideoClipTrackItem:3 was not converted: Premiere does not draw a track-matte source",
        ),
    ];
    for (case, xml, drawn, omitted) in [
        (
            "video fill, the supported key",
            keyed(SOURCE, 5 * TICKS),
            &[("Video", 0, Some("alpha")), ("Video", 5000, None)][..],
            &[][..],
        ),
        (
            "still fill",
            keyed(&still_xml(false), STILL_SOURCE_IN_TICKS),
            &[("Image", 5000, None)][..],
            &omitted_still[..],
        ),
    ] {
        let (project, omissions) =
            crate::format::inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
        let document = crate::tests::support::project_document_with_media(
            project.single_sequence().unwrap(),
            &project.media,
        );
        // FX draws neither a hidden layer nor a track matte source as content.
        let layers = document["composition"]["layers"].as_array().unwrap();
        let sources: Vec<_> = layers
            .iter()
            .filter_map(|layer| layer.get("trackMatte"))
            .map(|matte| &matte["layer"])
            .collect();
        let shown: Vec<_> = layers
            .iter()
            .filter(|layer| layer["isHidden"] != true && !sources.contains(&&layer["id"]))
            .map(|layer| {
                (
                    layer["type"].as_str().unwrap(),
                    (*crate::test_support::layer_range(layer))["start"]
                        .as_i64()
                        .unwrap(),
                    layer["trackMatte"]["mode"].as_str(),
                )
            })
            .collect();
        // The black canvas is the bottom layer.
        let (canvas, content) = shown.split_last().unwrap();
        assert_eq!((content, *canvas), (drawn, ("Rect", 0, None)), "{case}");
        let reported: Vec<_> = omissions
            .iter()
            .map(|omission| {
                (
                    omission.scope,
                    omission.record.as_str(),
                    omission.reason.as_str(),
                )
            })
            .collect();
        assert_eq!(reported, omitted, "{case}");
    }
}
