//! Premiere graphic Shape payloads: the Path and Appearance values of an
//! `AE.ADBE Shape` component.
//!
//! Path version 2 is `u32 2`, `u32 n`, then per vertex a `u32` flag (`0`
//! corner, `1` smooth) and six `f32` (point, in tangent and out tangent, in
//! layer pixels), then a closed byte. The 119 surveyed paths (a Premiere 24.3
//! template and two 13.0 MOGRTs) and their Premiere 26.5.1 re-saves all have
//! 8 + 28n + 1 bytes and closed byte `1`; calibration-2's polyline with closed
//! byte `0` rendered open. There is no contour count or separator, so a Path
//! holds one contour.
//!
//! An Appearance value has the Source Text frame (`text_payload`): a byte
//! count, the magic and one FlatBuffer, read with the same bounded reader.
//! Root slot 0 holds the Appearance table. Slot meanings come from AME renders
//! of our own payloads, which Premiere 26.5.1 saved byte-identically
//! (calibration runs 1 and 2), and of the gradient fills that Premiere
//! 26.5.1 saved for fixture `premiere_isolated_gradient_fills_26_5`. The meaning of the other slots that those payloads wrote is
//! unknown. Their saved values are not restored, but differing optional fields
//! produce diagnostics rather than discard the known path and paint.
//!
//! A legacy Appearance is UTF-16 JSON instead: a little-endian `u32` byte
//! count of the UTF-16LE text that follows, a zero `u32`, then one object
//! `{"mStyle": {...}, "mVersion": 1}` without a byte order mark or
//! terminator. Its style holds an integer color and a visibility switch for
//! each of the fill, the stroke and the shadow, the stroke width, and the
//! shadow angle, blur, offset and opacity. Required framing and active paint
//! switches remain checked. Optional authoring fields and an unrecognized version
//! are diagnosed without discarding the supported fill; active masks remain
//! unsupported rather than expose their concealed content.
//! No render measured the legacy form. By inference from the field names,
//! a visible gray fill (equal channels, which no channel order changes) and a
//! fill switched off convert; any other fill color, a color beyond 24 bits
//! and an enabled stroke or shadow do not. Export writes the FlatBuffer of
//! the current paint.

use super::text_payload::{color_table, framed, slot, Buffer, Table, TableOffset};
use crate::error::{ensure, unsupported, BuildError, Result};
use crate::schema::{
    text::{
        validate_stroke_width, PrAppearance, PrFill, PrGradient, PrGradientKind,
        PrGradientOpacityStop, PrGradientStop, PrMaskSource, PrPathVertex, PrRgb, PrShapePath,
        PrShapeStroke, DEFAULT_SHAPE_FILL, GRADIENT_Y_UNCONVERTED, SHAPE_SHADOW_ANGLE,
    },
    text_shadow::PrTextShadow,
};
use flatbuffers::{FlatBufferBuilder, ForwardsUOffset, Vector, WIPOffset};
use serde::{
    de::{value::MapAccessDeserializer, MapAccess, Visitor},
    Deserialize, Deserializer,
};
use std::{collections::BTreeMap, fmt};

const PATH_VERSION: u32 = 2;
const PATH_HEADER_BYTES: usize = 8;
/// One path vertex: the smooth flag and six coordinates. A mask path
/// (`schema::mask`) stores the same vertex.
pub(crate) const VERTEX_BYTES: usize = 28;
/// How messages name an Appearance payload.
const APPEARANCE: &str = "Appearance";
/// How messages name a legacy JSON Appearance payload.
const LEGACY_APPEARANCE: &str = "legacy JSON Appearance";
/// The first text unit of a legacy Appearance, `{` in UTF-16LE, where a
/// FlatBuffer frame holds its magic.
const LEGACY_JSON_START: &[u8] = b"{\0";

/// Appearance table slots whose meaning calibration runs and the gradient
/// fixture measured.
mod slots {
    pub(super) const FILL_COLOR: usize = 0;
    /// `0` draws no fill and keeps the stroke (calibration-2).
    pub(super) const FILL_ENABLED: usize = 1;
    pub(super) const STROKE_COLOR: usize = 2;
    pub(super) const STROKE_ENABLED: usize = 3;
    /// Layer pixels.
    pub(super) const STROKE_WIDTH: usize = 4;
    pub(super) const SHADOW_COLOR: usize = 5;
    pub(super) const SHADOW_ENABLED: usize = 6;
    /// Percent.
    pub(super) const SHADOW_OPACITY: usize = 7;
    /// Pixels, down and to the right.
    pub(super) const SHADOW_DISTANCE: usize = 9;
    pub(super) const SHADOW_SIZE: usize = 10;
    pub(super) const SHADOW_BLUR: usize = 11;
    /// `1` is Mask with Shape: the shape masks the objects below it instead
    /// of drawing; native controls draw neither fill nor stroke.
    pub(super) const MASK: usize = 12;
    /// `1` inverts a Mask with Shape; without slot 12
    /// it draws like the base at `1` or `2` ([`super::UNMEASURED_SLOTS`]).
    pub(super) const MASK_INVERTED: usize = 13;
    /// `1` a linear and `2` a radial gradient fill; absent, a solid fill.
    pub(super) const FILL_TYPE: usize = 19;
    /// The gradient: [`super::gradient_slots`].
    pub(super) const GRADIENT: usize = 20;
    /// `0` or absent centre, `1` inside, `2` outside; `3` drew both sides.
    /// Premiere 26.5.1 saved the gradient fixture's centred stroke (D, which
    /// rendered centred) without it.
    pub(super) const STROKE_POSITION: usize = 31;
    pub(super) const MEASURED: [usize; 14] = [
        FILL_COLOR,
        FILL_ENABLED,
        STROKE_COLOR,
        STROKE_ENABLED,
        STROKE_WIDTH,
        SHADOW_COLOR,
        SHADOW_ENABLED,
        SHADOW_OPACITY,
        SHADOW_DISTANCE,
        SHADOW_SIZE,
        SHADOW_BLUR,
        MASK,
        FILL_TYPE,
        STROKE_POSITION,
    ];
}

