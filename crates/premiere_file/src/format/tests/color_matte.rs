use super::{
    adjustment::opacity,
    animation::animation_fixture::{animated_xml, SOURCE},
    effects::{blur, tint, with_chain, DEFAULT_FLAGS},
    reader::version_6_dissolve,
};
use crate::{
    format::{
        inspect_project_with_media, inspect_project_with_omissions, read_xml, FrameRate, MediaId,
        PrMedia, PrSequence,
    },
    schema::{
        color_matte::COLOR_MATTE_INTRINSIC_TICKS, PrAfterEffectsComposition, PrBlendMode,
        PrColorMatte, PrKeyframeEasing, PrMatteChannel, PrMediaKind, PrPropertyAnimation,
        PrScalarKeyframe, PrTrackMatte, PrVideoTrack, PrVideoTransitionKind, TICKS,
    },
    tests::support::{clip_of, named_media, project_document_with_media},
    Omission, OmissionKind, OmissionScope,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};

/// A Color Matte placement's source in-point on the 30 fps test sequences.
const COLOR_MATTE_SOURCE_IN_TICKS: i64 = crate::format::FrameRate::Fps30.generator_in_ticks();

const SEQUENCE_UID: &str = "c8acf9c1-34b2-4086-9f55-d528950a7059";
const RED: PrColorMatte = PrColorMatte { rgb: [255, 0, 0] };
const BLUE: PrColorMatte = PrColorMatte { rgb: [0, 0, 255] };
const BLACK_VIDEO: &str = include_str!("../../../tests/fixtures/black-video/occurrence-1172.xml");
const BLACK_VIDEO_SEQUENCE: &str = "d40c25d5-0359-473e-974e-24f423007763";

fn assert_black_video_feature_notes(omissions: &[Omission]) {
    assert!(omissions
        .iter()
        .all(|note| note.scope == OmissionScope::Feature));
    assert_eq!(
        omissions
            .iter()
            .map(|note| note.reason.as_str())
            .collect::<Vec<_>>(),
        [
            "IsCreatedWithNewColorManagement not converted",
            "nondefault tone mapping not converted"
        ]
    );
}

#[test]
fn native_black_video_reads_as_a_black_solid_with_its_source_clock() {
    use sha2::{Digest, Sha256};
    assert_eq!(
        format!("{:x}", Sha256::digest(BLACK_VIDEO.as_bytes())),
        "826f4a1f034272a74b6f5f545f7ce427f294cf90a25439c92cbfe9ee3affcdb7"
    );
    let (project, omissions) =
        inspect_project_with_omissions(BLACK_VIDEO, Some(BLACK_VIDEO_SEQUENCE)).unwrap();
    assert_black_video_feature_notes(&omissions);
    let sequence = project.single_sequence().unwrap();
    assert_eq!([sequence.width, sequence.height], [2160, 3840]);
    assert_eq!(sequence.frame_rate, FrameRate::Fps30000Over1001);
    let clip = sequence.video_occurrences().next().unwrap();
    assert_eq!(clip.timeline_ticks(), 7_662_003_148_800..8_094_262_176_000);
    assert_eq!(
        clip.source_ticks(),
        914_456_685_542_400..914_888_944_569_600
    );
    assert_eq!(clip.transform, Default::default());
    assert_eq!(clip.opacity, 100.0);
    assert!(clip.enabled);
    let media = project.media(clip).unwrap();
    assert_eq!(media.name(), "Black Video");
    assert!(media.is_generator() && !media.is_adjustment());
    assert!(media.relative_paths.is_empty() && media.absolute_paths.is_empty());
    assert!(media.audio.is_none());
    let stream = media.video.as_ref().unwrap();
    assert_eq!(
        stream.kind,
        PrMediaKind::ColorMatte(PrColorMatte { rgb: [0; 3] })
    );
    assert_eq!(stream.intrinsic_ticks, COLOR_MATTE_INTRINSIC_TICKS);
    assert_eq!(stream.frame_rate.ticks_per_frame(), 8_475_667_200);
    assert_eq!([stream.width, stream.height], [2160, 3840]);
}

