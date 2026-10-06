use super::super::text_document::{TextDocumentKey, TextDocumentSpec};
use super::*;
use crate::structure_document::text::cos::{self, Value};

mod p012_cache;

fn spec(text: &str) -> TextDocumentSpec {
    TextDocumentSpec {
        text: text.into(),
        font_postscript: "Arial-Regular".into(),
        font_format: None,
        font_size: 48.0,
        apply_fill: true,
        fill_color: [0.2, 0.4, 0.6, 0.8],
        apply_stroke: false,
        stroke_color: Some([0.1, 0.3, 0.5, 0.7]),
        stroke_width: 1.0,
        stroke_over_fill: true,
        justification: Justification::Center,
        tracking: 25.0,
        leading: Some(52.0),
        baseline_shift: 3.0,
        box_size: Some([660.0, 68.0]),
        box_position: Some([10.0, 20.0]),
        vertical_align: None,
        all_caps: false,
    }
}

fn timeline(document: TextDocumentSpec) -> TextDocumentTimeline {
    TextDocumentTimeline {
        keyed: false,
        keys: vec![TextDocumentKey {
            time_millis: 0,
            document,
        }],
    }
}

#[test]
fn boxed_vertical_alignment_changes_across_hold_keys_are_rejected() {
    let mut first = spec("First");
    first.vertical_align = Some(fx_schema::VerticalAlign::Center);
    let mut second = first.clone();
    second.vertical_align = Some(fx_schema::VerticalAlign::Bottom);
    let keyed = TextDocumentTimeline {
        keyed: true,
        keys: vec![
            TextDocumentKey {
                time_millis: 0,
                document: first,
            },
            TextDocumentKey {
                time_millis: 500,
                document: second,
            },
        ],
    };
    assert!(keyed.validate().is_err());
    assert!(profile::render(&keyed).is_err());
}

fn payload(group: &Chunk) -> &[u8] {
    source_in_group(group)
        .unwrap()
        .children()
        .unwrap()
        .iter()
        .find_map(Chunk::opaque_payload)
        .unwrap()
}

fn at<'a>(mut value: &'a Value, keys: &[&str]) -> &'a Value {
    for key in keys {
        value = value.get(key).unwrap();
    }
    value
}

fn document(value: &Value, index: usize) -> &Value {
    at(value, &["1", "1"]).index(index).unwrap()
}

fn run_style<'a>(value: &'a Value, name: &str) -> &'a Value {
    let run = at(value, &["0", name, "0"]).index(0).unwrap();
    at(run, &["0", "0", name])
}

#[test]
fn native_tracking_uses_integer_tokens_not_fixed_point_reals() {
    for tracking in [260.0, -120.0, 260.4] {
        let mut value = spec("Hello, World.");
        value.tracking = tracking;
        let expected = format!("/8 {} /9", tracking.round());
        for point in [false, true] {
            if point {
                value.box_size = None;
                value.box_position = None;
            }
            let bytes = if point {
                profile::render_point(&timeline(value.clone())).unwrap()
            } else {
                profile::render(&timeline(value.clone())).unwrap()
            };
            assert!(
                bytes
                    .windows(expected.len())
                    .any(|bytes| bytes == expected.as_bytes())
            );
        }
    }
}

#[test]
fn native_font_size_and_leading_use_single_precision_without_rounding_geometry() {
    for point in [false, true] {
        let mut input = spec("DEEP BLUE");
        input.font_size = 646.9260017548991;
        input.leading = Some(29.700000000000003);
        input.box_size = Some([660.1234567890123, 68.0]);
        input.box_position = Some([0.0, 0.0]);
        if point {
            input.box_size = None;
            input.box_position = None;
        }
        let bytes = if point {
            profile::render_point(&timeline(input)).unwrap()
        } else {
            profile::render(&timeline(input)).unwrap()
        };
        for expected in ["/1 646.926 /2 false", "/5 29.7 /6"] {
            assert!(
                bytes
                    .windows(expected.len())
                    .any(|part| part == expected.as_bytes())
            );
        }
        if !point {
            let geometry = b"660.1234567890123";
            assert!(bytes.windows(geometry.len()).any(|part| part == geometry));
        }
        let root = cos::parse(&bytes).unwrap();
        let style = run_style(document(&root, 0), "6");
        assert_eq!(
            style.get("1").unwrap().as_f64().unwrap() as f32,
            646.9260017548991_f64 as f32
        );
        assert_eq!(
            style.get("5").unwrap().as_f64().unwrap() as f32,
            29.700000000000003_f64 as f32
        );
    }
}

