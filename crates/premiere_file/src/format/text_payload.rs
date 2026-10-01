//! Premiere 26 Source Text payload (`FormattedTextData`).
//!
//! A Text component stores its document as a little-endian `u64` byte count,
//! the `0x11223344` magic, then one FlatBuffer. Premiere 26.3 and 26.5 write
//! the tables below. Older projects use UTF-16 JSON or an earlier FlatBuffer
//! revision; neither is supported. Slot meanings come from Premiere 26
//! projects and from renders of generated payloads in Premiere 26. Any other
//! present slot fails closed, so unmodeled styling cannot disappear silently.
//!
//! Adobe publishes no schema for these tables, so the `flatbuffers` crate
//! cannot generate a verifier, and its untyped `Table` access is `unsafe`.
//! The reader is therefore a small bounds-checked decoder that can also list
//! every present slot. The writer uses `FlatBufferBuilder`, which owns layout
//! and alignment.

use crate::error::{ensure, unsupported, Result};
use crate::schema::text::{
    normalize_line_breaks, PrJustification, PrRgb, PrTextBackground, PrTextDocument, PrTextFrame,
    PrTextStroke, PrVerticalAlign,
};
use flatbuffers::{FlatBufferBuilder, VOffsetT, WIPOffset};

mod shadow;

const MAGIC: u32 = 0x1122_3344;
pub(super) const HEADER_BYTES: usize = 12;

/// Document table slots.
mod document {
    pub(super) const RUNS: usize = 0;
    pub(super) const FONTS: usize = 1;
    pub(super) const BOX_WIDTH: usize = 2;
    pub(super) const BOX_HEIGHT: usize = 3;
    pub(super) const JUSTIFICATION: usize = 4;
    pub(super) const BOX_ALIGNMENT: usize = 5;
    pub(super) const LEADING: usize = 6;
    pub(super) const SHADOW_COLOR: usize = 10;
    pub(super) const SHADOW_ENABLED: usize = 11;
    /// Shadow opacity, angle, distance, size, and blur.
    pub(super) const SHADOW_VALUES: [usize; 5] = [12, 13, 14, 15, 16];
    pub(super) const BACKGROUND_COLOR: usize = 17;
    pub(super) const BACKGROUND_ENABLED: usize = 18;
    /// Background opacity, size, and corner radius.
    pub(super) const BACKGROUND_VALUES: [usize; 3] = [19, 20, 34];
    /// The table that moves a caption off its default position. AME renders
    /// of Premiere 26.5.1 caption payloads measured its sub-slots as linear
    /// offsets of the frame size, but not the base point of the placement,
    /// so a payload that stores it fails closed.
    pub(super) const POSITION: usize = 33;
    /// Unnamed slots that every Premiere 26 graphic writes with these values.
    pub(super) const FIXED_FLAGS: [(usize, u8); 3] = [(26, 1), (43, 0), (44, 1)];
    /// A caption block's `FormattedTextData` writes slot 38 instead of slot 26
    /// (all 42 cues and the track style of the caption sequence in the
    /// `practice_files_transcription_magic` corpus project, last saved by
    /// Premiere 25.5, project Version 43, and the 10 payloads of the
    /// `feature_caption_styles_26_5_strict` fixture as Premiere 26.5.1 saved
    /// them).
    pub(super) const CAPTION_FIXED_FLAGS: [(usize, u8); 3] = [(38, 1), (43, 0), (44, 1)];
    /// The release whose graphics showed `FIXED_FLAGS`, named in rejections.
    pub(super) const GRAPHIC_LAYOUT: &str = "Premiere 26";
    /// The release whose caption blocks first showed `CAPTION_FIXED_FLAGS`.
    pub(super) const CAPTION_LAYOUT: &str = "Premiere 25.5 caption";
    pub(super) const FIXED_EMPTY_TABLE: usize = 40;
}

/// Run table slots.
mod run {
    pub(super) const TEXT: usize = 0;
    pub(super) const STYLE: usize = 1;
}