#[test]
fn black_video_retains_static_opacity_and_rejects_malformed_generator_records() {
    // Supplemental control mutation: the pinned placement has default Opacity.
    let xml = BLACK_VIDEO
        .replace("<DefaultOpacity>true</DefaultOpacity>", "")
        .replace(
            "</ComponentChain>",
            "<Components><Component ObjectRef=\"50\"/></Components></ComponentChain>",
        );
    let xml = xml.replace(
        "</PremiereData>",
        &format!("{}\n</PremiereData>", opacity("45.", "")),
    );
    let (project, omissions) =
        inspect_project_with_omissions(&xml, Some(BLACK_VIDEO_SEQUENCE)).unwrap();
    assert_black_video_feature_notes(&omissions);
    let document = project_document_with_media(project.single_sequence().unwrap(), &project.media);
    let solid = &document["composition"]["layers"][0];
    assert_eq!(solid["type"], "Rect");
    assert_eq!(solid["rect"]["fillColor"], json!([0.0, 0.0, 0.0, 1.0]));
    assert_eq!(solid["transform"]["opacity"], 45.0);

    for (from, to, expected) in [
        (
            "<Infinite>true</Infinite>",
            "",
            "Black Video media must be Infinite",
        ),
        (
            "<IsStill>true</IsStill>",
            "",
            "Black Video media must be an IsStill stream",
        ),
        (
            "<Title>Black Video</Title>",
            "<Title>Black Video</Title><RelativePath>black.mp4</RelativePath>",
            "Black Video media must not reference files or audio",
        ),
        (
            "<Title>Black Video</Title>",
            "<Title>Black Video</Title><ImporterPrefs Encoding=\"base64\" BinaryHash=\"prefs\"/>",
            "Black Video media must not carry ImporterPrefs",
        ),
        (
            "<ActualMediaFilePath>1112293707</ActualMediaFilePath>",
            "<ActualMediaFilePath>1129270354</ActualMediaFilePath>",
            "unexpected graphic generator media",
        ),
        (
            "<FrameRect>0,0,2160,3840</FrameRect>\n\t</VideoStream>",
            "<FrameRect>0,0,1920,1080</FrameRect>\n\t</VideoStream>",
            "a Color Matte of 1920x1080 on a 2160x3840 sequence",
        ),
    ] {
        let xml = BLACK_VIDEO.replace(from, to);
        assert_ne!(xml, BLACK_VIDEO);
        let error = inspect_project_with_omissions(&xml, Some(BLACK_VIDEO_SEQUENCE))
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

/// The one-clip fixture `xml` with its file media replaced by a Color Matte
/// whose stream is `IsStill`, as on every corpus matte.
fn as_matte(xml: &str, prefs_base64: &str) -> String {
    let media = format!(
        r#"<Media ObjectUID="media-1"><VideoStream ObjectRef="8"/><ImporterPrefs Encoding="base64" BinaryHash="8faeedf7-eb02-d2a5-c178-492000000014">{prefs_base64}
    </ImporterPrefs><FilePath>1129270354</FilePath><Infinite>true</Infinite><ImplementationID>42008e7a-de6f-4270-96de-7e287abb9b4b</ImplementationID><Title>BG</Title><ActualMediaFilePath>1129270354</ActualMediaFilePath></Media>"#
    );
    let matte = xml
        .replace(
            r#"<Media ObjectUID="media-1"><VideoStream ObjectRef="8"/><RelativePath>media/source.mp4</RelativePath></Media>"#,
            &media,
        )
        .replace(
            "<Duration>2540160000000</Duration>",
            &format!("<IsStill>true</IsStill><Duration>{COLOR_MATTE_INTRINSIC_TICKS}</Duration>"),
        )
        .replace(
            "<OriginalDuration>2540160000000</OriginalDuration>",
            &format!("<OriginalDuration>{COLOR_MATTE_INTRINSIC_TICKS}</OriginalDuration>"),
        )
        .replace(
            "<InPoint>0</InPoint><OutPoint>1270080000000</OutPoint>",
            &format!(
                "<InPoint>{COLOR_MATTE_SOURCE_IN_TICKS}</InPoint><OutPoint>{}</OutPoint>",
                COLOR_MATTE_SOURCE_IN_TICKS + 5 * TICKS
            ),
        );
    assert_ne!(matte, xml);
    matte
}

pub(in crate::format) fn matte_xml(prefs_base64: &str) -> String {
    as_matte(SOURCE, prefs_base64)
}

#[test]
fn a_color_matte_occurrence_reads_its_colour_infinite_source_and_name() {
    let project =
        inspect_project_with_media(&matte_xml("ZEGlAAEAAAA="), Some("sequence-1")).unwrap();
    let sequence = project.sequences().next().unwrap();
    let clip = sequence.video_occurrences().next().unwrap();
    let media = project.media(clip).unwrap();
    let video = media.video.as_ref().unwrap();
    assert_eq!(
        video.kind,
        PrMediaKind::ColorMatte(PrColorMatte {
            rgb: [0x64, 0x41, 0xa5]
        })
    );
    assert_eq!(video.intrinsic_ticks, COLOR_MATTE_INTRINSIC_TICKS);
    assert_eq!(media.name(), "BG");
    assert!(media.relative_path.is_none() && media.absolute_paths.is_empty());
    assert_eq!(clip.timeline_ticks(), 0..5 * TICKS);
    assert_eq!(
        clip.source_ticks(),
        COLOR_MATTE_SOURCE_IN_TICKS..COLOR_MATTE_SOURCE_IN_TICKS + 5 * TICKS
    );
}

/// Convert a native Color Matte source through the public editable-document
/// path, retaining format-reader and mapping diagnostics separately.
fn convert_matte_effects(xml: &str) -> (Value, Vec<Omission>, Vec<Omission>) {
    let (project, reader_notes) = inspect_project_with_omissions(xml, Some("sequence-1")).unwrap();
    let sequence = project.single_sequence().unwrap();
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, &project.media);
    let mut mapping_notes = Vec::new();
    let document =
        crate::convert::premiere_to_tesseract(sequence, &project.media, &ids, &mut mapping_notes)
            .unwrap()
            .to_json_value()
            .unwrap();
    (document, reader_notes, mapping_notes)
}

#[test]
fn native_color_matte_tint_retains_editable_values_stack_and_picture() {
    // Both effect records come from Premiere 12.1 projects. Premiere applies
    // the component chain in reverse Index order, so Blur stays before Tint.
    let native = with_chain(SOURCE, DEFAULT_FLAGS, &[(20, tint(20)), (30, blur(30))]);
    let (document, reader_notes, mapping_notes) =
        convert_matte_effects(&as_matte(&native, "ZEGlAAEAAAA="));
    assert!(reader_notes.is_empty(), "{reader_notes:?}");
    assert!(mapping_notes.is_empty(), "{mapping_notes:?}");

    let matte = &document["composition"]["layers"][0];
    assert_eq!(matte["type"], "Rect");
    assert_eq!(
        matte["rect"]["fillColor"],
        json!([100.0 / 255.0, 65.0 / 255.0, 165.0 / 255.0, 1.0])
    );
    assert!(matte.get("source").is_none());
    let effects = matte["effects"].as_array().unwrap();
    assert_eq!(effects.len(), 2);
    assert_eq!(effects[0]["effect"]["type"], "gaussianBlur");
    assert_eq!(effects[0]["effect"]["blurriness"], 25.0);
    assert_eq!(effects[1]["enabled"], true);
    assert_eq!(effects[1]["effect"]["type"], "tintTritone");
    assert_eq!(effects[1]["effect"]["blackR"], 163.0 / 255.0);
    assert_eq!(effects[1]["effect"]["blackG"], 247.0 / 255.0);
    assert_eq!(effects[1]["effect"]["blackB"], 143.0 / 255.0);
    assert_eq!(effects[1]["effect"]["whiteR"], 240.0 / 255.0);
    assert_eq!(effects[1]["effect"]["whiteG"], 242.0 / 255.0);
    assert_eq!(effects[1]["effect"]["whiteB"], 22.0 / 255.0);
    assert_eq!(effects[1]["effect"]["amount"], 100.0);
}

#[test]
fn invalid_native_color_matte_tint_keeps_picture_and_supported_sibling() {
    let invalid_tint = tint(20).replace(
        "<StartKeyframe>-91445760000000000,100.,",
        "<StartKeyframe>-91445760000000000,150.,",
    );
    assert_ne!(invalid_tint, tint(20));
    let native = with_chain(SOURCE, DEFAULT_FLAGS, &[(20, invalid_tint), (30, blur(30))]);
    let (document, reader_notes, mapping_notes) =
        convert_matte_effects(&as_matte(&native, "ZEGlAAEAAAA="));
    assert!(
        reader_notes.iter().any(|note| note
            .reason
            .contains("Amount to Tint \"150.\" is not a number from 0 to 100")),
        "{reader_notes:?}"
    );
    assert!(mapping_notes.is_empty(), "{mapping_notes:?}");

    let matte = &document["composition"]["layers"][0];
    assert_eq!(matte["type"], "Rect");
    assert_eq!(
        matte["rect"]["fillColor"],
        json!([100.0 / 255.0, 65.0 / 255.0, 165.0 / 255.0, 1.0])
    );
    let effects = matte["effects"].as_array().unwrap();
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0]["effect"]["type"], "gaussianBlur");
}