#[test]
fn native_style_scalars_reject_nonfinite_single_precision_casts() {
    for size in [f64::MAX, f64::INFINITY, f64::NAN] {
        let mut input = spec("Range");
        input.font_size = size;
        assert!(profile::render(&timeline(input)).is_err());
    }
    let mut input = spec("Range");
    input.leading = Some(f64::MAX);
    assert!(profile::render(&timeline(input)).is_err());
}

#[test]
fn native_positive_style_scalars_reject_single_precision_underflow() {
    for point in [false, true] {
        for leading in [false, true] {
            let mut input = spec("Positive range");
            if point {
                input.box_size = None;
                input.box_position = None;
            }
            if leading {
                input.leading = Some(1e-50);
            } else {
                input.font_size = 1e-50;
            }
            let result = if point {
                profile::render_point(&timeline(input))
            } else {
                profile::render(&timeline(input))
            };
            let error = result.expect_err("positive authored style must not become native zero");
            assert!(
                error
                    .to_string()
                    .contains(if leading { "leading" } else { "font size" }),
                "{error}"
            );
        }
    }
}

#[test]
fn native_positive_style_scalars_keep_representable_subnormal_values() {
    for point in [false, true] {
        let mut input = spec("Positive subnormal");
        let expected = f64::from(f32::from_bits(1));
        input.font_size = expected;
        input.leading = Some(expected);
        if point {
            input.box_size = None;
            input.box_position = None;
        }
        let bytes = if point {
            profile::render_point(&timeline(input)).unwrap()
        } else {
            profile::render(&timeline(input)).unwrap()
        };
        let root = cos::parse(&bytes).unwrap();
        let style = run_style(document(&root, 0), "6");
        for slot in ["1", "5"] {
            assert_eq!(
                style.get(slot).unwrap().as_f64().unwrap() as f32,
                expected as f32
            );
        }
    }
}

#[test]
fn byte_backed_font_formats_are_serialized_in_point_and_box_profiles() {
    use super::super::text_document::FontFormat;
    for (format, expected) in [(FontFormat::TrueType, 1), (FontFormat::Cff, 0)] {
        let mut input = spec("Format");
        input.font_format = Some(format);
        let boxed = cos::parse(&profile::render(&timeline(input.clone())).unwrap()).unwrap();
        let font = at(&boxed, &["0", "1", "0"]).index(0).unwrap();
        assert_eq!(at(font, &["0", "0", "2"]).as_i64(), Some(expected));
        input.box_size = None;
        input.box_position = None;
        let point = cos::parse(&profile::render_point(&timeline(input)).unwrap()).unwrap();
        let font = at(&point, &["0", "1", "0"]).index(0).unwrap();
        assert_eq!(at(font, &["0", "0", "2"]).as_i64(), Some(expected));
    }
}

#[test]
fn point_root_does_not_replay_unknown_font_vendor_version() {
    fn version(root: &Value) -> Option<&Value> {
        at(root, &["1", "5"])
            .index(0)
            .unwrap()
            .get("4")
            .unwrap()
            .index(0)
            .unwrap()
            .get("1")
    }
    let source = include_bytes!(
        "../../../tests/fixtures/point_text_envelope/native_point_fresh49_vt323.aep"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(source)),
        "dd03022c630825da8ecf1f422b9ebfe1942780766915518d875f99b35c72862f"
    );
    let native = Rifx::parse_with(source, |kind| kind == *b"btdk").unwrap();
    let root = cos::parse(payload(
        native.chunks().iter().find_map(find_box_group).unwrap(),
    ))
    .unwrap();
    // This independently authored missing-face source omits the version slot.
    // Its failed strict render is NOT a resolved-font or fidelity oracle.
    assert!(version(&root).is_none());
    for font in ["ArialMT", "VT323-Regular"] {
        let mut input = spec("Fresh 49");
        input.font_postscript = font.into();
        input.box_size = None;
        input.box_position = None;
        let generated = point_properties(&timeline(input), 49, PropertyClock::DEFAULT).unwrap();
        let parsed = cos::parse(payload(&generated)).unwrap();
        assert!(
            version(&parsed).is_none(),
            "semantic input has no vendor version"
        );
    }
}