/// Style table slots.
mod style {
    pub(super) const FONT_INDEX: usize = 0;
    pub(super) const SIZE: usize = 1;
    pub(super) const FILL_COLOR: usize = 2;
    pub(super) const FILL_ENABLED: usize = 3;
    pub(super) const STROKE_COLOR: usize = 4;
    pub(super) const STROKE_ENABLED: usize = 5;
    pub(super) const STROKE_WIDTH: usize = 6;
    pub(super) const TRACKING: usize = 8;
    pub(super) const CAPS: usize = 12;
    pub(super) const FIXED_EMPTY_TABLES: [usize; 2] = [21, 23];
    /// Unnamed slot that every Premiere 26 run writes as 2.
    pub(super) const FIXED_VALUE: (usize, u32) = (24, 2);
    /// Premiere omits a 100 px size, and renders an omitted size at 100 px
    /// (Premiere 26.5.1 renders of generated graphics).
    pub(super) const DEFAULT_SIZE: f32 = 100.0;
    /// Premiere stores this width even when the stroke is disabled.
    pub(super) const DEFAULT_STROKE_WIDTH: f32 = 4.0;
    pub(super) const ALL_CAPS: u32 = 2;
}

/// Absent color components read as 255; Premiere always writes all three.
const COLOR_COMPONENT_DEFAULT: u8 = u8::MAX;
/// Premiere omits a white fill; an absent fill color renders white.
const DEFAULT_FILL: PrRgb = PrRgb([COLOR_COMPONENT_DEFAULT; 3]);

/// Premiere text features that decode but that a Type-tool graphic does not
/// retain: its background is calibrated for caption cues only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OmittedTextFeature {
    Background,
}

impl std::fmt::Display for OmittedTextFeature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Background => "text background (JRB-1995)",
        })
    }
}

/// A decoded Source Text value and the enabled features it could not keep.
#[derive(Debug)]
pub(crate) struct DecodedText {
    pub(crate) document: PrTextDocument,
    pub(crate) omitted: Vec<OmittedTextFeature>,
    /// The stored alignment, also used for a caption's default placement.
    pub(crate) box_alignment: PrVerticalAlign,
}

/// FX supports a uniform document or independent complete lines, not inline spans.
#[derive(Debug)]
pub(crate) enum TextDocuments {
    Uniform(PrTextDocument),
    Lines(Vec<PrTextDocument>),
}

#[derive(Debug)]
pub(crate) struct DecodedGraphicText {
    pub(crate) documents: TextDocuments,
    pub(crate) omitted: Vec<OmittedTextFeature>,
    box_alignment: PrVerticalAlign,
}

impl DecodedGraphicText {
    fn uniform(self) -> Result<DecodedText> {
        let TextDocuments::Uniform(document) = self.documents else {
            return Err(unsupported(
                "mixed text styles are unsupported in captions or keyed Source Text",
            ));
        };
        Ok(DecodedText {
            document,
            omitted: self.omitted,
            box_alignment: self.box_alignment,
        })
    }
}

/// Decode one Premiere 26 Source Text value. An enabled background is
/// reported as omitted, not kept: no render calibrated a Type-tool text's box.
///
/// # Errors
/// Rejects legacy encodings, malformed buffers, mixed run styles, and slots
/// whose meaning is unknown. `PrText::validate` checks the value ranges.
pub(crate) fn decode(payload: &[u8]) -> Result<DecodedText> {
    decode_graphic(payload)?.uniform()
}

pub(crate) fn decode_graphic(payload: &[u8]) -> Result<DecodedGraphicText> {
    let mut decoded =
        decode_with_markers(payload, &document::FIXED_FLAGS, document::GRAPHIC_LAYOUT)?;
    let documents = match &mut decoded.documents {
        TextDocuments::Uniform(document) => std::slice::from_mut(document),
        TextDocuments::Lines(lines) => lines.as_mut_slice(),
    };
    let mut background = false;
    for document in documents {
        background |= document.background.take().is_some();
    }
    if background && !decoded.omitted.contains(&OmittedTextFeature::Background) {
        decoded.omitted.push(OmittedTextFeature::Background);
    }
    Ok(decoded)
}

/// Decode one caption block's `FormattedTextData`: the Source Text encoding
/// with the caption document markers seen in Premiere 25.5 and 26.5.1. An
/// enabled background is kept in the document.
///
/// # Errors
/// Rejects everything [`decode`] rejects.
pub(crate) fn decode_caption(payload: &[u8]) -> Result<DecodedText> {
    decode_with_markers(
        payload,
        &document::CAPTION_FIXED_FLAGS,
        document::CAPTION_LAYOUT,
    )?
    .uniform()
}

