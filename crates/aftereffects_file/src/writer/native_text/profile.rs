//! Independent Adobe-authored Box Text defaults. COS scalar types and class
//! header order are part of this bounded writer profile, not just its values.
//! Only explicit input slots are emitted; no user's native project is replayed.

use super::super::text_document::{TextDocumentSpec, TextDocumentTimeline};
use super::{AepWriteError, INVALID, Justification};

const ROOT: &str = include_str!("box_root.cos");
const DOCUMENT: &str = include_str!("box_document.cos");
const POINT_ROOT: &str = include_str!("point_root.cos");
const POINT_DOCUMENT: &str = include_str!("point_document.cos");
const PARAGRAPH: &str = include_str!("paragraph.cos");
const FONT: &str = " << /0 << /99 /CoolTypeFont /0 << /0 {{font}}{{format}} >> >> >>";

fn format(spec: &TextDocumentSpec) -> Vec<u8> {
    spec.font_format.map_or_else(Vec::new, |format| {
        format!(" /2 {}", format.cos_value()).into_bytes()
    })
}

pub(super) fn render(timeline: &TextDocumentTimeline) -> Result<Vec<u8>, AepWriteError> {
    let first = &timeline.keys.first().ok_or(INVALID)?.document;
    let [left, top] = first.box_position.ok_or(INVALID)?;
    let [width, height] = first.box_size.ok_or(INVALID)?;
    // Preserve the pinned rectangle's segment/control-point ordering, including
    // its closing corner; equal corners do not make arbitrary order equivalent.
    let right = left + width;
    let bottom = top + height;
    let vertices = [
        left, top, left, top, right, top, right, top, right, top, right, top, right, bottom, right,
        bottom, right, bottom, right, bottom, left, bottom, left, bottom, left, bottom, left,
        bottom, left, top, left, top,
    ];
    let mut documents = vec![b'[', b' '];
    for key in &timeline.keys {
        if key.document.font_postscript != first.font_postscript
            || key.document.font_format != first.font_format
        {
            return Err(AepWriteError::Invalid(
                "keyed box Text changes font identity",
            ));
        }
        if key.document.box_size != first.box_size
            || key.document.box_position != first.box_position
        {
            return Err(AepWriteError::Invalid(
                "keyed box Text changes box geometry",
            ));
        }
        if key.document.vertical_align != first.vertical_align {
            return Err(AepWriteError::Invalid(
                "keyed box Text changes vertical alignment",
            ));
        }
        documents.extend(document(&key.document)?);
        documents.push(b' ');
    }
    documents.push(b']');
    expand(
        ROOT,
        &[
            ("font", literal(&first.font_postscript)),
            ("format", format(first)),
            ("vertices", real_array(&vertices)),
            (
                "vertical_alignment",
                match first.vertical_align {
                    Some(fx_schema::VerticalAlign::Center) => b"/13 1".to_vec(),
                    Some(fx_schema::VerticalAlign::Bottom) => b"/13 2".to_vec(),
                    None | Some(fx_schema::VerticalAlign::Top) => Vec::new(),
                },
            ),
            ("documents", documents),
        ],
    )
}

pub(super) fn render_point(timeline: &TextDocumentTimeline) -> Result<Vec<u8>, AepWriteError> {
    let first = &timeline.keys.first().ok_or(INVALID)?.document;
    let mut font_indices = std::collections::BTreeMap::new();
    let mut additional_fonts = Vec::new();
    let mut documents = vec![b'[', b' '];
    for key in &timeline.keys {
        if key.document.box_size.is_some() || key.document.box_position.is_some() {
            return Err(AepWriteError::Invalid("Point Text has box geometry"));
        }
        let font = (
            key.document.font_postscript.as_str(),
            key.document.font_format,
        );
        let font_index = match font_indices.get(&font) {
            Some(index) => *index,
            None => {
                // Native default styles refer to slots 1/2/3; new requested faces
                // follow those defaults rather than changing their dependencies.
                let index = if font_indices.is_empty() {
                    0
                } else {
                    font_indices.len() + 3
                };
                if index != 0 {
                    additional_fonts.extend(expand(
                        FONT,
                        &[("font", literal(font.0)), ("format", format(&key.document))],
                    )?);
                }
                font_indices.insert(font, index);
                index
            }
        };
        // Automatic leading is governed by the paragraph's native 1.2 factor.
        // The inactive manual-leading slot is a native default, not font_size * 1.2.
        documents.extend(document_with_template(
            &key.document,
            POINT_DOCUMENT,
            0.01,
            font_index,
        )?);
        documents.push(b' ');
    }
    documents.push(b']');
    expand(
        POINT_ROOT,
        &[
            ("font", literal(&first.font_postscript)),
            ("format", format(first)),
            ("additional_fonts", additional_fonts),
            ("documents", documents),
        ],
    )
}

fn document(spec: &TextDocumentSpec) -> Result<Vec<u8>, AepWriteError> {
    // Automatic leading uses the paragraph's 1.2 factor. Native nonempty
    // Box Text keeps the inactive manual-leading slot at 0.01, like Point Text.
    document_with_template(spec, DOCUMENT, 0.01, 0)
}

