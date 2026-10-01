//! Caption track reading. Records follow the corpus caption sequence
//! (`practice_files_transcription_magic`, `CaptionDataClipTrackItem:135`) on
//! the 30 fps one-clip project; each case changes one native field.

use crate::format::{
    inspect_project, inspect_project_with_omissions,
    text_payload::tests::{
        caption_payload, with_caption_marker, BEFORE, CAPTION_STYLE_EFFECTS, CORPUS_CAPTION_CUE,
        CORPUS_CAPTION_SHADOW, CORPUS_CAPTION_TEMPLATE,
    },
};
use crate::schema::{
    text::{
        PrJustification, PrRgb, PrTextBackground, PrTextDocument, PrTextFrame, PrTextStroke,
        PrTextTransform, PrVerticalAlign,
    },
    text_shadow::PrTextShadow,
    PrGraphic, PrSequence, PrText, PrVideoItem, TICKS,
};
use crate::{Omission, OmissionKind, OmissionScope};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::ops::Range;

const SOURCE: &str = include_str!("../../../tests/fixtures/one-clip.xml");
const FRAME: i64 = TICKS / 30;
/// Premiere places a caption's synthetic source at 01:00:00:00 plus its start.
const GENERATOR_IN: i64 = 3600 * TICKS;

/// One cue: its first ObjectID, timeline range in 30 fps frames, and payload.
pub(in crate::format) struct Cue {
    id: u32,
    frames: Range<i64>,
    payload: Vec<u8>,
}

pub(in crate::format) fn cue(id: u32, frames: Range<i64>, text: &str) -> Cue {
    Cue {
        id,
        frames,
        payload: caption_payload(&caption_document(text), PrVerticalAlign::Bottom),
    }
}

/// Single-style caption text that converts: white Arial Bold, centered.
fn caption_document(text: &str) -> PrTextDocument {
    PrTextDocument {
        text: text.into(),
        font: "Arial-BoldMT".into(),
        size: 48.0,
        fill: Some(PrRgb([255; 3])),
        stroke: None,
        shadow: None,
        all_caps: false,
        tracking: 0.0,
        leading: 0.0,
        justification: PrJustification::Center,
        frame: PrTextFrame::Point {
            vertical: PrVerticalAlign::Top,
        },
        background: None,
    }
}

/// The track template in the cues' style; its text is "a", as in the corpus.
fn template() -> Vec<u8> {
    caption_payload(&caption_document("a"), PrVerticalAlign::Bottom)
}

/// The record chain Premiere 25.5 writes for one caption cue.
fn cue_records(cue: &Cue) -> String {
    let id = cue.id;
    let (start, end) = (cue.frames.start * FRAME, cue.frames.end * FRAME);
    let source_in = GENERATOR_IN + start;
    format!(
        r#"
  <CaptionDataClipTrackItem ObjectID="{id}"><DataClipTrackItem Version="1"><ClipTrackItem Version="8"><ComponentOwner Version="1"><Components ObjectRef="{chain}"/></ComponentOwner><TrackItem Version="3"><Start>{start}</Start><End>{end}</End></TrackItem><SubClip ObjectRef="{sub}"/></ClipTrackItem></DataClipTrackItem><BlockVector Version="1"><BlockVectorItem Index="0" ObjectRef="{block}"/></BlockVector><LanguageCodeISO>en-us</LanguageCodeISO></CaptionDataClipTrackItem>
  <DataComponentChain ObjectID="{chain}"><ComponentChain Version="3"></ComponentChain></DataComponentChain>
  <SubClip ObjectID="{sub}"><Clip ObjectRef="{clip}"/><MasterClip ObjectURef="caption-master-{id}"/><OrigChGrp>0</OrigChGrp><Name>SyntheticCaption</Name></SubClip>
  <Block ObjectID="{block}"><FormattedTextData Encoding="base64" BinaryHash="caption-text-{id}">{payload}</FormattedTextData></Block>
  <TranscriptClip ObjectID="{clip}"><DataClip Version="1"><Clip Version="18"><Source ObjectRef="{source}"/><ClipID>caption-clip-{id}</ClipID><InPoint>{source_in}</InPoint><OutPoint>{source_out}</OutPoint></Clip></DataClip></TranscriptClip>
  <DataMediaSource ObjectID="{source}"><MediaSource Version="4"><Content Version="10"></Content><Media ObjectURef="caption-media-{id}"/></MediaSource><OriginalDuration>10973491200000000</OriginalDuration></DataMediaSource>
  <Media ObjectUID="caption-media-{id}"><DataStream ObjectRef="{stream}"/><FilePath>1396920390</FilePath><ImplementationID>42008e7a-de6f-4270-96de-7e287abb9b4b</ImplementationID><Title>SyntheticCaption</Title><Infinite>true</Infinite><ActualMediaFilePath>1396920390</ActualMediaFilePath></Media>
  <DataStream ObjectID="{stream}"><FrameRate>8467200000</FrameRate><Duration>10973491200000000</Duration></DataStream>
  <MasterClip ObjectUID="caption-master-{id}"><Clips Version="1"><Clip Index="0" ObjectRef="{template}"/></Clips><Name>SyntheticCaption</Name><MasterClipChangeVersion>0</MasterClipChangeVersion></MasterClip>
  <TranscriptClip ObjectID="{template}"><DataClip Version="1"><Clip Version="18"><Source ObjectRef="{source}"/><ClipID>caption-template-{id}</ClipID></Clip></DataClip></TranscriptClip>"#,
        chain = id + 1,
        sub = id + 2,
        block = id + 3,
        clip = id + 4,
        source = id + 5,
        stream = id + 6,
        template = id + 7,
        source_out = source_in + end - start,
        payload = STANDARD.encode(&cue.payload),
    )
}

/// The one-clip project (video 0-5 s) with one caption track per entry of
/// `tracks`, at track Index 0, 1, ..., each with the [`template`] style.
pub(in crate::format) fn captions_xml(tracks: &[Vec<Cue>]) -> String {
    let mut references = String::new();
    let mut records = String::new();
    let style = STANDARD.encode(template());
    for (index, cues) in tracks.iter().enumerate() {
        references.push_str(&format!(
            r#"<Track Index="{index}" ObjectURef="caption-track-{index}"/>"#
        ));
        let items: String = cues
            .iter()
            .enumerate()
            .map(|(position, cue)| {
                format!(r#"<TrackItem Index="{position}" ObjectRef="{}"/>"#, cue.id)
            })
            .collect();
        records.push_str(&format!(
            r#"
  <CaptionDataClipTrack ObjectUID="caption-track-{index}"><DataClipTrack Version="1"><ClipTrack Version="2"><Track Version="3"><ID>{id}</ID><IsLocked>false</IsLocked><MediaType>d8143ffe-eec4-4d2a-a909-d5f7bf094dc5</MediaType><Index>{index}</Index><IsMuted>false</IsMuted><IsSyncLocked>true</IsSyncLocked></Track><ClipItems Version="3"><TrackItems Version="1">{items}</TrackItems><MediaType>d8143ffe-eec4-4d2a-a909-d5f7bf094dc5</MediaType><Index>{index}</Index></ClipItems><TransitionItems Version="3"><MediaType>d8143ffe-eec4-4d2a-a909-d5f7bf094dc5</MediaType><Index>{index}</Index></TransitionItems></ClipTrack></DataClipTrack><CaptionDataTemplateStyle Encoding="base64" BinaryHash="caption-style-{index}">{style}</CaptionDataTemplateStyle></CaptionDataClipTrack>"#,
            id = index + 1
        ));
        for cue in cues {
            records.push_str(&cue_records(cue));
        }
    }
    SOURCE
        .replace(
            "</TrackGroups>",
            r#"<TrackGroup><Second ObjectRef="100"/></TrackGroup></TrackGroups>"#,
        )
        .replace(
            "</PremiereData>",
            &format!(
                r#"  <ProjectSettings ObjectID="90"><TitleSafeWidth>20</TitleSafeWidth><TitleSafeHeight>20</TitleSafeHeight></ProjectSettings>
  <DataTrackGroup ObjectID="100"><TrackGroup><Tracks>{references}</Tracks><FrameRate>8467200000</FrameRate></TrackGroup></DataTrackGroup>{records}
</PremiereData>"#
            ),
        )
}

/// `xml` with the `CaptionDataTemplateStyle` text of track `index` replaced.
fn with_template(xml: &str, index: usize, base64: &str) -> String {
    let style = |value: &str| {
        format!(r#"BinaryHash="caption-style-{index}">{value}</CaptionDataTemplateStyle>"#)
    };
    let default = style(&STANDARD.encode(template()));
    assert!(
        xml.contains(&default),
        "track {index} keeps the default template"
    );
    xml.replace(&default, &style(base64))
}

fn graphics(track: &[PrVideoItem]) -> Vec<&PrGraphic> {
    track
        .iter()
        .map(|item| match item {
            PrVideoItem::Graphic(graphic) => graphic,
            PrVideoItem::Media(_) => panic!("caption lanes hold graphics only"),
        })
        .collect()
}

/// One caption as `(record, name, ticks, text)`.
type CaptionRow<'a> = (&'a str, &'a str, Range<i64>, &'a str);

/// Each caption lane above `video_tracks` video tracks.
fn caption_lanes(sequence: &PrSequence, video_tracks: usize) -> Vec<Vec<CaptionRow<'_>>> {
    sequence
        .video_tracks()
        .skip(video_tracks)
        .map(|track| {
            graphics(track)
                .into_iter()
                .map(|graphic| {
                    (
                        graphic.id().unwrap(),
                        graphic.text().name.as_str(),
                        graphic.timeline_ticks(),
                        graphic.text().document.text.as_str(),
                    )
                })
                .collect()
        })
        .collect()
}