fn decode_with_markers(
    payload: &[u8],
    markers: &[(usize, u8)],
    layout: &str,
) -> Result<DecodedGraphicText> {
    ensure!(
        payload.get(8..10) != Some(b"{\0".as_slice()),
        "legacy UTF-16 JSON text from Premiere before 26 is unsupported"
    );
    let buffer = Buffer::from_payload(payload, SOURCE_TEXT)?;
    let root = buffer.table(buffer.offset(0)?)?;
    root.allow_only(&[0], "root")?;
    let doc = root
        .table(0)?
        .ok_or_else(|| unsupported("Source Text has no document"))?;
    decode_document(doc, markers, layout)
}

fn decode_document(
    doc: Table<'_>,
    markers: &[(usize, u8)],
    layout: &str,
) -> Result<DecodedGraphicText> {
    use document::*;
    let mut allowed = vec![
        RUNS,
        FONTS,
        BOX_WIDTH,
        BOX_HEIGHT,
        JUSTIFICATION,
        BOX_ALIGNMENT,
        LEADING,
        SHADOW_COLOR,
        SHADOW_ENABLED,
        BACKGROUND_COLOR,
        BACKGROUND_ENABLED,
        FIXED_EMPTY_TABLE,
    ];
    allowed.extend(SHADOW_VALUES);
    allowed.extend(BACKGROUND_VALUES);
    allowed.extend(markers.iter().map(|&(slot, _)| slot));
    ensure!(
        doc.table(POSITION)?.is_none(),
        "positioned {layout} text (document slot {POSITION}) is unsupported: its base point is unverified"
    );
    doc.allow_only(&allowed, "document")?;
    for &(slot, expected) in markers {
        ensure!(
            doc.u8(slot)? == Some(expected),
            "Source Text lacks the {layout} document marker in slot {slot}"
        );
    }
    doc.table(FIXED_EMPTY_TABLE)?
        .ok_or_else(|| unsupported(format!("Source Text lacks its {layout} document table")))?
        .allow_only(&[], "document[40]")?;
    for slot in SHADOW_VALUES.into_iter().chain(BACKGROUND_VALUES) {
        doc.f32(slot)?;
    }
    for slot in [SHADOW_COLOR, BACKGROUND_COLOR] {
        if let Some(table) = doc.table(slot)? {
            color(table)?;
        }
    }
    let shadow = shadow::decode(doc)?;
    let mut omitted = Vec::new();
    let background = match doc.u8(BACKGROUND_ENABLED)?.unwrap_or(0) {
        0 => None,
        1 => {
            let [opacity, size, radius] = BACKGROUND_VALUES;
            match (
                doc.table(BACKGROUND_COLOR)?.map(color).transpose()?,
                doc.f32(opacity)?,
                doc.f32(size)?,
                doc.f32(radius)?,
            ) {
                (Some(color), Some(opacity), Some(size), Some(radius)) => Some(PrTextBackground {
                    color,
                    opacity,
                    size,
                    radius,
                }),
                // Every calibrated payload stores all five slots. A Premiere
                // 25.5 caption style preview omits its opacity and radius,
                // and what Premiere draws for an omitted slot is unverified.
                _ => {
                    omitted.push(OmittedTextFeature::Background);
                    None
                }
            }
        }
        other => return Err(unsupported(format!("invalid text background flag {other}"))),
    };

    let fonts = doc.strings(FONTS)?;
    let runs = doc.tables(RUNS)?;
    ensure!(!runs.is_empty(), "Source Text has no text runs");
    // Payload sizes are untrusted, so an allocation failure is an error
    // rather than an abort.
    let allocation = |error: std::collections::TryReserveError| {
        unsupported(format!("cannot allocate Source Text runs: {error}"))
    };
    let mut styled_runs: Vec<(String, RunStyle<'_>)> = Vec::new();
    styled_runs.try_reserve(runs.len()).map_err(allocation)?;
    let mut previous_ended_cr = false;
    for run_table in runs {
        run_table.allow_only(&[run::TEXT, run::STYLE], "run")?;
        let raw_text = run_table.string(run::TEXT)?.unwrap_or_default();
        // A CRLF can straddle native style runs. Its LF continues the prior
        // paragraph break; it must not create an empty line of the new style.
        let run_text = if previous_ended_cr {
            raw_text.strip_prefix('\n').unwrap_or(raw_text)
        } else {
            raw_text
        };
        if !raw_text.is_empty() {
            previous_ended_cr = raw_text.ends_with('\r');
        }
        let style = run_style(
            run_table
                .table(run::STYLE)?
                .ok_or_else(|| unsupported("text run has no style"))?,
            &fonts,
            layout,
        )?;
        match styled_runs.last_mut() {
            Some((text, previous)) if *previous == style => {
                text.try_reserve(run_text.len()).map_err(allocation)?;
                text.push_str(run_text);
            }
            _ => {
                let mut text = String::new();
                text.try_reserve(run_text.len()).map_err(allocation)?;
                text.push_str(run_text);
                styled_runs.push((text, style));
            }
        }
    }

    let justification = match doc.u32(JUSTIFICATION)?.unwrap_or(0) {
        0 => PrJustification::Left,
        1 => PrJustification::Right,
        2 => PrJustification::Center,
        3 => PrJustification::Justify,
        other => {
            return Err(unsupported(format!(
                "paragraph justification {other} (full justification without a left last line) is unsupported"
            )))
        }
    };
    let vertical = match doc.u32(BOX_ALIGNMENT)?.unwrap_or(0) {
        0 => PrVerticalAlign::Top,
        1 => PrVerticalAlign::Center,
        2 => PrVerticalAlign::Bottom,
        other => return Err(unsupported(format!("unknown box alignment {other}"))),
    };
    let frame = match (doc.f32(BOX_WIDTH)?, doc.f32(BOX_HEIGHT)?) {
        (None, None) => PrTextFrame::Point { vertical },
        (Some(width), Some(height)) => PrTextFrame::Box {
            width,
            height,
            vertical,
        },
        _ => return Err(unsupported("text box has only one dimension")),
    };
    let leading = doc.f32(LEADING)?.unwrap_or(0.0);
    let make_document = |text: String, style: &RunStyle<'_>| PrTextDocument {
        text,
        font: style.font.to_owned(),
        size: style.size,
        fill: style.fill,
        stroke: style.stroke,
        shadow,
        all_caps: style.all_caps,
        tracking: style.tracking,
        leading,
        justification,
        frame,
        background,
    };
    let documents = if let [(text, style)] = styled_runs.as_slice() {
        TextDocuments::Uniform(make_document(normalize_line_breaks(text), style))
    } else {
        ensure!(
            matches!(frame, PrTextFrame::Point { .. }),
            "mixed text styles in box text are unsupported"
        );
        ensure!(
            leading == 0.0,
            "mixed text styles with explicit native leading are unsupported"
        );
        ensure!(
            justification != PrJustification::Justify,
            "mixed text styles with full justification are unsupported"
        );
        let mut lines = Vec::new();
        for (index, (text, style)) in styled_runs.iter().enumerate() {
            let normalized = normalize_line_breaks(text);
            let complete = if index + 1 < styled_runs.len() {
                normalized.strip_suffix('\n').ok_or_else(|| {
                    unsupported(
                        "mixed text styles within a line are unsupported; style changes must start a complete line",
                    )
                })?
            } else {
                &normalized
            };
            for line in complete.split('\n') {
                lines.push(make_document(line.to_owned(), style));
            }
        }
        TextDocuments::Lines(lines)
    };
    Ok(DecodedGraphicText {
        documents,
        omitted,
        box_alignment: vertical,
    })
}