#[test]
fn file_media_with_an_empty_importer_prefs_keeps_its_occurrence() {
    // Adobe writes this empty element on ordinary file media
    // (`copy_and_paste_effects`), not only generator prefs on mattes.
    let xml = SOURCE.replace(
        r#"<Media ObjectUID="media-1"><VideoStream ObjectRef="8"/><RelativePath>"#,
        r#"<Media ObjectUID="media-1"><VideoStream ObjectRef="8"/><ImporterPrefs Encoding="base64" BinaryHash="99041274-7d9f-012e-bd9a-751500000014"/><RelativePath>"#,
    );
    assert_ne!(xml, SOURCE);
    let project = inspect_project_with_media(&xml, Some("sequence-1")).unwrap();
    let sequence = project.sequences().next().unwrap();
    assert_eq!(sequence.video_occurrences().count(), 1);
}

/// The matte fixture with other `FilePath`/`ActualMediaFilePath` markers under
/// the same shared generator `ImplementationID`; `None` drops the element.
fn generator_xml(file_path: Option<&str>, actual_media_file_path: Option<&str>) -> String {
    let element = |tag: &str, value: Option<&str>| {
        value.map_or_else(String::new, |value| format!("<{tag}>{value}</{tag}>"))
    };
    matte_xml("/wAAAAEAAAA=")
        .replace(
            "<FilePath>1129270354</FilePath>",
            &element("FilePath", file_path),
        )
        .replace(
            "<ActualMediaFilePath>1129270354</ActualMediaFilePath>",
            &element("ActualMediaFilePath", actual_media_file_path),
        )
}