/// The layer name and picture output of every caption, lane by lane.
fn caption_visibility(sequence: &PrSequence, video_tracks: usize) -> Vec<Vec<(&str, bool)>> {
    sequence
        .video_tracks()
        .skip(video_tracks)
        .map(|track| {
            graphics(track)
                .into_iter()
                .map(|graphic| (graphic.text().name.as_str(), graphic.enabled))
                .collect()
        })
        .collect()
}

fn seconds(range: Range<f64>) -> Range<i64> {
    (range.start * TICKS as f64) as i64..(range.end * TICKS as f64) as i64
}

#[test]
fn caption_tracks_become_timed_text_lanes_above_the_video() {
    let xml = captions_xml(&[
        vec![
            cue(200, 30..90, "First cue"),
            cue(220, 180..240, "Third after gap\nwith a second line"),
        ],
        vec![cue(210, 75..135, "Second cue overlaps")],
    ]);
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    // Captions are synthetic: the only media is the video source.
    assert_eq!(project.media.len(), 1);
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.video_occurrences().count(), 1);
    // Overlapping cues stay separate layers; the gap from 3 to 6 s stays empty.
    assert_eq!(
        caption_lanes(sequence, 1),
        [
            vec![
                (
                    "CaptionDataClipTrackItem:200",
                    "C1 caption 1",
                    seconds(1.0..3.0),
                    "First cue"
                ),
                (
                    "CaptionDataClipTrackItem:220",
                    "C1 caption 2",
                    seconds(6.0..8.0),
                    "Third after gap\nwith a second line"
                ),
            ],
            vec![(
                "CaptionDataClipTrackItem:210",
                "C2 caption 1",
                seconds(2.5..4.5),
                "Second cue overlaps"
            )],
        ]
    );
    assert_eq!(
        caption_visibility(sequence, 1),
        [
            vec![("C1 caption 1", true), ("C1 caption 2", true)],
            vec![("C2 caption 1", true)],
        ]
    );
    assert_eq!(sequence.end_ticks(), 8 * TICKS);

    // Text-only caption timelines convert.
    let captions_only = xml.replace(r#"<TrackItem ObjectRef="3"/>"#, "");
    let sequence = inspect_project(&captions_only, None).unwrap();
    assert_eq!(sequence.video_occurrences().count(), 0);
    assert_eq!(sequence.video_items().count(), 3);
}

/// The baselines of `lines` lines of `text` in the FX renderer's bottom-aligned box
/// text layout (`scene::box_text_line_capacity`, `box_vertical_align_offset`):
/// without an authored first baseline, the first sits one size below the box
/// top, and the text moves down by whole slots of 120% of the size.
fn box_text_baselines(text: &PrText, lines: u32) -> Vec<f64> {
    let PrTextFrame::Box { height, .. } = text.document.frame else {
        panic!("captions are box text");
    };
    let size = f64::from(text.document.size);
    let slot = 1.2 * size;
    let capacity = 1 + ((f64::from(height) - size) / slot).floor() as u32;
    assert!(lines <= capacity, "{lines} lines fit the box");
    let top = text.transform.position[1] - text.transform.anchor[1];
    let first = top + size + f64::from(capacity - lines) * slot;
    (0..lines)
        .map(|line| first + f64::from(line) * slot)
        .collect()
}

#[test]
fn bottom_centre_captions_end_on_premieres_baseline_at_95_percent_height() {
    // AME renders of the pinned fixture and its derivatives put the last
    // baseline at y = 1026 on 1920x1080 for 48 and 72 px and one or two lines.
    for (size, box_height, pivot) in [(48.0, 998.0, 969.6), (72.0, 1066.0, 1022.4)] {
        let document = PrTextDocument {
            size,
            ..caption_document("First cue")
        };
        let style = PrTextDocument {
            text: "a".into(),
            ..document.clone()
        };
        let xml = with_template(
            &captions_xml(&[vec![Cue {
                id: 200,
                frames: 30..90,
                payload: caption_payload(&document, PrVerticalAlign::Bottom),
            }]]),
            0,
            &STANDARD.encode(caption_payload(&style, PrVerticalAlign::Bottom)),
        );
        let sequence = inspect_project(&xml, None).unwrap();
        let lane = sequence.video_tracks().nth(1).unwrap();
        let text = &graphics(lane)[0].text();
        assert_eq!(
            text.document,
            PrTextDocument {
                frame: PrTextFrame::Box {
                    width: 1536.0,
                    height: box_height,
                    vertical: PrVerticalAlign::Bottom,
                },
                ..document
            }
        );
        // The pivot is the last baseline, centred on the frame.
        assert_eq!(
            text.transform,
            PrTextTransform {
                position: [960.0, 1026.0],
                anchor: [768.0, pivot],
                scale: 100.0,
                rotation: 0.0,
                opacity: 100.0,
            }
        );
        let slot = 1.2 * f64::from(size);
        for lines in 1..=4 {
            let baselines = box_text_baselines(text, lines);
            let expected = (0..lines).map(|line| 1026.0 - f64::from(lines - 1 - line) * slot);
            for (actual, expected) in baselines.iter().zip(expected) {
                assert!((actual - expected).abs() < 1e-9, "{size} px, {lines} lines");
            }
        }
    }
}

#[test]
fn corpus_caption_cue_converts_with_its_shadow() {
    // The corpus cue's first ten frames, on the 30 fps clock, under its own
    // track template.
    let corpus = Cue {
        id: 200,
        frames: 0..10,
        payload: STANDARD.decode(CORPUS_CAPTION_CUE).unwrap(),
    };
    let xml = with_template(&captions_xml(&[vec![corpus]]), 0, CORPUS_CAPTION_TEMPLATE);
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let lane = sequence.video_tracks().nth(1).unwrap();
    let graphic = graphics(lane)[0];
    assert_eq!(graphic.timeline_ticks(), 0..10 * FRAME);
    let document = &graphic.text().document;
    assert_eq!(
        (
            document.text.as_str(),
            document.font.as_str(),
            document.size,
            document.shadow
        ),
        (
            "It's been",
            "MinionPro-Regular",
            48.0,
            Some(CORPUS_CAPTION_SHADOW)
        )
    );
}

