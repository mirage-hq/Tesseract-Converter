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
    assert!(error(&bytes(PRE_26_BOX)).contains("unsupported Source Text field document[8]"));

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
    let mut fbb = FlatBufferBuilder::new();
    let tables: Vec<_> = (0..3).map(|_| empty_table(&mut fbb)).collect();
    let mut runs = Vec::new();
    for &(text, size) in source {
        let style_table = {
            let table = fbb.start_table();
            fbb.push_slot_always(slot(1), size);
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
    let runs = fbb.create_vector(&runs);
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
    // Each layout rejects the other's marker slot instead of guessing, and
    // names the release its markers were recorded from.
    assert_eq!(
        error(&corpus),
        "unsupported conversion: unsupported Source Text field document[38]"
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
