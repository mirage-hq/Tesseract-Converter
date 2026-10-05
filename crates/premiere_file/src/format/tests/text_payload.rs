//! Source Text codec tests. Reader inputs are real Premiere payloads, so the
//! decoder is not checked only against this crate's encoder.

use super::*;
use crate::schema::text_shadow::PrTextShadow;
use base64::{engine::general_purpose::STANDARD, Engine};

// Premiere 26.3.0 Type-tool graphic: "py" in LucidaConsole at 1.8 px with the
// default white fill.
pub(crate) const EG_TEXT_PY: &str = concat!(
    "QAEAAAAAAABEMyIRDAAAAAAABgAKAAQABgAAAGQAAAAAAF4AGAAQAAwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAFgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAAAAAABcABwBeAAAA",
    "AAAAARAAAAAcAAAANAAAAAAAAQBU////WP///1z///9g////AQAAAAQAAAANAAAATHVjaWRhQ29uc29sZQAAAAEA",
    "AAAMAAAACAAMAAQACAAIAAAACAAAAEQAAAACAAAAcHkAAAAANgAYAAAAFAAAAAAAAAAAABAAAAAAAAAAAAAAAAAA",
    "AAAAAAAAAAAAAAAAAAAAAAwAAAAIAAQANgAAAAIAAAAQAAAAEAAAAAAAgEBlZuY/9P////j////8////BAAEAAQA",
    "AAA=",
);

// Premiere 26.5.1 graphics from a Captions marketing edit: left point text
// with negative tracking.
pub(crate) const BEFORE: &str = concat!(
    "SAEAAAAAAABEMyIRDAAAAAAABgAKAAQABgAAAGQAAAAAAF4AGAAQAAwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAFgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAAAAAABcABwBeAAAA",
    "AAAAARAAAAAcAAAANAAAAAAAAQBY////XP///2D///9k////AQAAAAQAAAANAAAAT3BlblNhbnMtQm9sZAAAAAEA",
    "AAAMAAAACAAOAAQACAAIAAAAcAAAADwAAAAAADYAHAAAABgAAAAAAAAAAAAUAAAAEAAAAAAAAAAAAAAAAAAAAAAA",
    "AAAAAAAAAAAMAAAACAAEADYAAAACAAAAFAAAABQAAAAAABDCAACAQMawFkL0////+P////z///8EAAQABAAAAAYA",
    "AABCZWZvcmUAAA==",
);

// Centered all-caps text with a yellow fill; the disabled shadow and
// background still store values.
pub(crate) const CHOOSE_CENTERED_CAPS: &str = concat!(
    "pAEAAAAAAABEMyIRDAAAAAAABgAKAAQABgAAAGQAAAAAAF4AOAAQAAwAAAAAADQAMAAAAAAAAAAAAAAAAAAAAAAA",
    "LAAoACQAIAAAABwAGAAAAAAAAAAAAAAAFgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAAAAAABcABwBeAAAA",
    "AAAAATAAAABAAAAAXAAAAAAAAQDlhAdCAADIQiAAAAAAAHpDqupFQgAA4UEBAAAAAgAAAFD///9U////Rv///wD2",
    "9vZg////AQAAAAQAAAATAAAAQm93bGJ5T25lU0MtUmVndWxhcgABAAAADAAAAAgADgAEAAgACAAAAIwAAAA8AAAA",
    "AAA2ACQAAAAgABwAAAAAAAAAGAAAABQAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAADAAAAAgABAA2AAAAAgAAABwA",
    "AAAgAAAAAgAAAAAAuMEAAIBAKAAAAMQFm0L8////BAAEAAQAAAAEAAYABAAAAAAACgAIAAUABgAHAAoAAAAA9s0O",
    "HgAAAENob29zZSBmcm9tIA0xMDArIHZpcmFsIHN0eWxlcwAA",
);

// Two runs with different fonts, sizes, and colors.
pub(crate) const MIXED_RUNS: &str = concat!(
    "SAIAAAAAAABEMyIRDAAAAAAABgAKAAQABgAAAGQAAAAAAF4AOAAQAAwAAAAAADQAMAAAAAAAAAAAAAAAAAAAAAAA",
    "LAAoACQAIAAAABwAGAAAAAAAAAAAAAAAFgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAAAAAABcABwBeAAAA",
    "AAAAATAAAABAAAAAdAAAAAAAAQDlhAdCAADIQiAAAAAAAHpDqupFQgAA4UEBAAAAAgAAAKD+//+k/v//lv7//wD2",
    "9vaw/v//AgAAABwAAAAEAAAADgAAAE1vbmFTYW5zLUJsYWNrAAATAAAAQm93bGJ5T25lU0MtUmVndWxhcgACAAAA",
    "pAAAAAQAAABs////eAAAADwAAAAAADYAJAAgABwAGAAAAAAAAAAUAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    "AAAMAAAACAAEADYAAAACAAAAHAAAABwAAAAAAOjBAACAQBgAAAAk0G9CAQAAAGD///9k////aP///1r///8A9vb2",
    "EwAAAEZpeCBpbiBvbmUgdGFwIPCflKUACAAOAAQACAAIAAAAjAAAADwAAAAAADYAJAAAACAAHAAAAAAAAAAYAAAA",
    "FAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAMAAAACAAEADYAAAACAAAAHAAAACAAAAACAAAAAADAQAAAgEAoAAAA",
    "9C2gQvz///8EAAQABAAAAAQABgAEAAAAAAAKAAgABQAGAAcACgAAAAD2zQ4RAAAAQmFkIGF0IGVkaXRpbmc/IA0A",
    "AAA=",
);

// A caption style preview in the same 45-slot layout with its shadow and
// background enabled: `ArbVideoComponentParam` 41 of the pinned
// `practice_files_transcription_magic.prproj` (SHA-256
// `a6aab608caafb9b19ef1900b815c04e3a6c0a32a4dd9d989527d9d8d430e28a1`).
pub(crate) const CAPTION_STYLE_EFFECTS: &str = concat!(
    "kAEAAAAAAABEMyIRDAAAAAAABgAKAAQABgAAAGQAAAAAAF4ARAAQAAwAAAAAAEAAPAAAAAAAAAAAADgANwAwAAAA",
    "LAAoACQAIAAfAAAAGAAAAAAAAAAAAAAAFgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAAAAAABcABwBeAAAA",
    "AAAAATwAAABsAAAAgAAAAAAAAQAjI49BAAAAATwAAAAAAEBBAADAQAAAQEAAAMhCAAAAATgAAAACAAAAAgAAADj/",
    "//8EAAYABAAAAAAACgAKAAcACAAJAAoAAAAAAADixHIKAAgABQAGAAcACgAAAAAAAAABAAAABAAAAAsAAABHaWJz",
    "b24tQm9sZAABAAAADAAAAAgADgAEAAgACAAAAHAAAAA8AAAAAAA2ABwAAAAYAAAAAAAAAAAAFAAAAAAAAAAAAAAA",
    "EAAAAAAAAAAAAAAAAAAAAAAADAAAAAgABAA2AAAAAgAAABQAAAAUAAAAAgAAAAAAgEAAAEBC9P////j////8////",
    "BAAEAAQAAAACAAAAQWEAAA==",
);

// Cue C4 of the `feature_caption_styles_26_5_strict` fixture as Premiere
// 26.5.1 saved it (`BinaryHash` a7b45a2b…): "C4 BACKGROUND" in Arial-BoldMT
// at 48 px, centred, bottom-aligned, with a blue background of opacity 100,
// size 10 and corner radius 15. Its AME render draws a 440.4 x 54.7 px box
// with 14.75 px corners around the 422 x 36 px ink.
pub(crate) const FIXTURE_BACKGROUND_CUE: &str = concat!(
    "bAEAAAAAAABEMyIRDAAAAAAABgAKAAQABgAAAGQAAAAAAF4AOAA0ADAAAAAAACAAJAAAAAAAAAAAAAAAAAAAAAAA",
    "AAAAAAAAHAAbABQAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAwAAAAAAAAACwAAAAQAAAAAAAoACQBeAAAA",
    "OAAAAAABAAEAAHBBAAAgQQAAyEIAAAABMAAAAAIAAAACAAAAAACAPwAAgD8kAAAAPAAAAAQABgAEAAAAAAAKAAgA",
    "BwAGAAUACgAAAAD/AAABAAAABAAAAAwAAABBcmlhbC1Cb2xkTVQAAAAAAQAAAAwAAAAIAAwACAAEAAgAAABQAAAA",
    "BAAAAA0AAABDNCBCQUNLR1JPVU5EADYAGAAAABQAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    "AAAMAAAACAAEADYAAAACAAAAEAAAABQAAAAAAIBAAABAQvz///8EAAQABAAAAA==",
);