#[test]
fn single_font_point_has_stable_cache_free_encoding() {
    let mut input = spec("Fresh 49");
    input.font_postscript = "ArialMT".into();
    input.fill_color = [1.0; 4];
    input.stroke_color = Some([0.0, 0.0, 0.0, 1.0]);
    input.justification = Justification::Left;
    input.tracking = 0.0;
    input.leading = None;
    input.baseline_shift = 0.0;
    input.box_size = None;
    input.box_position = None;
    let owner = point_properties(&timeline(input), 49, PropertyClock::DEFAULT).unwrap();
    // Supplementary wire snapshot: unknown root font-version metadata is not
    // replayed, and Tracking now uses the verified native integer token. Native
    // proof is separate; this hash is not an independent feature oracle.
    assert_eq!(
        format!("{:x}", Sha256::digest(payload(&owner))),
        "c36bff9c278de1fac6c2c797a6e96fa88502f83330b19436eed64bf3763f6e2f"
    );
}

#[test]
fn point_automatic_leading_keeps_independent_native_inactive_default() {
    for (bytes, hash, size) in [
        (
            include_bytes!(
                "../../../tests/fixtures/point_text_envelope/native_point_fresh49_size64.aep"
            )
            .as_slice(),
            "5bf9308791ddb3c9af9a6ed26c5d4e98f1b08767095e0a7c88320eee417494d7",
            64.0,
        ),
        (
            include_bytes!(
                "../../../tests/fixtures/point_text_envelope/native_point_fresh49_size48.aep"
            )
            .as_slice(),
            "9067f2f5e03f18702d4456154b3e7f9401f127756ab0618b685b278c00573f4b",
            48.0,
        ),
    ] {
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), hash);
        let native = Rifx::parse_with(bytes, |kind| kind == *b"btdk").unwrap();
        let owner = native.chunks().iter().find_map(find_box_group).unwrap();
        let original = cos::parse(payload(owner)).unwrap();
        let original_style = run_style(document(&original, 0), "6");
        assert_eq!(original_style.get("1").unwrap().as_f64(), Some(size));
        assert_eq!(original_style.get("4").unwrap().as_bool(), Some(true));
        assert_eq!(original_style.get("5").unwrap().as_f64(), Some(0.01));
        let mut input = spec("Fresh 49");
        input.font_size = size;
        input.font_postscript = "ArialMT".into();
        input.leading = None;
        input.box_size = None;
        input.box_position = None;
        let generated = point_properties(&timeline(input), 49, PropertyClock::DEFAULT).unwrap();
        let parsed = cos::parse(payload(&generated)).unwrap();
        let style = run_style(document(&parsed, 0), "6");
        for key in ["1", "4", "5"] {
            assert_eq!(style.get(key), original_style.get(key));
        }
    }
}

