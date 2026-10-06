//! Shape Path and Appearance payloads from native calibration controls:
//! Premiere 26.5.1 saved `C1`, `C2` and `K1` byte-identically, and AME
//! rendered them and the payload-swapped copies `C4`-`C6`. Editable recovery of
//! optional Appearance data keeps required path, paint and concealment checks.

use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};

/// The run 1 rectangle (−300, −150)–(300, 150): four corner vertices, closed.
pub(in crate::format) const RECTANGLE: &str = "AgAAAAQAAAAAAAAAAACWwwAAFsMAAJbDAAAWwwAAlsMAABbDAAAAAAAAlkMAABbDAACWQwAAFsMAAJZDAAAWwwAAAAAAAJZDAAAWQwAAlkMAABZDAACWQwAAFkMAAAAAAACWwwAAFkMAAJbDAAAWQwAAlsMAABZDAQ==";
/// V1 (`C1`): fill (0, 96, 255) and the base slots.
pub(in crate::format) const FILL: &str = "lAEAAAAAAABEMyIRDAAAAAAABgAIAAQABgAAAFAAAAAAAEoALAAEAAAAAAAAAAAACAAAAAwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAAAAAFAAYABwAAAAAACAAAAAkAAAAAAAAACgASgAAADQAAABEAAAAAABwQlgAAAABAAAAAACWQwAAFkQIAQAADAEAAAEAAAAAAAoACAAEAAUABgAKAAAAAGD/AAAACgAIAAQABQAGAAoAAAAoKCgAFAAcAAQACAAMABAAAAAAABQAGAAUAAAAAACWwwAAAAAAAJZDAAAAAAgAAABsAAAAAgAAABQAAAA8AAAAAAAKAAwABAAAAAgACgAAABQAAAAAAAA/AAAKAAgABAAFAAYACgAAAABg/wAAAAoAEAAEAAgADAAKAAAAGAAAAAAAgD8AAAA/AAAKAAgABAAFAAYACgAAAAAAAAACAAAAFAAAACQAAAAAAAoACAAAAAAABAAKAAAAAAAAPwAACgAMAAAABAAIAAoAAAAAAIA/AAAAPwQABAAEAAAABAAEAAQAAAA=";
/// V2 (`C1`): the base slots without a fill color; rendered grey 128.
const NO_FILL: &str = "fAEAAAAAAABEMyIRDAAAAAAABgAIAAQABgAAAFAAAAAAAEoAKAAAAAAAAAAAAAAABAAAAAgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADAAAAAAAEAAUABgAAAAAABwAAAAgAAAAAAAAACQASgAAADAAAAAAAHBCRAAAAAEAAAAAAJZDAAAWRPQAAAD4AAAAAQAAAAAACgAIAAQABQAGAAoAAAAoKCgAFAAcAAQACAAMABAAAAAAABQAGAAUAAAAAACWwwAAAAAAAJZDAAAAAAgAAABsAAAAAgAAABQAAAA8AAAAAAAKAAwABAAAAAgACgAAABQAAAAAAAA/AAAKAAgABAAFAAYACgAAAABg/wAAAAoAEAAEAAgADAAKAAAAGAAAAAAAgD8AAAA/AAAKAAgABAAFAAYACgAAAAAAAAACAAAAFAAAACQAAAAAAAoACAAAAAAABAAKAAAAAAAAPwAACgAMAAAABAAIAAoAAAAAAIA/AAAAPwQABAAEAAAABAAEAAQAAAA=";
/// V5 (`C1`): fill, stroke values without the stroke switch (no stroke drawn),
/// and the shadow on: 6 = 1, (40, 40, 40), 60 %, distance 0, size 15, blur 0.
const OFF_STROKE_SHADOW: &str = "wAEAAAAAAABEMyIRDAAAAAAABgAIAAQABgAAAFAAAAAAAEoARAAEAAAACAAAAAwAEABAABQAAAAYABwAIAAAAAAAAAAAAAAAAAAAAAAAJAAAAAAAKAAsADAAAAAAADQAAAA4ADwAAAAAAEEASgAAAEwAAABcAAAAAADAQWgAAAAAAHBCAAAAAAAAcEEAAAAAcAAAAAEAAAAAAJZDAAAWRCABAAAkAQAAAgAAAAEBAAAAAAoACAAEAAUABgAKAAAAAGD/AAAACgAIAAQABQAGAAoAAAAA/0AAAAAKAAgABAAFAAYACgAAACgoKAAUABwABAAIAAwAEAAAAAAAFAAYABQAAAAAAJbDAAAAAAAAlkMAAAAACAAAAGwAAAACAAAAFAAAADwAAAAAAAoADAAEAAAACAAKAAAAFAAAAAAAAD8AAAoACAAEAAUABgAKAAAAAGD/AAAACgAQAAQACAAMAAoAAAAYAAAAAACAPwAAAD8AAAoACAAEAAUABgAKAAAAAAAAAAIAAAAUAAAAJAAAAAAACgAIAAAAAAAEAAoAAAAAAAA/AAAKAAwAAAAEAAgACgAAAAAAgD8AAAA/BAAEAAQAAAAEAAQABAAAAA==";
/// W2 (`C2`): fill with slot 13 = 1, which drew no visible change.
const SLOT_13: &str = "lAEAAAAAAABEMyIRDAAAAAAABgAIAAQABgAAAFAAAAAAAEoALAAEAAAAAAAAAAAACAAAAAwAAAAAAAAAAAAAACgAAAAAAAAAAAAAAAAAEAAAAAAAFAAYABwAAAAAACAAAAAkAAAAAAAAACkASgAAADQAAABEAAAAAABwQlgAAAABAAAAAACWQwAAFkQIAQAADAEAAAEBAAAAAAoACAAEAAUABgAKAAAAAGD/AAAACgAIAAQABQAGAAoAAAAoKCgAFAAcAAQACAAMABAAAAAAABQAGAAUAAAAAACWwwAAAAAAAJZDAAAAAAgAAABsAAAAAgAAABQAAAA8AAAAAAAKAAwABAAAAAgACgAAABQAAAAAAAA/AAAKAAgABAAFAAYACgAAAABg/wAAAAoAEAAEAAgADAAKAAAAGAAAAAAAgD8AAAA/AAAKAAgABAAFAAYACgAAAAAAAAACAAAAFAAAACQAAAAAAAoACAAAAAAABAAKAAAAAAAAPwAACgAMAAAABAAIAAoAAAAAAIA/AAAAPwQABAAEAAAABAAEAAQAAAA=";
/// Z4 (`C6`, AME only): fill and the centred stroke: 3 = 1, 31 = 0,
/// (0, 255, 64), 24 px.
pub(in crate::format) const CENTRED: &str = "tAEAAAAAAABEMyIRDAAAAAAABgAIAAQABgAAAFAAAAAAAEoAOAAEAAAACAA0AAwAEAAAABQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAGAAAAAAAHAAgACQAAAAAACgAAAAsADAAAAAAADUASgAAAEAAAABQAAAAAADAQVwAAAAAAHBCcAAAAAEAAAAAAJZDAAAWRCABAAAkAQAAAAAAAAEBAAAAAAoACAAEAAUABgAKAAAAAGD/AAAACgAIAAQABQAGAAoAAAAA/0AAAAAKAAgABAAFAAYACgAAACgoKAAUABwABAAIAAwAEAAAAAAAFAAYABQAAAAAAJbDAAAAAAAAlkMAAAAACAAAAGwAAAACAAAAFAAAADwAAAAAAAoADAAEAAAACAAKAAAAFAAAAAAAAD8AAAoACAAEAAUABgAKAAAAAGD/AAAACgAQAAQACAAMAAoAAAAYAAAAAACAPwAAAD8AAAoACAAEAAUABgAKAAAAAAAAAAIAAAAUAAAAJAAAAAAACgAIAAAAAAAEAAoAAAAAAAA/AAAKAAwAAAAEAAgACgAAAAAAgD8AAAA/BAAEAAQAAAAEAAQABAAAAA==";