/// The background of [`FIXTURE_BACKGROUND_CUE`].
pub(crate) const FIXTURE_BACKGROUND: PrTextBackground = PrTextBackground {
    color: PrRgb([0, 0, 255]),
    opacity: 100.0,
    size: 10.0,
    radius: 15.0,
};

// Paragraph text from a project version 40 file: the pre-26 FlatBuffer
// revision.
pub(crate) const PRE_26_BOX: &str = concat!(
    "bAMAAAAAAABEMyIRDAAAAAAABgAKAAQABgAAABwAAAAAABYAHAAMAAQAGAAUABAAAAAAAAAACAAWAAAAJAAAAFgA",
    "AAAsAQAAAwAAAACAqUMAQBJE9P7///j+///8/v//AgAAABwAAAAEAAAADgAAAFF1aWNrc2FuZC1Cb2xkAAARAAAA",
    "UXVpY2tzYW5kLVJlZ3VsYXIAAAAg////CAAAADwAAAAAAAAAAAAAADAAGAAUABAADAAAAAAAAAAAAAAAAAAAAAAA",
    "CwAAAAAAAAAAAAAAAAAAAAAAAAAEADAAAAAUAAAAAAAAAIAAAAAAAOJCAQAAACL///8IAAAAMAAAAAIAAAAgAAAA",
    "EAAAAAAACgAMAAAABAAIAAoAAAAAAIA/AAAAPxb///8AAAA/AgAAABwAAAAEAAAACv///wwAAAAAAIA/AAAAP+j/",
    "///+/v//CAAAAAAAAD/i/v//AElJSQQABAAEAAAA8v7//wA6OjoBAAAADAAAAAgADAAEAAgACAAAAAQBAAA0AAAA",
    "MAAWAAAAEAAMAAAAAAAAAAAAAAAAAAAAAAALAAAAAAAAAAAAAAAAAAAAAAAAAAQAMAAAABwAAAAAAAAAuAAAAAAA",
    "yEEAAAoADAAAAAgABAAKAAAACAAAADwAAAACAAAALAAAABAAAAAAAAoADgAAAAQACAAKAAAAAACAPwAAAD8AAAoA",
    "CAAAAAAABAAKAAAAAAAAPwIAAAA0AAAAEAAAAAAACgAQAAQACAAMAAoAAAAMAAAAAACAPwAAAD/c////AAAKAAwA",
    "BAAAAAgACgAAAAgAAAAAAAA/7v///wBJSUkEAAYABAAAAAAACgAIAAUABgAHAAoAAAAAOjo66AAAAExvcmVtIGlw",
    "c3VtIGRvbG9yIHNpdCBhbWV0LCBjb25zZWN0ZXR1ciBhZGlwaXNjaW5nIGVsaXQsIHNlZCBkbyBlaXVzbW9kIHRl",
    "bXBvciBpbmNpZGlkdW50IHV0IGxhYm9yZSBldCBkb2xvcmUgbWFnbmEgYWxpcXVhLiBVdCBlbmltIGFkIG1pbmlt",
    "IHZlbmlhbSwgcXVpcyBub3N0cnVkIGV4ZXJjaXRhdGlvbiB1bGxhbWNvIGxhYm9yaXMgbmlzaSB1dCBhbGlxdWlw",
    "IGV4IGVhIGNvbW1vZG8gY29uc2VxdWF0LiAAAAAA",
);

const WHITE: Option<PrRgb> = Some(PrRgb([255, 255, 255]));

fn bytes(value: &str) -> Vec<u8> {
    STANDARD.decode(value).unwrap()
}

fn error(payload: &[u8]) -> String {
    decode(payload).unwrap_err().to_string()
}

fn document(text: &str) -> PrTextDocument {
    PrTextDocument {
        text: text.into(),
        font: "Arial-BoldMT".into(),
        size: 64.0,
        fill: WHITE,
        stroke: None,
        shadow: None,
        all_caps: false,
        tracking: 0.0,
        leading: 0.0,
        justification: PrJustification::Left,
        frame: PrTextFrame::Point {
            vertical: PrVerticalAlign::Top,
        },
        background: None,
    }
}

/// The document table of a payload.
fn document_table(payload: &[u8]) -> Table<'_> {
    let buffer = Buffer::from_payload(payload, "Source Text").unwrap();
    let root = buffer.table(buffer.offset(0).unwrap()).unwrap();
    root.table(0).unwrap().unwrap()
}

/// The style table of the first run in a payload.
fn style_table(payload: &[u8]) -> Table<'_> {
    let run = document_table(payload).tables(document::RUNS).unwrap()[0];
    run.table(run::STYLE).unwrap().unwrap()
}

/// Supplementary authored white paint: point slot 4 at the native empty
/// table in slot 21. Empty RGB tables explicitly encode white in this codec.
/// This does not establish the native meaning of an absent stroke color.
pub(crate) fn with_explicit_white_stroke(mut payload: Vec<u8>) -> Vec<u8> {
    let style = style_table(&payload);
    assert!(style.table(style::STROKE_COLOR).unwrap().is_none());
    let entry = HEADER_BYTES + style.vtable + 4;
    let empty = payload[entry + 21 * 2..entry + 21 * 2 + 2].to_vec();
    payload[entry + style::STROKE_COLOR * 2..entry + style::STROKE_COLOR * 2 + 2]
        .copy_from_slice(&empty);
    payload
}

#[test]
fn an_absent_run_marker_preserves_known_style_with_a_diagnostic() {
    let original = document("OUTLINE");
    let mut payload = encode(&original).unwrap();
    let style = style_table(&payload);
    let entry = HEADER_BYTES + style.vtable + 4 + style::FIXED_VALUE.0 * 2;
    let field = HEADER_BYTES + style.field(style::FIXED_VALUE.0, 4).unwrap().unwrap();
    let mut unknown = payload.clone();
    unknown[field..field + 4].copy_from_slice(&9_u32.to_le_bytes());
    assert!(error(&unknown).contains("unsupported Premiere 26 run marker 9"));
    payload[entry..entry + 2].fill(0);
    let decoded = decode(&payload).unwrap();
    assert_eq!(decoded.document, original);
    assert_eq!(decoded.omitted, [OmittedTextFeature::MissingRunMarker]);
    let caption = with_caption_marker(payload);
    assert!(decode_caption(&caption)
        .unwrap_err()
        .to_string()
        .contains("text style lacks the Premiere 25.5 caption run marker"));
}

#[test]
fn decodes_premiere_26_graphic_payloads() {
    let py = decode(&bytes(EG_TEXT_PY)).unwrap();
    assert!(py.omitted.is_empty());
    let doc = py.document;
    assert_eq!(
        (doc.text.as_str(), doc.font.as_str(), doc.fill, doc.stroke),
        ("py", "LucidaConsole", WHITE, None)
    );
    assert!((doc.size - 1.8).abs() < 1e-6);
    assert_eq!(doc.justification, PrJustification::Left);
    assert_eq!(
        doc.frame,
        PrTextFrame::Point {
            vertical: PrVerticalAlign::Top
        }
    );

    let before = decode(&bytes(BEFORE)).unwrap().document;
    assert_eq!(
        (before.text.as_str(), before.font.as_str()),
        ("Before", "OpenSans-Bold")
    );
    assert!((before.size - 37.6726).abs() < 1e-4);
    assert_eq!(
        (before.tracking, before.leading, before.all_caps),
        (-36.0, 0.0, false)
    );

    let centered = decode(&bytes(CHOOSE_CENTERED_CAPS)).unwrap();
    // Stored but disabled shadow and background values do not render.
    assert!(centered.omitted.is_empty());
    let doc = centered.document;
    assert_eq!(doc.shadow, None);
    assert_eq!(doc.text, "Choose from \n100+ viral styles");
    assert_eq!(doc.font, "BowlbyOneSC-Regular");
    assert_eq!(doc.fill, Some(PrRgb([246, 205, 14])));
    assert!(doc.all_caps);
    assert_eq!(doc.tracking, -23.0);
    assert_eq!(doc.justification, PrJustification::Center);
    // Point text retains the block alignment around its origin.
    assert_eq!(
        doc.frame,
        PrTextFrame::Point {
            vertical: PrVerticalAlign::Center
        }
    );
}