#[test]
fn malformed_or_non_colr_generator_media_is_omitted_precisely() {
    const GRAPHIC: &str = "1196574294";
    const BLACK_VIDEO: &str = "1112293707";
    const COLR: &str = "1129270354";
    // Other generators keep fail-closed validation; an absent or mismatched
    // marker is no matte. BLAK has no colour preferences.
    for (xml, expected) in [
        (matte_xml("AAAAAAEAAAA"), "invalid ImporterPrefs"),
        (matte_xml("/wAAAAEAAA=="), "must be 8 bytes"),
        (matte_xml("/wAAAQEAAAA="), "unknown layout"),
        (matte_xml("/wAAAAIAAAA="), "unknown layout"),
        (
            matte_xml("/wAAAAEAAAA=").replace("<Infinite>true</Infinite>", ""),
            "must be Infinite",
        ),
        (
            matte_xml("/wAAAAEAAAA=").replace(
                r#"<ImporterPrefs Encoding="base64""#,
                r#"<ImporterPrefs Encoding="hex""#,
            ),
            "must be base64",
        ),
        (
            matte_xml("/wAAAAEAAAA=").replacen(
                "</Clip></VideoClip>",
                "</Clip><ScaleToFramePolicy>1</ScaleToFramePolicy></VideoClip>",
                1,
            ),
            "VideoClip:6: Scale to Frame Size on a Color Matte is not converted",
        ),
        (
            generator_xml(Some(GRAPHIC), Some(GRAPHIC)),
            "a graphic without text or shape objects is unsupported",
        ),
        (
            generator_xml(Some(BLACK_VIDEO), Some(BLACK_VIDEO)),
            "Black Video media must not carry ImporterPrefs",
        ),
        (
            generator_xml(None, Some(COLR)),
            "unexpected graphic generator media",
        ),
        (
            generator_xml(Some(COLR), Some(GRAPHIC)),
            "unexpected graphic generator media",
        ),
    ] {
        let error = inspect_project_with_omissions(&xml, Some("sequence-1"))
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

#[test]
fn nondefault_static_motion_on_a_matte_keeps_the_editable_rectangle() {
    let xml = as_matte(&animated_xml(""), "/wAAAAEAAAA=");
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let document = project_document_with_media(project.single_sequence().unwrap(), &project.media);
    assert_eq!(document["composition"]["layers"][0]["type"], "Rect");

    for (start, changed, property) in [
        (
            "<Name>Position</Name><ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,0.5:0.5,",
            "<Name>Position</Name><ParameterID>1</ParameterID><StartKeyframe>-91445760000000000,0.25:0.5,",
            "Motion Position",
        ),
        (
            "<Name>Anchor Point</Name><ParameterID>6</ParameterID><StartKeyframe>-91445760000000000,0.5:0.5,",
            "<Name>Anchor Point</Name><ParameterID>6</ParameterID><StartKeyframe>-91445760000000000,0.25:0.5,",
            "Motion Anchor Point",
        ),
        // Uniform Scale edits Scale and Scale Width alike.
        (
            "<StartKeyframe>-91445760000000000,100.,0,0,0,0,0,0</StartKeyframe>",
            "<StartKeyframe>-91445760000000000,150.,0,0,0,0,0,0</StartKeyframe>",
            "Motion Scale",
        ),
        (
            "<Name>Rotation</Name><ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,0.,",
            "<Name>Rotation</Name><ParameterID>5</ParameterID><StartKeyframe>-91445760000000000,30.,",
            "Motion Rotation",
        ),
    ] {
        let edited = xml.replace(start, changed);
        assert_ne!(edited, xml, "{property}");
        let (project, omissions) = inspect_project_with_omissions(&edited, Some("sequence-1")).unwrap();
        assert!(omissions.is_empty(), "{property}: {omissions:?}");
        let document = project_document_with_media(project.single_sequence().unwrap(), &project.media);
        let rect = &document["composition"]["layers"][0];
        assert_eq!(rect["type"], "Rect");
        assert_eq!(rect["rect"]["fillColor"], json!([1.0, 0.0, 0.0, 1.0]));
        let (field, expected) = match property {
            "Motion Position" => ("position", json!([480.0, 540.0])),
            "Motion Anchor Point" => ("anchorPoint", json!([480.0, 540.0])),
            "Motion Scale" => ("scale", json!([150.0, 150.0])),
            "Motion Rotation" => ("rotation", json!(30.0)),
            _ => unreachable!(),
        };
        assert_eq!(rect["transform"][field], expected);
    }
}

#[test]
fn matte_linear_wipe_omits_the_occurrence_and_retiming_only_loses_the_clock() {
    // Synthetic COLR media on pinned main fixtures exercises feature
    // interaction, not Adobe-authored retimed or wiped Color Matte evidence.
    for (fixture, sequence, omission, kept_ms) in [
        (
            "feature_constant_reverse_0_905_strict.prproj",
            SEQUENCE_UID,
            (
                OmissionScope::Feature,
                "VideoClipTrackItem:145",
                "track 0, range 0..1879718400000 ticks: playback rate on a Color Matte is not retained; a solid has no time-varying picture",
            ),
            7400,
        ),
        (
            "feature_time_remap_variable_speed_strict.prproj",
            "9a10a3b7-a83b-47d9-a68c-91d06d937738",
            (
                OmissionScope::Feature,
                "VideoClipTrackItem:145",
                "track 0, range 0..508032000000 ticks: time remap on a Color Matte is not retained; a solid has no time-varying picture",
            ),
            2000,
        ),
        (
            "feature_linear_wipe_strict.prproj",
            "49b892d1-dfc3-4be2-a83a-93789626be7c",
            (
                OmissionScope::Occurrence,
                "VideoClipTrackItem:153",
                "track 1, range 508032000000..1270080000000 ticks: Linear Wipe on a Color Matte is unsupported; occurrence omitted",
            ),
            3000,
        ),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture);
        let xml = read_xml(&path).unwrap();
        let parsed = roxmltree::Document::parse(&xml).unwrap();
        let mut mattes = xml.clone();
        for media in parsed
            .descendants()
            .filter(|node| node.has_tag_name("Media") && node.attribute("ObjectUID").is_some())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            let stream = media
                .children()
                .find(|node| node.has_tag_name("VideoStream"))
                .unwrap();
            let uid = media.attribute("ObjectUID").unwrap();
            mattes.replace_range(media.range(), &format!(
                "<Media ObjectUID=\"{uid}\">{}<ImporterPrefs Encoding=\"base64\" BinaryHash=\"8faeedf7-eb02-d2a5-c178-492000000014\">/wAAAAEAAAA=</ImporterPrefs><FilePath>1129270354</FilePath><Infinite>true</Infinite><ImplementationID>42008e7a-de6f-4270-96de-7e287abb9b4b</ImplementationID><Title>Color Matte</Title></Media>",
                &xml[stream.range()]
            ));
        }
        let (project, omissions) = inspect_project_with_omissions(&mattes, Some(sequence)).unwrap();
        let (scope, record, reason) = omission;
        assert!(
            omissions.contains(&Omission {
                scope,
                kind: OmissionKind::Omitted,
                record: record.into(),
                reason: reason.into(),
            }),
            "{fixture}: {omissions:#?}"
        );
        let sequence = project.single_sequence().unwrap();
        assert_eq!(sequence.video_occurrences().count(), 1, "{fixture}");
        let document = project_document_with_media(sequence, &project.media);
        let layer = &document["composition"]["layers"][0];
        assert_eq!(layer["type"], "Rect", "{fixture}");
        assert_eq!(layer["rect"]["fillColor"], json!([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(
            layer["activeRange"],
            json!({"start": 0, "duration": kept_ms}),
            "{fixture}"
        );
    }
}

/// `xml` with the `FrameRect` element of the `VideoStream` record `stream`
/// replaced by `markup`.
fn with_stream_frame(xml: &str, stream: u32, markup: &str) -> String {
    let record = xml
        .find(&format!(r#"<VideoStream ObjectID="{stream}""#))
        .expect("stream record exists");
    let start = record + xml[record..].find("<FrameRect>").unwrap();
    let end = start + xml[start..].find("</FrameRect>").unwrap() + "</FrameRect>".len();
    format!("{}{markup}{}", &xml[..start], &xml[end..])
}

#[test]
fn a_matte_the_size_of_its_custom_canvas_is_full_frame_and_another_size_is_omitted() {
    // A synthetic 1080x1920 mutant of the Adobe-saved fixture: every frame,
    // the sequence's, its three placements' and its three streams', is portrait.
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_color_matte_strict.prproj");
    let native = read_xml(&path).unwrap();
    assert_eq!(
        native
            .matches("<FrameRect>0,0,1920,1080</FrameRect>")
            .count(),
        7
    );
    let portrait = native.replace(
        "<FrameRect>0,0,1920,1080</FrameRect>",
        "<FrameRect>0,0,1080,1920</FrameRect>",
    );
    let (project, omissions) =
        inspect_project_with_omissions(&portrait, Some(SEQUENCE_UID)).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.video_occurrences().count(), 3);
    let document = project_document_with_media(sequence, &project.media);
    let mattes: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| {
            layer["name"]
                .as_str()
                .unwrap()
                .starts_with("Premiere color matte")
        })
        .collect();
    assert_eq!(mattes.len(), 2);
    for matte in mattes {
        assert_eq!(matte["rect"]["size"], json!([1080.0, 1920.0]));
        assert_eq!(matte["transform"]["position"], json!([0.0, 0.0]));
    }

    // The blue matte (stream 30) of another size, not at the origin or in
    // non-square pixels would otherwise cover the canvas: only its placement
    // goes.
    for (markup, reason) in [
        (
            "<FrameRect>0,0,540,960</FrameRect>",
            "VideoClipTrackItem:153: a Color Matte of 540x960 on a 1080x1920 sequence is not converted",
        ),
        (
            "<FrameRect>0,0,1920,1080</FrameRect>",
            "VideoClipTrackItem:153: a Color Matte of 1920x1080 on a 1080x1920 sequence is not converted",
        ),
        (
            "<FrameRect>8,0,1088,1920</FrameRect>",
            "VideoStream:30: invalid FrameRect",
        ),
        (
            "<FrameRect>0,0,1080,1920</FrameRect><PixelAspectRatio>2,1</PixelAspectRatio>",
            "VideoStream:30: non-square source pixels",
        ),
    ] {
        let (project, omissions) = inspect_project_with_omissions(
            &with_stream_frame(&portrait, 30, markup),
            Some(SEQUENCE_UID),
        )
        .unwrap();
        let kinds: Vec<_> = project
            .single_sequence()
            .unwrap()
            .video_occurrences()
            .map(|clip| project.media(clip).unwrap().video.as_ref().unwrap().kind)
            .collect();
        assert_eq!(
            kinds,
            [
                PrMediaKind::Video {
                    codec: None,
                    hdr_profile: None
                },
                PrMediaKind::ColorMatte(RED)
            ],
            "{markup}"
        );
        assert_eq!(omissions.len(), 1, "{markup}: {omissions:?}");
        assert_eq!(
            (omissions[0].scope, omissions[0].record.as_str()),
            (OmissionScope::Occurrence, "153")
        );
        assert!(omissions[0].reason.contains(reason), "{omissions:?}");
    }
}

#[test]
fn isolated_fixture_places_red_and_blue_mattes_and_keeps_only_the_empty_second_as_a_gap() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_color_matte_strict.prproj");
    let (project, omissions) =
        inspect_project_with_omissions(&read_xml(&path).unwrap(), Some(SEQUENCE_UID)).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.sequences().next().unwrap();
    assert_eq!(sequence.name(), "Color matte");
    let tracks: Vec<Vec<_>> = sequence
        .video_tracks()
        .map(|track| {
            track
                .iter()
                .map(|item| item.media().unwrap())
                .map(|clip| {
                    let kind = project.media(clip).unwrap().video.as_ref().unwrap().kind;
                    (clip.start_ticks / TICKS, clip.end_ticks / TICKS, kind)
                })
                .collect()
        })
        .collect();
    assert_eq!(
        tracks,
        [
            vec![
                (
                    0,
                    6,
                    PrMediaKind::Video {
                        codec: None,
                        hdr_profile: None
                    }
                ),
                (7, 9, PrMediaKind::ColorMatte(BLUE))
            ],
            vec![(2, 5, PrMediaKind::ColorMatte(RED))]
        ]
    );
    // 7–9 s is covered only by the blue matte: opaque content, not a gap.
    assert_eq!(sequence.gaps(&project.media), vec![6 * TICKS..7 * TICKS]);
    assert_eq!(sequence.end_ticks(), 9 * TICKS);
    let blue = sequence.video_tracks().next().unwrap()[1].media().unwrap();
    assert_eq!(
        blue.source_ticks(),
        COLOR_MATTE_SOURCE_IN_TICKS..COLOR_MATTE_SOURCE_IN_TICKS + 2 * TICKS
    );
    assert_eq!(project.media(blue).unwrap().name(), "Color Matte");
}

/// [`matte_xml`] whose chain holds a synthetic intrinsic Opacity of 60 % with
/// the Normal blend pair, keyed by `keys` when they are not empty.
fn matte_with_opacity(keys: &str) -> String {
    let chain = "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><DefaultOpacity>true</DefaultOpacity><ComponentChain/></VideoComponentChain>";
    let with_opacity = "<VideoComponentChain ObjectID=\"4\"><DefaultMotion>true</DefaultMotion><ComponentChain><Components><Component Index=\"0\" ObjectRef=\"50\"/></Components></ComponentChain></VideoComponentChain>";
    let xml = matte_xml("AAAAAAEAAAA=").replace(chain, with_opacity);
    assert!(xml.contains(with_opacity));
    xml.replace(
        "</PremiereData>",
        &format!("{}</PremiereData>", opacity("60.", keys)),
    )
}

#[test]
fn a_matte_keeps_its_static_opacity_on_its_rectangle() {
    let (project, omissions) =
        inspect_project_with_omissions(&matte_with_opacity(""), Some("sequence-1")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let clip = sequence.video_occurrences().next().unwrap();
    assert_eq!(clip.opacity, 60.0);
    let document = project_document_with_media(sequence, &project.media);
    let rect = &document["composition"]["layers"][0];
    assert_eq!(rect["type"], "Rect");
    assert_eq!(rect["name"], "Premiere color matte 1");
    assert_eq!(rect["transform"]["opacity"], 60.0);
    assert_eq!(rect["rect"]["fillColor"], json!([0.0, 0.0, 0.0, 1.0]));
    assert_eq!(rect["activeRange"], json!({"start": 0, "duration": 5000}));
}

#[test]
fn a_matte_with_opacity_keys_keeps_the_editable_keys_and_static_value() {
    let keyed = matte_with_opacity(&format!(
        "{COLOR_MATTE_SOURCE_IN_TICKS},0.,0,0,0,0,0,0;{},60.,0,0,0,0,0,0;",
        COLOR_MATTE_SOURCE_IN_TICKS + TICKS
    ));
    let (project, omissions) = inspect_project_with_omissions(&keyed, Some("sequence-1")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let document = project_document_with_media(project.single_sequence().unwrap(), &project.media);
    let rect = &document["composition"]["layers"][0];
    assert_eq!(rect["transform"]["opacity"], 60.0);
    let entry = &document["composition"]["dynamics"]["entries"][0];
    assert_eq!(entry["target"]["layerId"], rect["id"]);
    assert_eq!(entry["target"]["propertyType"], "opacity");
    let keys = &entry["animator"]["keyframes"];
    assert_eq!(keys[0]["layerTime"], 0);
    assert_eq!(keys[0]["value"]["value"], 0.0);
    assert_eq!(keys[1]["layerTime"], 1000);
    assert_eq!(keys[1]["value"]["value"], 60.0);
}

/// [`matte_with_opacity`] placed at 1-5 s with a synthetic Version 6 head
/// Cross Dissolve (Legacy), `VideoTransitionTrackItem:60`, over its first 15
/// frames.
fn matte_with_head_dissolve() -> String {
    const FRAME: i64 = FrameRate::Fps30.ticks_per_frame();
    let edits = [
        (
            "<TrackItem><End>1270080000000</End></TrackItem><SubClip ObjectRef=\"5\"/></ClipTrackItem>"
                .to_owned(),
            format!(
                "<TrackItem><Start>{TICKS}</Start><End>{}</End></TrackItem><SubClip ObjectRef=\"5\"/><HeadTransition ObjectRef=\"60\"/></ClipTrackItem>",
                5 * TICKS
            ),
        ),
        (
            format!(
                "<InPoint>{COLOR_MATTE_SOURCE_IN_TICKS}</InPoint><OutPoint>{}</OutPoint>",
                COLOR_MATTE_SOURCE_IN_TICKS + 5 * TICKS
            ),
            format!(
                "<InPoint>{COLOR_MATTE_SOURCE_IN_TICKS}</InPoint><OutPoint>{}</OutPoint>",
                COLOR_MATTE_SOURCE_IN_TICKS + 4 * TICKS
            ),
        ),
        (
            "</ClipItems></ClipTrack>".to_owned(),
            "</ClipItems><TransitionItems><TrackItems><TrackItem ObjectRef=\"60\"/></TrackItems></TransitionItems></ClipTrack>".to_owned(),
        ),
        (
            "</PremiereData>".to_owned(),
            format!(
                "{}</PremiereData>",
                version_6_dissolve(TICKS, TICKS + 15 * FRAME)
            ),
        ),
    ];
    edits
        .iter()
        .fold(matte_with_opacity(""), |xml, (from, to)| {
            assert_eq!(xml.matches(from.as_str()).count(), 1, "{from}");
            xml.replace(from.as_str(), to)
        })
}

/// The editable document of `sequence` over `media` and the import reports.
fn import(sequence: &PrSequence, media: &BTreeMap<MediaId, PrMedia>) -> (Value, Vec<Omission>) {
    let ids = crate::tesseract_output::asset_ids_in_order(sequence, media);
    let mut omissions = Vec::new();
    let document = crate::convert::premiere_to_tesseract(sequence, media, &ids, &mut omissions)
        .unwrap()
        .to_json_value()
        .unwrap();
    (document, omissions)
}

/// The converted matte keeps its colour, static Opacity 60 and its 1-5 s
/// lifetime.
fn assert_static_matte(rect: &Value, context: &str) {
    assert_eq!(rect["type"], "Rect", "{context}");
    assert_eq!(
        rect["rect"]["fillColor"],
        json!([0.0, 0.0, 0.0, 1.0]),
        "{context}"
    );
    assert_eq!(rect["transform"]["opacity"], 60.0, "{context}");
    assert_eq!(
        rect["activeRange"],
        json!({"start": 1000, "duration": 4000}),
        "{context}"
    );
}

/// A head Cross Dissolve (Legacy) on a static Normal Color Matte fades the
/// matte in up to its static Opacity once. Without this mapping, import shows
/// the matte at its static Opacity from its first frame. Synthetic structural
/// check: the native Legacy curve is not measured here.
#[test]
fn a_head_cross_dissolve_fades_its_matte_in_to_the_static_opacity_once() {
    let (project, omissions) =
        inspect_project_with_omissions(&matte_with_head_dissolve(), Some("sequence-1")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let transition = &sequence.video_tracks[0].transitions[0];
    assert_eq!(
        (
            transition.kind,
            transition.start_ticks,
            transition.cut_ticks,
            transition.end_ticks,
            transition.outgoing_clip.as_deref(),
            transition.incoming_clip.as_deref(),
        ),
        (
            PrVideoTransitionKind::CrossDissolve,
            254_016_000_000,
            254_016_000_000,
            381_024_000_000,
            None,
            Some("VideoClipTrackItem:3"),
        )
    );
    let (document, omissions) = import(sequence, &project.media);
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Approximated,
            record: "VideoTransitionTrackItem:60".into(),
            reason: "Cross Dissolve New retained as editable linear opacity at the picture boundary; opaque SDR controls follow the measured linear ramp, while alpha and general edited native fidelity remain unmeasured".into(),
        }]
    );
    let rect = &document["composition"]["layers"][0];
    assert_static_matte(rect, "converted head");
    // One Opacity track replaces that static value: 0 at the cut, linearly up
    // to 60 (neither 100 nor 60 % of 60) 15 frames later, then held.
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0]["target"]["layerId"], rect["id"]);
    assert_eq!(entries[0]["target"]["propertyType"], "opacity");
    let keys: Vec<_> = entries[0]["animator"]["keyframes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| {
            (
                key["layerTime"].clone(),
                key["value"].clone(),
                key["easing"].clone(),
            )
        })
        .collect();
    assert_eq!(
        keys,
        [
            (
                json!(0),
                json!({"type": "float", "value": 0.0}),
                json!({"type": "linear"})
            ),
            (
                json!(500),
                json!({"type": "float", "value": 60.0}),
                json!({"type": "linear"})
            ),
        ]
    );
}