/// Gradient table slots as Premiere 26.5.1 saved them: start
/// and end in layer pixels, and the color and opacity stop vectors, whose
/// tables hold a value (a color table or an opacity), a position and a
/// midpoint. An absent number is 0.
mod gradient_slots {
    pub(super) const START_X: usize = 0;
    pub(super) const START_Y: usize = 1;
    pub(super) const END_X: usize = 2;
    pub(super) const END_Y: usize = 3;
    pub(super) const COLOR_STOPS: usize = 6;
    pub(super) const OPACITY_STOPS: usize = 7;
    pub(super) const VALUE: usize = 0;
    pub(super) const POSITION: usize = 1;
    pub(super) const MIDPOINT: usize = 2;
}

/// The only gradient stop midpoint that converts: FX interpolates each pair
/// of stops evenly.
const GRADIENT_MIDPOINT: f32 = 0.5;

/// The stroke position that converts: centred on the outline.
const CENTRED_STROKE: u32 = 0;

/// The fill color of run 1's base payload, which the writer stores, switched
/// off, for a shape without a fill.
const BASE_FILL: PrRgb = PrRgb([0, 96, 255]);

/// The shadow color and opacity of run 1's base payload, which the writer
/// stores when the shadow is off (it draws nothing then).
const OFF_SHADOW_COLOR: PrRgb = PrRgb([40; 3]);
const OFF_SHADOW_OPACITY: f32 = 60.0;