#[derive(Debug, PartialEq)]
struct RunStyle<'a> {
    font: &'a str,
    size: f32,
    fill: Option<PrRgb>,
    stroke: Option<PrTextStroke>,
    all_caps: bool,
    tracking: f32,
}

fn run_style<'a>(table: Table<'_>, fonts: &[&'a str], layout: &str) -> Result<RunStyle<'a>> {
    use style::*;
    let mut allowed = vec![
        FONT_INDEX,
        SIZE,
        FILL_COLOR,
        FILL_ENABLED,
        STROKE_COLOR,
        STROKE_ENABLED,
        STROKE_WIDTH,
        TRACKING,
        CAPS,
        FIXED_VALUE.0,
    ];
    allowed.extend(FIXED_EMPTY_TABLES);
    table.allow_only(&allowed, "text style")?;
    ensure!(
        table.u32(FIXED_VALUE.0)? == Some(FIXED_VALUE.1),
        "text style lacks the {layout} run marker"
    );
    for slot in FIXED_EMPTY_TABLES {
        if let Some(empty) = table.table(slot)? {
            empty.allow_only(&[], "text style table")?;
        }
    }
    let font_index = usize::try_from(table.u32(FONT_INDEX)?.unwrap_or(0))
        .map_err(|_| unsupported("text font index overflows"))?;
    let font = fonts
        .get(font_index)
        .copied()
        .filter(|font| !font.is_empty())
        .ok_or_else(|| unsupported("text run references a missing font"))?;
    let size = table.f32(SIZE)?.unwrap_or(DEFAULT_SIZE);
    let fill_color = table.table(FILL_COLOR)?.map(color).transpose()?;
    let fill = match table.u8(FILL_ENABLED)?.unwrap_or(1) {
        0 => None,
        1 => Some(fill_color.unwrap_or(DEFAULT_FILL)),
        other => return Err(unsupported(format!("invalid fill flag {other}"))),
    };
    let stroke_color = table.table(STROKE_COLOR)?.map(color).transpose()?;
    let stroke_width = table.f32(STROKE_WIDTH)?;
    let stroke = match table.u8(STROKE_ENABLED)?.unwrap_or(0) {
        0 => None,
        1 => {
            let (Some(color), Some(width)) = (stroke_color, stroke_width) else {
                return Err(unsupported("enabled text stroke lacks a color or width"));
            };
            Some(PrTextStroke { color, width })
        }
        other => return Err(unsupported(format!("invalid stroke flag {other}"))),
    };
    let all_caps = match table.u32(CAPS)?.unwrap_or(0) {
        0 => false,
        ALL_CAPS => true,
        other => {
            return Err(unsupported(format!(
                "text caps option {other} is unsupported"
            )))
        }
    };
    let tracking = table.f32(TRACKING)?.unwrap_or(0.0);
    Ok(RunStyle {
        font,
        size,
        fill,
        stroke,
        all_caps,
        tracking,
    })
}

fn color(table: Table<'_>) -> Result<PrRgb> {
    table.allow_only(&[0, 1, 2], "color")?;
    let mut rgb = [COLOR_COMPONENT_DEFAULT; 3];
    for (slot, component) in rgb.iter_mut().enumerate() {
        if let Some(value) = table.u8(slot)? {
            *component = value;
        }
    }
    Ok(PrRgb(rgb))
}

// Conservative bound for this fixed encoder's non-string bytes: at most 11
// tables with 45 slots (4-byte values + 2-byte vtable offsets, 8 header bytes
// and 3 alignment bytes), two one-entry vectors, two string headers/terminators
// and the root offset. Keep this in sync with encode's fixed table layout.
const TEXT_FIXED_BUFFER_BOUND: usize =
    (9 + style::FIXED_EMPTY_TABLES.len()) * (45 * 6 + 8 + 3) + 2 * 11 + 2 * 8 + 7;

fn validate_text_buffer_lengths(text: usize, font: usize) -> crate::format::Result<()> {
    let bound = text
        .checked_add(font)
        .and_then(|bytes| bytes.checked_add(TEXT_FIXED_BUFFER_BOUND));
    crate::format::ensure_valid!(
        bound.is_some_and(|bytes| bytes <= flatbuffers::FLATBUFFERS_MAX_BUFFER_SIZE),
        "Source Text exceeds the FlatBuffer builder's signed-offset representation"
    );
    Ok(())
}

/// Encode a document using Premiere's Source Text framing.
pub(crate) fn encode(doc: &PrTextDocument) -> crate::format::Result<Vec<u8>> {
    validate_text_buffer_lengths(doc.text.len(), doc.font.len())?;
    let mut fbb = FlatBufferBuilder::with_capacity(512);
    let fill_color = doc
        .fill
        .filter(|rgb| *rgb != DEFAULT_FILL)
        .map(|rgb| color_table(&mut fbb, rgb));
    let stroke_color = doc.stroke.map(|stroke| color_table(&mut fbb, stroke.color));
    let style_tables = style::FIXED_EMPTY_TABLES.map(|_| empty_table(&mut fbb));
    let style_table = {
        let table = fbb.start_table();
        fbb.push_slot(slot(style::SIZE), doc.size, style::DEFAULT_SIZE);
        if doc.fill.is_none() {
            fbb.push_slot_always(slot(style::FILL_ENABLED), 0_u8);
        } else if let Some(color) = fill_color {
            fbb.push_slot_always(slot(style::FILL_COLOR), color);
        }
        if let (Some(stroke), Some(color)) = (doc.stroke, stroke_color) {
            fbb.push_slot_always(slot(style::STROKE_COLOR), color);
            fbb.push_slot_always(slot(style::STROKE_ENABLED), 1_u8);
            fbb.push_slot_always(slot(style::STROKE_WIDTH), stroke.width);
        } else {
            fbb.push_slot_always(slot(style::STROKE_WIDTH), style::DEFAULT_STROKE_WIDTH);
        }
        fbb.push_slot(slot(style::TRACKING), doc.tracking, 0.0);
        fbb.push_slot(
            slot(style::CAPS),
            if doc.all_caps { style::ALL_CAPS } else { 0 },
            0,
        );
        for (index, table) in style::FIXED_EMPTY_TABLES.into_iter().zip(style_tables) {
            fbb.push_slot_always(slot(index), table);
        }
        fbb.push_slot_always(slot(style::FIXED_VALUE.0), style::FIXED_VALUE.1);
        fbb.end_table(table)
    };
    let text = fbb.create_string(&doc.text.replace('\n', "\r"));
    let run_table = {
        let table = fbb.start_table();
        fbb.push_slot_always(slot(run::TEXT), text);
        fbb.push_slot_always(slot(run::STYLE), style_table);
        fbb.end_table(table)
    };
    let runs = fbb.create_vector(&[run_table]);
    let font = fbb.create_string(&doc.font);
    let fonts = fbb.create_vector(&[font]);
    let shadow_color = shadow::color_offset(&mut fbb, doc.shadow)?;
    let background_color = doc
        .background
        .map(|background| color_table(&mut fbb, background.color));
    let document_table = empty_table(&mut fbb);
    let document = {
        let table = fbb.start_table();
        fbb.push_slot_always(slot(document::RUNS), runs);
        fbb.push_slot_always(slot(document::FONTS), fonts);
        let vertical = match doc.frame {
            PrTextFrame::Point { vertical } => vertical,
            PrTextFrame::Box {
                width,
                height,
                vertical,
            } => {
                fbb.push_slot_always(slot(document::BOX_WIDTH), width);
                fbb.push_slot_always(slot(document::BOX_HEIGHT), height);
                vertical
            }
        };
        let vertical: u32 = match vertical {
            PrVerticalAlign::Top => 0,
            PrVerticalAlign::Center => 1,
            PrVerticalAlign::Bottom => 2,
        };
        fbb.push_slot(slot(document::BOX_ALIGNMENT), vertical, 0);
        let justification: u32 = match doc.justification {
            PrJustification::Left => 0,
            PrJustification::Right => 1,
            PrJustification::Center => 2,
            PrJustification::Justify => 3,
        };
        fbb.push_slot(slot(document::JUSTIFICATION), justification, 0);
        fbb.push_slot(slot(document::LEADING), doc.leading, 0.0);
        shadow::push(&mut fbb, doc.shadow, shadow_color);
        // Every background slot is written, as for the shadow, so that
        // Premiere never reads a default in place of an exported value.
        if let (Some(background), Some(color)) = (doc.background, background_color) {
            fbb.push_slot_always(slot(document::BACKGROUND_COLOR), color);
            fbb.push_slot_always(slot(document::BACKGROUND_ENABLED), 1_u8);
            let values = [background.opacity, background.size, background.radius];
            for (index, value) in document::BACKGROUND_VALUES.into_iter().zip(values) {
                fbb.push_slot_always(slot(index), value);
            }
        }
        for (index, value) in document::FIXED_FLAGS {
            fbb.push_slot_always(slot(index), value);
        }
        fbb.push_slot_always(slot(document::FIXED_EMPTY_TABLE), document_table);
        fbb.end_table(table)
    };
    let root = {
        let table = fbb.start_table();
        fbb.push_slot_always(slot(0), document);
        fbb.end_table(table)
    };
    fbb.finish_minimal(root);
    framed(fbb.finished_data(), SOURCE_TEXT)
}

/// The payload named `name` that holds `buffer`: its byte count, the magic,
/// then the buffer.
pub(super) fn framed(buffer: &[u8], name: &str) -> crate::format::Result<Vec<u8>> {
    let length = HEADER_BYTES
        .checked_add(buffer.len())
        .ok_or_else(|| crate::format::invalid(format!("{name} payload size overflows")))?;
    let mut payload = Vec::new();
    payload
        .try_reserve_exact(length)
        .map_err(|error| crate::format::invalid(format!("cannot allocate {name}: {error}")))?;
    payload.extend_from_slice(&(buffer.len() as u64).to_le_bytes());
    payload.extend_from_slice(&MAGIC.to_le_bytes());
    payload.extend_from_slice(buffer);
    Ok(payload)
}

/// How messages name a Source Text payload.
const SOURCE_TEXT: &str = "Source Text";

pub(super) type TableOffset = WIPOffset<flatbuffers::TableFinishedWIPOffset>;

pub(super) fn slot(index: usize) -> VOffsetT {
    VOffsetT::try_from(4 + 2 * index).expect("payload slots fit a vtable")
}

pub(super) fn color_table(fbb: &mut FlatBufferBuilder<'_>, PrRgb(rgb): PrRgb) -> TableOffset {
    let table = fbb.start_table();
    for (index, component) in rgb.into_iter().enumerate() {
        fbb.push_slot_always(slot(index), component);
    }
    fbb.end_table(table)
}

pub(super) fn empty_table(fbb: &mut FlatBufferBuilder<'_>) -> TableOffset {
    let table = fbb.start_table();
    fbb.end_table(table)
}

/// Bounds-checked little-endian access to one FlatBuffer, whose messages
/// name its payload.
#[derive(Clone, Copy)]
pub(super) struct Buffer<'a> {
    bytes: &'a [u8],
    name: &'a str,
}