/// Current peak opacity and fill alpha must survive a native head or tail.
#[test]
fn cross_dissolve_matte_exports_edited_translucent_head_and_tail() {
    let (project, _) =
        inspect_project_with_omissions(&matte_with_head_dissolve(), Some("sequence-1")).unwrap();
    for tail in [false, true] {
        let mut sequence = project.single_sequence().unwrap().clone();
        if tail {
            let end = sequence.video_tracks[0].clip(0).end_ticks;
            let transition = &mut sequence.video_tracks[0].transitions[0];
            transition.outgoing_clip = transition.incoming_clip.take();
            transition.start_ticks = end - TICKS / 2;
            transition.cut_ticks = end;
            transition.end_ticks = end;
        }
        let (mut document, _) = import(&sequence, &project.media);
        document["composition"]["layers"][0]["rect"]["fillColor"][3] = json!(0.5);
        document["composition"]["dynamics"]["entries"][0]["animator"]["keyframes"]
            [usize::from(!tail)]["value"]["value"] = json!(65.0);
        let document = fx_schema::EditableFxCompositionDocument::from_json_value(document).unwrap();
        let mut omissions = Vec::new();
        let exported = crate::convert::tesseract_to_premiere(
            &document,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            FrameRate::Fps30,
            &mut omissions,
        )
        .unwrap();
        assert!(
            !omissions.iter().any(|o| o.kind == OmissionKind::Omitted),
            "{omissions:?}"
        );
        let track = exported
            .single_sequence()
            .unwrap()
            .video_tracks
            .iter()
            .find(|track| !track.transitions.is_empty())
            .unwrap();
        assert_eq!(track.clip(0).opacity, 32.5);
        assert!(track.clip(0).animations.is_empty());
        assert!(matches!(
            exported.media[&track.clip(0).media]
                .video
                .as_ref()
                .unwrap()
                .kind,
            PrMediaKind::ColorMatte(_)
        ));
        let actual = &track.transitions[0];
        let expected = &sequence.video_tracks[0].transitions[0];
        assert_eq!(
            (actual.start_ticks, actual.cut_ticks, actual.end_ticks),
            (expected.start_ticks, expected.cut_ticks, expected.end_ticks)
        );
        assert_eq!(actual.outgoing_clip.is_some(), tail);
        assert_eq!(actual.incoming_clip.is_some(), !tail);
    }
}