#[test]
fn point_text_uses_independent_empty_point_defaults_and_fresh_semantics() {
    assert_eq!(
        format!("{:x}", Sha256::digest(POINT_FIXTURE)),
        "02ef66ccaae8337b9421ae910842e7c94fcccd0fcbece00eeacf27b461b48707"
    );
    let native = Rifx::parse_with(POINT_FIXTURE, |kind| kind == *b"btdk").unwrap();
    let original_group = native.chunks().iter().find_map(find_box_group).unwrap();
    let original = cos::parse(payload(original_group)).unwrap();
    assert_eq!(at(document(&original, 0), &["0", "0"]).as_str(), Some("\r"));
    let mut input = spec("Fresh editable Point 49");
    input.box_size = None;
    input.box_position = None;
    input.font_postscript = "ArialMT".into();
    let emitted = point_properties(&timeline(input), 49, PropertyClock::DEFAULT).unwrap();
    let parsed = cos::parse(payload(&emitted)).unwrap();
    assert_eq!(
        at(document(&parsed, 0), &["0", "0"]).as_str(),
        Some("Fresh editable Point 49\r")
    );
    assert_eq!(at(&parsed, &["0", "8"]), at(&original, &["0", "8"]));
    let point = at(&parsed, &["0", "8", "0"]).index(0).unwrap();
    assert!(at(point, &["0"]).get("1").is_none(), "no Box vertices");
    let Value::Dict(defaults) = original.get("0").unwrap() else {
        panic!("native defaults")
    };
    for (key, value) in defaults {
        if key != "1" {
            assert_eq!(at(&parsed, &["0", key]), value);
        }
    }
    for name in [
        b"ADBE Text Path Options".as_slice(),
        b"ADBE Text More Options",
    ] {
        assert_eq!(
            property(&emitted, name, *b"tdgp"),
            property(original_group, name, *b"tdgp")
        );
    }
    assert!(property(&emitted, b"ADBE Text Animators", *b"tdgp").is_none());
    assert_eq!(
        run_style(document(&parsed, 0), "6")
            .get("1")
            .unwrap()
            .as_f64(),
        Some(48.0)
    );
    // Source geometry/layout is never a substitute for shaping the edited text.
    assert!(
        at(document(&parsed, 0), &["1", "2"])
            .as_array()
            .unwrap()
            .is_empty()
    );
    for class in ["PC", "F", "R", "L", "S", "G"] {
        let marker = format!("/99 /{class} ");
        assert!(
            !payload(&emitted)
                .windows(marker.len())
                .any(|bytes| bytes == marker.as_bytes())
        );
    }
}

#[test]
fn boxed_automatic_leading_matches_native_without_inherited_layout() {
    let bytes = include_bytes!("../../../tests/fixtures/box_text_envelope/native_box_menlo18.aep");
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "9cb3ea4f56f2db5b01f90a6175b1486e74e270ea8c9fb156cf2ce422eec56436"
    );
    let native = Rifx::parse_with(bytes, |kind| kind == *b"btdk").unwrap();
    let owner = native.chunks().iter().find_map(find_box_group).unwrap();
    let original = cos::parse(payload(owner)).unwrap();
    let original_style = run_style(document(&original, 0), "6");
    assert_eq!(original_style.get("1").unwrap().as_f64(), Some(18.0));
    assert_eq!(original_style.get("4").unwrap().as_bool(), Some(true));
    assert_eq!(original_style.get("5").unwrap().as_f64(), Some(0.01));
    let mut input = spec("1080P / 30 FPS");
    input.font_postscript = "Menlo-Regular".into();
    input.font_size = 18.0;
    input.leading = None;
    input.box_size = Some([306.5, 46.8]);
    input.box_position = Some([0.0, 0.0]);
    let generated = boxed_properties(&timeline(input), 2500045, PropertyClock::DEFAULT).unwrap();
    let parsed = cos::parse(payload(&generated)).unwrap();
    let style = run_style(document(&parsed, 0), "6");
    for key in ["1", "4", "5"] {
        assert_eq!(style.get(key), original_style.get(key));
    }
    // An explicit empty layout cache opens but crashes native CloneCache while
    // rendering P012. Omit the optional cache instead of copying native glyphs.
    assert!(at(document(&parsed, 0), &["1"]).get("2").is_none());
}

#[test]
fn boxed_text_preserves_native_real_tokens_and_class_headers() {
    let native = Rifx::parse_with(BOX_FIXTURE, |kind| kind == *b"btdk").unwrap();
    let group = native.chunks().iter().find_map(find_box_group).unwrap();
    let original = cos::parse(payload(group)).unwrap();
    let emitted =
        boxed_properties(&timeline(spec("new input")), 8000, PropertyClock::DEFAULT).unwrap();
    let bytes = payload(&emitted);
    assert!(bytes.starts_with(b" /98 << /0 14 >> /0 <<"));
    // Full AST serialization previously moved /99 behind its object and changed
    // native real 1.0 to integer 1. These assertions fail with that serializer.
    for marker in [
        b"/53 << /99 /SimplePaint".as_slice(),
        b"/63 1.0 /64 4.0",
        b"/0 << /99 /CoolTypeFont",
    ] {
        assert!(bytes.windows(marker.len()).any(|window| window == marker));
    }
    let parsed = cos::parse(bytes).unwrap();
    assert_eq!(at(&parsed, &["98"]), at(&original, &["98"]));
    // Root fields outside the typed font/box/documents slots are independent
    // defaults, not newly serialized guesses.
    let Value::Dict(defaults) = original.get("0").unwrap() else {
        panic!("native root")
    };
    for (key, value) in defaults {
        if key != "1" && key != "8" {
            assert_eq!(at(&parsed, &["0", key]), value);
        }
    }
}