impl<'a> Buffer<'a> {
    /// The FlatBuffer of the payload named `name`, after its byte count and
    /// magic.
    pub(super) fn from_payload(payload: &'a [u8], name: &'a str) -> Result<Self> {
        ensure!(payload.len() > HEADER_BYTES, "truncated {name} payload");
        let (declared, rest) = payload.split_at(8);
        let (magic, bytes) = rest.split_at(4);
        ensure!(
            u32::from_le_bytes(magic.try_into().expect("four magic bytes")) == MAGIC,
            "unknown {name} encoding"
        );
        ensure!(
            u64::from_le_bytes(declared.try_into().expect("eight length bytes"))
                == bytes.len() as u64,
            "{name} length prefix does not match its payload"
        );
        Ok(Self { bytes, name })
    }

    fn bytes<const N: usize>(self, at: usize) -> Result<[u8; N]> {
        at.checked_add(N)
            .and_then(|end| self.bytes.get(at..end))
            .map(|bytes| bytes.try_into().expect("slice has N bytes"))
            .ok_or_else(|| unsupported(format!("{} offset is out of bounds", self.name)))
    }

    fn u16(self, at: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(at)?))
    }

    fn u32(self, at: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(at)?))
    }

    /// Follow the forward `uoffset` stored at `at`.
    pub(super) fn offset(self, at: usize) -> Result<usize> {
        let relative = usize::try_from(self.u32(at)?)
            .map_err(|_| unsupported(format!("{} offset overflows", self.name)))?;
        at.checked_add(relative)
            .filter(|target| *target < self.bytes.len())
            .ok_or_else(|| unsupported(format!("{} offset is out of bounds", self.name)))
    }

    pub(super) fn table(self, position: usize) -> Result<Table<'a>> {
        let relative = i64::from(i32::from_le_bytes(self.bytes(position)?));
        let vtable = usize::try_from(position as i64 - relative)
            .map_err(|_| unsupported(format!("{} vtable is out of bounds", self.name)))?;
        let vtable_bytes = usize::from(self.u16(vtable)?);
        let inline_bytes = usize::from(self.u16(vtable + 2)?);
        ensure!(
            vtable_bytes >= 4
                && vtable_bytes % 2 == 0
                && vtable
                    .checked_add(vtable_bytes)
                    .is_some_and(|end| end <= self.bytes.len())
                && inline_bytes >= 4
                && position
                    .checked_add(inline_bytes)
                    .is_some_and(|end| end <= self.bytes.len()),
            "malformed {} table",
            self.name
        );
        Ok(Table {
            buffer: self,
            position,
            vtable,
            slots: (vtable_bytes - 4) / 2,
            inline_bytes,
        })
    }

    fn string(self, position: usize) -> Result<&'a str> {
        let name = self.name;
        let length = usize::try_from(self.u32(position)?)
            .map_err(|_| unsupported(format!("{name} string overflows")))?;
        let start = position + 4;
        let end = start
            .checked_add(length)
            .filter(|end| *end < self.bytes.len())
            .ok_or_else(|| unsupported(format!("{name} string is out of bounds")))?;
        ensure!(self.bytes[end] == 0, "{name} string is not NUL-terminated");
        std::str::from_utf8(&self.bytes[start..end])
            .map_err(|_| unsupported(format!("{name} string is not UTF-8")))
    }

    /// Positions of the `uoffset` elements in the vector at `position`.
    fn vector(self, position: usize) -> Result<impl Iterator<Item = usize>> {
        let name = self.name;
        let count = usize::try_from(self.u32(position)?)
            .map_err(|_| unsupported(format!("{name} vector overflows")))?;
        ensure!(
            count
                .checked_mul(4)
                .and_then(|bytes| bytes.checked_add(position + 4))
                .is_some_and(|end| end <= self.bytes.len()),
            "{name} vector is out of bounds"
        );
        Ok((0..count).map(move |index| position + 4 + index * 4))
    }
}

