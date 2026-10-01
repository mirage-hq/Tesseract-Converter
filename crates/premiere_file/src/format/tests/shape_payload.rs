//! Shape Path and Appearance payloads from calibration runs 1 and 2 (our own
//! values in the survey layout; `oracle/7/work/saved*/*.edits.json`):
//! Premiere 26.5.1 saved `C1`, `C2` and `K1` byte-identically, and AME
//! rendered them and the payload-swapped copies `C4`-`C6`. Round trips and
//! fail-closed slots.

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
/// fixture `premiere_isolated_gradient_fills_26_5` (Oracle run 23), which
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
fn appearance_slots_decode_only_at_values_that_rendered_like_the_base() {
    let base = appearance_fields(&PrAppearance {
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
    // Values that calibration-2 rendered like the base decode.
    for changes in [
        vec![(13, Some(Field::U8(2)))],
        vec![(34, Some(Field::U8(2)))],
        vec![(23, Some(Field::U32(0)))],
        vec![(23, Some(Field::U32(2)))],
        vec![(24, Some(Field::F32(390.0))), (25, Some(Field::F32(500.0)))],
    ] {
        let decoded = decode_appearance(&with(&changes));
        assert!(decoded.is_ok(), "{changes:?}: {decoded:?}");
    }
    let unrendered = "holds a value that no render covers";
    for (changes, reason) in [
        (vec![(1, Some(Field::U8(1)))], unrendered),
        (
            vec![(8, Some(Field::U8(0)))],
            "unsupported Appearance slot 8",
        ),
        (
            vec![(26, Some(Field::U32(16)))],
            "unsupported Appearance slot 26",
        ),
        (
            vec![(15, Some(Field::U8(1)))],
            "unsupported Appearance slot 15",
        ),
        (vec![(13, Some(Field::U8(3)))], unrendered),
        (vec![(23, Some(Field::U32(3)))], unrendered),
        (vec![(23, None)], unrendered),
        (vec![(34, Some(Field::U8(0)))], unrendered),
        // Only 23 = 1 frees the layout slots; they stay typed and present.
        (
            vec![(23, Some(Field::U32(2))), (24, Some(Field::F32(390.0)))],
            unrendered,
        ),
        (vec![(24, None)], unrendered),
        (
            vec![(28, Some(Field::Table(&[(1, Field::F32(1.0))])))],
            unrendered,
        ),
        (
            vec![(12, Some(Field::U8(1)))],
            "a shape that Appearance slot 12 hides is unsupported",
        ),
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
    let mut legacy = 4_u64.to_le_bytes().to_vec();
    legacy.extend("{}".encode_utf16().flat_map(u16::to_le_bytes));
    let error = decode_appearance(&legacy).unwrap_err().to_string();
    assert!(error.contains("legacy UTF-16 JSON Appearance"), "{error}");
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
fn gradient_appearances_decode_only_in_the_form_the_fixture_rendered() {
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
        (
            gradient_form(&saved, &[(23, Some(Field::U32(1)))]),
            unrendered(23),
        ),
        (
            gradient_form(&saved, &[(34, Some(Field::U8(2)))]),
            unrendered(34),
        ),
        (gradient_form(&saved, &[(28, None)]), unrendered(28)),
        (gradient_form(&saved, &[(20, None)]), unrendered(20)),
        // Only a fill color alone reads without the calibration slots.
        (
            encode_fields(&[(0, blue), (2, blue)]).unwrap(),
            unrendered(23),
        ),
    ] {
        let error = decode_appearance(&payload).unwrap_err().to_string();
        assert!(error.contains(&reason), "{reason}: {error}");
    }
}