#[test]
fn corpus_caption_cues_keep_their_shadows_and_report_their_font_once() {
    use crate::{convert, tesseract_output::asset_ids_in_order};
    use fx_schema::{EffectData, EffectPayload, LayerEffect};
    let payload = STANDARD.decode(CORPUS_CAPTION_CUE).unwrap();
    let cues = (0..42)
        .map(|index| Cue {
            id: 200 + 10 * index,
            frames: i64::from(index) * 10..i64::from(index + 1) * 10,
            payload: payload.clone(),
        })
        .collect();
    let xml = with_template(&captions_xml(&[cues]), 0, CORPUS_CAPTION_TEMPLATE);
    let (project, mut omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let document = convert::premiere_to_tesseract(
        sequence,
        &project.media,
        &asset_ids_in_order(sequence, &project.media),
        &mut omissions,
    )
    .unwrap();
    let text: Vec<_> = document
        .composition()
        .layers()
        .iter()
        .filter_map(|layer| {
            if let fx_schema::LayerData::Text(text) = layer.data() {
                Some(text)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(text.len(), 42);
    for text in text {
        assert_eq!(text.source_text.font_family.as_ref(), "MinionPro-Regular");
        assert_eq!(text.source_text.font_style.as_ref(), "");
        // The cue's shadow is the layer's one effect.
        let [effect] = text.effects.as_slice() else {
            panic!("{}: expected one effect", text.name);
        };
        assert!(
            matches!(
                effect.data(),
                EffectData::Identified {
                    enabled: true,
                    effect: EffectPayload::Known(LayerEffect::DropShadow(_)),
                    ..
                }
            ),
            "{}: {effect:?}",
            text.name
        );
    }
    assert_eq!(omissions, [Omission {
        scope: OmissionScope::Feature,
        kind: OmissionKind::Omitted,
        record: "CaptionDataClipTrackItem:200 (\"C1 caption 1\") and 41 more text layers".into(),
        reason: "font \"MinionPro-Regular\" is not packaged in this document; import it with tsrct project import-font before preview or export.".into(),
    }]);
}

fn omission_reasons(xml: &str) -> (usize, Vec<(OmissionScope, String, String)>) {
    let (project, omissions) = inspect_project_with_omissions(xml, None).unwrap();
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.video_occurrences().count(), 1, "video survives");
    let reasons = omissions
        .into_iter()
        .map(|omission| (omission.scope, omission.record, omission.reason))
        .collect();
    (sequence.video_items().count() - 1, reasons)
}

/// One expected omission as returned by [`omission_reasons`].
fn omitted(scope: OmissionScope, record: &str, reason: &str) -> (OmissionScope, String, String) {
    (scope, record.to_owned(), reason.to_owned())
}

#[test]
fn caption_styling_beyond_font_size_fill_stroke_shadow_and_background_is_omitted_by_attribute() {
    let styled = |document: PrTextDocument, alignment| {
        vec![Cue {
            id: 200,
            frames: 30..90,
            payload: caption_payload(&document, alignment),
        }]
    };
    let base = caption_document("Styled");
    let bottom = PrVerticalAlign::Bottom;
    let cases = [
        (
            styled(
                PrTextDocument {
                    all_caps: true,
                    ..base.clone()
                },
                bottom,
            ),
            "all caps",
        ),
        (
            styled(
                PrTextDocument {
                    fill: None,
                    ..base.clone()
                },
                bottom,
            ),
            "disabled text fill",
        ),
        (
            styled(
                PrTextDocument {
                    tracking: 50.0,
                    leading: 12.0,
                    ..base.clone()
                },
                bottom,
            ),
            "tracking, leading",
        ),
        (
            styled(
                PrTextDocument {
                    justification: PrJustification::Right,
                    ..base.clone()
                },
                PrVerticalAlign::Top,
            ),
            "justification other than center or left, alignment other than bottom",
        ),
        (
            styled(
                PrTextDocument {
                    frame: PrTextFrame::Box {
                        width: 800.0,
                        height: 200.0,
                        vertical: bottom,
                    },
                    ..base.clone()
                },
                bottom,
            ),
            "text box",
        ),
        (
            // A caption style preview: centered and bottom-aligned like the
            // corpus cues, with shadow, background and all caps enabled; the
            // shadow and the background are not reasons.
            vec![Cue {
                id: 200,
                frames: 30..90,
                payload: with_caption_marker(STANDARD.decode(CAPTION_STYLE_EFFECTS).unwrap()),
            }],
            "all caps",
        ),
    ];
    for (cues, attributes) in cases {
        let (captions, reasons) = omission_reasons(&captions_xml(&[cues]));
        assert_eq!(captions, 0, "{attributes}");
        assert_eq!(
            reasons,
            [omitted(
                OmissionScope::Occurrence,
                "CaptionDataClipTrackItem:200",
                &format!("C1 caption 1: unsupported conversion: {attributes} not converted")
            )]
        );
    }

    // A stroke and a shadow convert through the Type-tool text mappings when
    // the track template has the same style.
    let stroked = PrTextDocument {
        stroke: Some(PrTextStroke {
            color: PrRgb([0; 3]),
            width: 4.0,
        }),
        shadow: Some(CORPUS_CAPTION_SHADOW),
        ..base
    };
    let xml = with_template(
        &captions_xml(&[styled(stroked.clone(), bottom)]),
        0,
        &STANDARD.encode(caption_payload(
            &PrTextDocument {
                text: "a".into(),
                ..stroked.clone()
            },
            bottom,
        )),
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let document = &graphics(sequence.video_tracks().nth(1).unwrap())[0]
        .text()
        .document;
    assert_eq!(
        (document.stroke, document.shadow),
        (stroked.stroke, stroked.shadow)
    );
}

#[test]
fn a_cue_converts_from_its_own_style_whatever_its_track_template() {
    let one = || captions_xml(&[vec![cue(200, 30..90, "Only cue")]]);
    let track = "CaptionDataClipTrack:caption-track-0";
    let differing = |document: PrTextDocument| {
        STANDARD.encode(caption_payload(&document, PrVerticalAlign::Bottom))
    };
    // Premiere draws the cue's own payload (AME renders: a cue's own fill and
    // a cue without a background over a background template), so a template
    // in another font, size and shadow only has to decode.
    for template in [
        differing(PrTextDocument {
            font: "MinionPro-Regular".into(),
            size: 60.0,
            ..caption_document("a")
        }),
        CORPUS_CAPTION_TEMPLATE.to_owned(),
        differing(PrTextDocument {
            shadow: Some(CORPUS_CAPTION_SHADOW),
            ..caption_document("a")
        }),
    ] {
        let (project, omissions) =
            inspect_project_with_omissions(&with_template(&one(), 0, &template), None).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let sequence = project.single_sequence().unwrap();
        let document = &graphics(sequence.video_tracks().nth(1).unwrap())[0]
            .text()
            .document;
        assert_eq!(
            PrTextDocument {
                frame: PrTextFrame::Point {
                    vertical: PrVerticalAlign::Top
                },
                ..document.clone()
            },
            caption_document("Only cue")
        );
    }
    // A template that does not decode still omits its track: an unknown
    // caption layout fails closed before any cue is read.
    let cases = [
        (
            with_template(&one(), 0, "AA=="),
            omitted(
                OmissionScope::Track,
                track,
                "unsupported conversion: CaptionDataClipTrack:caption-track-0: CaptionDataTemplateStyle: truncated Source Text payload",
            ),
        ),
        (
            one().replace(
                &format!(
                    r#"<CaptionDataTemplateStyle Encoding="base64" BinaryHash="caption-style-0">{}</CaptionDataTemplateStyle>"#,
                    STANDARD.encode(template())
                ),
                "",
            ),
            omitted(
                OmissionScope::Track,
                track,
                "unsupported conversion: CaptionDataClipTrack:caption-track-0: missing CaptionDataTemplateStyle",
            ),
        ),
    ];
    for (xml, expected) in cases {
        let (captions, reasons) = omission_reasons(&xml);
        assert_eq!(captions, 0, "{expected:?}");
        assert_eq!(reasons, [expected]);
    }
}

#[test]
fn caption_backgrounds_convert_at_the_calibrated_size_or_are_reported() {
    use crate::format::text_payload::tests::FIXTURE_BACKGROUND;
    let cue_with = |text: &str, document: PrTextDocument| {
        captions_xml(&[vec![Cue {
            id: 200,
            frames: 30..90,
            payload: caption_payload(
                &PrTextDocument {
                    text: text.into(),
                    ..document
                },
                PrVerticalAlign::Bottom,
            ),
        }]])
    };
    let converted = |xml: &str| {
        let (project, omissions) = inspect_project_with_omissions(xml, None).unwrap();
        let sequence = project.single_sequence().unwrap();
        let background = graphics(sequence.video_tracks().nth(1).unwrap())[0]
            .text()
            .document
            .background;
        let reasons: Vec<_> = omissions
            .into_iter()
            .map(|omission| (omission.scope, omission.record, omission.reason))
            .collect();
        (background, reasons)
    };
    let with_background = |background| PrTextDocument {
        background: Some(background),
        ..caption_document("")
    };
    // The fixture's C4 box, and the same box behind two lines, whose block
    // is taller: the cue converts with its background and no omission.
    for text in ["C4 BACKGROUND", "C5 FIRST LINE\nSECOND LINE"] {
        assert_eq!(
            converted(&cue_with(text, with_background(FIXTURE_BACKGROUND))),
            (Some(FIXTURE_BACKGROUND), Vec::new()),
            "{text}"
        );
    }
    // A box calibration does not cover is reported alone; the text converts.
    for (text, document, reason) in [
        (
            "Other size",
            PrTextDocument {
                size: 60.0,
                ..with_background(FIXTURE_BACKGROUND)
            },
            "the box is calibrated at 48 px text only, not 60 px",
        ),
        (
            "Translucent",
            with_background(PrTextBackground {
                opacity: 60.0,
                ..FIXTURE_BACKGROUND
            }),
            "only opacity 100 was rendered, not 60",
        ),
        (
            "Round ends",
            with_background(PrTextBackground {
                radius: 30.0,
                ..FIXTURE_BACKGROUND
            }),
            "how Premiere clamps a corner radius (30) above half the box height (at least 22) is unknown",
        ),
    ] {
        let xml = cue_with(text, document);
        assert_eq!(
            converted(&xml),
            (
                None,
                vec![omitted(
                    OmissionScope::Feature,
                    "CaptionDataClipTrackItem:200",
                    &format!("C1 caption 1: text background not converted: {reason}")
                )]
            ),
            "{text}"
        );
    }
}

#[test]
fn left_justified_cues_start_at_the_inferred_margin_and_right_ones_are_omitted() {
    let justified = |justification| {
        captions_xml(&[vec![Cue {
            id: 200,
            frames: 30..90,
            payload: caption_payload(
                &PrTextDocument {
                    justification,
                    ..caption_document("Justified")
                },
                PrVerticalAlign::Bottom,
            ),
        }]])
    };
    // One left cue's ink started at x 242 and one right cue's ended at x
    // 1677 on 1920 (AME renders), so a left cue's box spans the middle 75%.
    let (project, omissions) =
        inspect_project_with_omissions(&justified(PrJustification::Left), None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let text = graphics(sequence.video_tracks().nth(1).unwrap())[0].text();
    assert_eq!(text.document.justification, PrJustification::Left);
    assert_eq!(
        text.document.frame,
        PrTextFrame::Box {
            width: 1440.0,
            height: 998.0,
            vertical: PrVerticalAlign::Bottom,
        }
    );
    assert_eq!(text.transform.position, [960.0, 1026.0]);
    assert_eq!(text.transform.anchor, [720.0, 969.6]);
    // No render shows a right-justified cue.
    assert_eq!(
        omission_reasons(&justified(PrJustification::Right)),
        (
            0,
            vec![omitted(
                OmissionScope::Occurrence,
                "CaptionDataClipTrackItem:200",
                "C1 caption 1: unsupported conversion: justification other than center or left not converted",
            )]
        )
    );
}

#[test]
fn hidden_caption_tracks_and_disabled_cues_import_as_hidden_text_layers() {
    use crate::convert;
    use crate::tesseract_output::asset_ids_in_order;
    use fx_schema::LayerData;
    let xml = captions_xml(&[
        vec![cue(200, 30..90, "Shown"), cue(220, 180..240, "Disabled")],
        vec![cue(210, 75..135, "On a hidden track")],
    ])
    // Cue Enable off on C1 caption 2 and track output off on C2.
    .replace(
        r#"<SubClip ObjectRef="222"/></ClipTrackItem>"#,
        r#"<SubClip ObjectRef="222"/><IsMuted>true</IsMuted></ClipTrackItem>"#,
    )
    .replace(
        "<Index>1</Index><IsMuted>false</IsMuted>",
        "<Index>1</Index><IsMuted>true</IsMuted>",
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    assert_eq!(
        caption_visibility(sequence, 1),
        [
            vec![("C1 caption 1", true), ("C1 caption 2", false)],
            vec![("C2 caption 1", false)],
        ]
    );
    // Hidden captions keep their wording, timing and placement.
    assert_eq!(caption_lanes(sequence, 1)[1][0].2, seconds(2.5..4.5));

    let mut omissions = Vec::new();
    let document = convert::premiere_to_tesseract(
        sequence,
        &project.media,
        &asset_ids_in_order(sequence, &project.media),
        &mut omissions,
    )
    .unwrap();
    assert_eq!(omissions.len(), 1);
    assert!(omissions[0]
        .reason
        .contains("is not packaged in this document"));
    omissions.clear();
    let hidden: Vec<_> = document
        .composition()
        .layers()
        .iter()
        .filter_map(|layer| match layer.data() {
            LayerData::Text(text) => Some((text.name.as_str(), text.is_hidden)),
            _ => None,
        })
        .collect();
    assert_eq!(
        hidden,
        [
            ("C2 caption 1", true),
            ("C1 caption 1", false),
            ("C1 caption 2", true),
        ]
    );
    // Hidden text exports as a disabled graphic, so both hidden captions keep
    // their clips with Enable off. The 8 s document end, set by the hidden C1
    // cue, stays as a trailing tail after the 5 s video.
    let media = project
        .media
        .values()
        .next()
        .unwrap()
        .video
        .as_ref()
        .unwrap();
    let facts = std::collections::BTreeMap::from([(
        "premiere-video-1".to_owned(),
        crate::media::MediaFacts::Video(crate::media::VideoMedia {
            orientation: crate::schema::VideoOrientation::Identity,
            codec: crate::schema::VideoCodec::H264,
            bit_depth: 8,
            colour: None,
            width: media.width,
            height: media.height,
            timing: crate::media::VideoTiming::for_test(
                media.frame_rate.supported().unwrap(),
                media.intrinsic_ticks,
            ),
        }),
    )]);
    let exported = convert::tesseract_to_premiere(
        &document,
        &facts,
        &Default::default(),
        &Default::default(),
        crate::format::FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        exported
            .sequences()
            .map(PrSequence::end_ticks)
            .collect::<Vec<_>>(),
        [8 * TICKS]
    );
    let mut exported_text: Vec<_> = exported
        .sequences()
        .flat_map(PrSequence::video_items)
        .filter_map(|item| match item {
            PrVideoItem::Graphic(graphic) => Some((graphic.text().name.as_str(), graphic.enabled)),
            PrVideoItem::Media(_) => None,
        })
        .collect();
    exported_text.sort_unstable();
    assert_eq!(
        exported_text,
        [
            ("C1 caption 1", true),
            ("C1 caption 2", false),
            ("C2 caption 1", false),
        ]
    );
}

#[test]
fn unsupported_caption_structure_is_omitted_without_failing_the_sequence() {
    let one = || captions_xml(&[vec![cue(200, 30..90, "Only cue")]]);
    let track = |reason: &str| {
        omitted(
            OmissionScope::Track,
            "CaptionDataClipTrack:caption-track-0",
            reason,
        )
    };
    let item = |reason: &str| {
        omitted(
            OmissionScope::Occurrence,
            "CaptionDataClipTrackItem:200",
            &format!("C1 caption 1: unsupported conversion: {reason}"),
        )
    };
    let payload = STANDARD.encode(&cue(200, 30..90, "Only cue").payload);
    let cases = [
        (
            one().replace(
                "<IsMuted>false</IsMuted><IsSyncLocked>",
                "<IsMuted>maybe</IsMuted><IsSyncLocked>",
            ),
            track("unsupported conversion: CaptionDataClipTrack:caption-track-0: invalid IsMuted"),
        ),
        (
            one().replace(
                "<SubClip ObjectRef=\"202\"/></ClipTrackItem>",
                "<SubClip ObjectRef=\"202\"/><IsMuted>1</IsMuted></ClipTrackItem>",
            ),
            item("CaptionDataClipTrackItem:200: invalid IsMuted"),
        ),
        (
            one().replace("<Index>0</Index><IsMuted>", "<Index>-1</Index><IsMuted>"),
            track("unsupported conversion: caption track Index must be nonnegative"),
        ),
        (
            one().replace(
                "</TrackItems><MediaType>d8143ffe-eec4-4d2a-a909-d5f7bf094dc5</MediaType><Index>0</Index>",
                "</TrackItems><MediaType>d8143ffe-eec4-4d2a-a909-d5f7bf094dc5</MediaType><Index>1</Index>",
            ),
            track("unsupported conversion: conflicting caption ClipItems Index or MediaType"),
        ),
        (
            one().replace(
                "<IsLocked>false</IsLocked><MediaType>d8143ffe-eec4-4d2a-a909-d5f7bf094dc5</MediaType>",
                "<IsLocked>false</IsLocked><MediaType>228cda18-3625-4d2d-951e-348879e4ed93</MediaType>",
            ),
            track("unsupported conversion: conflicting caption Track Index or MediaType"),
        ),
        (
            one().replace(
                "<TransitionItems Version=\"3\"><MediaType>d8143ffe-eec4-4d2a-a909-d5f7bf094dc5</MediaType>",
                "<TransitionItems Version=\"3\"><MediaType>228cda18-3625-4d2d-951e-348879e4ed93</MediaType>",
            ),
            track("unsupported conversion: conflicting caption TransitionItems Index or MediaType"),
        ),
        (
            one().replace(
                r#"<BlockVectorItem Index="0" ObjectRef="203"/>"#,
                r#"<BlockVectorItem Index="0" ObjectRef="203"/><BlockVectorItem Index="1" ObjectRef="203"/>"#,
            ),
            item("captions with 2 text blocks are unsupported"),
        ),
        (
            one().replace(
                r#"<ComponentChain Version="3"></ComponentChain>"#,
                r#"<ComponentChain Version="3"><Components><Component Index="0" ObjectRef="4"/></Components></ComponentChain>"#,
            ),
            item("DataComponentChain:201: caption effects are unsupported"),
        ),
        (
            one().replace(
                &format!("<OutPoint>{}</OutPoint>", GENERATOR_IN + 90 * FRAME),
                &format!("<OutPoint>{}</OutPoint>", GENERATOR_IN + 91 * FRAME),
            ),
            item("TranscriptClip:204: caption retiming is unsupported"),
        ),
        (
            one().replace(
                "<ClipID>caption-clip-200</ClipID>",
                "<PlaybackSpeed>2</PlaybackSpeed><ClipID>caption-clip-200</ClipID>",
            ),
            item("TranscriptClip:204: caption retiming is unsupported"),
        ),
        (
            one().replace(
                "<ClipID>caption-clip-200</ClipID>",
                "<PlayBackwards>true</PlayBackwards><ClipID>caption-clip-200</ClipID>",
            ),
            item("TranscriptClip:204: caption retiming is unsupported"),
        ),
        (
            one().replace(
                "<ClipID>caption-clip-200</ClipID>",
                "<TimeRemapping ObjectRef=\"4\"/><ClipID>caption-clip-200</ClipID>",
            ),
            item("TranscriptClip:204: caption retiming is unsupported"),
        ),
        (
            one()
                .replace(
                    &format!("<Start>{}</Start>", 30 * FRAME),
                    &format!("<Start>{}</Start>", 30 * FRAME + 1),
                )
                .replace(
                    &format!("<End>{}</End>", 90 * FRAME),
                    &format!("<End>{}</End>", 90 * FRAME + 1),
                ),
            omitted(
                OmissionScope::Occurrence,
                "CaptionDataClipTrackItem:200",
                "C1 caption 1: invalid Premiere project: timeline start must align to a 30 fps sequence frame boundary",
            ),
        ),
        (
            one().replace(
                "<OriginalDuration>10973491200000000</OriginalDuration>",
                "<OriginalDuration>2540160000000</OriginalDuration>",
            ),
            item("DataMediaSource:205: unexpected caption source duration"),
        ),
        (
            one().replace(
                "<FilePath>1396920390</FilePath>",
                "<FilePath>1196574294</FilePath>",
            ),
            item("Media:caption-media-200: unexpected caption generator media"),
        ),
        (
            one().replace(
                "<DataStream ObjectID=\"206\"><FrameRate>8467200000</FrameRate>",
                "<DataStream ObjectID=\"206\"><FrameRate>8475667200</FrameRate>",
            ),
            item("DataStream:206: caption stream duration or rate differs from its sequence"),
        ),
        (
            one().replace(
                r#"<Clips Version="1"><Clip Index="0" ObjectRef="207"/></Clips>"#,
                r#"<Clips Version="1"><Clip Index="0" ObjectRef="207"/><Clip Index="1" ObjectRef="207"/></Clips>"#,
            ),
            item("MasterClip:caption-master-200: multiple caption source clips are unsupported"),
        ),
        (
            // The master clip's template plays another source (here the video's).
            one().replace(
                r#"<Clip Version="18"><Source ObjectRef="205"/><ClipID>caption-template-200</ClipID>"#,
                r#"<Clip Version="18"><Source ObjectRef="7"/><ClipID>caption-template-200</ClipID>"#,
            ),
            item("MasterClip:caption-master-200: caption source identity mismatch"),
        ),
        (
            captions_xml(&[vec![Cue {
                id: 200,
                frames: 30..90,
                payload: STANDARD.decode(BEFORE).unwrap(),
            }]]),
            item("Block:203: FormattedTextData: unsupported Source Text field document[26]"),
        ),
        (
            one().replace(
                r#"<FormattedTextData Encoding="base64""#,
                r#"<FormattedTextData Encoding="hex""#,
            ),
            item("Block:203: unsupported FormattedTextData encoding"),
        ),
        (
            one().replace(&payload, "!!!!"),
            item("Block:203: invalid FormattedTextData base64: Invalid symbol 33, offset 0."),
        ),
    ];
    for (xml, expected) in cases {
        let (captions, reasons) = omission_reasons(&xml);
        assert_eq!(captions, 0, "{expected:?}");
        assert_eq!(reasons, [expected]);
    }
}

#[test]
fn caption_placement_ignores_the_project_safe_margins() {
    // AME renders with TitleSafe 10/10 or ActionSafe 20/20 decode identically
    // to the default-margin render, so no margin setting moves a caption.
    let one = || captions_xml(&[vec![cue(200, 30..90, "Only cue")]]);
    let placement = |xml: &str| {
        let (project, omissions) = inspect_project_with_omissions(xml, None).unwrap();
        assert!(omissions.is_empty(), "{omissions:?}");
        let sequence = project.single_sequence().unwrap();
        let text = graphics(sequence.video_tracks().nth(1).unwrap())[0]
            .text()
            .clone();
        (text.document.frame, text.transform)
    };
    let default = placement(&one());
    for xml in [
        one()
            .replace(
                "<TitleSafeWidth>20</TitleSafeWidth>",
                "<TitleSafeWidth>10</TitleSafeWidth>",
            )
            .replace(
                "<TitleSafeHeight>20</TitleSafeHeight>",
                "<TitleSafeHeight>10</TitleSafeHeight>",
            ),
        one().replace(
            "<TitleSafeWidth>",
            "<ActionSafeWidth>20</ActionSafeWidth><ActionSafeHeight>20</ActionSafeHeight><TitleSafeWidth>",
        ),
        one().replace(
            r#"<ProjectSettings ObjectID="90"><TitleSafeWidth>20</TitleSafeWidth><TitleSafeHeight>20</TitleSafeHeight></ProjectSettings>"#,
            "",
        ),
    ] {
        assert_eq!(placement(&xml), default);
    }
}

#[test]
fn a_custom_canvas_keeps_its_captions_at_their_stored_size_with_one_warning() {
    // An AME render placed the 48 px cues by the frame rule on 720x1280 (last
    // baseline at y 1216) but drew them at 2/3 size. Which dimension scales
    // them is unmeasured, so the cues keep their stored size and the sequence
    // reports that approximation once, not once per cue.
    let portrait = "<FrameRect>0,0,720,1280</FrameRect>";
    let xml = captions_xml(&[
        vec![cue(200, 30..90, "First cue")],
        vec![cue(300, 120..150, "Second cue")],
    ])
    .replacen(
        "</TrackGroup><FrameRect>0,0,1920,1080</FrameRect>",
        &format!("</TrackGroup>{portrait}"),
        1,
    )
    .replacen(
        "</ClipTrackItem><FrameRect>0,0,1920,1080</FrameRect>",
        &format!("</ClipTrackItem>{portrait}"),
        1,
    );
    assert_eq!(xml.matches(portrait).count(), 2);
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.video_occurrences().count(), 1);
    for (lane, text) in [(1, "First cue"), (2, "Second cue")] {
        let cues = graphics(sequence.video_tracks().nth(lane).unwrap());
        let cue = cues[0].text();
        assert_eq!(cue.document.text, text);
        assert_eq!(cue.document.size, 48.0);
        assert_eq!(cue.transform.position, [360.0, 1216.0]);
        assert_eq!(cue.transform.scale, 100.0);
    }
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Feature,
            kind: OmissionKind::Approximated,
            record: "sequence-1 (\"Main\")".into(),
            reason: "caption size on a 720x1280 canvas: every converted cue keeps its stored pixel size; Premiere draws a caption at that size on 1920x1080 and scales it with another frame (48 px cues drew at 2/3 size on 720x1280) by an unmeasured rule, so a cue's text, line spacing, stroke, shadow and background box can draw larger or smaller than in Premiere".into(),
        }]
    );
}

#[test]
fn the_largest_caption_track_index_has_a_one_based_label_without_overflow() {
    let xml = captions_xml(&[vec![cue(200, 30..90, "Last index")]])
        .replace("<Index>0</Index>", "<Index>9223372036854775807</Index>");
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    assert_eq!(
        caption_lanes(sequence, 1)[0][0].1,
        "C9223372036854775808 caption 1"
    );
}

#[test]
fn a_repeated_caption_track_index_omits_the_later_track() {
    let xml = captions_xml(&[
        vec![cue(200, 30..90, "First track")],
        vec![cue(210, 30..90, "Same Index")],
    ])
    .replace("<Index>1</Index>", "<Index>0</Index>");
    let (captions, reasons) = omission_reasons(&xml);
    assert_eq!(captions, 1);
    assert_eq!(
        reasons,
        [omitted(
            OmissionScope::Track,
            "CaptionDataClipTrack:caption-track-1",
            "unsupported conversion: duplicate caption track Index 0"
        )]
    );
}

#[test]
fn a_dangling_cue_reference_omits_only_that_cue() {
    let xml = captions_xml(&[vec![cue(200, 30..90, "First"), cue(210, 180..240, "Third")]])
        .replace(
            r#"<TrackItem Index="1" ObjectRef="210"/>"#,
            r#"<TrackItem Index="1" ObjectRef="999"/><TrackItem Index="2" ObjectRef="210"/>"#,
        );
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Occurrence,
            kind: OmissionKind::Omitted,
            record: "999".into(),
            reason: "C1 caption 2: invalid Premiere project: missing reference at CaptionDataClipTrack:caption-track-0".into(),
        }]
    );
    let sequence = project.single_sequence().unwrap();
    let names: Vec<_> = caption_lanes(sequence, 1)[0]
        .iter()
        .map(|(_, name, _, text)| (*name, *text))
        .collect();
    assert_eq!(
        names,
        [("C1 caption 1", "First"), ("C1 caption 3", "Third")]
    );
}