/// One FlatBuffer table and its vtable.
#[derive(Clone, Copy)]
pub(super) struct Table<'a> {
    buffer: Buffer<'a>,
    position: usize,
    vtable: usize,
    slots: usize,
    inline_bytes: usize,
}

impl<'a> Table<'a> {
    /// The absolute position of a present field of `width` bytes.
    fn field(self, slot: usize, width: usize) -> Result<Option<usize>> {
        if slot >= self.slots {
            return Ok(None);
        }
        let relative = usize::from(self.buffer.u16(self.vtable + 4 + slot * 2)?);
        if relative == 0 {
            return Ok(None);
        }
        ensure!(
            relative + width <= self.inline_bytes,
            "{} field is outside its table",
            self.buffer.name
        );
        Ok(Some(self.position + relative))
    }

    pub(super) fn present(self) -> Result<Vec<usize>> {
        (0..self.slots)
            .filter_map(|slot| match self.buffer.u16(self.vtable + 4 + slot * 2) {
                Ok(0) => None,
                Ok(_) => Some(Ok(slot)),
                Err(error) => Some(Err(error)),
            })
            .collect()
    }

    pub(super) fn allow_only(self, allowed: &[usize], table: &str) -> Result<()> {
        if let Some(slot) = self
            .present()?
            .into_iter()
            .find(|slot| !allowed.contains(slot))
        {
            return Err(unsupported(format!(
                "unsupported {} field {table}[{slot}]",
                self.buffer.name
            )));
        }
        Ok(())
    }