#[test]
fn enabled_shadow_decodes_and_background_is_reported_without_dropping_text() {
    let decoded = decode(&bytes(CAPTION_STYLE_EFFECTS)).unwrap();
    assert_eq!(decoded.omitted, [OmittedTextFeature::Background]);
    assert_eq!(decoded.document.background, None);
    assert_eq!(decoded.document.text, "Aa");
    assert_eq!(decoded.document.font, "Gibson-Bold");
    // The pinned preview omits its angle, which reads as the inferred 135°.
    assert_eq!(
        decoded.document.shadow,
        Some(PrTextShadow {
            color: PrRgb([0, 0, 0]),
            opacity: 100.0,
            angle: 135.0,
            distance: 3.0,
            size: 6.0,
            blur: 12.0,
        })
    );
}

#[test]
fn rejects_payloads_it_cannot_represent() {
    assert!(error(&bytes(MIXED_RUNS)).contains("mixed text styles"));
    assert!(error(&bytes(PRE_26_BOX)).contains("lacks the Premiere 26 document marker"));

    let mut legacy = 4_u64.to_le_bytes().to_vec();
    legacy.extend("{}".encode_utf16().flat_map(u16::to_le_bytes));
    assert!(error(&legacy).contains("legacy UTF-16 JSON"));

    // The frame checks are shared with the Shape Appearance; their Source
    // Text messages are pinned here.
    let payload = bytes(BEFORE);
    assert_eq!(
        error(&payload[..HEADER_BYTES]),
        "unsupported conversion: truncated Source Text payload"
    );
    let mut magic = payload.clone();
    magic[8] ^= 1;
    assert_eq!(
        error(&magic),
        "unsupported conversion: unknown Source Text encoding"
    );
    let mut length = payload.clone();
    length[0] ^= 1;
    assert!(error(&length).contains("length prefix"));
    let mut root = payload.clone();
    root[HEADER_BYTES..HEADER_BYTES + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(error(&root).contains("out of bounds"));
}

#[test]
fn malformed_native_text_strings_reject() {
    let mut payload = bytes(BEFORE);
    let run = document_table(&payload).tables(document::RUNS).unwrap()[0];
    let text = run.target(run::TEXT).unwrap().unwrap();
    let terminator = HEADER_BYTES + text + 4 + run.buffer.u32(text).unwrap() as usize;
    payload[terminator] = b'x';
    assert!(error(&payload).contains("not NUL-terminated"));
}

#[test]
fn unknown_style_slots_fail_closed() {
    let mut payload = bytes(BEFORE);
    let style = style_table(&payload);
    // Mark slot 7 present by pointing it at the tracking field.
    let tracking_entry = HEADER_BYTES + style.vtable + 4 + style::TRACKING * 2;
    let unknown_entry = HEADER_BYTES + style.vtable + 4 + 7 * 2;
    let offset = [payload[tracking_entry], payload[tracking_entry + 1]];
    payload[unknown_entry..unknown_entry + 2].copy_from_slice(&offset);
    assert!(error(&payload).contains("unsupported Source Text field text style[7]"));
}

/// The Source Text of the empty second Text of the Source Graphic in the
/// Premiere 26.5.1 save `feature_source_graphic_26_5`
/// (ArbVideoComponentParam 96): a document of the fixed graphic slots only,
/// with no runs and no fonts.
const EMPTY_TEXT: &str = concat!(
    "mAAAAAAAAABEMyIRDAAAAAAABgAKAAQABgAAAGQAAAAAAF4AEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAAAAAAA8ABwBeAAAA",
    "AAAAAQgAAAAAAAEA9P////j////8////BAAEAAQAAAA=",
);

#[test]
fn a_graphic_document_without_runs_or_fonts_is_an_empty_text() {
    let payload = bytes(EMPTY_TEXT);
    let slots = document_table(&payload).present().unwrap();
    assert_eq!(slots, [26, 40, 43, 44]);
    // Zero characters and no font. Its style is this reader's for omitted
    // style slots: the document stores none.
    let empty = decode(&payload).unwrap();
    assert!(empty.omitted.is_empty());
    let expected = PrTextDocument {
        font: String::new(),
        size: 100.0,
        ..document("")
    };
    assert_eq!(empty.document, expected);
    empty.document.validate().unwrap();
    // Text needs a font: with characters, the same document is invalid.
    let unnamed = PrTextDocument {
        text: "py".into(),
        ..expected.clone()
    };
    assert_eq!(
        unnamed.validate().unwrap_err().to_string(),
        "invalid Premiere project: text font must be a nonempty PostScript name"
    );
    assert!(expected.validate_font().is_err());

    // A styled document without either slot is also empty; without only one
    // it is malformed, and no caption block without runs has been seen.
    let without = |slots: &[usize]| {
        let mut payload = bytes(BEFORE);
        for &slot in slots {
            let entry = vtable_entry(&payload, slot);
            payload[entry].fill(0);
        }
        payload
    };
    let stripped = decode(&without(&[document::RUNS, document::FONTS]))
        .unwrap()
        .document;
    assert_eq!((stripped.text.as_str(), stripped.font.as_str()), ("", ""));
    assert_eq!(
        error(&without(&[document::RUNS])),
        "unsupported conversion: Source Text has no text runs"
    );
    assert_eq!(
        error(&without(&[document::FONTS])),
        "unsupported conversion: text run references a missing font"
    );
    assert_eq!(
        decode_caption(&with_caption_marker(payload))
            .unwrap_err()
            .to_string(),
        "unsupported conversion: Source Text has no text runs"
    );
}

#[test]
fn supported_documents_round_trip() {
    let box_frame = |vertical| PrTextFrame::Box {
        width: 800.0,
        height: 400.5,
        vertical,
    };
    let cases = [
        document("Left point"),
        PrTextDocument {
            text: "Line one\nLine two ✓".into(),
            justification: PrJustification::Center,
            leading: 50.0,
            ..document("")
        },
        PrTextDocument {
            fill: Some(PrRgb([0, 0, 0])),
            stroke: Some(PrTextStroke {
                color: PrRgb([0, 0, 255]),
                width: 10.5,
            }),
            justification: PrJustification::Right,
            all_caps: true,
            tracking: -25.0,
            ..document("Stroke")
        },
        PrTextDocument {
            fill: None,
            size: 100.0,
            ..document("No fill")
        },
        PrTextDocument {
            frame: box_frame(PrVerticalAlign::Top),
            justification: PrJustification::Justify,
            ..document("Box text wraps inside its box")
        },
        PrTextDocument {
            frame: box_frame(PrVerticalAlign::Center),
            ..document("Centered box")
        },
        PrTextDocument {
            frame: box_frame(PrVerticalAlign::Bottom),
            ..document("Bottom box")
        },
    ];
    for expected in cases {
        let payload = encode(&expected).unwrap();
        let decoded = decode(&payload).unwrap();
        assert!(decoded.omitted.is_empty());
        assert_eq!(decoded.document, expected);
    }
}

#[test]
fn caption_backgrounds_decode_and_round_trip_while_graphics_report_them() {
    let fixture = bytes(FIXTURE_BACKGROUND_CUE);
    let decoded = decode_caption(&fixture).unwrap();
    assert!(decoded.omitted.is_empty());
    let expected = PrTextDocument {
        text: "C4 BACKGROUND".into(),
        size: 48.0,
        justification: PrJustification::Center,
        background: Some(FIXTURE_BACKGROUND),
        frame: PrTextFrame::Point {
            vertical: PrVerticalAlign::Bottom,
        },
        ..document("")
    };
    assert_eq!(decoded.document, expected);
    assert_eq!(decoded.box_alignment, PrVerticalAlign::Bottom);
    // The encoder writes every background slot; the fixture payload is the
    // same writer's output, so it reproduces the bytes Premiere saved.
    assert_eq!(caption_payload(&expected, PrVerticalAlign::Bottom), fixture);
    let generated = encode(&expected).unwrap();
    assert_eq!(decode(&generated).unwrap().document.background, None);
    assert_eq!(
        decode(&generated).unwrap().omitted,
        [OmittedTextFeature::Background]
    );
    // A background that omits a slot (here the radius) is reported, not
    // read with a guessed default, in both layouts.
    let mut without_radius = fixture.clone();
    let entry = vtable_entry(&without_radius, document::BACKGROUND_VALUES[2]);
    without_radius[entry].fill(0);
    let decoded = decode_caption(&without_radius).unwrap();
    assert_eq!(decoded.omitted, [OmittedTextFeature::Background]);
    assert_eq!(decoded.document.background, None);
    // A stored position table fails closed by name.
    let mut positioned = fixture.clone();
    let table = vtable_entry(&positioned, document::FIXED_EMPTY_TABLE);
    let position = vtable_entry(&positioned, document::POSITION);
    positioned.copy_within(table, position.start);
    assert_eq!(
        decode_caption(&positioned).unwrap_err().to_string(),
        "unsupported conversion: positioned Premiere 25.5 caption text (document slot 33) is unsupported: its base point is unverified"
    );
}

#[test]
fn line_breaks_use_premiere_carriage_returns() {
    let payload = encode(&document("one\ntwo")).unwrap();
    let run = document_table(&payload).tables(document::RUNS).unwrap()[0];
    assert_eq!(run.string(run::TEXT).unwrap(), Some("one\rtwo"));
    assert_eq!(decode(&payload).unwrap().document.text, "one\ntwo");
}

/// A minimal valid payload whose run vector repeats one run table.
fn repeated_runs(text: &str, copies: usize) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let tables: Vec<_> = (0..3).map(|_| empty_table(&mut fbb)).collect();
    let style_table = {
        let table = fbb.start_table();
        fbb.push_slot_always(slot(21), tables[0]);
        fbb.push_slot_always(slot(23), tables[1]);
        fbb.push_slot_always(slot(24), 2_u32);
        fbb.end_table(table)
    };
    let string = fbb.create_string(text);
    let run_table = {
        let table = fbb.start_table();
        fbb.push_slot_always(slot(0), string);
        fbb.push_slot_always(slot(1), style_table);
        fbb.end_table(table)
    };
    let runs = fbb.create_vector(&vec![run_table; copies]);
    let font = fbb.create_string("Arial-BoldMT");
    let fonts = fbb.create_vector(&[font]);
    let document = {
        let table = fbb.start_table();
        fbb.push_slot_always(slot(0), runs);
        fbb.push_slot_always(slot(1), fonts);
        fbb.push_slot_always(slot(26), 1_u8);
        fbb.push_slot_always(slot(40), tables[2]);
        fbb.push_slot_always(slot(43), 0_u8);
        fbb.push_slot_always(slot(44), 1_u8);
        fbb.end_table(table)
    };
    let root = {
        let table = fbb.start_table();
        fbb.push_slot_always(slot(0), document);
        fbb.end_table(table)
    };
    fbb.finish_minimal(root);
    let buffer = fbb.finished_data();
    let mut payload = (buffer.len() as u64).to_le_bytes().to_vec();
    payload.extend_from_slice(&MAGIC.to_le_bytes());
    payload.extend_from_slice(buffer);
    payload
}