fn document_with_template(
    spec: &TextDocumentSpec,
    template: &str,
    automatic_leading_default: f64,
    font_index: usize,
) -> Result<Vec<u8>, AepWriteError> {
    let mut text = spec.text.replace("\r\n", "\n").replace(['\r', '\n'], "\r");
    // Preserve editable trailing empty paragraphs before the native terminator.
    text.push('\r');
    let units = text.encode_utf16().count().to_string().into_bytes();
    let justification = match spec.justification {
        Justification::Left => "0",
        Justification::Right => "1",
        Justification::Center => "2",
        Justification::Justify => "3",
    };
    // Independent native Box and Point controls split paragraph runs at every
    // CR, including the terminal marker. A whole-document character run does
    // not supply these boundaries to Adobe's paragraph layout.
    let mut paragraphs = vec![b'[', b' '];
    for paragraph in text.split_inclusive('\r') {
        paragraphs.extend(expand(
            PARAGRAPH,
            &[
                (
                    "units",
                    paragraph.encode_utf16().count().to_string().into_bytes(),
                ),
                ("justification", justification.as_bytes().to_vec()),
            ],
        )?);
        paragraphs.push(b' ');
    }
    paragraphs.push(b']');
    expand(
        template,
        &[
            ("text", literal(&text)),
            ("units", units),
            ("paragraph_runs", paragraphs),
            ("font_index", font_index.to_string().into_bytes()),
            (
                "font_size",
                style_real(
                    spec.font_size,
                    "Source Text font size is not representable as a positive native f32",
                )?,
            ),
            ("auto_leading", boolean(spec.leading.is_none())),
            (
                "leading",
                style_real(
                    spec.leading.unwrap_or(automatic_leading_default),
                    "Source Text leading is not representable as a native f32",
                )?,
            ),
            // Tracking is a native integer, not a COS real. A real token is
            // interpreted as 16.16 and multiplies the visible tracking by 65536.
            ("tracking", spec.tracking.round().to_string().into_bytes()),
            ("baseline_shift", real(spec.baseline_shift)),
            (
                "all_caps",
                if spec.all_caps {
                    b"2".to_vec()
                } else {
                    b"0".to_vec()
                },
            ),
            ("fill", color(spec.fill_color)),
            (
                "stroke",
                color(spec.stroke_color.unwrap_or(spec.fill_color)),
            ),
            ("apply_fill", boolean(spec.apply_fill)),
            ("apply_stroke", boolean(spec.apply_stroke)),
            ("stroke_over_fill", boolean(spec.stroke_over_fill)),
            ("stroke_width", real(spec.stroke_width)),
        ],
    )
}

fn boolean(value: bool) -> Vec<u8> {
    if value {
        b"true".to_vec()
    } else {
        b"false".to_vec()
    }
}

fn style_real(value: f64, range_error: &'static str) -> Result<Vec<u8>, AepWriteError> {
    // Adobe stores font size and leading as single-precision scalars. Its native
    // COS uses short single-precision tokens; long f64 arithmetic tails make
    // otherwise valid Text documents unreadable (P009/P046 native controls).
    let single = value as f32;
    // Positive authored font size/manual leading must remain positive. Genuine
    // zero inactive automatic-leading defaults are not positive authored values.
    if !single.is_finite() || (value > 0.0 && single == 0.0) {
        return Err(AepWriteError::Invalid(range_error));
    }
    let mut text = single.to_string();
    if !text.contains('.') {
        text.push_str(".0");
    }
    Ok(text.into_bytes())
}

fn real(value: f64) -> Vec<u8> {
    // COS distinguishes integer and real tokens; f64::to_string alone erases
    // that distinction for integral reals. Display emits plain decimal, not E.
    let mut text = value.to_string();
    if !text.contains('.') {
        text.push_str(".0");
    }
    text.into_bytes()
}

fn real_array(values: &[f64]) -> Vec<u8> {
    let mut bytes = vec![b'[', b' '];
    for value in values {
        bytes.extend(real(*value));
        bytes.push(b' ');
    }
    bytes.push(b']');
    bytes
}

fn color([red, green, blue, alpha]: [f64; 4]) -> Vec<u8> {
    real_array(&[alpha, red, green, blue])
}

fn literal(text: &str) -> Vec<u8> {
    let mut bytes = vec![b'(', 0xfe, 0xff];
    for byte in text.encode_utf16().flat_map(u16::to_be_bytes) {
        // Literal-string line endings are normalized by COS readers. Escape
        // byte values, including those inside non-ASCII UTF-16 code units.
        match byte {
            b'\r' => bytes.extend_from_slice(b"\\r"),
            b'\n' => bytes.extend_from_slice(b"\\n"),
            b'(' | b')' | b'\\' => bytes.extend_from_slice(&[b'\\', byte]),
            _ => bytes.push(byte),
        }
    }
    bytes.push(b')');
    bytes
}

fn expand(template: &str, slots: &[(&str, Vec<u8>)]) -> Result<Vec<u8>, AepWriteError> {
    let mut output = Vec::with_capacity(template.len());
    let mut used = vec![false; slots.len()];
    let mut rest = template;
    // Scan only the template, never replacement bytes: literal input braces or
    // slot-looking user text cannot insert new native records.
    while let Some(start) = rest.find("{{") {
        output.extend_from_slice(&rest.as_bytes()[..start]);
        rest = &rest[start + 2..];
        let end = rest.find("}}").ok_or(INVALID)?;
        let (index, (_, bytes)) = slots
            .iter()
            .enumerate()
            .find(|(_, (name, _))| *name == &rest[..end])
            .ok_or(INVALID)?;
        used[index] = true;
        output.extend_from_slice(bytes);
        rest = &rest[end + 2..];
    }
    if used.iter().any(|used| !used) {
        return Err(INVALID);
    }
    output.extend_from_slice(rest.as_bytes());
    Ok(output)
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_inactive_automatic_leading_zero_remains_representable() {
        assert_eq!(
            super::style_real(0.0, "invalid native leading").unwrap(),
            b"0.0"
        );
    }
}