    pub(super) fn u8(self, slot: usize) -> Result<Option<u8>> {
        self.field(slot, 1)?
            .map(|at| self.buffer.bytes::<1>(at).map(|[value]| value))
            .transpose()
    }

    pub(super) fn u32(self, slot: usize) -> Result<Option<u32>> {
        self.field(slot, 4)?
            .map(|at| self.buffer.u32(at))
            .transpose()
    }

    pub(super) fn f32(self, slot: usize) -> Result<Option<f32>> {
        Ok(self.u32(slot)?.map(f32::from_bits))
    }

    fn target(self, slot: usize) -> Result<Option<usize>> {
        self.field(slot, 4)?
            .map(|at| self.buffer.offset(at))
            .transpose()
    }

    pub(super) fn table(self, slot: usize) -> Result<Option<Table<'a>>> {
        self.target(slot)?
            .map(|position| self.buffer.table(position))
            .transpose()
    }

    fn string(self, slot: usize) -> Result<Option<&'a str>> {
        self.target(slot)?
            .map(|position| self.buffer.string(position))
            .transpose()
    }

    pub(super) fn tables(self, slot: usize) -> Result<Vec<Table<'a>>> {
        let Some(vector) = self.target(slot)? else {
            return Ok(Vec::new());
        };
        self.buffer
            .vector(vector)?
            .map(|element| self.buffer.table(self.buffer.offset(element)?))
            .collect()
    }

    fn strings(self, slot: usize) -> Result<Vec<&'a str>> {
        let Some(vector) = self.target(slot)? else {
            return Ok(Vec::new());
        };
        self.buffer
            .vector(vector)?
            .map(|element| self.buffer.string(self.buffer.offset(element)?))
            .collect()
    }
}

#[cfg(test)]
#[path = "tests/text_payload.rs"]
pub(super) mod tests;