/// A small independent wire input with one font and a size per native run.
fn text_runs_payload(source: &[(&str, f32)]) -> Vec<u8> {
    text_runs_with_default(source, None)
}

// Supplemental reduced wire layout from saved pre-26 documents: slot 8 is
// a run table, not a second text vector. No proprietary source bytes retained.
fn text_runs_with_default(source: &[(&str, f32)], default: Option<(&str, f32)>) -> Vec<u8> {
    text_runs_with_default_control(source, default, false)
}

fn text_runs_with_default_control(
    source: &[(&str, f32)],
    default: Option<(&str, f32)>,
    unknown_insertion_control: bool,
) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let tables: Vec<_> = (0..3).map(|_| empty_table(&mut fbb)).collect();
    let mut runs = Vec::new();
    for (index, (text, size)) in source.iter().copied().chain(default).enumerate() {
        let style_table = {
            let table = fbb.start_table();
            fbb.push_slot_always(slot(1), size);
            if unknown_insertion_control && index == source.len() {
                fbb.push_slot_always(slot(7), 1_u32);
            }
            fbb.push_slot_always(slot(21), tables[0]);
            fbb.push_slot_always(slot(23), tables[1]);
            fbb.push_slot_always(slot(24), 2_u32);
            fbb.end_table(table)
        };
        let string = fbb.create_string(text);
        let run_table = {
            let table = fbb.start_table();
            fbb.push_slot_always(slot(0), string);
            fbb.push_slot_always(slot(1), style_table);
            fbb.end_table(table)
        };
        runs.push(run_table);
    }
    let default_run = default.map(|_| runs.pop().unwrap());
    let runs = fbb.create_vector(&runs);
    let font = fbb.create_string("Arial-BoldMT");
    let fonts = fbb.create_vector(&[font]);
    let document = {
        let table = fbb.start_table();
        fbb.push_slot_always(slot(0), runs);
        fbb.push_slot_always(slot(1), fonts);
        if let Some(default_run) = default_run {
            fbb.push_slot_always(slot(8), default_run);
        }
        fbb.push_slot_always(slot(26), 1_u8);
        fbb.push_slot_always(slot(40), tables[2]);
        fbb.push_slot_always(slot(43), 0_u8);
        fbb.push_slot_always(slot(44), 1_u8);
        fbb.end_table(table)
    };
    let root = {
        let table = fbb.start_table();
        fbb.push_slot_always(slot(0), document);
        fbb.end_table(table)
    };
    fbb.finish_minimal(root);
    let buffer = fbb.finished_data();
    let mut payload = (buffer.len() as u64).to_le_bytes().to_vec();
    payload.extend_from_slice(&MAGIC.to_le_bytes());
    payload.extend_from_slice(buffer);
    payload
}

#[test]
fn empty_default_run_preserves_the_identically_styled_text() {
    let source = [("Editable text", 80.0)];
    let expected = decode(&text_runs_payload(&source)).unwrap();
    let actual = decode(&text_runs_with_default(&source, Some(("", 80.0)))).unwrap();
    assert_eq!(actual.document, expected.document);
    assert_eq!(actual.omitted, expected.omitted);
}

#[test]
fn empty_default_run_differing_style_preserves_actual_text_with_warning() {
    let source = [("Editable text", 80.0)];
    let expected = decode(&text_runs_payload(&source)).unwrap();
    let actual = decode(&text_runs_with_default(&source, Some(("", 60.0)))).unwrap();
    assert_eq!(actual.document, expected.document);
    assert_eq!(actual.omitted.len(), 1);
    assert!(actual.omitted[0]
        .to_string()
        .contains("insertion/default style"));
}

#[test]
fn empty_default_run_unknown_insertion_control_does_not_discard_actual_text() {
    let source = [("Editable text", 80.0)];
    let expected = decode(&text_runs_payload(&source)).unwrap();
    let actual = decode(&text_runs_with_default_control(
        &source,
        Some(("", 80.0)),
        true,
    ))
    .unwrap();
    assert_eq!(actual.document, expected.document);
    assert_eq!(actual.omitted, [OmittedTextFeature::DefaultRunStyle]);
}

#[test]
fn empty_default_run_rejects_content_and_invalid_offsets() {
    assert!(decode(&text_runs_with_default(
        &[("A", 80.0)],
        Some(("Extra text", 80.0))
    ))
    .is_err());
    let mut payload = text_runs_with_default(&[("A", 80.0)], Some(("", 80.0)));
    let field = HEADER_BYTES + document_table(&payload).field(8, 4).unwrap().unwrap();
    payload[field..field + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode(&payload).is_err());
}

#[test]
fn whole_line_styles_preserve_unicode_empty_lines_and_split_crlf() {
    for first in ["A\r", "A\n", "A\r\n"] {
        let next = if first == "A\r" {
            "\n🔥\n\n"
        } else {
            "🔥\n\n"
        };
        let decoded = decode_graphic(&text_runs_payload(&[(first, 80.0), (next, 60.0)])).unwrap();
        let TextDocuments::Lines(lines) = decoded.documents else {
            panic!("mixed complete lines");
        };
        assert_eq!(
            lines
                .iter()
                .map(|line| (line.text.as_str(), line.size))
                .collect::<Vec<_>>(),
            [("A", 80.0), ("🔥", 60.0), ("", 60.0), ("", 60.0)]
        );
    }
    let payload = text_runs_payload(&[("Inline ", 80.0), ("style", 60.0)]);
    assert!(decode_graphic(&payload)
        .unwrap_err()
        .to_string()
        .contains("within a line"));
    let payload = text_runs_payload(&[("A\n", 80.0), ("B", 60.0)]);
    assert!(decode(&payload)
        .unwrap_err()
        .to_string()
        .contains("captions or keyed Source Text"));
}

#[test]
fn shared_run_strings_preserve_text_past_the_former_byte_quota() {
    assert_eq!(
        decode(&repeated_runs("ab", 2)).unwrap().document.text,
        "abab"
    );
    let long = "x".repeat(400_000);
    assert_eq!(
        decode(&repeated_runs(&long, 3)).unwrap().document.text,
        long.repeat(3)
    );
}