/// P1 (`K1`): the stroke on (3 = 1, (0, 255, 64), 32 px, centred) and slot
/// 1 = 0, which drew no fill.
const NO_FILL_SWITCH: &str = "tAEAAAAAAABEMyIRDAAAAAAABgAIAAQABgAAAFAAAAAAAEoAOAAEADQACAA1AAwAEAAAABQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAGAAAAAAAHAAgACQAAAAAACgAAAAsADAAAAAAADYASgAAAEAAAABQAAAAAAAAQlwAAAAAAHBCcAAAAAEAAAAAAJZDAAAWRCABAAAkAQAAAAAAAAABAQAAAAoACAAEAAUABgAKAAAAAGD/AAAACgAIAAQABQAGAAoAAAAA/0AAAAAKAAgABAAFAAYACgAAACgoKAAUABwABAAIAAwAEAAAAAAAFAAYABQAAAAAAJbDAAAAAAAAlkMAAAAACAAAAGwAAAACAAAAFAAAADwAAAAAAAoADAAEAAAACAAKAAAAFAAAAAAAAD8AAAoACAAEAAUABgAKAAAAAGD/AAAACgAQAAQACAAMAAoAAAAYAAAAAACAPwAAAD8AAAoACAAEAAUABgAKAAAAAAAAAAIAAAAUAAAAJAAAAAAACgAIAAAAAAAEAAoAAAAAAAA/AAAKAAwAAAAEAAgACgAAAAAAgD8AAAA/BAAEAAQAAAAEAAQABAAAAA==";
/// T5 (`K1`): the same stroke on a fill, with our own corpus-form values in
/// the layout slots 20, 24, 25, 28 and 30 and 23 = 1; AME drew it
/// RGB-identically to the base-slot T0.
const CORPUS_LAYOUT: &str = "xAIAAAAAAABEMyIRDAAAAAAABgAIAAQABgAAAFAAAAAAAEoAOAAEAAAACAA0AAwAEAAAABQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAGAAAAAAAHAAgACQAAAAAACgAAAAsADAAAAAAADUASgAAAEAAAABQAAAAAAAAQlwAAAAAAHBCcAAAAAEAAAAAAHpDAAD6QxgBAACkAQAAAAAAAAEBAAAAAAoACAAEAAUABgAKAAAAAGD/AAAACgAIAAQABQAGAAoAAAAA/0AAAAAKAAgABAAFAAYACgAAACgoKAAUABwABAAIAAwAEAAAAAAAFAAYABQAAAAAAHDDAACgQQAAcEMAAKBBCAAAAFQAAAACAAAAFAAAADwAAAAAAAoADAAEAAAACAAKAAAAFAAAAAAAAD8AAAoACAAEAAUABgAKAAAA/6AAAAAACgAMAAAABAAIAAoAAAAAAIA/AAAAPwIAAAAUAAAAJAAAAAAACgAIAAAAAAAEAAoAAAAAAAA/AAAKAAwAAAAEAAgACgAAAAAAgD8AAAA/FAAMAAAAAAAAAAAAAAAAAAQACAAUAAAACAAAADwAAAACAAAAFAAAACQAAAAAAAoACAAAAAAABAAKAAAAAAAAPwAACgAMAAAABAAIAAoAAAAAAIA/AAAAPwIAAAAUAAAAJAAAAAAACgAIAAAAAAAEAAoAAAAAAAA/AAAKAAwAAAAEAAgACgAAAAAAgD8AAAA/FAAMAAAAAAAAAAAAAAAAAAQACAAUAAAACAAAAFQAAAACAAAAFAAAADwAAAAAAAoADAAEAAAACAAKAAAAFAAAAAAAAD8AAAoACAAEAAUABgAKAAAAWlpaAAAACgAMAAAABAAIAAoAAAAAAIA/AAAAPwIAAAAUAAAAJAAAAAAACgAIAAAAAAAEAAoAAAAAAAA/AAAKAAwAAAAEAAgACgAAAAAAgD8AAAA/";