#[test]
fn boxed_text_replaces_native_authored_content_with_editable_fx_values() {
    let group = boxed_properties(
        &timeline(spec("Kasparov won the first match")),
        8000,
        PropertyClock::DEFAULT,
    )
    .unwrap();
    let parsed = cos::parse(payload(&group)).unwrap();
    let doc = document(&parsed, 0);
    let text = at(doc, &["0", "0"]).as_str().unwrap();
    assert_eq!(text, "Kasparov won the first match\r");
    let font = at(&parsed, &["0", "1", "0"]).index(0).unwrap();
    let Value::Dict(identity) = at(font, &["0", "0"]) else {
        panic!("font identity")
    };
    assert_eq!(identity.get("0").unwrap().as_str(), Some("Arial-Regular"));
    assert!(!identity.contains_key("2") && !identity.contains_key("5"));
    for name in ["5", "6"] {
        let runs = at(doc, &["0", name, "0"]).as_array().unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].get("1").unwrap().as_i64(), Some(29));
    }
    let style = run_style(doc, "6");
    for (name, expected) in [
        ("0", 0.0),
        ("1", 48.0),
        ("5", 52.0),
        ("8", 25.0),
        ("9", 3.0),
        ("12", 0.0),
        ("63", 1.0),
    ] {
        assert_eq!(style.get(name).unwrap().as_f64(), Some(expected));
    }
    assert_eq!(style.get("4").unwrap().as_bool(), Some(false));
    assert_eq!(run_style(doc, "5").get("0").unwrap().as_i64(), Some(2));
    for (name, expected) in [("53", [0.8, 0.2, 0.4, 0.6]), ("54", [0.7, 0.1, 0.3, 0.5])] {
        let actual: Vec<_> = at(style, &[name, "0", "1"])
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap())
            .collect();
        assert_eq!(actual, expected);
    }
    let rectangle = at(&parsed, &["0", "8", "0"]).index(0).unwrap();
    let vertices: Vec<_> = at(rectangle, &["0", "1", "0"])
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect();
    assert_eq!(&vertices[..4], &[10.0, 20.0, 10.0, 20.0]);
    assert_eq!(&vertices[12..16], &[670.0, 88.0, 670.0, 88.0]);
    assert_eq!(&vertices[28..], &[10.0, 20.0, 10.0, 20.0]);
    let donor = "Editable AEP\rPR 4442\r"
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect::<Vec<_>>();
    assert!(!payload(&group).windows(donor.len()).any(|x| x == donor));
}

#[test]
fn trailing_paragraph_box_text_keeps_authored_breaks_and_terminal_run_unit() {
    // The independent native control in trailing_paragraph/native.aep stores
    // A\r\r\r for the managed author/readback edit to A\r\r. The terminal
    // paragraph marker is additional content, not an existing editable break.
    for (input, expected) in [
        ("", "\r"),
        ("A", "A\r"),
        ("A\n", "A\r\r"),
        ("A\r\r", "A\r\r\r"),
        ("A\r\n\r\n", "A\r\r\r"),
        ("😀\n", "😀\r\r"),
    ] {
        let group = boxed_properties(&timeline(spec(input)), 1, PropertyClock::DEFAULT).unwrap();
        let parsed = cos::parse(payload(&group)).unwrap();
        let doc = document(&parsed, 0);
        assert_eq!(at(doc, &["0", "0"]).as_str(), Some(expected));
        for name in ["5", "6"] {
            let run = at(doc, &["0", name, "0"]).index(0).unwrap();
            assert_eq!(
                run.get("1").unwrap().as_i64(),
                Some(i64::try_from(expected.encode_utf16().count()).unwrap())
            );
        }
    }
}