/// A matte dissolve outside the bounded profile is reported with its reason
/// adds no ramp; the matte keeps its authored keys, colour, Opacity and lifetime.
#[test]
fn unsafe_matte_dissolve_clocks_and_bindings_are_reported_not_keyed() {
    const FRAME: i64 = FrameRate::Fps30.ticks_per_frame();
    let (read, _) =
        inspect_project_with_omissions(&matte_with_head_dissolve(), Some("sequence-1")).unwrap();
    type Mutation = fn(&mut PrSequence);
    let cases: [(Mutation, &str); 9] = [
        (
            |sequence| {
                sequence.video_tracks[0].transitions[0].outgoing_clip =
                    Some("VideoClipTrackItem:2".into());
            },
            "a linked physical picture was not retained",
        ),
        (
            |sequence| {
                let track = &mut sequence.video_tracks[0];
                track.transitions.push(track.transitions[0].clone());
            },
            "multiple dissolves target the same clip opacity",
        ),
        (
            |sequence| {
                let track = &mut sequence.video_tracks[0];
                track.transitions[0].end_ticks = track.clip(0).end_ticks + FRAME;
            },
            "must lie inside its linked clip",
        ),
        // An earlier start keeps the cut at the matte start: a nonzero Alignment.
        (
            |sequence| sequence.video_tracks[0].transitions[0].start_ticks -= FRAME,
            "must lie inside its linked clip",
        ),
        (
            |sequence| sequence.video_tracks[0].clip_mut(0).blend_mode = PrBlendMode::Multiply,
            "with Normal blend",
        ),
        (
            |sequence| {
                let clip = sequence.video_tracks[0].clip_mut(0);
                clip.animations
                    .push(PrPropertyAnimation::Opacity(vec![PrScalarKeyframe {
                        source_ticks: clip.in_ticks,
                        value: 60.0,
                        easing: PrKeyframeEasing::Linear,
                    }]));
            },
            "no animated opacity",
        ),
        (
            |sequence| sequence.video_tracks[0].clip_mut(0).active_transforms = 1,
            "Transform stage",
        ),
        (
            |sequence| {
                sequence.video_tracks[0].transitions[0].incoming_clip =
                    Some("VideoClipTrackItem:9".into());
            },
            "no retained physical occurrence",
        ),
        // Film Impact's measured dissolve still converts on no Color Matte.
        (
            |sequence| {
                sequence.video_tracks[0].transitions[0].kind =
                    PrVideoTransitionKind::FilmImpactDissolve;
            },
            "one-sided dissolve requires an unstaged picture",
        ),
    ];
    for (mutate, reason) in cases {
        let mut sequence = read.sequences[0].clone();
        mutate(&mut sequence);
        let built_in =
            sequence.video_tracks[0].transitions[0].kind == PrVideoTransitionKind::CrossDissolve;
        let (document, omissions) = import(&sequence, &read.media);
        let reports: Vec<_> = omissions
            .iter()
            .filter(|omission| omission.record == "VideoTransitionTrackItem:60")
            .collect();
        assert!(
            !reports.is_empty()
                && reports.iter().all(|omission| {
                    omission.scope == OmissionScope::Feature
                        && omission.kind == OmissionKind::Omitted
                        && omission.reason.contains(reason)
                        && omission.reason.contains("Cross Dissolve detected") == built_in
                }),
            "{reason}: {omissions:?}"
        );
        let entries = document["composition"]["dynamics"]["entries"].as_array();
        if reason == "no animated opacity" {
            let entry = &entries.unwrap()[0];
            let keys = entry["animator"]["keyframes"].as_array().unwrap();
            assert_eq!(
                keys.len(),
                1,
                "no dissolve ramp may replace authored opacity"
            );
            assert_eq!(keys[0]["layerTime"], 0);
            assert_eq!(keys[0]["value"]["value"], 60.0);
        } else {
            assert!(entries.is_none_or(Vec::is_empty), "{reason}: {document}");
        }
        assert_static_matte(&document["composition"]["layers"][0], reason);
    }

    // A saved nondefault control is the reader's rejection: no transition is
    // left to convert, and the matte stays static.
    let xml = matte_with_head_dissolve().replace(
        "</TransitionTrackItem>",
        "</TransitionTrackItem><StartPercent>0.25</StartPercent>",
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("sequence-1")).unwrap();
    assert!(
        omissions.iter().any(|omission| {
            omission.record == "60"
                && omission
                    .reason
                    .contains("nondefault StartPercent is unsupported")
        }),
        "{omissions:?}"
    );
    let (document, omissions) = import(project.single_sequence().unwrap(), &project.media);
    assert!(omissions.is_empty(), "{omissions:?}");
    assert!(document["composition"]["dynamics"]["entries"]
        .as_array()
        .is_none_or(Vec::is_empty));
    assert_static_matte(&document["composition"]["layers"][0], "nondefault control");
}