/// The Appearance payloads that Premiere 26.5.1 saved for the gradient
/// fixture `premiere_isolated_gradient_fills_26_5`, which
/// AME build 85 rendered: A a solid control whose payload only slot 0 holds,
/// B linear, C radial, and D linear with an opacity ramp and a stroke.
const GRADIENT_A: &str =
    "OAAAAAAAAABEMyIRDAAAAAAABgAIAAQABgAAAAwAAAAAAAYACAAEAAYAAAAQAAAAAAAKAAgABAAFAAYACgAAAABg/gA=";
pub(in crate::format) const GRADIENT_B: &str = "ZAEAAAAAAABEMyIRDAAAAAAABgAKAAQABgAAAFAAAAAAAEoAHAAYAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABQAEAAAAAAAAAAAAAAAAAAAAAwAAAAIAAAAAAAAAAcASgAAAAAAAAEUAAAAFAAAACgAAAABAAAA3AAAACD///8k////FAAUABAAAAAMAAAAAAAAAAgABAAUAAAAEAAAAEQAAAAAABZDAAAWwwIAAAAsAAAAEAAAAAAACgAOAAAABAAIAAoAAAAAAIA/AAAAPwAACgAIAAAAAAAEAAoAAAAAAAA/AgAAAEQAAAAQAAAAAAAKABIABAAIAAwACgAAABgAAAAAAIA/AAAAPwAACgAKAAcACAAJAAoAAAAAAAAAyAAKAAwABAAAAAgACgAAAAgAAAAAAAA/7v///wAAYP4EAAYABAAAAAAACgAIAAUABgAHAAoAAAAAAGD+BAAEAAQAAAA=";
const GRADIENT_C: &str = "iAEAAAAAAABEMyIRDAAAAAAABgAKAAQABgAAAFAAAAAAAEoAHAAYAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABQAEAAAAAAAAAAAAAAAAAAAAAwAAAAIAAAAAAAAAAcASgAAAAAAAAEUAAAAFAAAACgAAAACAAAAAAEAAPz+//8A////FAAUABAAAAAMAAAAAAAAAAgABAAUAAAAEAAAAEQAAAAAABZDAAAWwwIAAAAsAAAAEAAAAAAACgAOAAAABAAIAAoAAAAAAIA/AAAAPwAACgAIAAAAAAAEAAoAAAAAAAA/AwAAAGwAAAA4AAAAEAAAAAAACgAQAAQACAAMAAoAAAAMAAAAAACAPwAAAD/a////AAAAAAAACgASAAQACAAMAAoAAAAYAAAA45n/PgAAAD8AAAoACgAHAAgACQAKAAAAAAAAAGD+CgAMAAQAAAAIAAoAAAAIAAAAAAAAP+D///8EAAYABAAAAAAACgAIAAUABgAHAAoAAAAAAGD+BAAEAAQAAAA=";
const GRADIENT_D: &str = "dAEAAAAAAABEMyIRDAAAAAAABgAKAAQABgAAAFAAAAAAAEoAKAAkAAAAIAAfABgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABQAEAAAAAAAAAAAAAAAAAAAAAwAAAAIAAAAAAAAAAcASgAAAAAAAAEgAAAAIAAAADQAAAABAAAAAABAQQAAAAHQAAAA4AAAABz///8g////FAAUABAAAAAMAAAAAAAAAAgABAAUAAAAEAAAAEgAAAAAABZDAAAWwwIAAAAwAAAAEAAAAAAACgASAAQACAAMAAoAAAAAAAAAAACAPwAAAD8AAAoACAAAAAAABAAKAAAAAAAAPwIAAAA4AAAAEAAAAAAACgAQAAQACAAMAAoAAAAMAAAAAACAPwAAAD/W////AAAAAMgACgAMAAQAAAAIAAoAAAAIAAAAAAAAP/b///8AAAAAYP4KAAoABwAIAAkACgAAAAAAAAAAAAoACAAFAAYABwAKAAAAAABg/gQABAAEAAAA";

fn bytes(base64: &str) -> Vec<u8> {
    STANDARD.decode(base64).unwrap()
}

const BLUE: PrRgb = PrRgb([0, 96, 255]);
const GREEN: PrRgb = PrRgb([0, 255, 64]);