#[test]
fn run_vectors_past_the_former_item_quota_preserve_all_text() {
    assert_eq!(
        decode(&repeated_runs("x", 4097)).unwrap().document.text,
        "x".repeat(4097)
    );
}

#[test]
fn text_buffer_preflight_retains_native_width_and_overflow_checks() {
    let maximum = flatbuffers::FLATBUFFERS_MAX_BUFFER_SIZE - TEXT_FIXED_BUFFER_BOUND;
    validate_text_buffer_lengths(maximum, 0).unwrap();
    assert!(validate_text_buffer_lengths(maximum, 1).is_err());
    assert!(validate_text_buffer_lengths(usize::MAX, 1).is_err());
}

#[test]
fn empty_vtable_slots_past_the_former_slot_quota_are_valid() {
    let vtable_bytes = 4 + 257 * 2;
    let mut bytes = vec![0; vtable_bytes + 4];
    bytes[..2].copy_from_slice(&u16::try_from(vtable_bytes).unwrap().to_le_bytes());
    bytes[2..4].copy_from_slice(&4_u16.to_le_bytes());
    bytes[vtable_bytes..].copy_from_slice(&i32::try_from(vtable_bytes).unwrap().to_le_bytes());
    let buffer = Buffer {
        bytes: &bytes,
        name: SOURCE_TEXT,
    };
    buffer
        .table(vtable_bytes)
        .unwrap()
        .allow_only(&[], "empty")
        .unwrap();
}

#[test]
fn text_payload_past_the_former_byte_quota_round_trips() {
    let source = document(&"x".repeat(1 << 20));
    let payload = encode(&source).unwrap();
    assert!(payload.len() > 1 << 20);
    assert_eq!(decode(&payload).unwrap().document, source);
}

/// Present slots of the document and first-run style tables.
fn slots(payload: &[u8]) -> (Vec<usize>, Vec<usize>) {
    (
        document_table(payload).present().unwrap(),
        style_table(payload).present().unwrap(),
    )
}

#[test]
fn encoder_writes_the_fields_premiere_writes_for_the_same_document() {
    for native in [EG_TEXT_PY, BEFORE] {
        let native = bytes(native);
        let encoded = encode(&decode(&native).unwrap().document).unwrap();
        assert_eq!(slots(&encoded), slots(&native));
    }
}

#[test]
fn encoded_layout_matches_the_premiere_render_experiments() {
    // Slot numbers are the ones Premiere 26.5.1 rendered as box size, box
    // alignment, justification, leading, fill, and stroke.
    let payload = encode(&PrTextDocument {
        fill: Some(PrRgb([255, 0, 0])),
        stroke: Some(PrTextStroke {
            color: PrRgb([0, 0, 255]),
            width: 10.0,
        }),
        leading: 50.0,
        justification: PrJustification::Right,
        frame: PrTextFrame::Box {
            width: 800.0,
            height: 400.0,
            vertical: PrVerticalAlign::Center,
        },
        ..document("Layout")
    })
    .unwrap();
    let doc = document_table(&payload);
    assert_eq!(doc.f32(2).unwrap(), Some(800.0));
    assert_eq!(doc.f32(3).unwrap(), Some(400.0));
    assert_eq!(doc.u32(4).unwrap(), Some(1));
    assert_eq!(doc.u32(5).unwrap(), Some(1));
    assert_eq!(doc.f32(6).unwrap(), Some(50.0));
    let style = style_table(&payload);
    assert_eq!(
        color(style.table(2).unwrap().unwrap()).unwrap(),
        PrRgb([255, 0, 0])
    );
    assert_eq!(style.u8(3).unwrap(), None);
    assert_eq!(
        color(style.table(4).unwrap().unwrap()).unwrap(),
        PrRgb([0, 0, 255])
    );
    assert_eq!(style.u8(5).unwrap(), Some(1));
    assert_eq!(style.f32(6).unwrap(), Some(10.0));

    let hidden = encode(&PrTextDocument {
        fill: None,
        justification: PrJustification::Justify,
        ..document("Hidden fill")
    })
    .unwrap();
    assert_eq!(document_table(&hidden).u32(4).unwrap(), Some(3));
    assert_eq!(style_table(&hidden).u8(3).unwrap(), Some(0));
}

const SHADOW: PrTextShadow = PrTextShadow {
    color: PrRgb([20, 40, 60]),
    opacity: 80.0,
    angle: 45.0,
    distance: 24.0,
    size: 2.0,
    blur: 8.0,
};

#[test]
fn shadows_round_trip_with_every_slot_written() {
    let expected = PrTextDocument {
        shadow: Some(SHADOW),
        ..document("Shadow")
    };
    let payload = encode(&expected).unwrap();
    assert_eq!(decode(&payload).unwrap().document, expected);
    // Premiere must not fall back to the inferred defaults, even for 135°.
    let default_angle = encode(&PrTextDocument {
        shadow: Some(PrTextShadow {
            angle: 135.0,
            ..SHADOW
        }),
        ..document("Shadow")
    })
    .unwrap();
    for payload in [&payload, &default_angle] {
        let doc = document_table(payload);
        assert_eq!(doc.u8(document::SHADOW_ENABLED).unwrap(), Some(1));
        let color_slot = doc.table(document::SHADOW_COLOR).unwrap().unwrap();
        assert_eq!(color_slot.present().unwrap(), [0, 1, 2]);
        for slot in document::SHADOW_VALUES {
            assert!(doc.f32(slot).unwrap().is_some(), "slot {slot}");
        }
    }
    assert_eq!(document_table(&default_angle).f32(13).unwrap(), Some(135.0));
    // No shadow writes none of its slots, as Premiere's unshadowed text.
    let (plain, _) = slots(&encode(&document("Plain")).unwrap());
    assert!(!plain.iter().any(|slot| (10..=16).contains(slot)));
}

#[test]
fn out_of_range_shadows_are_not_encoded() {
    // The conversion omits such a shadow; the writer must never emit it.
    for shadow in [
        PrTextShadow {
            opacity: 150.0,
            ..SHADOW
        },
        PrTextShadow {
            blur: f32::NAN,
            ..SHADOW
        },
    ] {
        let error = encode(&PrTextDocument {
            shadow: Some(shadow),
            ..document("Shadow")
        })
        .unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("invalid Premiere project: text shadow "),
            "{error}"
        );
    }
}

/// A minimal Premiere 26 document whose shadow is enabled with no values.
fn enabled_shadow_without_values() -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let tables: Vec<_> = (0..3).map(|_| empty_table(&mut fbb)).collect();
    let style_table = {
        let table = fbb.start_table();
        fbb.push_slot_always(slot(21), tables[0]);
        fbb.push_slot_always(slot(23), tables[1]);
        fbb.push_slot_always(slot(24), 2_u32);
        fbb.end_table(table)
    };
    let string = fbb.create_string("Default shadow");
    let run_table = {
        let table = fbb.start_table();
        fbb.push_slot_always(slot(0), string);
        fbb.push_slot_always(slot(1), style_table);
        fbb.end_table(table)
    };
    let runs = fbb.create_vector(&[run_table]);
    let font = fbb.create_string("Arial-BoldMT");
    let fonts = fbb.create_vector(&[font]);
    let document = {
        let table = fbb.start_table();
        fbb.push_slot_always(slot(0), runs);
        fbb.push_slot_always(slot(1), fonts);
        fbb.push_slot_always(slot(11), 1_u8);
        fbb.push_slot_always(slot(26), 1_u8);
        fbb.push_slot_always(slot(40), tables[2]);
        fbb.push_slot_always(slot(43), 0_u8);
        fbb.push_slot_always(slot(44), 1_u8);
        fbb.end_table(table)
    };
    let root = {
        let table = fbb.start_table();
        fbb.push_slot_always(slot(0), document);
        fbb.end_table(table)
    };
    fbb.finish_minimal(root);
    let buffer = fbb.finished_data();
    let mut payload = (buffer.len() as u64).to_le_bytes().to_vec();
    payload.extend_from_slice(&MAGIC.to_le_bytes());
    payload.extend_from_slice(buffer);
    payload
}