/// One FlatBuffer field as calibration run 1 wrote it, or a gradient.
#[derive(Debug, Clone, Copy)]
enum Field<'a> {
    U8(u8),
    U32(u32),
    F32(f32),
    /// A color table: red, green and blue.
    Color([u8; 3]),
    Table(&'a [(usize, Field<'a>)]),
    Tables(&'a [&'a [(usize, Field<'a>)]]),
    /// A gradient table that the writer stores; never a value to match.
    Gradient(&'a PrGradient),
}

const GRADIENT_STOPS: [&[(usize, Field)]; 2] = [
    &[(0, Field::Color([0, 96, 255])), (2, Field::F32(0.5))],
    &[
        (0, Field::Color([0, 0, 0])),
        (1, Field::F32(1.0)),
        (2, Field::F32(0.5)),
    ],
];
/// Also the opacity stops of every opaque gradient that Premiere 26.5.1
/// saved.
const GRADIENT_ALPHA_STOPS: [&[(usize, Field)]; 2] = [
    &[(2, Field::F32(0.5))],
    &[(1, Field::F32(1.0)), (2, Field::F32(0.5))],
];
const GRADIENT: [(usize, Field); 6] = [
    (0, Field::F32(-300.0)),
    (1, Field::F32(0.0)),
    (2, Field::F32(300.0)),
    (3, Field::F32(0.0)),
    (6, Field::Tables(&GRADIENT_STOPS)),
    (7, Field::Tables(&GRADIENT_ALPHA_STOPS)),
];

/// The gradient and layout slots that every payload of calibration run 1
/// wrote (the survey shows gradient geometry in 20 and sizes in 24 and 25),
/// at its base values, which the writer writes. With slot 23 at 1 any value
/// of these types draws like them (calibration-2 T5 is RGB-identical to T0);
/// with 23 at another value only these values were rendered.
const LAYOUT_SLOTS: [(usize, Field); 5] = [
    (20, Field::Table(&GRADIENT)),
    (24, Field::F32(300.0)),
    (25, Field::F32(600.0)),
    (28, Field::Table(&[])),
    (30, Field::Table(&[])),
];

/// Slots of unknown meaning, whether the writer stores them, and the values
/// that rendered like run 1's base (calibration runs 1 and 2). Import does not
/// require these optional fields; the writer retains its calibrated profile.
const UNMEASURED_SLOTS: [(usize, bool, &[Field]); 3] = [
    (13, false, &[Field::U8(1), Field::U8(2)]),
    (23, true, &[Field::U32(1), Field::U32(0), Field::U32(2)]),
    (34, true, &[Field::U8(1), Field::U8(2)]),
];

/// Slot 23's value under which any well-formed layout slot draws like the
/// base.
const LAYOUT_FREE: u32 = 1;

/// The layout slots that Premiere 26.5.1 saved beside every gradient of the
/// gradient fixture (G1), which convert there only at these values, and the
/// layout and unmeasured slots that it did not save, which must be absent.
const GRADIENT_LAYOUT: [(usize, Field); 3] = [
    (28, Field::Table(&[])),
    (30, Field::Table(&[])),
    (34, Field::U8(1)),
];
const NOT_BESIDE_GRADIENT: [usize; 4] = [13, 23, 24, 25];

/// Decode one Path value.
///
/// # Errors
/// Rejects other versions, sizes, vertex flags and closed bytes, and
/// coordinates that are not finite.
pub(crate) fn decode_path(payload: &[u8]) -> Result<PrShapePath> {
    let word = |at: usize| {
        payload
            .get(at..at + 4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four bytes")))
            .ok_or_else(|| unsupported("truncated Path payload"))
    };
    let version = word(0)?;
    ensure!(
        version == PATH_VERSION,
        "Path version {version} is unsupported"
    );
    let count =
        usize::try_from(word(4)?).map_err(|_| unsupported("Path vertex count overflows"))?;
    let vertex_bytes = count
        .checked_mul(VERTEX_BYTES)
        .filter(|bytes| bytes.checked_add(PATH_HEADER_BYTES + 1) == Some(payload.len()))
        .ok_or_else(|| unsupported(format!("Path size does not match its {count} vertices")))?;
    let vertices = payload[PATH_HEADER_BYTES..PATH_HEADER_BYTES + vertex_bytes]
        .chunks_exact(VERTEX_BYTES)
        .map(decode_vertex)
        .collect::<Result<Vec<_>>>()?;
    let closed = match payload[payload.len() - 1] {
        0 => false,
        1 => true,
        other => return Err(unsupported(format!("unknown Path closed byte {other}"))),
    };
    Ok(PrShapePath { vertices, closed })
}

/// Encode one Path value.
pub(crate) fn encode_path(path: &PrShapePath) -> crate::format::Result<Vec<u8>> {
    let count = u32::try_from(path.vertices.len())
        .map_err(|_| crate::format::invalid("Path vertex count exceeds its u32 field"))?;
    let length = path
        .vertices
        .len()
        .checked_mul(VERTEX_BYTES)
        .and_then(|bytes| bytes.checked_add(PATH_HEADER_BYTES + 1))
        .ok_or_else(|| crate::format::invalid("Path payload size overflows"))?;
    let mut payload = Vec::new();
    payload.try_reserve_exact(length).map_err(|error| {
        crate::format::invalid(format!("cannot allocate Path payload: {error}"))
    })?;
    payload.extend_from_slice(&PATH_VERSION.to_le_bytes());
    payload.extend_from_slice(&count.to_le_bytes());
    for vertex in &path.vertices {
        encode_vertex(vertex, &mut payload);
    }
    payload.push(u8::from(path.closed));
    Ok(payload)
}

/// Decode one [`VERTEX_BYTES`] vertex: a `u32` flag (`0` corner, `1` smooth)
/// and six `f32` (point, in tangent, out tangent).
///
/// # Errors
/// Rejects other flags and coordinates that are not finite.
pub(crate) fn decode_vertex(vertex: &[u8]) -> Result<PrPathVertex> {
    let smooth = match u32::from_le_bytes(vertex[..4].try_into().expect("four bytes")) {
        0 => false,
        1 => true,
        flag => return Err(unsupported(format!("unknown Path vertex flag {flag}"))),
    };
    let value = |index: usize| {
        let at = 4 + 4 * index;
        f32::from_le_bytes(vertex[at..at + 4].try_into().expect("four bytes"))
    };
    let values: [f32; 6] = std::array::from_fn(value);
    ensure!(
        values.iter().all(|value| value.is_finite()),
        "Path vertices must be finite"
    );
    Ok(PrPathVertex {
        smooth,
        point: [values[0], values[1]],
        in_tangent: [values[2], values[3]],
        out_tangent: [values[4], values[5]],
    })
}

/// Append one vertex in the layout that [`decode_vertex`] reads.
pub(crate) fn encode_vertex(vertex: &PrPathVertex, payload: &mut Vec<u8>) {
    payload.extend_from_slice(&u32::from(vertex.smooth).to_le_bytes());
    for value in [vertex.point, vertex.in_tangent, vertex.out_tangent]
        .into_iter()
        .flatten()
    {
        payload.extend_from_slice(&value.to_le_bytes());
    }
}

/// Decode supported paint and report unconverted optional Appearance fields.
///
/// # Errors
/// Rejects malformed required data, unsupported active paint and mask forms,
/// and an enabled stroke or shadow that lacks a required value.
pub(crate) fn decode_appearance_with_notes(payload: &[u8]) -> Result<(PrAppearance, Vec<String>)> {
    use slots::*;
    if payload.get(8..10) == Some(LEGACY_JSON_START) {
        return decode_legacy_appearance(payload);
    }
    let buffer = Buffer::from_payload(payload, APPEARANCE)?;
    let root = buffer.table(buffer.offset(0)?)?;
    let mut notes: Vec<_> = root
        .present()?
        .into_iter()
        .filter(|slot| *slot != 0)
        .map(|slot| {
            format!("optional Appearance root slot {slot} not converted; supported paint retained")
        })
        .collect();
    let table = root
        .table(0)?
        .ok_or_else(|| unsupported("Appearance has no style table"))?;
    let known = |slot: &usize| {
        MEASURED.contains(slot)
            || LAYOUT_SLOTS.iter().any(|(known, _)| known == slot)
            || UNMEASURED_SLOTS.iter().any(|(known, _, _)| known == slot)
    };
    notes.extend(
        table
            .present()?
            .into_iter()
            .filter(|slot| !known(slot))
            .map(|slot| {
                format!("optional Appearance slot {slot} not converted; supported paint retained")
            }),
    );
    // The gradient fixture's B and D are linear, C radial (G1).
    let gradient = match table.u32(FILL_TYPE)? {
        None => None,
        Some(1) => Some(PrGradientKind::Linear),
        Some(2) => Some(PrGradientKind::Radial),
        Some(other) => {
            return Err(unsupported(format!(
                "Appearance slot 19 holds a value that no render covers ({other})"
            )))
        }
    };
    notes.extend(optional_appearance_notes(table, gradient.is_some())?);
    let mask_source = match table.u8(MASK)? {
        None => None,
        Some(1) => Some(PrMaskSource {
            inverted: match table.u8(MASK_INVERTED)? {
                None => false,
                Some(1) => true,
                Some(other) => {
                    return Err(unsupported(format!(
                        "Appearance slot 13 value {other} beside Mask with Shape is unmeasured"
                    )))
                }
            },
        }),
        Some(other) => {
            return Err(unsupported(format!(
                "invalid Appearance slot 12 value {other}"
            )))
        }
    };
    let fill_color = table
        .table(FILL_COLOR)?
        .map(color)
        .transpose()?
        .unwrap_or(DEFAULT_SHAPE_FILL);
    let fill = match table.u8(FILL_ENABLED)? {
        None => Some(match gradient {
            Some(kind) => PrFill::Gradient(decode_gradient(table, kind)?),
            None => PrFill::Solid(fill_color),
        }),
        Some(0) if gradient.is_none() => None,
        Some(other) => {
            return Err(unsupported(format!(
                "Appearance slot 1 holds a value that no render covers ({other})"
            )))
        }
    };
    // A disabled stroke or shadow draws nothing, whatever it stores; its
    // values must still be well formed, and they are not kept.
    let stroke_color = table.table(STROKE_COLOR)?.map(color).transpose()?;
    let stroke_width = table.f32(STROKE_WIDTH)?;
    let stroke_position = table.u32(STROKE_POSITION)?;
    let stroke = if switch(table, STROKE_ENABLED)? {
        let (Some(color), Some(width)) = (stroke_color, stroke_width) else {
            return Err(unsupported(
                "an enabled shape stroke lacks its color or width",
            ));
        };
        let side = match stroke_position.unwrap_or(CENTRED_STROKE) {
            CENTRED_STROKE => None,
            1 => Some("inside"),
            2 => Some("outside"),
            3 => Some("inside and outside"),
            other => {
                return Err(unsupported(format!(
                    "unknown shape stroke position {other}"
                )))
            }
        };
        if let Some(side) = side {
            return Err(unsupported(format!(
                "shape strokes {side} the outline are unsupported; only centred strokes convert"
            )));
        }
        Some(PrShapeStroke { color, width })
    } else {
        None
    };
    let shadow_color = table.table(SHADOW_COLOR)?.map(color).transpose()?;
    let [opacity, distance, size, blur] =
        [SHADOW_OPACITY, SHADOW_DISTANCE, SHADOW_SIZE, SHADOW_BLUR].map(|slot| table.f32(slot));
    let [opacity, distance, size, blur] = [opacity?, distance?, size?, blur?];
    let shadow = if switch(table, SHADOW_ENABLED)? {
        let (Some(color), Some(opacity), Some(distance), Some(size), Some(blur)) =
            (shadow_color, opacity, distance, size, blur)
        else {
            return Err(unsupported(
                "an enabled shape shadow lacks its color, opacity, distance, size or blur",
            ));
        };
        Some(PrTextShadow {
            color,
            opacity,
            angle: SHAPE_SHADOW_ANGLE,
            distance,
            size,
            blur,
        })
    } else {
        None
    };
    Ok((
        PrAppearance {
            fill,
            stroke,
            shadow,
            mask_source,
        },
        notes,
    ))
}

// Existing paint-value tests do not consume the reader's contextual notes.
#[cfg(test)]
pub(crate) fn decode_appearance(payload: &[u8]) -> Result<PrAppearance> {
    decode_appearance_with_notes(payload).map(|(appearance, _)| appearance)
}

/// Required legacy paint fields are separate from unconverted optional data.
#[derive(Deserialize)]
struct LegacyAppearance {
    #[serde(rename = "mStyle", deserialize_with = "legacy_style_object")]
    style: LegacyStyle,
    #[serde(rename = "mVersion")]
    version: u32,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

/// Active paint and concealment switches remain typed. Inactive stroke and
/// shadow values cannot affect the supported fill and need no admission checks.
#[derive(Deserialize)]
struct LegacyStyle {
    #[serde(rename = "mFillColor")]
    fill_color: u32,
    #[serde(rename = "mFillVisible")]
    fill_visible: bool,
    #[serde(rename = "mStrokeColor", default)]
    _stroke_color: Option<serde_json::Value>,
    #[serde(rename = "mStrokeVisible")]
    stroke_visible: bool,
    #[serde(rename = "mStrokeWidth", default)]
    _stroke_width: Option<serde_json::Value>,
    #[serde(rename = "mShadowAngle", default)]
    _shadow_angle: Option<serde_json::Value>,
    #[serde(rename = "mShadowBlur", default)]
    _shadow_blur: Option<serde_json::Value>,
    #[serde(rename = "mShadowColor", default)]
    _shadow_color: Option<serde_json::Value>,
    #[serde(rename = "mShadowOffset", default)]
    _shadow_offset: Option<serde_json::Value>,
    #[serde(rename = "mShadowOpacity", default)]
    _shadow_opacity: Option<serde_json::Value>,
    #[serde(rename = "mShadowVisible")]
    shadow_visible: bool,
    #[serde(rename = "mIsMask", default)]
    is_mask: bool,
    #[serde(rename = "mIsMaskInverted", default)]
    is_mask_inverted: bool,
    #[serde(rename = "mFillColorType", default)]
    fill_color_type: u32,
    #[serde(rename = "mAdditionalStrokes", default)]
    additional_strokes: Vec<serde_json::Value>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

/// Require named style fields rather than the positional sequence that a
/// derived struct deserializer also accepts.
fn legacy_style_object<'de, D>(deserializer: D) -> std::result::Result<LegacyStyle, D::Error>
where
    D: Deserializer<'de>,
{
    struct StyleObject;
    impl<'de> Visitor<'de> for StyleObject {
        type Value = LegacyStyle;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an object with named fields")
        }

        fn visit_map<M: MapAccess<'de>>(
            self,
            map: M,
        ) -> std::result::Result<LegacyStyle, M::Error> {
            LegacyStyle::deserialize(MapAccessDeserializer::new(map))
        }
    }
    deserializer.deserialize_map(StyleObject)
}

/// Decode a legacy gray fill without interpreting optional authoring fields.
///
/// # Errors
/// Rejects invalid required framing, UTF-16 or paint fields, active masks,
/// unsupported active paint and a visible non-gray fill of unknown channel order.
fn decode_legacy_appearance(payload: &[u8]) -> Result<(PrAppearance, Vec<String>)> {
    let Some((frame, text)) = payload.split_first_chunk::<8>() else {
        return Err(unsupported(format!("truncated {LEGACY_APPEARANCE}")));
    };
    let (count, upper) = frame.split_at(4);
    let [count, upper] =
        [count, upper].map(|word| u32::from_le_bytes(word.try_into().expect("four bytes")));
    ensure!(
        upper == 0,
        "{LEGACY_APPEARANCE}: the word after its byte count is {upper}, not 0"
    );
    ensure!(
        usize::try_from(count).is_ok_and(|count| count == text.len()),
        "{LEGACY_APPEARANCE}: its byte count {count} does not match the {} bytes after it",
        text.len()
    );
    ensure!(
        text.len() % 2 == 0,
        "{LEGACY_APPEARANCE}: an odd byte count {} is not UTF-16",
        text.len()
    );
    let json: String = char::decode_utf16(
        text.chunks_exact(2)
            .map(|unit| u16::from_le_bytes([unit[0], unit[1]])),
    )
    .collect::<std::result::Result<_, _>>()
    .map_err(|error| unsupported(format!("{LEGACY_APPEARANCE}: invalid UTF-16: {error}")))?;
    let LegacyAppearance {
        style,
        version,
        extra,
    } = serde_json::from_str(&json)
        .map_err(|error| unsupported(format!("{LEGACY_APPEARANCE}: {error}")))?;
    ensure!(
        !style.is_mask
            && !style.is_mask_inverted
            && !style.extra.contains_key("mMaskSource")
            && !extra.iter().any(|(name, value)| match name.as_str() {
                "mMaskSource" => true,
                "mIsMask" | "mIsMaskInverted" => *value != serde_json::Value::Bool(false),
                _ => false,
            }),
        "{LEGACY_APPEARANCE}: unsupported mask fields"
    );
    ensure!(
        !style.fill_visible || style.fill_color_type == 0,
        "{LEGACY_APPEARANCE}: active fill type {} is unsupported",
        style.fill_color_type
    );
    ensure!(
        style.additional_strokes.is_empty(),
        "{LEGACY_APPEARANCE}: additional strokes are unsupported"
    );
    let fill_color = legacy_color("mFillColor", style.fill_color)?;
    ensure!(
        !style.stroke_visible,
        "{LEGACY_APPEARANCE}: an enabled stroke is unsupported"
    );
    ensure!(
        !style.shadow_visible,
        "{LEGACY_APPEARANCE}: an enabled shadow is unsupported"
    );
    let fill = if style.fill_visible {
        let [red, green, blue] = fill_color;
        ensure!(
            red == green && green == blue,
            "{LEGACY_APPEARANCE}: mFillColor {:#08x} is not gray; only a gray fill converts, since the channel order is unknown",
            style.fill_color
        );
        Some(PrFill::Solid(PrRgb(fill_color)))
    } else {
        None
    };
    let mut notes: Vec<_> = extra
        .keys()
        .chain(style.extra.keys())
        .map(|name| {
            format!(
                "optional Appearance legacy field {name} not converted; supported paint retained"
            )
        })
        .collect();
    if version != 1 {
        notes.push(format!(
            "optional Appearance legacy version {version} not restored; supported paint retained"
        ));
    }
    Ok((
        PrAppearance {
            fill,
            stroke: None,
            shadow: None,
            mask_source: None,
        },
        notes,
    ))
}

/// The three low bytes of the legacy JSON color `value` of the field `name`,
/// in an order that is unknown. The saved colors fit 24 bits; a higher byte
/// has no known meaning.
fn legacy_color(name: &str, value: u32) -> Result<[u8; 3]> {
    let [high, low @ ..] = value.to_be_bytes();
    ensure!(
        high == 0,
        "{LEGACY_APPEARANCE}: {name} {value:#08x} is outside the 24-bit color form"
    );
    Ok(low)
}

/// Why an Appearance fails closed at `slot`.
fn unrendered(slot: usize) -> BuildError {
    unsupported(format!(
        "Appearance slot {slot} holds a value that no render covers"
    ))
}

/// Optional layout metadata never gates known paint. Calibration comparisons
/// only decide whether an unconverted saved value needs a diagnostic; an absent
/// field has no saved value to omit. Active gradient and mask data read separately.
fn optional_appearance_notes(table: Table<'_>, gradient: bool) -> Result<Vec<String>> {
    let present = table.present()?;
    let mut checks = Vec::new();
    if gradient {
        for &(slot, field) in &GRADIENT_LAYOUT {
            if present.contains(&slot) {
                checks.push((slot, matches(table, slot, field)));
            }
        }
        for slot in NOT_BESIDE_GRADIENT {
            if present.contains(&slot) {
                checks.push((slot, Ok(false)));
            }
        }
    } else {
        for &(slot, _, values) in &UNMEASURED_SLOTS {
            if present.contains(&slot) {
                let rendered = values.iter().try_fold(false, |rendered, &value| {
                    matches(table, slot, value).map(|matched| rendered || matched)
                });
                checks.push((slot, rendered));
            }
        }
        let free = matches!(table.u32(23), Ok(Some(LAYOUT_FREE)));
        for &(slot, field) in &LAYOUT_SLOTS {
            if present.contains(&slot) {
                checks.push((
                    slot,
                    if free {
                        well_formed(table, slot, field)
                    } else {
                        matches(table, slot, field)
                    },
                ));
            }
        }
    }
    Ok(checks
        .into_iter()
        .filter_map(|(slot, rendered)| match rendered {
            Ok(true) => None,
            Ok(false) => Some(format!(
                "optional Appearance slot {slot} not converted; supported paint retained"
            )),
            Err(error) => Some(format!(
                "optional Appearance slot {slot} not converted ({error}); supported paint retained"
            )),
        })
        .collect())
}

/// The `kind` gradient of an Appearance in the form Premiere 26.5.1 saved for
/// the gradient fixture: start and end x, whose y are absent
/// (G1, G3), the color stops with a position kept as written (C's middle
/// stop is 0.49922094, not 0.5; G2), and the opacity stops, whose absent
/// value is full (G5); every stop with a midpoint. The ranges are left to
/// [`PrShape::validate`].
///
/// [`PrShape::validate`]: crate::schema::text::PrShape::validate
fn decode_gradient(appearance: Table<'_>, kind: PrGradientKind) -> Result<PrGradient> {
    use gradient_slots::*;
    let gradient = appearance
        .table(slots::GRADIENT)?
        .ok_or_else(|| unrendered(slots::GRADIENT))?;
    gradient.allow_only(
        &[START_X, START_Y, END_X, END_Y, COLOR_STOPS, OPACITY_STOPS],
        "20",
    )?;
    let number = |table: Table<'_>, slot| Ok::<_, BuildError>(table.f32(slot)?.unwrap_or(0.0));
    ensure!(
        number(gradient, START_Y)? == 0.0 && number(gradient, END_Y)? == 0.0,
        "{GRADIENT_Y_UNCONVERTED}"
    );
    let position = |stop: Table<'_>, vector: &str| -> Result<f32> {
        stop.allow_only(&[VALUE, POSITION, MIDPOINT], vector)?;
        let midpoint = number(stop, MIDPOINT)?;
        ensure!(
            midpoint == GRADIENT_MIDPOINT,
            "gradient midpoint {midpoint} is not converted"
        );
        number(stop, POSITION)
    };
    let stops = gradient
        .tables(COLOR_STOPS)?
        .into_iter()
        .map(|stop| {
            let position = position(stop, "20.6")?;
            let color = stop
                .table(VALUE)?
                .ok_or_else(|| unrendered(slots::GRADIENT))?;
            Ok(PrGradientStop {
                position,
                color: stop_color(color)?,
            })
        })
        .collect::<Result<_>>()?;
    let opacity_stops = gradient
        .tables(OPACITY_STOPS)?
        .into_iter()
        .map(|stop| {
            Ok(PrGradientOpacityStop {
                position: position(stop, "20.7")?,
                opacity: stop.f32(VALUE)?.unwrap_or(1.0),
            })
        })
        .collect::<Result<_>>()?;
    Ok(PrGradient {
        kind,
        start_x: number(gradient, START_X)?,
        end_x: number(gradient, END_X)?,
        stops,
        opacity_stops,
    })
}

/// A gradient stop's color table: red, green and blue, or no field, which
/// Premiere 26.5.1 saved for the gradient fixture's white stop and AME drew
/// white (G2).
fn stop_color(table: Table<'_>) -> Result<PrRgb> {
    if table.present()?.is_empty() {
        return Ok(PrRgb([255; 3]));
    }
    color(table)
}

/// Whether `slot` of `table` holds a value of `field`'s type as the survey
/// saw it: a float, or a gradient or stop table whose own slots hold
/// floats, colors and stop vectors (every stop an optional color and two
/// floats).
fn well_formed(table: Table<'_>, slot: usize, field: Field) -> Result<bool> {
    const GRADIENT_SLOTS: [usize; 6] = [0, 1, 2, 3, 6, 7];
    const STOP_VECTORS: [usize; 2] = [6, 7];
    let stops_well_formed = |gradient: Table<'_>| -> Result<bool> {
        for vector in STOP_VECTORS {
            for stop in gradient.tables(vector)? {
                if stop.present()?.iter().any(|slot| *slot > 2) {
                    return Ok(false);
                }
                stop.table(0)?.map(color).transpose()?;
                stop.f32(1)?;
                stop.f32(2)?;
            }
        }
        Ok(true)
    };
    Ok(match field {
        Field::F32(_) => table.f32(slot)?.is_some(),
        Field::Table(_) => match table.table(slot)? {
            // 20 is a gradient with its geometry; 28 and 30 hold stops only.
            Some(gradient) => {
                let allowed: &[usize] = if slot == 20 {
                    &GRADIENT_SLOTS
                } else {
                    &STOP_VECTORS
                };
                if gradient
                    .present()?
                    .iter()
                    .any(|slot| !allowed.contains(slot))
                {
                    return Ok(false);
                }
                for geometry in [0, 1, 2, 3] {
                    gradient.f32(geometry)?;
                }
                stops_well_formed(gradient)?
            }
            None => false,
        },
        Field::U8(_) | Field::U32(_) | Field::Color(_) | Field::Tables(_) | Field::Gradient(_) => {
            false
        }
    })
}

/// Whether `slot` of `table` holds `expected`, with floats compared by bits.
fn matches(table: Table<'_>, slot: usize, expected: Field) -> Result<bool> {
    Ok(match expected {
        Field::U8(value) => table.u8(slot)? == Some(value),
        Field::U32(value) => table.u32(slot)? == Some(value),
        Field::F32(value) => table.f32(slot)?.map(f32::to_bits) == Some(value.to_bits()),
        Field::Color(rgb) => table.table(slot)?.map(color).transpose()? == Some(PrRgb(rgb)),
        Field::Table(fields) => match table.table(slot)? {
            Some(inner) => table_matches(inner, fields)?,
            None => false,
        },
        Field::Tables(items) => {
            let tables = table.tables(slot)?;
            tables.len() == items.len()
                && tables
                    .into_iter()
                    .zip(items)
                    .map(|(inner, fields)| table_matches(inner, fields))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .all(|matched| matched)
        }
        Field::Gradient(_) => false,
    })
}

/// Whether `table` holds exactly `fields`, listed by increasing slot.
fn table_matches(table: Table<'_>, fields: &[(usize, Field)]) -> Result<bool> {
    if table.present()? != fields.iter().map(|&(slot, _)| slot).collect::<Vec<_>>() {
        return Ok(false);
    }
    for &(slot, field) in fields {
        if !matches(table, slot, field)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// A switch that is off when absent and on at `1`; no other value rendered.
fn switch(table: Table<'_>, slot: usize) -> Result<bool> {
    match table.u8(slot)? {
        None => Ok(false),
        Some(1) => Ok(true),
        Some(other) => Err(unsupported(format!(
            "invalid Appearance slot {slot} value {other}"
        ))),
    }
}

/// A color table that stores red, green and blue, as every surveyed and
/// calibrated Appearance color does.
fn color(table: Table<'_>) -> Result<PrRgb> {
    ensure!(
        table.present()? == [0, 1, 2],
        "an Appearance color must store red, green and blue"
    );
    let [Some(red), Some(green), Some(blue)] = [table.u8(0)?, table.u8(1)?, table.u8(2)?] else {
        return Err(unsupported(
            "an Appearance color must store red, green and blue",
        ));
    };
    Ok(PrRgb([red, green, blue]))
}

/// Encode one Appearance value in run 1's layout, or with a gradient in the
/// layout Premiere 26.5.1 saved for the gradient fixture: the measured slots
/// of what it draws, and the other slots at their base values.
pub(crate) fn encode_appearance(appearance: &PrAppearance) -> crate::format::Result<Vec<u8>> {
    encode_fields(&appearance_fields(appearance)?)
}

/// The Appearance slots that the writer stores for `appearance`.
fn appearance_fields(appearance: &PrAppearance) -> crate::format::Result<Vec<(usize, Field<'_>)>> {
    use slots::*;
    // Without a solid fill, the base color stays, switched off by slot 1 as
    // calibration-2 rendered it, or unused beside a gradient.
    let solid = match &appearance.fill {
        Some(PrFill::Solid(color)) => *color,
        _ => BASE_FILL,
    };
    let mut fields = vec![(FILL_COLOR, Field::Color(solid.0))];
    if appearance.fill.is_none() {
        fields.push((FILL_ENABLED, Field::U8(0)));
    }
    if let Some(stroke) = appearance.stroke {
        validate_stroke_width(stroke.width, "shape")?;
        fields.extend([
            (STROKE_COLOR, Field::Color(stroke.color.0)),
            (STROKE_ENABLED, Field::U8(1)),
            (STROKE_WIDTH, Field::F32(stroke.width)),
            (STROKE_POSITION, Field::U32(CENTRED_STROKE)),
        ]);
    }
    match appearance.shadow {
        Some(shadow) => {
            shadow.validate()?;
            crate::format::ensure_valid!(
                shadow.angle == SHAPE_SHADOW_ANGLE,
                "a shape shadow falls at {SHAPE_SHADOW_ANGLE} degrees; Appearance stores no angle"
            );
            fields.extend([
                (SHADOW_COLOR, Field::Color(shadow.color.0)),
                (SHADOW_ENABLED, Field::U8(1)),
                (SHADOW_OPACITY, Field::F32(shadow.opacity)),
                (SHADOW_DISTANCE, Field::F32(shadow.distance)),
                (SHADOW_SIZE, Field::F32(shadow.size)),
                (SHADOW_BLUR, Field::F32(shadow.blur)),
            ]);
        }
        None => fields.extend([
            (SHADOW_COLOR, Field::Color(OFF_SHADOW_COLOR.0)),
            (SHADOW_OPACITY, Field::F32(OFF_SHADOW_OPACITY)),
        ]),
    }
    if let Some(mask) = appearance.mask_source {
        fields.push((MASK, Field::U8(1)));
        if mask.inverted {
            fields.push((MASK_INVERTED, Field::U8(1)));
        }
    }
    if let Some(PrFill::Gradient(gradient)) = &appearance.fill {
        let fill_type = match gradient.kind {
            PrGradientKind::Linear => 1,
            PrGradientKind::Radial => 2,
        };
        fields.extend([
            (FILL_TYPE, Field::U32(fill_type)),
            (GRADIENT, Field::Gradient(gradient)),
        ]);
        fields.extend(GRADIENT_LAYOUT);
        return Ok(fields);
    }
    fields.extend(LAYOUT_SLOTS);
    fields.extend(
        UNMEASURED_SLOTS
            .iter()
            .filter(|(_, required, _)| *required)
            .map(|&(slot, _, values)| (slot, values[0])),
    );
    Ok(fields)
}

/// The gradient table in the form Premiere 26.5.1 saved:
/// start and end x without y, and the color and opacity stops at midpoint
/// 50 %. As in its saves, an opacity stop omits a full opacity and a
/// position of 0 (G5), so an opaque gradient's opacity stops are
/// [`GRADIENT_ALPHA_STOPS`].
fn gradient_table(fbb: &mut FlatBufferBuilder<'_>, gradient: &PrGradient) -> TableOffset {
    use gradient_slots::*;
    let stops: Vec<[(usize, Field<'_>); 3]> = gradient
        .stops
        .iter()
        .map(|stop| {
            [
                (VALUE, Field::Color(stop.color.0)),
                (POSITION, Field::F32(stop.position)),
                (MIDPOINT, Field::F32(GRADIENT_MIDPOINT)),
            ]
        })
        .collect();
    let stops: Vec<_> = stops.iter().map(|stop| stop.as_slice()).collect();
    let opacity_stops: Vec<Vec<(usize, Field<'_>)>> = gradient
        .opacity_stops
        .iter()
        .map(|stop| {
            [
                (stop.opacity != 1.0).then_some((VALUE, Field::F32(stop.opacity))),
                (stop.position != 0.0).then_some((POSITION, Field::F32(stop.position))),
                Some((MIDPOINT, Field::F32(GRADIENT_MIDPOINT))),
            ]
            .into_iter()
            .flatten()
            .collect()
        })
        .collect();
    let opacity_stops: Vec<_> = opacity_stops.iter().map(Vec::as_slice).collect();
    build(
        fbb,
        &[
            (START_X, Field::F32(gradient.start_x)),
            (END_X, Field::F32(gradient.end_x)),
            (COLOR_STOPS, Field::Tables(&stops)),
            (OPACITY_STOPS, Field::Tables(&opacity_stops)),
        ],
    )
}

/// An Appearance payload whose table holds `fields`.
fn encode_fields(fields: &[(usize, Field)]) -> crate::format::Result<Vec<u8>> {
    let mut fbb = FlatBufferBuilder::with_capacity(512);
    let style = build(&mut fbb, fields);
    let root = {
        let table = fbb.start_table();
        fbb.push_slot_always(slot(0), style);
        fbb.end_table(table)
    };
    fbb.finish_minimal(root);
    framed(fbb.finished_data(), APPEARANCE)
}

/// A field whose table and vector children are already built.
enum Built<'b> {
    U8(u8),
    U32(u32),
    F32(f32),
    Table(TableOffset),
    Tables(WIPOffset<Vector<'b, ForwardsUOffset<flatbuffers::TableFinishedWIPOffset>>>),
}

/// Build a table of `fields`, children first as FlatBuffers requires.
fn build<'b>(fbb: &mut FlatBufferBuilder<'b>, fields: &[(usize, Field)]) -> TableOffset {
    let built: Vec<_> = fields
        .iter()
        .map(|&(index, field)| {
            let value = match field {
                Field::U8(value) => Built::U8(value),
                Field::U32(value) => Built::U32(value),
                Field::F32(value) => Built::F32(value),
                Field::Color(rgb) => Built::Table(color_table(fbb, PrRgb(rgb))),
                Field::Table(inner) => Built::Table(build(fbb, inner)),
                Field::Tables(items) => {
                    let tables: Vec<_> = items.iter().map(|inner| build(fbb, inner)).collect();
                    Built::Tables(fbb.create_vector(&tables))
                }
                Field::Gradient(gradient) => Built::Table(gradient_table(fbb, gradient)),
            };
            (index, value)
        })
        .collect();
    let table = fbb.start_table();
    for (index, value) in built {
        let slot = slot(index);
        match value {
            Built::U8(value) => fbb.push_slot_always(slot, value),
            Built::U32(value) => fbb.push_slot_always(slot, value),
            Built::F32(value) => fbb.push_slot_always(slot, value),
            Built::Table(offset) => fbb.push_slot_always(slot, offset),
            Built::Tables(offset) => fbb.push_slot_always(slot, offset),
        }
    }
    fbb.end_table(table)
}

#[cfg(test)]
#[path = "tests/shape_payload.rs"]
pub(super) mod tests;