#[cfg(feature = "ffmpeg-library")]
#[test]
fn native_appearance_optional_details_keep_published_shape_geometry() {
    use crate::tests::support::{legacy_appearance, legacy_json};
    use std::{fs, io::Read, path::Path};
    use tesseract_file::TesseractFile;

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut xml = String::new();
    flate2::read::GzDecoder::new(
        fs::File::open(fixtures.join("feature_graphic_shapes_26_5_strict.prproj")).unwrap(),
    )
    .read_to_string(&mut xml)
    .unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    // The first native Rectangle's Appearance; its unchanged Path is param 176.
    let value = document
        .descendants()
        .find(|node| node.attribute("ObjectID") == Some("177"))
        .unwrap()
        .children()
        .find(|node| node.has_tag_name("StartKeyframeValue"))
        .unwrap();
    let saved = STANDARD
        .decode(value.text().unwrap().split_whitespace().collect::<String>())
        .unwrap();
    let mut missing_layout = saved.clone();
    let word = |at: usize| u32::from_le_bytes(saved[at..at + 4].try_into().unwrap()) as usize;
    let root = 12 + word(12);
    let root_vtable = root - word(root);
    let style_slot = root
        + usize::from(u16::from_le_bytes(
            saved[root_vtable + 4..root_vtable + 6].try_into().unwrap(),
        ));
    let style = style_slot + word(style_slot);
    let vtable = style - word(style);
    missing_layout[vtable + 4 + 34 * 2..vtable + 6 + 34 * 2].fill(0);
    let legacy = legacy_appearance(&legacy_json(&[
        ("mIsMask", Some("false")),
        ("mIsMaskInverted", Some("false")),
        ("mFillColorType", Some("0")),
        ("mAdditionalStrokes", Some("[]")),
        (
            "mGradientInfo",
            Some(r#"{"mColorStops":[],"mOpacityStops":[]}"#),
        ),
        ("mShadowSize", Some("0")),
        ("mFutureLayout", Some(r#"{"size":12}"#)),
    ]));
    for (name, payload, expected_color, diagnosed) in [
        ("legacy", legacy, [128.0 / 255.0; 3], true),
        (
            "missing-layout",
            missing_layout,
            [0.0, 96.0 / 255.0, 1.0],
            false,
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        fs::copy(
            fixtures.join("video-30fps-10s.mp4"),
            dir.path().join("video-30fps-10s.mp4"),
        )
        .unwrap();
        let replacement = value
            .text()
            .map(|old| xml[value.range()].replace(old, &STANDARD.encode(payload)))
            .unwrap();
        let mut changed = xml.clone();
        changed.replace_range(value.range(), &replacement);
        let input = dir.path().join("project.prproj");
        fs::write(&input, changed).unwrap();
        let output = dir.path().join("converted");
        let omissions = crate::premiere_to_tesseract(
            &input,
            &output,
            Some("c8acf9c1-34b2-4086-9f55-d528950a7059"),
            false,
        )
        .unwrap();
        let archive = fs::read_dir(output)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let file = TesseractFile::open(archive).unwrap();
        let project = file.project_json().unwrap();
        let layers = project["composition"]["layers"].as_array().unwrap();
        assert_eq!(layers.len(), 11, "{name}: {project}");
        let rectangle = &layers[0]["layers"][1];
        assert_eq!(rectangle["type"], "Shape", "{name}: {project}");
        assert_eq!(rectangle["name"], "Rectangle");
        assert_eq!(
            rectangle["shape"]["path"]["commands"][1],
            serde_json::json!({"type":"lineTo", "x":100.0, "y":-90.0})
        );
        assert_eq!(
            rectangle["shape"]["fills"][0]["paint"]["color"],
            serde_json::json!([expected_color[0], expected_color[1], expected_color[2], 1.0])
        );
        assert_eq!(layers[0]["layers"][0]["type"], "Text");
        assert_eq!(
            layers[2]["layers"][1]["shape"]["path"]["commands"][1]["type"],
            "cubicTo"
        );
        assert_eq!(
            omissions
                .iter()
                .any(|omission| omission.reason.contains("optional Appearance")),
            diagnosed,
            "{name}: {omissions:?}"
        );
    }
}

#[test]
fn native_path_past_the_former_payload_quota_round_trips() {
    let mut path = decode_path(&bytes(RECTANGLE)).unwrap();
    path.vertices = path.vertices.repeat((1 << 20) / (4 * VERTEX_BYTES) + 1);
    let payload = encode_path(&path).unwrap();
    assert!(payload.len() > 1 << 20);
    assert_eq!(decode_path(&payload).unwrap(), path);
}

#[test]
fn run_one_paths_decode_and_encode_byte_for_byte() {
    let payload = bytes(RECTANGLE);
    let path = decode_path(&payload).unwrap();
    let corner = |x: f32, y: f32| PrPathVertex {
        smooth: false,
        point: [x, y],
        in_tangent: [x, y],
        out_tangent: [x, y],
    };
    assert_eq!(
        path,
        PrShapePath {
            vertices: vec![
                corner(-300.0, -150.0),
                corner(300.0, -150.0),
                corner(300.0, 150.0),
                corner(-300.0, 150.0),
            ],
            closed: true,
        }
    );
    assert_eq!(encode_path(&path).unwrap(), payload);
}

#[test]
fn calibration_appearances_decode_to_what_ame_drew() {
    let shadow = PrTextShadow {
        color: PrRgb([40; 3]),
        opacity: 60.0,
        angle: SHAPE_SHADOW_ANGLE,
        distance: 0.0,
        size: 15.0,
        blur: 0.0,
    };
    let green = |width| {
        Some(PrShapeStroke {
            color: GREEN,
            width,
        })
    };
    for (payload, (fill, stroke, shadow)) in [
        (FILL, (Some(BLUE), None, None)),
        (NO_FILL, (Some(DEFAULT_SHAPE_FILL), None, None)),
        (OFF_STROKE_SHADOW, (Some(BLUE), None, Some(shadow))),
        (SLOT_13, (Some(BLUE), None, None)),
        (CENTRED, (Some(BLUE), green(24.0), None)),
        (NO_FILL_SWITCH, (None, green(32.0), None)),
        (CORPUS_LAYOUT, (Some(BLUE), green(32.0), None)),
    ] {
        assert_eq!(
            decode_appearance(&bytes(payload)).unwrap(),
            PrAppearance {
                mask_source: None,
                fill: fill.map(PrFill::Solid),
                stroke,
                shadow
            }
        );
    }
}

#[test]
fn the_writer_stores_the_calibrated_slots_and_reads_back() {
    // The base payload that V1 rendered: the same slots, and every value
    // that the reader requires.
    let fill = PrAppearance {
        mask_source: None,
        fill: Some(PrFill::Solid(BLUE)),
        stroke: None,
        shadow: None,
    };
    let written = encode_appearance(&fill).unwrap();
    let table = |payload: &[u8]| {
        let buffer = Buffer::from_payload(payload, "Appearance").unwrap();
        let root = buffer.table(buffer.offset(0).unwrap()).unwrap();
        root.table(0).unwrap().unwrap().present().unwrap()
    };
    assert_eq!(table(&written), table(&bytes(FILL)));
    assert_eq!(decode_appearance(&written).unwrap(), fill);
    // Without a fill, slot 1 = 0 switches the base color off, as P1 drew it.
    let styled = PrAppearance {
        mask_source: None,
        fill: None,
        stroke: Some(PrShapeStroke {
            color: GREEN,
            width: 8.0,
        }),
        shadow: Some(PrTextShadow {
            color: PrRgb([10, 20, 30]),
            opacity: 100.0,
            angle: SHAPE_SHADOW_ANGLE,
            distance: 50.0,
            size: 20.0,
            blur: 0.0,
        }),
    };
    let written = encode_appearance(&styled).unwrap();
    assert!(table(&written).contains(&1));
    assert_eq!(decode_appearance(&written).unwrap(), styled);
}

#[test]
fn optional_appearance_slots_do_not_discard_known_paint() {
    let base = appearance_fields(&PrAppearance {
        mask_source: None,
        fill: Some(PrFill::Solid(BLUE)),
        stroke: None,
        shadow: None,
    })
    .unwrap();
    let with = |changes: &[(usize, Option<Field>)]| {
        let mut fields: Vec<_> = base
            .iter()
            .copied()
            .filter(|(slot, _)| changes.iter().all(|(changed, _)| changed != slot))
            .collect();
        fields.extend(
            changes
                .iter()
                .filter_map(|&(slot, field)| field.map(|field| (slot, field))),
        );
        encode_fields(&fields).unwrap()
    };
    let stroke = |position: u32| {
        [
            (2, Some(Field::Color([0, 255, 64]))),
            (3, Some(Field::U8(1))),
            (4, Some(Field::F32(24.0))),
            (31, Some(Field::U32(position))),
        ]
    };
    // Calibration values and absent/unknown optional metadata retain the fill.
    for changes in [
        vec![(13, Some(Field::U8(2)))],
        vec![(34, Some(Field::U8(2)))],
        vec![(23, Some(Field::U32(0)))],
        vec![(23, Some(Field::U32(2)))],
        vec![(24, Some(Field::F32(390.0))), (25, Some(Field::F32(500.0)))],
        vec![(8, Some(Field::U8(0)))],
        vec![(26, Some(Field::U32(16)))],
        vec![(15, Some(Field::U8(1)))],
        vec![(13, Some(Field::U8(3)))],
        vec![(23, Some(Field::U32(3)))],
        vec![(23, None)],
        vec![(34, None)],
        vec![(34, Some(Field::U8(0)))],
        vec![(23, Some(Field::U32(2))), (24, Some(Field::F32(390.0)))],
        vec![(24, None)],
        vec![(28, Some(Field::Table(&[(1, Field::F32(1.0))])))],
    ] {
        let (decoded, _) = decode_appearance_with_notes(&with(&changes)).unwrap();
        assert_eq!(decoded.fill, Some(PrFill::Solid(BLUE)), "{changes:?}");
        assert_eq!(decoded.mask_source, None);
    }
    let (_, notes) = decode_appearance_with_notes(&with(&[(26, Some(Field::U32(16)))])).unwrap();
    assert!(notes
        .iter()
        .any(|note| note.contains("optional Appearance slot 26")));
    let unrendered = "holds a value that no render covers";
    for (changes, reason) in [
        (vec![(1, Some(Field::U8(1)))], unrendered),
        (stroke(1).to_vec(), "shape strokes inside the outline"),
        (stroke(2).to_vec(), "shape strokes outside the outline"),
        (
            stroke(3).to_vec(),
            "shape strokes inside and outside the outline",
        ),
        (
            vec![(3, Some(Field::U8(1)))],
            "an enabled shape stroke lacks its color or width",
        ),
        (
            vec![(3, Some(Field::U8(0)))],
            "invalid Appearance slot 3 value 0",
        ),
        (
            vec![(6, Some(Field::U8(1)))],
            "an enabled shape shadow lacks its color, opacity, distance, size or blur",
        ),
        (
            vec![(0, Some(Field::Table(&[(0, Field::U8(0))])))],
            "an Appearance color must store red, green and blue",
        ),
    ] {
        let error = decode_appearance(&with(&changes)).unwrap_err().to_string();
        assert!(error.contains(reason), "{changes:?}: {error}");
    }
    // A legacy JSON Appearance reads only with its style and version.
    let mut legacy = 4_u64.to_le_bytes().to_vec();
    legacy.extend("{}".encode_utf16().flat_map(u16::to_le_bytes));
    let error = decode_appearance(&legacy).unwrap_err().to_string();
    assert!(
        error.contains("legacy JSON Appearance: missing field `mStyle`"),
        "{error}"
    );
}

#[test]
fn legacy_json_appearance_rejects_a_positional_style() {
    use crate::tests::support::legacy_appearance;
    let json = r#"{"mVersion":1,"mStyle":[8421504,true,0,false,1,0,0,0,0,0,false]}"#;
    let error = decode_appearance(&legacy_appearance(json))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("legacy JSON Appearance: invalid type: sequence"),
        "{error}"
    );
}

#[test]
fn legacy_json_appearance_v1_reads_a_gray_fill_and_the_fill_switch() {
    use crate::tests::support::{legacy_appearance, legacy_json};
    let solid = |gray: u8| PrAppearance {
        mask_source: None,
        fill: Some(PrFill::Solid(PrRgb([gray; 3]))),
        stroke: None,
        shadow: None,
    };
    // A disabled stroke or shadow draws nothing, whatever it stores.
    let other_inactive_values = [
        ("mFillColor", Some("0")),
        ("mStrokeColor", Some("0")),
        ("mStrokeWidth", Some("0")),
        ("mShadowAngle", Some("-90.25")),
        ("mShadowBlur", Some("1000")),
        ("mShadowColor", Some("16777215")),
        ("mShadowOffset", Some("0")),
        ("mShadowOpacity", Some("100")),
    ];
    let cases = [
        (legacy_json(&[]), solid(128)),
        (legacy_json(&[("mFillColor", Some("16777215"))]), solid(255)),
        (legacy_json(&other_inactive_values), solid(0)),
        // A fill switched off draws nothing, in a color that would not convert.
        (
            legacy_json(&[
                ("mFillVisible", Some("false")),
                ("mFillColor", Some("1193046")),
            ]),
            PrAppearance {
                mask_source: None,
                fill: None,
                stroke: None,
                shadow: None,
            },
        ),
        // JSON spacing and field order carry no meaning.
        (
            r#"{ "mStyle": { "mShadowVisible": false, "mShadowOpacity": 0, "mShadowOffset": 0,
                "mShadowColor": 0, "mShadowBlur": 0, "mShadowAngle": 0, "mStrokeWidth": 1,
                "mStrokeVisible": false, "mStrokeColor": 0, "mFillVisible": true,
                "mFillColor": 16777215 }, "mVersion": 1 }"#
                .to_owned(),
            solid(255),
        ),
    ];
    let decoded: Vec<_> = cases
        .iter()
        .map(|(json, _)| {
            decode_appearance(&legacy_appearance(json)).map_err(|error| error.to_string())
        })
        .collect();
    let expected: Vec<_> = cases
        .iter()
        .map(|(_, appearance)| Ok(appearance.clone()))
        .collect();
    assert_eq!(decoded, expected);
}

#[test]
fn legacy_json_appearance_keeps_required_framing_paint_and_mask_guards() {
    use crate::tests::support::{legacy_appearance, legacy_json};
    let payload = |changes: &[(&str, Option<&str>)]| legacy_appearance(&legacy_json(changes));
    let gray = payload(&[]);
    let text = gray.len() - 8;
    let patched = |at: usize, bytes: &[u8]| {
        let mut patched = gray.clone();
        patched[at..at + bytes.len()].copy_from_slice(bytes);
        patched
    };
    let count = |bytes: usize| u32::try_from(bytes).unwrap().to_le_bytes();
    let mut odd = gray.clone();
    odd.push(b' ');
    odd[..4].copy_from_slice(&count(text + 1));
    let cases: Vec<(Vec<u8>, String)> = vec![
        (
            patched(0, &count(text + 2)),
            format!(
                "its byte count {} does not match the {text} bytes after it",
                text + 2
            ),
        ),
        (
            patched(4, &1_u32.to_le_bytes()),
            "the word after its byte count is 1, not 0".into(),
        ),
        (odd, format!("an odd byte count {} is not UTF-16", text + 1)),
        // A lone high surrogate in place of the closing brace.
        (
            patched(gray.len() - 2, &0xd800_u16.to_le_bytes()),
            "invalid UTF-16: unpaired surrogate found: d800".into(),
        ),
        (
            legacy_appearance(&format!("{}{{}}", legacy_json(&[]))),
            "trailing characters".into(),
        ),
        (
            payload(&[("mMaskSource", Some("1"))]),
            "unsupported mask fields".into(),
        ),
        (
            payload(&[("mIsMask", Some("true"))]),
            "unsupported mask fields".into(),
        ),
        (
            payload(&[("mIsMaskInverted", Some("true"))]),
            "unsupported mask fields".into(),
        ),
        (
            payload(&[("mIsMask", Some("null"))]),
            "invalid type: null, expected a boolean".into(),
        ),
        (
            legacy_appearance(&legacy_json(&[]).replacen('{', r#"{"mIsMask":true,"#, 1)),
            "unsupported mask fields".into(),
        ),
        (
            legacy_appearance(&legacy_json(&[]).replacen('{', r#"{"mIsMaskInverted":true,"#, 1)),
            "unsupported mask fields".into(),
        ),
        (
            payload(&[("mFillColorType", Some("1"))]),
            "active fill type 1 is unsupported".into(),
        ),
        (
            payload(&[("mAdditionalStrokes", Some("[{}]"))]),
            "additional strokes are unsupported".into(),
        ),
        (
            legacy_appearance(
                &legacy_json(&[]).replace(r#""mVersion":1"#, r#""mVersion":1,"mVersion":1"#),
            ),
            "duplicate field `mVersion`".into(),
        ),
        (
            legacy_appearance(&legacy_json(&[]).replace(
                r#""mFillVisible":true"#,
                r#""mFillVisible":true,"mFillVisible":true"#,
            )),
            "duplicate field `mFillVisible`".into(),
        ),
        // A switch is never read as on or off by default.
        (
            payload(&[("mFillVisible", None)]),
            "missing field `mFillVisible`".into(),
        ),
        (
            payload(&[("mShadowVisible", Some("null"))]),
            "invalid type: null, expected a boolean".into(),
        ),
        (
            payload(&[("mFillColor", Some("33554431"))]),
            "mFillColor 0x1ffffff is outside the 24-bit color form".into(),
        ),
        // Only gray reads alike in either channel order.
        (
            payload(&[("mFillColor", Some("24831"))]),
            "mFillColor 0x0060ff is not gray".into(),
        ),
        (
            payload(&[("mStrokeVisible", Some("true"))]),
            "an enabled stroke is unsupported".into(),
        ),
        (
            payload(&[("mShadowVisible", Some("true"))]),
            "an enabled shadow is unsupported".into(),
        ),
    ];
    let unmet: Vec<String> = cases
        .iter()
        .filter_map(|(payload, reason)| {
            let outcome = decode_appearance(payload).map_err(|error| error.to_string());
            match &outcome {
                Err(error)
                    if error.starts_with("unsupported conversion: legacy JSON Appearance: ")
                        && error.contains(reason.as_str()) =>
                {
                    None
                }
                _ => Some(format!("{reason}: {outcome:?}")),
            }
        })
        .collect();
    assert!(unmet.is_empty(), "{unmet:#?}");
}

#[test]
fn legacy_optional_appearance_fields_keep_gray_paint_and_report_saved_details() {
    use crate::tests::support::{legacy_appearance, legacy_json};
    let json = legacy_json(&[
        ("mIsMask", Some("false")),
        ("mIsMaskInverted", Some("false")),
        ("mFillColorType", Some("0")),
        ("mAdditionalStrokes", Some("[]")),
        (
            "mGradientInfo",
            Some(r#"{"mColorStops":[],"mOpacityStops":[]}"#),
        ),
        ("mLineJoinType", Some("0")),
        ("mShadowSize", Some("0")),
        ("mFutureLayout", Some("17")),
        ("mStrokeColor", Some("4294967295")),
        ("mStrokeWidth", None),
    ])
    .replace(r#""mVersion":1"#, r#""mVersion":2,"mName":"Box""#);
    let (appearance, notes) = decode_appearance_with_notes(&legacy_appearance(&json)).unwrap();
    assert_eq!(appearance.fill, Some(PrFill::Solid(PrRgb([128; 3]))));
    assert_eq!(appearance.mask_source, None);
    assert_eq!(appearance.stroke, None);
    assert_eq!(appearance.shadow, None);
    for field in [
        "mName",
        "mGradientInfo",
        "mLineJoinType",
        "mShadowSize",
        "mFutureLayout",
        "version 2",
    ] {
        assert!(
            notes.iter().any(|note| note.contains(field)),
            "{field}: {notes:?}"
        );
    }
}

#[test]
fn malformed_paths_fail_closed() {
    let payload = bytes(RECTANGLE);
    let patched = |at: usize, value: &[u8]| {
        let mut patched = payload.clone();
        patched[at..at + value.len()].copy_from_slice(value);
        patched
    };
    for (payload, reason) in [
        (
            patched(0, &3_u32.to_le_bytes()),
            "Path version 3 is unsupported",
        ),
        (
            payload[..payload.len() - 1].to_vec(),
            "Path size does not match its 4 vertices",
        ),
        (
            patched(8, &2_u32.to_le_bytes()),
            "unknown Path vertex flag 2",
        ),
        (
            patched(12, &f32::NAN.to_le_bytes()),
            "Path vertices must be finite",
        ),
        (
            patched(payload.len() - 1, &[2]),
            "unknown Path closed byte 2",
        ),
    ] {
        let error = decode_path(&payload).unwrap_err().to_string();
        assert!(error.contains(reason), "{error}");
    }
    // A closed byte of 0 is an open path, which Premiere draws open.
    assert!(
        !decode_path(&patched(payload.len() - 1, &[0]))
            .unwrap()
            .closed
    );
}

#[test]
fn premiere_gradient_appearances_decode_to_what_ame_drew() {
    use crate::schema::text::OPAQUE_OPACITY_STOPS;
    let stop = |position, color| PrGradientStop {
        position,
        color: PrRgb(color),
    };
    let blue = [0, 96, 254];
    let gradient = |kind, stops, opacity_stops| {
        Some(PrFill::Gradient(PrGradient {
            kind,
            start_x: -150.0,
            end_x: 150.0,
            stops,
            opacity_stops,
        }))
    };
    let only = |fill| PrAppearance {
        mask_source: None,
        fill,
        stroke: None,
        shadow: None,
    };
    let ramp = vec![stop(0.0, blue), stop(1.0, [0, 200, 0])];
    // A drew solid blue (f38 interior MAE 0.003), B a clamped blue-to-green
    // ramp over x 810-1110 after the shape's translation to 960, C a circle
    // about x 810 of radius 300 through white, blue at 49.922 % and black,
    // and D B's ramp fading out under a centred black 12 px stroke, which
    // Premiere saved without slot 31 (G5).
    for (payload, expected) in [
        (GRADIENT_A, only(Some(PrFill::Solid(PrRgb(blue))))),
        (
            GRADIENT_B,
            only(gradient(
                PrGradientKind::Linear,
                ramp.clone(),
                OPAQUE_OPACITY_STOPS.to_vec(),
            )),
        ),
        (
            GRADIENT_C,
            only(gradient(
                PrGradientKind::Radial,
                vec![
                    stop(0.0, [255; 3]),
                    stop(f32::from_bits(0x3eff_99e3), blue),
                    stop(1.0, [0; 3]),
                ],
                OPAQUE_OPACITY_STOPS.to_vec(),
            )),
        ),
        (
            GRADIENT_D,
            PrAppearance {
                mask_source: None,
                fill: gradient(
                    PrGradientKind::Linear,
                    ramp,
                    vec![
                        OPAQUE_OPACITY_STOPS[0],
                        PrGradientOpacityStop {
                            position: 1.0,
                            opacity: 0.0,
                        },
                    ],
                ),
                stroke: Some(PrShapeStroke {
                    color: PrRgb([0; 3]),
                    width: 12.0,
                }),
                shadow: None,
            },
        ),
    ] {
        let decoded = decode_appearance(&bytes(payload)).unwrap();
        assert_eq!(decoded, expected);
        let written = encode_appearance(&decoded).unwrap();
        assert_eq!(decode_appearance(&written).unwrap(), decoded);
    }
    // The writer stores D's opacity stops as Premiere saved them: the full
    // one without a value or position, the transparent end as 0.
    let fading: [&[(usize, Field)]; 2] = [
        &[(2, Field::F32(0.5))],
        &[
            (0, Field::F32(0.0)),
            (1, Field::F32(1.0)),
            (2, Field::F32(0.5)),
        ],
    ];
    let saved = bytes(GRADIENT_D);
    let written = encode_appearance(&decode_appearance(&saved).unwrap()).unwrap();
    for payload in [saved, written] {
        let buffer = Buffer::from_payload(&payload, APPEARANCE).unwrap();
        let root = buffer.table(buffer.offset(0).unwrap()).unwrap();
        let gradient = root.table(0).unwrap().unwrap().table(20).unwrap().unwrap();
        assert!(matches(gradient, 7, Field::Tables(&fading)).unwrap());
    }
}

/// A gradient table as Premiere 26.5.1 saved B's, x from -150 to 150, with
/// `stops`, `opacity` stops and the `extra` fields.
fn gradient_of<'a>(
    stops: &'a [&'a [(usize, Field<'a>)]],
    opacity: &'a [&'a [(usize, Field<'a>)]],
    extra: &'a [(usize, Field<'a>)],
) -> Vec<(usize, Field<'a>)> {
    let mut fields = vec![
        (0, Field::F32(-150.0)),
        (2, Field::F32(150.0)),
        (6, Field::Tables(stops)),
        (7, Field::Tables(opacity)),
    ];
    fields.extend_from_slice(extra);
    fields
}

/// An Appearance in the form Premiere 26.5.1 saved B, a linear `gradient`,
/// with `changes` (`None` removes a slot).
fn gradient_form<'a>(
    gradient: &'a [(usize, Field<'a>)],
    changes: &[(usize, Option<Field<'a>>)],
) -> Vec<u8> {
    let mut fields = vec![
        (0, Field::Color([0, 96, 254])),
        (19, Field::U32(1)),
        (20, Field::Table(gradient)),
    ];
    fields.extend(GRADIENT_LAYOUT);
    fields.retain(|(slot, _)| changes.iter().all(|(changed, _)| changed != slot));
    fields.extend(
        changes
            .iter()
            .filter_map(|&(slot, field)| field.map(|field| (slot, field))),
    );
    encode_fields(&fields).unwrap()
}

#[test]
fn gradient_appearance_keeps_active_paint_guards_but_not_optional_layout_guards() {
    use crate::schema::text::OPAQUE_OPACITY_STOPS;
    let blue = Field::Color([0, 96, 254]);
    let first = [(0, blue), (2, Field::F32(0.5))];
    let last = [(0, blue), (1, Field::F32(1.0)), (2, Field::F32(0.5))];
    let early = [(0, blue), (2, Field::F32(0.4))];
    let partial = [
        (0, Field::Table(&[(0, Field::U8(0))])),
        (2, Field::F32(0.5)),
    ];
    let moved = [(1, Field::F32(0.5)), (2, Field::F32(0.5))];
    let late = [(1, Field::F32(1.0)), (2, Field::F32(0.4))];
    let stops: [&[(usize, Field)]; 2] = [&first, &last];
    let early_stops: [&[(usize, Field)]; 2] = [&early, &last];
    let partial_stops: [&[(usize, Field)]; 2] = [&partial, &last];
    let moved_stops = [GRADIENT_ALPHA_STOPS[0], &moved];
    let late_stops = [GRADIENT_ALPHA_STOPS[0], &late];
    let saved = gradient_of(&stops, &GRADIENT_ALPHA_STOPS, &[]);
    for changes in [
        vec![(23, Some(Field::U32(1)))],
        vec![(34, Some(Field::U8(2)))],
        vec![(28, None)],
    ] {
        let (decoded, _) = decode_appearance_with_notes(&gradient_form(&saved, &changes)).unwrap();
        assert!(matches!(decoded.fill, Some(PrFill::Gradient(_))));
    }
    let decoded = decode_appearance(&encode_fields(&[(0, blue), (2, blue)]).unwrap()).unwrap();
    assert_eq!(decoded.fill, Some(PrFill::Solid(PrRgb([0, 96, 254]))));
    // Opacity stops read at any position; an absent value is full.
    let moved = gradient_of(&stops, &moved_stops, &[]);
    let decoded = decode_appearance(&gradient_form(&moved, &[])).unwrap();
    let Some(PrFill::Gradient(gradient)) = decoded.fill else {
        panic!("{decoded:?}");
    };
    assert_eq!(
        gradient.opacity_stops,
        [
            OPAQUE_OPACITY_STOPS[0],
            PrGradientOpacityStop {
                position: 0.5,
                opacity: 1.0
            }
        ]
    );
    let [midpoint, color, opacity_midpoint, y, highlight] = [
        gradient_of(&early_stops, &GRADIENT_ALPHA_STOPS, &[]),
        gradient_of(&partial_stops, &GRADIENT_ALPHA_STOPS, &[]),
        gradient_of(&stops, &late_stops, &[]),
        gradient_of(&stops, &GRADIENT_ALPHA_STOPS, &[(1, Field::F32(10.0))]),
        gradient_of(&stops, &GRADIENT_ALPHA_STOPS, &[(4, Field::F32(0.0))]),
    ];
    let unrendered =
        |slot: usize| format!("Appearance slot {slot} holds a value that no render covers");
    for (payload, reason) in [
        (
            gradient_form(&midpoint, &[]),
            "gradient midpoint 0.4 is not converted".to_owned(),
        ),
        (
            gradient_form(&color, &[]),
            "an Appearance color must store red, green and blue".to_owned(),
        ),
        (
            gradient_form(&opacity_midpoint, &[]),
            "gradient midpoint 0.4 is not converted".to_owned(),
        ),
        (gradient_form(&y, &[]), GRADIENT_Y_UNCONVERTED.to_owned()),
        (
            gradient_form(&highlight, &[]),
            "unsupported Appearance field 20[4]".to_owned(),
        ),
        (
            gradient_form(&saved, &[(19, Some(Field::U32(3)))]),
            format!("{} (3)", unrendered(19)),
        ),
        (
            gradient_form(&saved, &[(1, Some(Field::U8(0)))]),
            format!("{} (0)", unrendered(1)),
        ),
        (gradient_form(&saved, &[(20, None)]), unrendered(20)),
    ] {
        let error = decode_appearance(&payload).unwrap_err().to_string();
        assert!(error.contains(&reason), "{reason}: {error}");
    }
}

#[test]
fn appearance_mask_flags_round_trip_and_invert_alone_draws_the_shape() {
    use crate::schema::text::PrMaskSource;
    let solid = |mask_source| PrAppearance {
        fill: Some(PrFill::Solid(BLUE)),
        stroke: Some(PrShapeStroke {
            color: GREEN,
            width: 12.0,
        }),
        shadow: None,
        mask_source,
    };
    for mask_source in [
        None,
        Some(PrMaskSource { inverted: false }),
        Some(PrMaskSource { inverted: true }),
    ] {
        let appearance = solid(mask_source);
        let payload = encode_appearance(&appearance).unwrap();
        assert_eq!(
            decode_appearance(&payload).unwrap(),
            appearance,
            "{mask_source:?}"
        );
    }
    // Slot 13 without slot 12 drew like the base (calibration, s3).
    let plain = solid(None);
    for value in [1, 2] {
        let mut fields = appearance_fields(&plain).unwrap();
        fields.push((13, Field::U8(value)));
        let decoded = decode_appearance(&encode_fields(&fields).unwrap()).unwrap();
        assert_eq!(decoded, plain, "{value}");
    }
}