/// A Color Matte that a Track Matte Key names as its matte gets no Legacy
/// ramp, whether its keyed clip converts or not: a dissolving matte source is
/// unmeasured, and the matte of a keyed clip that conversion omits is dropped
/// after the transitions, which would leave the ramp without its layer.
#[test]
fn a_track_matte_key_source_gets_no_head_dissolve_whether_its_keyed_clip_converts() {
    let (read, _) =
        inspect_project_with_omissions(&matte_with_head_dissolve(), Some("sequence-1")).unwrap();
    let unresolved_composition = PrMediaKind::AfterEffectsComposition(
        PrAfterEffectsComposition::parse("00000001-0000-0000-0000-000000000000").unwrap(),
    );
    for (keyed_media, kind, keyed_converts) in [
        (
            "video",
            PrMediaKind::Video {
                codec: None,
                hdr_profile: None,
            },
            true,
        ),
        // No linked composition is resolved here, so this clip is omitted.
        ("linked", unresolved_composition, false),
    ] {
        let mut sequence = read.sequences[0].clone();
        let matte = sequence.video_tracks[0].clip(0).clone();
        let mut keyed = clip_of(keyed_media, matte.start_ticks..matte.end_ticks, 0);
        keyed.track_matte = Some(PrTrackMatte {
            track_index: 1,
            channel: PrMatteChannel::Alpha,
        });
        sequence
            .video_tracks
            .insert(0, PrVideoTrack::media([keyed]));
        let mut media = read.media.clone();
        let (id, mut source) = named_media(keyed_media);
        source.video.as_mut().unwrap().kind = kind;
        media.insert(id, source);

        let (document, omissions) = import(&sequence, &media);
        let reports: Vec<_> = omissions
            .iter()
            .filter(|omission| omission.record == "VideoTransitionTrackItem:60")
            .collect();
        assert!(
            matches!(reports.as_slice(), [report] if report.kind == OmissionKind::Omitted
                && report.reason.contains("Cross Dissolve detected")
                && report.reason.contains("Track Matte Key")),
            "{keyed_media}: {omissions:?}"
        );
        assert!(
            document["composition"]["dynamics"]["entries"]
                .as_array()
                .is_none_or(Vec::is_empty),
            "{keyed_media}: {document}"
        );
        let layers = document["composition"]["layers"].as_array().unwrap();
        let rect = layers
            .iter()
            .find(|layer| layer["name"] == "Premiere color matte 2");
        if keyed_converts {
            let rect = rect.expect("the keyed video's matte");
            assert_static_matte(rect, keyed_media);
            let video = layers
                .iter()
                .find(|layer| layer["type"] == "Video")
                .unwrap();
            assert_eq!(
                video["trackMatte"],
                json!({"mode": "alpha", "layer": rect["id"]})
            );
        } else {
            assert!(rect.is_none(), "{document}");
            assert!(
                omissions.contains(&Omission {
                    scope: OmissionScope::Occurrence,
                    kind: OmissionKind::Omitted,
                    record: "VideoClipTrackItem:3".into(),
                    reason: "matte source of the omitted clip linked was not converted: Premiere does not draw a track-matte source".into(),
                }),
                "{omissions:?}"
            );
        }
    }
}