#[test]
fn boxed_text_literal_input_cannot_inject_template_slots_or_cos_records() {
    let input = "{{font}} (\\) 😀\r\nsecond";
    let group = boxed_properties(&timeline(spec(input)), 1, PropertyClock::DEFAULT).unwrap();
    let parsed = cos::parse(payload(&group)).unwrap();
    assert_eq!(
        at(document(&parsed, 0), &["0", "0"]).as_str(),
        Some("{{font}} (\\) 😀\rsecond\r")
    );
    let units = "{{font}} (\\) 😀\rsecond\r".encode_utf16().count() as i64;
    let run = at(document(&parsed, 0), &["0", "6", "0"]).index(0).unwrap();
    assert_eq!(run.get("1").unwrap().as_i64(), Some(units));
}

#[test]
fn boxed_text_literal_line_endings_escape_all_utf16_bytes() {
    // CR/LF can occur in either byte of a UTF-16 code unit, not just in
    // paragraph separators. Raw bytes permit COS line-ending normalization.
    let input = "\u{010d}\u{010a}\u{0d01}\u{0a01}\nx";
    let group = boxed_properties(&timeline(spec(input)), 1, PropertyClock::DEFAULT).unwrap();
    let bytes = payload(&group);
    let encoded = b"(\xfe\xff\x01\\r\x01\\n\\r\x01\\n\x01\x00\\r\x00x\x00\\r)";
    assert!(bytes.windows(encoded.len()).any(|window| window == encoded));
    let parsed = cos::parse(bytes).unwrap();
    assert_eq!(
        at(document(&parsed, 0), &["0", "0"]).as_str(),
        Some("\u{010d}\u{010a}\u{0d01}\u{0a01}\rx\r")
    );
}

#[test]
fn boxed_text_holds_replace_documents_and_use_selected_native_clock_and_owner() {
    let mut input = timeline(spec("FIRST"));
    input.keyed = true;
    let mut second = spec("SECOND\nline");
    second.font_size = 36.0;
    second.all_caps = true;
    input.keys.push(TextDocumentKey {
        time_millis: 1250,
        document: second,
    });
    let clock = PropertyClock::for_rate(crate::timing::FrameRate::new(30.0).unwrap()).unwrap();
    let group = boxed_properties(&input, 8000, clock).unwrap();
    let parsed = cos::parse(payload(&group)).unwrap();
    assert_eq!(at(&parsed, &["1", "1"]).as_array().unwrap().len(), 2);
    assert_eq!(
        at(document(&parsed, 0), &["0", "0"]).as_str(),
        Some("FIRST\r")
    );
    assert_eq!(
        at(document(&parsed, 1), &["0", "0"]).as_str(),
        Some("SECOND\rline\r")
    );
    assert_eq!(
        run_style(document(&parsed, 1), "6")
            .get("1")
            .unwrap()
            .as_f64(),
        Some(36.0)
    );
    assert_eq!(
        run_style(document(&parsed, 1), "6")
            .get("12")
            .unwrap()
            .as_i64(),
        Some(2)
    );
    let metadata = source_in_group(&group)
        .unwrap()
        .children()
        .unwrap()
        .iter()
        .find(|x| x.list_kind() == Some(*b"tdbs"))
        .unwrap()
        .children()
        .unwrap();
    let descriptor = metadata
        .iter()
        .find(|x| x.id() == *b"tdb4")
        .unwrap()
        .data_payload()
        .unwrap();
    assert_eq!(&descriptor[12..16], &clock.ticks().to_be_bytes());
    assert!(!metadata.iter().any(|x| x.id() == *b"cdat"));
    let events = metadata
        .iter()
        .find(|x| x.list_kind() == Some(*b"list"))
        .unwrap()
        .children()
        .unwrap();
    let events = events
        .iter()
        .find(|x| x.id() == *b"ldat")
        .unwrap()
        .data_payload()
        .unwrap();
    assert_eq!(events.len(), 16);
    assert_eq!(&events[8..12], &clock.units(1250).unwrap().to_be_bytes());
    assert_eq!(&events[12..], &[3, 3, 0, 0]);
    let guid = |group: &Chunk| {
        group
            .children()
            .unwrap()
            .iter()
            .find(|x| x.list_kind() == Some(*b"btgu"))
            .unwrap()
            .children()
            .unwrap()[0]
            .data_payload()
            .unwrap()
            .to_vec()
    };
    assert_ne!(
        guid(&group),
        guid(&boxed_properties(&input, 8001, clock).unwrap())
    );
    assert_eq!(
        guid(&group),
        guid(&boxed_properties(&input, 8000, clock).unwrap())
    );
}