#[test]
fn a_caption_track_outside_the_data_group_is_reported_not_read() {
    let xml = captions_xml(&[vec![cue(200, 30..90, "In an audio group")]])
        .replace(
            r#"<DataTrackGroup ObjectID="100">"#,
            r#"<AudioTrackGroup ObjectID="100">"#,
        )
        .replace("</DataTrackGroup>", "</AudioTrackGroup>");
    let (captions, reasons) = omission_reasons(&xml);
    assert_eq!(captions, 0);
    assert_eq!(
        reasons,
        [omitted(
            OmissionScope::Track,
            "CaptionDataClipTrack:caption-track-0",
            "invalid Premiere project: AudioTrackGroup:100: expected AudioClipTrack, found CaptionDataClipTrack:caption-track-0 (unsupported or cyclic edge)"
        )]
    );
}

#[test]
fn binary_hash_deduplicated_caption_payloads_resolve() {
    // Premiere stores a repeated binary once; later copies are empty and name
    // it by BinaryHash, as for repeated wording or a second identical template.
    let payload = STANDARD.encode(&cue(210, 90..120, "Same words").payload);
    let xml = captions_xml(&[
        vec![
            cue(200, 30..60, "Same words"),
            cue(210, 90..120, "Same words"),
        ],
        vec![cue(220, 150..180, "Other track")],
    ]);
    let deduplicated = xml
        .replace(
            &format!(r#"BinaryHash="caption-text-210">{payload}</FormattedTextData>"#),
            r#"BinaryHash="caption-text-200"></FormattedTextData>"#,
        )
        .replace(
            &format!(
                r#"BinaryHash="caption-style-1">{}</CaptionDataTemplateStyle>"#,
                STANDARD.encode(template())
            ),
            r#"BinaryHash="caption-style-0"></CaptionDataTemplateStyle>"#,
        );
    let (project, omissions) = inspect_project_with_omissions(&deduplicated, None).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let texts: Vec<Vec<_>> = caption_lanes(sequence, 1)
        .into_iter()
        .map(|lane| lane.into_iter().map(|(_, _, _, text)| text).collect())
        .collect();
    assert_eq!(
        texts,
        [vec!["Same words", "Same words"], vec!["Other track"]]
    );

    // The shared graph rejects conflicting definitions for both empty cue
    // copies and empty template copies; captions must propagate that error.
    for (hash, expected_count, scope, record, reason) in [
        (
            "caption-text-200", 2, OmissionScope::Occurrence, "CaptionDataClipTrackItem:210",
            "C1 caption 2: invalid Premiere project: Block:213: BinaryHash caption-text-200 is defined with different values",
        ),
        (
            "caption-style-0", 2, OmissionScope::Track, "CaptionDataClipTrack:caption-track-1",
            "invalid Premiere project: CaptionDataClipTrack:caption-track-1: BinaryHash caption-style-0 is defined with different values",
        ),
    ] {
        let conflicting = deduplicated.replace("</PremiereData>", &format!(
            r#"<Block ObjectID="999"><FormattedTextData Encoding="base64" BinaryHash="{hash}">AA==</FormattedTextData></Block></PremiereData>"#,
        ));
        let (captions, reasons) = omission_reasons(&conflicting);
        assert_eq!(captions, expected_count);
        assert_eq!(reasons, [omitted(scope, record, reason)]);
    }

    let dangling = xml.replace(
        &format!(r#"BinaryHash="caption-text-210">{payload}</FormattedTextData>"#),
        r#"BinaryHash="caption-text-missing"></FormattedTextData>"#,
    );
    let (project, omissions) = inspect_project_with_omissions(&dangling, None).unwrap();
    assert_eq!(project.single_sequence().unwrap().video_items().count(), 3);
    assert_eq!(
        omissions,
        [Omission {
            scope: OmissionScope::Occurrence,
            kind: OmissionKind::Omitted,
            record: "CaptionDataClipTrackItem:210".into(),
            reason: "C1 caption 2: unsupported conversion: Block:213: FormattedTextData names missing binary caption-text-missing".into(),
        }]
    );
}

#[test]
fn overlapping_and_repeated_cues_on_one_track_keep_the_first() {
    let xml = captions_xml(&[vec![
        cue(200, 30..90, "First"),
        cue(210, 60..120, "Overlaps the first"),
    ]])
    .replace(
        r#"<TrackItem Index="1" ObjectRef="210"/>"#,
        r#"<TrackItem Index="1" ObjectRef="210"/><TrackItem Index="2" ObjectRef="200"/>"#,
    );
    let (captions, reasons) = omission_reasons(&xml);
    assert_eq!(captions, 1);
    assert_eq!(
        reasons,
        [
            omitted(
                OmissionScope::Occurrence,
                "CaptionDataClipTrackItem:200",
                "C1 caption 3: unsupported conversion: duplicate caption reference"
            ),
            omitted(
                OmissionScope::Occurrence,
                "CaptionDataClipTrackItem:210",
                "C1 caption 2: overlaps an earlier caption on this track"
            ),
        ]
    );
}

#[test]
fn repeated_caption_references_are_reported_individually() {
    // The first cue survives; all later references to its record are duplicates.
    let references: String = (0..256)
        .map(|position| format!(r#"<TrackItem Index="{position}" ObjectRef="200"/>"#))
        .collect();
    let xml = captions_xml(&[vec![cue(200, 30..90, "Repeated")]])
        .replace(r#"<TrackItem Index="0" ObjectRef="200"/>"#, &references);
    let (captions, reasons) = omission_reasons(&xml);
    assert_eq!(captions, 1);
    assert_eq!(reasons.len(), 255);
    assert!(reasons
        .iter()
        .all(|reason| reason.2.contains("duplicate caption reference")));
}

#[test]
fn audio_occurrences_do_not_drop_caption_tracks() {
    // A track with many repeated references still preserves its one valid cue
    // beside audio; duplicates are diagnosed individually.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_audio_clips_strict.prproj");
    let source = crate::format::reader::read_xml(&path).unwrap();
    let references: String = (0..255)
        .map(|position| format!(r#"<TrackItem Index="{position}" ObjectRef="20000"/>"#))
        .collect();
    let captions = captions_xml(&[vec![cue(20000, 30..90, "Over budget")]])
        .replace(r#"<TrackItem Index="0" ObjectRef="20000"/>"#, &references)
        .replace(
            r#"<DataTrackGroup ObjectID="100">"#,
            r#"<DataTrackGroup ObjectID="10000">"#,
        );
    let records = captions
        .split_once(r#"<DataTrackGroup ObjectID="10000">"#)
        .unwrap()
        .1;
    let xml = source
        .replace(
            "</TrackGroups>",
            r#"<TrackGroup><Second ObjectRef="10000"/></TrackGroup></TrackGroups>"#,
        )
        .replace(
            "</PremiereData>",
            &format!(r#"<DataTrackGroup ObjectID="10000">{records}"#),
        );
    let (project, omissions) = inspect_project_with_omissions(&xml, None).unwrap();
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.audio.len(), 2);
    assert_eq!(sequence.video_items().count(), 1);
    let caption_omissions: Vec<_> = omissions
        .into_iter()
        .filter(|omission| omission.record.starts_with("Caption"))
        .collect();
    assert_eq!(
        sequence
            .video_items()
            .filter_map(PrVideoItem::graphic)
            .count(),
        1
    );
    assert_eq!(caption_omissions.len(), 254);
    assert!(caption_omissions
        .iter()
        .all(|item| item.reason.contains("duplicate caption reference")));
}

#[test]
fn nested_placements_preserve_caption_tracks_beyond_former_limit() {
    use super::nested::{
        placement_records, sequence_records, sequence_source, with_records, Placement,
    };
    // Outer places a media clip and a nest of Main; all 255 captions survive
    // alongside them instead of being dropped by a shared item count.
    let cues: Vec<_> = (0..255)
        .map(|index| {
            let frame = i64::from(index);
            cue(20000 + 10 * index, frame..frame + 1, "Cue")
        })
        .collect();
    let captions = captions_xml(&[cues]).replace(
        r#"<DataTrackGroup ObjectID="100">"#,
        r#"<DataTrackGroup ObjectID="10000">"#,
    );
    let (_, caption_records) = captions.split_once("<ProjectSettings").unwrap();
    let mut records = sequence_records("outer", "Outer", 100, &[110, 120]).replace(
        "</TrackGroup></TrackGroups>",
        r#"</TrackGroup><TrackGroup><Second ObjectRef="10000"/></TrackGroup></TrackGroups>"#,
    );
    records.push_str(&sequence_source(102, "sequence-1"));
    for (id, source, start) in [(110, 7, 0), (120, 102, TICKS)] {
        let placement = Placement {
            start,
            end: start + TICKS,
            source_in: 0,
        };
        records.push_str(&placement_records(id, source, &placement));
    }
    let xml = with_records(SOURCE, &records).replace(
        "</PremiereData>",
        &format!("<ProjectSettings{caption_records}"),
    );
    let (project, omissions) = inspect_project_with_omissions(&xml, Some("outer")).unwrap();
    let outer = project.single_sequence().unwrap();
    assert_eq!(outer.video_occurrences().count(), 1);
    assert_eq!(outer.nest_occurrences().count(), 1);
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_eq!(
        outer.video_items().filter_map(PrVideoItem::graphic).count(),
        255
    );
}

#[test]
fn pinned_caption_fixture_reads_three_cues_on_two_tracks() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_captions_strict.prproj");
    let xml = crate::format::reader::read_xml(&path).unwrap();
    let (project, omissions) =
        inspect_project_with_omissions(&xml, Some("c8acf9c1-34b2-4086-9f55-d528950a7059")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    let video: Vec<_> = sequence
        .video_occurrences()
        .map(|clip| (clip.timeline_ticks(), clip.source_ticks()))
        .collect();
    assert_eq!(video, [(0..10 * TICKS, 0..10 * TICKS)]);
    // The scaffold keeps its empty V2 below the caption lanes.
    assert_eq!(sequence.video_tracks().len(), 4);
    let lanes = caption_lanes(sequence, 2);
    let rows: Vec<Vec<_>> = lanes
        .iter()
        .map(|lane| {
            lane.iter()
                .map(|(_, name, ticks, text)| (*name, ticks.clone(), *text))
                .collect()
        })
        .collect();
    assert_eq!(
        rows,
        [
            vec![
                ("C1 caption 1", seconds(1.0..3.0), "First cue"),
                (
                    "C1 caption 2",
                    seconds(6.0..8.0),
                    "Third after gap\nwith a second line"
                ),
            ],
            vec![("C2 caption 1", seconds(2.5..4.5), "Second cue overlaps")],
        ]
    );
    for lane in sequence.video_tracks().skip(2) {
        for graphic in graphics(lane) {
            assert!(graphic.enabled);
            let document = &graphic.text().document;
            assert_eq!(
                *document,
                PrTextDocument {
                    frame: PrTextFrame::Box {
                        width: 1536.0,
                        height: 998.0,
                        vertical: PrVerticalAlign::Bottom,
                    },
                    ..caption_document(&document.text)
                }
            );
            assert_eq!(graphic.text().transform.position, [960.0, 1026.0]);
            assert_eq!(graphic.text().transform.anchor, [768.0, 969.6]);
        }
    }
}

#[test]
fn pinned_caption_styles_fixture_reads_seven_cues_from_their_own_payloads() {
    use crate::format::text_payload::tests::FIXTURE_BACKGROUND;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/feature_caption_styles_26_5_strict.prproj");
    let xml = crate::format::reader::read_xml(&path).unwrap();
    let (project, omissions) =
        inspect_project_with_omissions(&xml, Some("c8acf9c1-34b2-4086-9f55-d528950a7059")).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let sequence = project.single_sequence().unwrap();
    assert_eq!(sequence.video_occurrences().count(), 1);
    let frames = |range: Range<i64>| range.start * FRAME..range.end * FRAME;
    // Every cue converts from its own payload; a higher track Index is a
    // higher lane, so C7 paints over the C1 cue it overlaps (AME render).
    let lanes: Vec<Vec<_>> = caption_lanes(sequence, 2)
        .iter()
        .map(|lane| {
            lane.iter()
                .map(|(_, name, ticks, text)| (*name, ticks.clone(), *text))
                .collect()
        })
        .collect();
    assert_eq!(
        lanes,
        [
            vec![
                ("C1 caption 1", frames(15..75), "C1 PLAIN"),
                ("C1 caption 2", frames(120..150), "C2 GREEN FILL"),
                (
                    "C1 caption 3",
                    frames(228..264),
                    "C5 FIRST LINE\nSECOND LINE"
                ),
                ("C1 caption 4", frames(264..300), "C6 LEFT"),
            ],
            vec![
                ("C2 caption 1", frames(45..105), "C7 OVERLAP"),
                ("C2 caption 2", frames(150..195), "C3 STROKE AND SHADOW"),
            ],
            vec![("C3 caption 1", frames(195..228), "C4 BACKGROUND")],
        ]
    );
    let stroked = PrTextDocument {
        stroke: Some(PrTextStroke {
            color: PrRgb([0, 0, 255]),
            width: 4.0,
        }),
        shadow: Some(PrTextShadow {
            color: PrRgb([0, 0, 0]),
            opacity: 100.0,
            angle: 135.0,
            distance: 6.0,
            size: 0.0,
            blur: 0.0,
        }),
        ..caption_document("")
    };
    let boxed = |width: f32| PrTextFrame::Box {
        width,
        height: 998.0,
        vertical: PrVerticalAlign::Bottom,
    };
    let centred = boxed(1536.0);
    let styles: Vec<_> = sequence
        .video_tracks()
        .skip(2)
        .flat_map(|lane| graphics(lane))
        .map(|graphic| graphic.text().document.clone())
        .collect();
    let expected = [
        PrTextDocument {
            frame: centred,
            ..caption_document("C1 PLAIN")
        },
        // The per-cue fill override against the white template draws green.
        PrTextDocument {
            fill: Some(PrRgb([0, 255, 0])),
            frame: centred,
            ..caption_document("C2 GREEN FILL")
        },
        PrTextDocument {
            frame: centred,
            ..caption_document("C5 FIRST LINE\nSECOND LINE")
        },
        // The left cue's box spans x 240-1680, where its ink started at 242.
        PrTextDocument {
            justification: PrJustification::Left,
            frame: boxed(1440.0),
            ..caption_document("C6 LEFT")
        },
        PrTextDocument {
            text: "C7 OVERLAP".into(),
            frame: centred,
            ..stroked.clone()
        },
        PrTextDocument {
            text: "C3 STROKE AND SHADOW".into(),
            frame: centred,
            ..stroked
        },
        PrTextDocument {
            background: Some(FIXTURE_BACKGROUND),
            frame: centred,
            ..caption_document("C4 BACKGROUND")
        },
    ];
    assert_eq!(styles, expected);
}

#[test]
fn native_caption_cues_survive_tesseract_and_back_as_graphics() {
    use crate::{convert, format::writer::project_xml, tesseract_output::asset_ids_in_order};
    let xml = captions_xml(&[
        vec![
            cue(200, 30..90, "First cue"),
            cue(220, 180..240, "Third after gap\nwith a second line"),
        ],
        vec![cue(210, 75..135, "Second cue overlaps")],
    ])
    .replace(r#"<TrackItem ObjectRef="3"/>"#, "");
    let native = inspect_project(&xml, None).unwrap();
    let mut omissions = Vec::new();
    let document = convert::premiere_to_tesseract(
        &native,
        &Default::default(),
        &asset_ids_in_order(&native, &Default::default()),
        &mut omissions,
    )
    .unwrap();
    assert_eq!(omissions.len(), 1);
    assert!(omissions[0]
        .reason
        .contains("is not packaged in this document"));
    omissions.clear();
    let project = convert::tesseract_to_premiere(
        &document,
        &Default::default(),
        &Default::default(),
        &Default::default(),
        crate::format::FrameRate::Fps30,
        &mut omissions,
    )
    .unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    let exported = project_xml(&project).unwrap();
    assert!(!exported.contains("CaptionDataClipTrack"));
    assert_eq!(
        exported
            .matches("<MatchName>AE.ADBE Text</MatchName>")
            .count(),
        3
    );
    let reopened = inspect_project(&exported, None).unwrap();
    // C2's cue still paints above the C1 cue it overlaps.
    let lane_names: Vec<Vec<_>> = reopened
        .video_tracks()
        .map(|track| {
            graphics(track)
                .into_iter()
                .map(|graphic| graphic.text().name.as_str())
                .collect()
        })
        .collect();
    assert_eq!(
        lane_names,
        [vec!["C1 caption 1", "C1 caption 2"], vec!["C2 caption 1"]]
    );
    let texts = |sequence: &PrSequence| {
        let mut texts: Vec<_> = sequence
            .video_items()
            .map(|item| match item {
                PrVideoItem::Graphic(graphic) => (graphic.timeline_ticks(), graphic.text().clone()),
                PrVideoItem::Media(_) => panic!("captions export as graphics only"),
            })
            .collect();
        texts.sort_by_key(|(ticks, _)| ticks.start);
        texts
    };
    let (before, after) = (texts(&native), texts(&reopened));
    assert_eq!(after.len(), 3);
    for ((ticks, text), (reopened_ticks, reopened_text)) in before.iter().zip(&after) {
        assert_eq!(reopened_ticks, ticks);
        assert_eq!(reopened_text.name, text.name);
        assert_eq!(reopened_text.document, text.document);
        assert_eq!(reopened_text.transform, text.transform);
    }
}