#[test]
fn omitted_shadow_values_read_as_the_inferred_legacy_defaults() {
    // Premiere's legacy JSON defaults: color 0x3F3F3F, opacity 75, angle 135,
    // offset 7, size 0 and blur 40 (`lower_third.prproj`, SHA-256
    // `a612c8db1cc74ff75aa9b0d45f04d930bb948692ce60bd9c611adbd24ce284eb`).
    let decoded = decode(&enabled_shadow_without_values()).unwrap();
    assert_eq!(
        decoded.document.shadow,
        Some(PrTextShadow {
            color: PrRgb([63, 63, 63]),
            opacity: 75.0,
            angle: 135.0,
            distance: 7.0,
            size: 0.0,
            blur: 40.0,
        })
    );
    assert!(decoded.omitted.is_empty());
}

#[test]
fn unknown_shadow_flags_reject() {
    let mut payload = bytes(CAPTION_STYLE_EFFECTS);
    let at = HEADER_BYTES
        + document_table(&payload)
            .field(document::SHADOW_ENABLED, 1)
            .unwrap()
            .unwrap();
    assert_eq!(payload[at], 1);
    payload[at] = 2;
    assert!(error(&payload).contains("invalid text shadow flag 2"));
}

// A Premiere 25.5 (project Version 43) caption cue: "It's been" in
// MinionPro-Regular at 48 px with the default white fill, center
// justification, bottom alignment and an enabled shadow, like all 42 cues of
// its track. From `practice_files_transcription_magic.prproj`, sequence
// `Practice-Sequence_CAPTIONS-ONLY`, `CaptionDataClipTrackItem:135` -> `Block:210`.
pub(crate) const CORPUS_CAPTION_CUE: &str = concat!(
    "hAEAAAAAAABEMyIRDAAAAAAABgAKAAQABgAAAGQAAAAAAF4APAAUABAAAAAAADgANAAAAAAAAAAAADAALwAoAAAA",
    "JAAgABwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADwAAAAgAAAAAABsABwBeAAAA",
    "AAAAATQAAAAAAAABUAAAAGwAAAAAAAAAAABAQQAAwEAAAEBAAADIQgAAAAEoAAAAAgAAAAIAAABE////SP///wQA",
    "BgAEAAAAAAAKAAgABQAGAAcACgAAAAAAAAABAAAABAAAABEAAABNaW5pb25Qcm8tUmVndWxhcgAAAAEAAAAMAAAA",
    "CAAOAAQACAAIAAAAbAAAADwAAAAAADYAGAAAABQAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    "AAAMAAAACAAEADYAAAACAAAAEAAAABAAAAAAAIBAAABAQvT////4/////P///wQABAAEAAAACQAAAEl0J3MgYmVl",
    "bgAAAA==",
);

// The `CaptionDataTemplateStyle` of the same corpus caption track: text "a"
// in the style of every cue on the track.
pub(crate) const CORPUS_CAPTION_TEMPLATE: &str = concat!(
    "fAEAAAAAAABEMyIRDAAAAAAABgAKAAQABgAAAGQAAAAAAF4APAAUABAAAAAAADgANAAAAAAAAAAAADAALwAoAAAA",
    "JAAgABwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADwAAAAgAAAAAABsABwBeAAAA",
    "AAAAATQAAAAAAAABUAAAAGwAAAAAAAAAAABAQQAAwEAAAEBAAADIQgAAAAEoAAAAAgAAAAIAAABE////SP///wQA",
    "BgAEAAAAAAAKAAgABQAGAAcACgAAAAAAAAABAAAABAAAABEAAABNaW5pb25Qcm8tUmVndWxhcgAAAAEAAAAMAAAA",
    "CAAOAAQACAAIAAAAbAAAADwAAAAAADYAGAAAABQAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    "AAAMAAAACAAEADYAAAACAAAAEAAAABAAAAAAAIBAAABAQvT////4/////P///wQABAAEAAAAAQAAAGEAAAA=",
);

/// The shadow of [`CORPUS_CAPTION_CUE`] and [`CORPUS_CAPTION_TEMPLATE`]:
/// black, 100% opaque, 3 px at the inferred 135°, size 6 and blur 12.
pub(crate) const CORPUS_CAPTION_SHADOW: PrTextShadow = PrTextShadow {
    color: PrRgb([0, 0, 0]),
    opacity: 100.0,
    angle: 135.0,
    distance: 3.0,
    size: 6.0,
    blur: 12.0,
};

/// `payload` with its enabled shadow switched off in place; the stored shadow
/// values stay, as Premiere keeps them for a disabled shadow.
fn without_shadow(payload: &[u8]) -> Vec<u8> {
    let flag = document_table(payload)
        .field(document::SHADOW_ENABLED, 1)
        .unwrap()
        .expect("the payload stores its shadow flag");
    let mut edited = payload.to_vec();
    edited[HEADER_BYTES + flag] = 0;
    edited
}

fn vtable_entry(payload: &[u8], slot: usize) -> std::ops::Range<usize> {
    let start = HEADER_BYTES + document_table(payload).vtable + 4 + 2 * slot;
    start..start + 2
}

/// A Premiere 26 graphic payload moved to the caption document layout by
/// renaming its marker slot; both markers store the value 1.
pub(crate) fn with_caption_marker(mut payload: Vec<u8>) -> Vec<u8> {
    let graphic = vtable_entry(&payload, document::FIXED_FLAGS[0].0);
    let caption = vtable_entry(&payload, document::CAPTION_FIXED_FLAGS[0].0);
    payload.copy_within(graphic.clone(), caption.start);
    payload[graphic].fill(0);
    payload
}

/// `document` as a caption block's `FormattedTextData`. Point text stores
/// `alignment` as its box alignment, as caption cues do: the encoder writes
/// it for a box, whose size is then removed.
pub(crate) fn caption_payload(doc: &PrTextDocument, alignment: PrVerticalAlign) -> Vec<u8> {
    let point = matches!(doc.frame, PrTextFrame::Point { .. });
    let frame = if point {
        PrTextFrame::Box {
            width: 1.0,
            height: 1.0,
            vertical: alignment,
        }
    } else {
        doc.frame
    };
    let mut payload = encode(&PrTextDocument {
        frame,
        ..doc.clone()
    })
    .unwrap();
    if point {
        for slot in [document::BOX_WIDTH, document::BOX_HEIGHT] {
            let entry = vtable_entry(&payload, slot);
            payload[entry].fill(0);
        }
    }
    with_caption_marker(payload)
}

#[test]
fn caption_blocks_decode_with_their_own_document_markers() {
    let corpus = bytes(CORPUS_CAPTION_CUE);
    let decoded = decode_caption(&corpus).unwrap();
    assert!(decoded.omitted.is_empty());
    assert_eq!(decoded.box_alignment, PrVerticalAlign::Bottom);
    let expected = PrTextDocument {
        text: "It's been".into(),
        font: "MinionPro-Regular".into(),
        size: 48.0,
        justification: PrJustification::Center,
        frame: PrTextFrame::Point {
            vertical: PrVerticalAlign::Bottom,
        },
        ..document("")
    };
    // The enabled shadow decodes like the caption style preview's.
    let shadowed = PrTextDocument {
        shadow: Some(CORPUS_CAPTION_SHADOW),
        ..expected.clone()
    };
    assert_eq!(decoded.document, shadowed);
    // The corpus cannot show whether a cue renders from its own style or from
    // its track template: the two are the same.
    let template = decode_caption(&bytes(CORPUS_CAPTION_TEMPLATE)).unwrap();
    assert_eq!(template.omitted, decoded.omitted);
    assert_eq!(template.box_alignment, decoded.box_alignment);
    assert_eq!(
        template.document,
        PrTextDocument {
            text: "a".into(),
            ..shadowed
        }
    );
    // Graphics retain the known document fields from the alternate profile
    // with a diagnostic. Caption admission remains strict.
    let graphic = decode(&corpus).unwrap();
    assert_eq!(graphic.document, decoded.document);
    assert_eq!(
        graphic.omitted,
        [OmittedTextFeature::AlternateDocumentMarkers]
    );
    assert_eq!(
        decode_caption(&bytes(BEFORE)).unwrap_err().to_string(),
        "unsupported conversion: unsupported Source Text field document[26]"
    );
    let unmarked = |mut payload: Vec<u8>, slot| {
        let entry = vtable_entry(&payload, slot);
        payload[entry].fill(0);
        payload
    };
    assert_eq!(
        decode_caption(&unmarked(corpus.clone(), 38))
            .unwrap_err()
            .to_string(),
        "unsupported conversion: Source Text lacks the Premiere 25.5 caption document marker in slot 38"
    );
    assert_eq!(
        error(&unmarked(bytes(BEFORE), 26)),
        "unsupported conversion: Source Text lacks the Premiere 26 document marker in slot 26"
    );

    let unshadowed = decode_caption(&without_shadow(&corpus)).unwrap();
    assert!(unshadowed.omitted.is_empty());
    assert_eq!(unshadowed.document, expected);
    let generated = decode_caption(&caption_payload(&expected, PrVerticalAlign::Bottom)).unwrap();
    assert!(generated.omitted.is_empty());
    assert_eq!(generated.box_alignment, PrVerticalAlign::Bottom);
    assert_eq!(generated.document, expected);
}