#[test]
fn color_matte_resolves_shared_binary_value() {
    // Supplemental wire mutation, not an independently Adobe-authored dedup case.
    let xml = matte_xml("").replace(
        "</PremiereData>",
        r#"<Shared Encoding="base64" BinaryHash="8faeedf7-eb02-d2a5-c178-492000000014">ZEGlAAEAAAA=</Shared></PremiereData>"#,
    );
    let project = inspect_project_with_media(&xml, Some("sequence-1")).unwrap();
    let sequence = project.sequences().next().unwrap();
    let clip = sequence.video_occurrences().next().unwrap();
    let video = project.media(clip).unwrap().video.as_ref().unwrap();
    assert_eq!(
        video.kind,
        PrMediaKind::ColorMatte(PrColorMatte {
            rgb: [0x64, 0x41, 0xa5]
        })
    );
    assert_eq!(
        clip.source_ticks(),
        COLOR_MATTE_SOURCE_IN_TICKS..COLOR_MATTE_SOURCE_IN_TICKS + 5 * TICKS
    );
}

#[test]
fn color_matte_missing_and_conflicting_binary_values_are_rejected() {
    for xml in [
        matte_xml(""),
        matte_xml("ZEGlAAEAAAA=").replace(
            "</PremiereData>",
            r#"<Shared BinaryHash="8faeedf7-eb02-d2a5-c178-492000000014">AAAAAAEAAAA=</Shared></PremiereData>"#,
        ),
    ] {
        assert!(inspect_project_with_media(&xml, Some("sequence-1")).is_err());
    }
}