#[test]
fn legacy_json_source_text_decodes_as_one_editable_point_text_document() {
    use crate::tests::support::{legacy_run, legacy_source_text, legacy_source_text_payload};
    use serde_json::json;
    let uniform = |payload: &[u8]| {
        let decoded = decode_graphic(payload).unwrap();
        assert!(decoded.omitted.is_empty());
        let TextDocuments::Uniform(document) = decoded.documents else {
            panic!("one uniform document");
        };
        document
    };
    // Our own text: its CR paragraph break becomes LF, and the BMP and
    // supplementary characters stay.
    let centred = PrTextDocument {
        text: "Night\nMarket \u{2713} \u{1f525}".into(),
        font: "Inter-SemiBold".into(),
        size: 64.5,
        fill: Some(PrRgb([128, 128, 128])),
        stroke: None,
        shadow: None,
        all_caps: false,
        tracking: 25.0,
        leading: 0.0,
        justification: PrJustification::Center,
        frame: PrTextFrame::Point {
            vertical: PrVerticalAlign::Top,
        },
        background: None,
    };
    let payload = legacy_source_text_payload(&legacy_source_text().to_string());
    assert_eq!(uniform(&payload), centred);
    // The fields may come in another order, here `mVersion` first.
    let reordered = format!(
        "{{\"mVersion\":1,\"mTextParam\":{}}}",
        legacy_source_text()["mTextParam"]
    );
    assert_eq!(uniform(&legacy_source_text_payload(&reordered)), centred);
    // Left-aligned black text, and text whose fill is off, which stays
    // invisible whatever its hidden color.
    let mut left = legacy_source_text();
    left["mTextParam"]["mAlignment"] = json!(0);
    left["mTextParam"]["mStyleSheet"]["mFillColor"] = legacy_run(json!(0));
    assert_eq!(
        uniform(&legacy_source_text_payload(&left.to_string())),
        PrTextDocument {
            fill: Some(PrRgb([0, 0, 0])),
            justification: PrJustification::Left,
            ..centred.clone()
        }
    );
    let mut unfilled = legacy_source_text();
    unfilled["mTextParam"]["mStyleSheet"]["mFillVisible"] = legacy_run(json!(false));
    unfilled["mTextParam"]["mStyleSheet"]["mFillColor"] = legacy_run(json!(0x00_ff_00));
    assert_eq!(
        uniform(&legacy_source_text_payload(&unfilled.to_string())),
        PrTextDocument {
            fill: None,
            ..centred.clone()
        }
    );
    // Source Text keys and caption blocks still read only Premiere 26 values.
    let reason = "unsupported conversion: legacy UTF-16 JSON text from Premiere before 26 converts only as a static graphic Source Text value";
    assert_eq!(error(&payload), reason);
    assert_eq!(decode_caption(&payload).unwrap_err().to_string(), reason);
}

#[test]
fn legacy_json_source_text_requires_paragraph_object() {
    use crate::tests::support::{legacy_source_text, legacy_source_text_payload};
    use serde_json::json;
    let mut text = legacy_source_text();
    let paragraph = &text["mTextParam"];
    // The same valid fields in the struct's declaration order must not
    // stand in for the required named object.
    text["mTextParam"] = json!([
        paragraph["mAlignment"],
        paragraph["mDefaultRun"],
        paragraph["mHeight"],
        paragraph["mHindiDigits"],
        paragraph["mIndic"],
        paragraph["mIsVerticalText"],
        paragraph["mLeading"],
        paragraph["mLigatures"],
        paragraph["mRTL"],
        paragraph["mShadowAngle"],
        paragraph["mShadowBlur"],
        paragraph["mShadowColor"],
        paragraph["mShadowOffset"],
        paragraph["mShadowOpacity"],
        paragraph["mShadowSize"],
        paragraph["mShadowVisible"],
        paragraph["mStyleSheet"],
        paragraph["mTabWidth"],
        paragraph["mWidth"]
    ]);
    let payload = legacy_source_text_payload(&text.to_string());
    let error = decode_graphic(&payload).unwrap_err().to_string();
    assert!(
        error.contains("expected an object with named fields"),
        "{error}"
    );
}

#[test]
fn legacy_json_source_text_requires_style_object() {
    use crate::tests::support::{legacy_source_text, legacy_source_text_payload};
    use serde_json::json;
    let mut text = legacy_source_text();
    let style = &text["mTextParam"]["mStyleSheet"];
    text["mTextParam"]["mStyleSheet"] = json!([
        style["mBaselineOption"],
        style["mBaselineShift"],
        style["mCapsOption"],
        style["mFauxBold"],
        style["mFauxItalic"],
        style["mFillColor"],
        style["mFillOverStroke"],
        style["mFillVisible"],
        style["mFontName"],
        style["mFontSize"],
        style["mKerning"],
        style["mStrokeColor"],
        style["mStrokeVisible"],
        style["mStrokeWidth"],
        style["mText"],
        style["mTracking"],
        style["mTsumi"]
    ]);
    let payload = legacy_source_text_payload(&text.to_string());
    let error = decode_graphic(&payload).unwrap_err().to_string();
    assert!(
        error.contains("expected an object with named fields"),
        "{error}"
    );
}

#[test]
fn legacy_json_source_text_requires_run_wrapper_object() {
    use crate::tests::support::{legacy_source_text, legacy_source_text_payload};
    use serde_json::json;
    let mut text = legacy_source_text();
    text["mTextParam"]["mStyleSheet"]["mFontSize"] = json!([[[0, 64.5]]]);
    let payload = legacy_source_text_payload(&text.to_string());
    let error = decode_graphic(&payload).unwrap_err().to_string();
    assert!(
        error.contains("expected an object with named fields"),
        "{error}"
    );
}

#[test]
fn legacy_json_source_text_rejects_nested_duplicate_fields() {
    use crate::tests::support::{legacy_source_text, legacy_source_text_payload};
    let json = legacy_source_text().to_string();
    for (field, value) in [
        ("mAlignment", "2"),
        ("mFontSize", r#"{"mParamValues":[[0,64.5]]}"#),
        ("mParamValues", "[[0,64.5]]"),
    ] {
        let member = format!("\"{field}\":{value}");
        assert!(json.contains(&member));
        // Raw JSON retains duplicate members, unlike a Value object.
        let duplicated = json.replacen(&member, &format!("{member},{member}"), 1);
        let error = decode_graphic(&legacy_source_text_payload(&duplicated))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(&format!("duplicate field `{field}`")),
            "{error}"
        );
    }
}

#[test]
fn legacy_json_source_text_rejects_hostile_mixed_and_unknown_active_forms() {
    use crate::tests::support::{legacy_run, legacy_source_text, legacy_source_text_payload};
    use serde_json::{json, Value};
    let edited = |edit: fn(&mut Value)| {
        let mut text = legacy_source_text();
        edit(&mut text);
        legacy_source_text_payload(&text.to_string())
    };
    let json = legacy_source_text().to_string();
    let valid = legacy_source_text_payload(&json);
    let mut miscounted = valid.clone();
    miscounted[0] ^= 2;
    let mut odd = valid.clone();
    odd.push(0);
    let odd_count = u64::try_from(odd.len() - 8).unwrap();
    odd[..8].copy_from_slice(&odd_count.to_le_bytes());
    // An unpaired high surrogate before the text.
    let unpaired = {
        let at = json[..json.find("Night").unwrap()].encode_utf16().count();
        let mut units: Vec<u16> = json.encode_utf16().collect();
        units.insert(at, 0xd800);
        let text: Vec<u8> = units.into_iter().flat_map(u16::to_le_bytes).collect();
        let mut payload = u64::try_from(text.len()).unwrap().to_le_bytes().to_vec();
        payload.extend(text);
        payload
    };
    let cases = [
        (miscounted, "its byte count"),
        (odd, "an odd byte count"),
        (unpaired, "invalid UTF-16: unpaired surrogate"),
        // A terminator, which the form does not have.
        (
            legacy_source_text_payload(&format!("{json}\0")),
            "trailing characters",
        ),
        (
            legacy_source_text_payload(&json.replacen(
                "\"mVersion\":1",
                "\"mVersion\":1,\"mVersion\":1",
                1,
            )),
            "duplicate field `mVersion`",
        ),
        (
            edited(|text| text["mVersion"] = json!(2)),
            "version 2 is unsupported",
        ),
        (
            edited(|text| text["mMask"] = json!(true)),
            "unknown field `mMask`",
        ),
        (
            edited(|text| {
                text["mTextParam"]["mStyleSheet"]["mUnderline"] = legacy_run(json!(true));
            }),
            "incomplete passive decoration layout",
        ),
        (
            edited(|text| {
                text["mTextParam"]
                    .as_object_mut()
                    .unwrap()
                    .remove("mShadowVisible");
            }),
            "missing field `mShadowVisible`",
        ),
        (
            edited(|text| text["mTextParam"]["mAlignment"] = json!(2.0)),
            "invalid type: floating point `2.0`, expected u32",
        ),
        (
            edited(|text| {
                text["mTextParam"]["mStyleSheet"]["mFontSize"] =
                    json!({"mParamValues": [[0, 64.5], [6, 40]]});
            }),
            "mixed text styles are unsupported: a style holds 2 runs, not one",
        ),
        (
            edited(|text| {
                text["mTextParam"]["mStyleSheet"]["mFillColor"] =
                    json!({"mParamValues": [[3, 0x80_80_80]]});
            }),
            "its one style run starts at character 3, not 0",
        ),
        (
            edited(|text| text["mTextParam"]["mAlignment"] = json!(1)),
            "alignment 1 is unsupported; only 0 (left) and 2 (centred) convert",
        ),
        (
            edited(|text| {
                text["mTextParam"]["mWidth"] = json!(800);
                text["mTextParam"]["mHeight"] = json!(200);
            }),
            "box text (800 x 200) is unsupported",
        ),
        (
            edited(|text| text["mTextParam"]["mLeading"] = json!(12)),
            "leading 12 is unsupported",
        ),
        (
            edited(|text| text["mTextParam"]["mDefaultRun"] = json!([{}])),
            "a nonempty mDefaultRun is unsupported",
        ),
        (
            edited(|text| text["mTextParam"]["mRTL"] = json!(true)),
            "mRTL true is unsupported",
        ),
        (
            edited(|text| {
                text["mTextParam"]["mStyleSheet"]["mCapsOption"] = legacy_run(json!(2));
            }),
            "mCapsOption 2 is unsupported",
        ),
        (
            edited(|text| {
                text["mTextParam"]["mStyleSheet"]["mKerning"] = legacy_run(json!(-40));
            }),
            "mKerning -40 is unsupported",
        ),
        (
            edited(|text| text["mTextParam"]["mStyleSheet"]["mText"] = json!("Tab\there")),
            "a tab is unsupported",
        ),
        (
            edited(|text| {
                text["mTextParam"]["mStyleSheet"]["mFillColor"] = legacy_run(json!(0x10_20_30));
            }),
            "mFillColor 0x102030 is not gray",
        ),
        (
            edited(|text| {
                text["mTextParam"]["mStyleSheet"]["mStrokeColor"] = legacy_run(json!(0x0100_0000));
            }),
            "mStrokeColor 0x1000000 is outside the 24-bit color form",
        ),
        (
            edited(|text| {
                text["mTextParam"]["mStyleSheet"]["mStrokeVisible"] = legacy_run(json!(true));
                text["mTextParam"]["mStyleSheet"]["mStrokeColor"] = legacy_run(json!(0x303030));
                text["mTextParam"]["mStyleSheet"]["mFillOverStroke"] = legacy_run(json!(false));
            }),
            "stroke over fill is unsupported",
        ),
        (
            edited(|text| text["mTextParam"]["mShadowVisible"] = json!(true)),
            "an enabled shadow is unsupported",
        ),
    ];
    for (payload, reason) in cases {
        let error = decode_graphic(&payload).unwrap_err().to_string();
        assert!(
            error.starts_with("unsupported conversion: legacy UTF-16 JSON Source Text: ")
                && error.contains(reason),
            "{reason}: {error}"
        );
    }
}

#[test]
fn mask_with_text_round_trips_on_graphic_text_and_fails_closed_elsewhere() {
    use crate::schema::text::PrMaskSource;
    for mask_source in [
        None,
        Some(PrMaskSource { inverted: false }),
        Some(PrMaskSource { inverted: true }),
    ] {
        let payload = super::encode_graphic(&document("Mask"), mask_source).unwrap();
        let decoded = decode(&payload).unwrap();
        assert_eq!(decoded.mask_source, mask_source);
        assert_eq!(decoded.document, document("Mask"));
    }
    // `payload` with document slot `slot` reading the inline u8 of slot
    // `from`, as the native control stores the flags.
    let alias = |payload: &[u8], slot: usize, from: usize| {
        let mut payload = payload.to_vec();
        let buffer = &mut payload[HEADER_BYTES..];
        let word = |buffer: &[u8], at: usize| {
            u32::from_le_bytes(buffer[at..at + 4].try_into().unwrap()) as usize
        };
        let vtable = |buffer: &[u8], table: usize| {
            (table as i64
                - i64::from(i32::from_le_bytes(
                    buffer[table..table + 4].try_into().unwrap(),
                ))) as usize
        };
        let entry = |buffer: &[u8], vtable: usize, slot: usize| {
            u16::from_le_bytes(
                buffer[vtable + 4 + 2 * slot..vtable + 6 + 2 * slot]
                    .try_into()
                    .unwrap(),
            )
        };
        let root = word(buffer, 0);
        let document = root + entry(buffer, vtable(buffer, root), 0) as usize;
        let document = document + word(buffer, document);
        let document_vtable = vtable(buffer, document);
        let value = entry(buffer, document_vtable, from);
        buffer[document_vtable + 4 + 2 * slot..document_vtable + 6 + 2 * slot]
            .copy_from_slice(&value.to_le_bytes());
        payload
    };
    let graphic = super::encode_graphic(&document("Mask"), None).unwrap();
    assert_eq!(
        decode(&alias(&graphic, 21, 26)).unwrap().mask_source,
        Some(PrMaskSource { inverted: false })
    );
    let error = decode(&alias(&graphic, 22, 26)).unwrap_err().to_string();
    assert!(
        error.contains(
            "Mask with Text (document slots 21 and 22) at None and Some(1) is unmeasured"
        ),
        "{error}"
    );
    // A caption keeps no mask.
    let caption = alias(&bytes(CORPUS_CAPTION_TEMPLATE), 21, 38);
    let error = decode_caption(&caption).unwrap_err().to_string();
    assert!(
        error.contains("Mask with Text on a caption is unsupported"),
        "{error}"
    );
}

#[test]
fn graphic_root_extensions_are_diagnosed_and_never_replayed() {
    let expected = document("Editable root document");
    let original = encode(&expected).unwrap();
    let old = Buffer::from_payload(&original, SOURCE_TEXT).unwrap();
    let document_at = old
        .table(old.offset(0).unwrap())
        .unwrap()
        .table(0)
        .unwrap()
        .unwrap()
        .position;
    // Wrap the existing document in a new root with an unidentified field.
    // Its bits deliberately are not a usable pointer; they are not followed.
    let mut buffer = vec![0_u8; 32];
    buffer[0..4].copy_from_slice(&16_u32.to_le_bytes());
    buffer[4..6].copy_from_slice(&8_u16.to_le_bytes());
    buffer[6..8].copy_from_slice(&12_u16.to_le_bytes());
    buffer[8..10].copy_from_slice(&4_u16.to_le_bytes());
    buffer[10..12].copy_from_slice(&8_u16.to_le_bytes());
    buffer[16..20].copy_from_slice(&12_i32.to_le_bytes());
    buffer[20..24].copy_from_slice(&u32::try_from(document_at + 12).unwrap().to_le_bytes());
    buffer[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
    buffer.extend_from_slice(old.bytes);
    let payload = super::framed(&buffer, SOURCE_TEXT).unwrap();
    let decoded = decode(&payload).unwrap();
    assert_eq!(decoded.document, expected);
    assert_eq!(decoded.omitted, [OmittedTextFeature::UnknownRootData(1)]);
    assert!(decode_caption(&payload)
        .unwrap_err()
        .to_string()
        .contains("root[1]"));
    assert!(decode(&encode(&decoded.document).unwrap())
        .unwrap()
        .omitted
        .is_empty());
    // An invalid inline field location is still a malformed buffer.
    buffer[10..12].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(decode(&super::framed(&buffer, SOURCE_TEXT).unwrap()).is_err());
}
