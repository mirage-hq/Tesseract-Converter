//! Fresh COS Source Text documents for the native Text writer.
//!
//! The numeric dictionary paths are the inverse of the bounded native reader in
//! `structure_document::text`.  This module accepts typed values only: imported
//! `btdk` payloads are never retained or replayed.

use std::collections::BTreeMap;

use fx_schema::Justification;

use crate::{rifx::Chunk, schema::view_records::StaticPropertyRecord};

use super::{AepWriteError, views};

/// One whole-layer native Source Text document.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextDocumentSpec {
    pub text: String,
    pub font_postscript: String,
    pub font_size: f64,
    pub apply_fill: bool,
    pub fill_color: [f64; 4],
    pub apply_stroke: bool,
    pub stroke_color: Option<[f64; 4]>,
    pub stroke_width: f64,
    pub stroke_over_fill: bool,
    pub justification: Justification,
    pub tracking: f64,
    /// `None` preserves AE auto-leading; `Some` is an explicit pixel value.
    pub leading: Option<f64>,
    pub baseline_shift: f64,
    pub box_size: Option<[f64; 2]>,
    pub box_position: Option<[f64; 2]>,
    pub all_caps: bool,
}

/// One authored Source Text document event on the owning Text layer's local clock.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextDocumentKey {
    pub time_millis: i64,
    pub document: TextDocumentSpec,
}

/// Static Source Text or an authored native Hold document timeline.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextDocumentTimeline {
    pub keyed: bool,
    pub keys: Vec<TextDocumentKey>,
}

impl TextDocumentTimeline {
    pub(super) fn validate(&self) -> Result<(), AepWriteError> {
        self.validate_with_clock(super::keyframes::PropertyClock::DEFAULT)
    }

    pub(super) fn validate_with_clock(
        &self,
        clock: super::keyframes::PropertyClock,
    ) -> Result<(), AepWriteError> {
        if self.keys.is_empty() || (self.keyed && u16::try_from(self.keys.len()).is_err()) {
            return Err(AepWriteError::Invalid(
                "Source Text document count exceeds the native key field",
            ));
        }
        if !self.keyed && (self.keys.len() != 1 || self.keys[0].time_millis != 0) {
            return Err(AepWriteError::Invalid(
                "static Source Text needs one zero-time document",
            ));
        }
        let mut previous = None;
        for key in &self.keys {
            key.document.validate()?;
            let units = clock.units(key.time_millis)?;
            if previous.is_some_and(|value| value >= units) {
                return Err(AepWriteError::Invalid(
                    "Source Text key times are not strictly increasing",
                ));
            }
            previous = Some(units);
        }
        Ok(())
    }
}

impl TextDocumentSpec {
    pub(super) fn validate(&self) -> Result<(), AepWriteError> {
        if self.text.contains('\0') {
            return Err(AepWriteError::Invalid("Source Text contains NUL"));
        }
        if self.font_postscript.is_empty() || self.font_postscript.contains('\0') {
            return Err(AepWriteError::Invalid(
                "invalid Source Text PostScript font identity",
            ));
        }
        if !self.font_size.is_finite()
            || self.font_size <= 0.0
            || !self.stroke_width.is_finite()
            || self.stroke_width < 0.0
            || !self.tracking.is_finite()
            || !self.baseline_shift.is_finite()
            || self
                .leading
                .is_some_and(|value| !value.is_finite() || value <= 0.0)
            || (self.leading.is_none() && !(self.font_size * 1.2).is_finite())
            || self
                .fill_color
                .iter()
                .chain(self.stroke_color.iter().flatten())
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
        {
            return Err(AepWriteError::Invalid("invalid Source Text style value"));
        }
        match (self.box_size, self.box_position) {
            (None, None) => {}
            (Some(size), Some(position))
                if size
                    .into_iter()
                    .all(|value| value.is_finite() && value > 0.0)
                    && position.into_iter().all(f64::is_finite)
                    && position
                        .into_iter()
                        .zip(size)
                        .all(|(origin, extent)| (origin + extent).is_finite()) => {}
            _ => {
                return Err(AepWriteError::Invalid(
                    "box Source Text needs finite positive size and finite position",
                ));
            }
        }
        if self.apply_stroke && self.stroke_color.is_none() {
            return Err(AepWriteError::Invalid(
                "enabled Source Text stroke has no color",
            ));
        }
        Ok(())
    }
}

/// Builds the typed `btds` property value used by `ADBE Text Document`.
#[cfg(test)]
pub(super) fn source_property(timeline: &TextDocumentTimeline) -> Result<Chunk, AepWriteError> {
    source_property_with_clock(timeline, super::keyframes::PropertyClock::DEFAULT)
}

pub(super) fn source_property_with_clock(
    timeline: &TextDocumentTimeline,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    timeline.validate_with_clock(clock)?;
    Ok(Chunk::list(
        *b"btds",
        vec![
            source_metadata(timeline, clock)?,
            Chunk::opaque_list(*b"btdk", encode_cos(timeline)),
        ],
    ))
}

fn source_metadata(
    timeline: &TextDocumentTimeline,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    // This is the inverse of the pinned AE26 Source Text decoder: the static
    // descriptor recipe is source-backed by text_ranges.aep, while keyed
    // documents are linked by ordinal to signed times in list/ldat.
    let mut descriptor = StaticPropertyRecord::new(1, 1, 0, 0x1_0004, 1, 8, false).encode();
    descriptor[12..16].copy_from_slice(&clock.ticks().to_be_bytes());
    if timeline.keyed {
        descriptor = super::keyframes::animated_descriptor(descriptor, false);
    }
    let mut children = vec![
        Chunk::data(*b"tdsb", 1_u32.to_be_bytes().to_vec())?,
        views::name_payload("-_0_/-")?,
        Chunk::data(*b"tdb4", descriptor.to_vec())?,
    ];
    if timeline.keyed {
        let count = u16::try_from(timeline.keys.len())
            .map_err(|_| AepWriteError::Invalid("Source Text key count exceeds u16"))?;
        let blocks = timeline.keys.len().div_ceil(4).max(1);
        let mut header = vec![0_u8; 52];
        header[..10].copy_from_slice(&[0, 0xd0, 0x0b, 0xee, 0, 0, 0, 0, 0, 0]);
        header[10..12].copy_from_slice(&count.to_be_bytes());
        header[12..16].copy_from_slice(
            &u32::try_from(blocks)
                .map_err(|_| AepWriteError::Invalid("Source Text key block overflow"))?
                .to_be_bytes(),
        );
        header[18..20].copy_from_slice(&8_u16.to_be_bytes());
        header[23] = 4;
        header[24..28].copy_from_slice(&1_u32.to_be_bytes());
        header[28..32].copy_from_slice(
            &u32::try_from(blocks * 4)
                .map_err(|_| AepWriteError::Invalid("Source Text key capacity overflow"))?
                .to_be_bytes(),
        );
        let mut data = Vec::with_capacity(timeline.keys.len() * 8);
        for key in &timeline.keys {
            data.extend_from_slice(&clock.units(key.time_millis)?.to_be_bytes());
            data.extend_from_slice(&[3, 3, 0, 0]);
        }
        children.push(Chunk::list(
            *b"list",
            vec![Chunk::data(*b"lhd3", header)?, Chunk::data(*b"ldat", data)?],
        ));
    } else {
        children.push(Chunk::data(*b"cdat", 0_u32.to_be_bytes().to_vec())?);
    }
    Ok(Chunk::list(*b"tdbs", children))
}

fn encode_cos(timeline: &TextDocumentTimeline) -> Vec<u8> {
    let mut fonts = Vec::<&str>::new();
    let mut font_indices = BTreeMap::<&str, usize>::new();
    let mut document_font_indices = Vec::with_capacity(timeline.keys.len());
    for key in &timeline.keys {
        let font = key.document.font_postscript.as_str();
        let index = if let Some(index) = font_indices.get(font) {
            *index
        } else {
            let index = fonts.len();
            fonts.push(font);
            font_indices.insert(font, index);
            index
        };
        document_font_indices.push(index);
    }
    // Grow with the encoded output rather than preallocating a saturating
    // multiple of all source text (which can overflow or over-reserve).
    let mut out = String::new();
    out.push_str("<< /0 << /1 << /0 [ ");
    // AE-authored entries are `<< /0 << /99 /CoolTypeFont /0 << /0 (name) /2 n
    // /5 (version) >> >> >>`. FX has no face format (`/2`) or font-file version
    // (`/5`), so only the tag and PostScript name are written, never guesses.
    for font in &fonts {
        out.push_str("<< /0 << /99 /CoolTypeFont /0 << /0 ");
        push_utf16_hex(&mut out, font);
        out.push_str(" >> >> >> ");
    }
    out.push_str("] >>");
    let first = &timeline.keys[0].document;
    if let (Some(size), Some(position)) = (first.box_size, first.box_position) {
        out.push_str(" /8 << /0 [ << /0 << /1 << /0 [ ");
        let [left, top] = position;
        let right = left + size[0];
        let bottom = top + size[1];
        for value in [
            left, top, right, top, right, bottom, left, bottom, left, top, right, top, right,
            bottom,
        ] {
            push_number(&mut out, value);
            out.push(' ');
        }
        out.push_str("] >> >> >> ] >>");
    }
    out.push_str(" >> /1 << /1 [ ");
    for (key, font_index) in timeline.keys.iter().zip(document_font_indices) {
        let document = &key.document;
        out.push_str("<< /0 << /0 ");
        let mut native_text = document
            .text
            .replace("\r\n", "\n")
            .replace(['\n', '\r'], "\r");
        if !native_text.ends_with('\r') {
            native_text.push('\r');
        }
        push_utf16_hex(&mut out, &native_text);
        out.push_str(" /6 << /0 [ << /0 << /0 << /6 << /0 ");
        out.push_str(&font_index.to_string());
        out.push_str(" /1 ");
        push_number(&mut out, document.font_size);
        out.push_str(" /2 false /3 false /4 ");
        push_bool(&mut out, document.leading.is_none());
        out.push_str(" /5 ");
        push_number(
            &mut out,
            document.leading.unwrap_or(document.font_size * 1.2),
        );
        out.push_str(" /6 1 /7 1 /8 ");
        push_number(&mut out, document.tracking);
        out.push_str(" /9 ");
        push_number(&mut out, document.baseline_shift);
        out.push_str(" /12 ");
        // Caps code as stored by independently AE-authored sources: 0 for
        // normal text, 2 for All Caps.
        out.push_str(if document.all_caps { "2" } else { "0" });
        out.push_str(" /53 << /0 << /1 [ ");
        push_native_color(&mut out, document.fill_color);
        out.push_str("] >> >> /54 << /0 << /1 [ ");
        push_native_color(
            &mut out,
            document.stroke_color.unwrap_or(document.fill_color),
        );
        out.push_str("] >> >> /56 ");
        push_bool(&mut out, document.apply_fill);
        out.push_str(" /57 ");
        push_bool(&mut out, document.apply_stroke);
        out.push_str(" /58 ");
        push_bool(&mut out, document.stroke_over_fill);
        out.push_str(" /63 ");
        push_number(&mut out, document.stroke_width);
        out.push_str(" >> >> >> >> ] >> /5 << /0 [ << /0 << /0 << /5 << /0 ");
        out.push_str(match document.justification {
            Justification::Left => "0",
            Justification::Right => "1",
            Justification::Center => "2",
            Justification::Justify => "3",
        });
        out.push_str(" >> >> >> >> ] >> >> >> ");
    }
    out.push_str("] >> >>");
    out.into_bytes()
}

fn push_native_color(out: &mut String, color: [f64; 4]) {
    for value in [color[3], color[0], color[1], color[2]] {
        push_number(out, value);
        out.push(' ');
    }
}

fn push_bool(out: &mut String, value: bool) {
    out.push_str(if value { "true" } else { "false" });
}

fn push_number(out: &mut String, value: f64) {
    // The bounded COS reader intentionally accepts decimal syntax only, not
    // exponent notation. Fixed precision keeps the fresh writer inside that
    // established grammar even for very small finite authored values.
    let mut encoded = format!("{value:.17}");
    if encoded.contains('.') {
        while encoded.ends_with('0') {
            encoded.pop();
        }
        if encoded.ends_with('.') {
            encoded.pop();
        }
    }
    if encoded == "-0" {
        out.push('0');
    } else {
        out.push_str(&encoded);
    }
}

/// COS hex strings with a UTF-16BE BOM avoid locale-dependent literal-string
/// decoding and represent the full Rust `str` range deterministically.
fn push_utf16_hex(out: &mut String, value: &str) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push('<');
    for unit in std::iter::once(0xfeff).chain(value.encode_utf16()) {
        for byte in unit.to_be_bytes() {
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    out.push('>');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_text_thirty_fps_descriptor_and_hold_times_match_native_clock() {
        let clock = super::super::keyframes::PropertyClock::for_rate(
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        let timeline = TextDocumentTimeline {
            keyed: true,
            keys: [250, 1100]
                .into_iter()
                .map(|time_millis| TextDocumentKey {
                    time_millis,
                    document: document(),
                })
                .collect(),
        };
        let source = source_property_with_clock(&timeline, clock).unwrap();
        let metadata = source.children().unwrap()[0].children().unwrap();
        let descriptor = crate::properties::data(metadata, *b"tdb4").unwrap();
        assert_eq!(
            u32::from_be_bytes(descriptor[12..16].try_into().unwrap()),
            30_720
        );
        let list = crate::properties::unique_list(metadata, *b"list").unwrap();
        let data = crate::properties::data(list, *b"ldat").unwrap();
        assert_eq!(i32::from_be_bytes(data[..4].try_into().unwrap()), 7680);
        assert_eq!(i32::from_be_bytes(data[8..12].try_into().unwrap()), 33792);
    }

    fn document() -> TextDocumentSpec {
        TextDocumentSpec {
            text: "Hello\n世界".into(),
            font_postscript: "Inter-Regular".into(),
            font_size: 72.0,
            apply_fill: true,
            fill_color: [1.0, 0.25, 0.0, 1.0],
            apply_stroke: true,
            stroke_color: Some([0.0, 0.0, 0.0, 0.5]),
            stroke_width: 2.0,
            stroke_over_fill: true,
            justification: Justification::Center,
            tracking: 25.0,
            leading: Some(80.0),
            baseline_shift: 3.0,
            box_size: Some([320.0, 180.0]),
            box_position: Some([-160.0, -90.0]),
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
    fn fresh_cos_contains_typed_document_without_accepting_source_bytes() {
        let bytes = encode_cos(&timeline(document()));
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.starts_with("<< /0"));
        assert!(text.contains("/53 << /0 << /1 [ 1 1 0.25 0"));
        assert!(text.contains("/8 << /0"));
        assert!(text.contains("/0 2"));
        assert!(!text.contains("Hello"), "text is encoded as UTF-16BE hex");
    }

    #[test]
    fn all_caps_writes_the_native_all_caps_code() {
        // Independent AE-authored All Caps sources store 2 in field 12 of the
        // character style (text/import_text_additional_controls.aep and
        // pr4442_native/sources/text_document_allcaps.aep, composition 1);
        // normal text stores 0.
        for (all_caps, character_style) in [(true, " /9 3 /12 2 /53 "), (false, " /9 3 /12 0 /53 ")]
        {
            let cos = encode_cos(&timeline(TextDocumentSpec {
                all_caps,
                ..document()
            }));
            let cos = String::from_utf8(cos).unwrap();
            assert!(cos.contains(character_style), "{cos}");
        }
    }

    #[test]
    fn font_entries_use_the_native_cool_type_dictionary_without_guessed_fields() {
        let cos = String::from_utf8(encode_cos(&timeline(document()))).unwrap();
        let entry = format!(
            "[ << /0 << /99 /CoolTypeFont /0 << /0 {} >> >> >> ] >>",
            utf16_hex(&document().font_postscript)
        );
        assert!(cos.contains(&entry), "{cos}");
    }

    #[test]
    fn point_and_box_layout_are_distinct_fresh_cos_shapes() {
        let boxed = encode_cos(&timeline(document()));
        let mut point = document();
        point.box_size = None;
        point.box_position = None;
        let point = encode_cos(&timeline(point));
        assert!(String::from_utf8(boxed).unwrap().contains("/8 <<"));
        assert!(!String::from_utf8(point).unwrap().contains("/8 <<"));
    }

    #[test]
    fn finite_derived_text_numbers_reject_box_extent_overflow() {
        for axis in 0..2 {
            let mut overflowing = document();
            let mut size = [1.0; 2];
            let mut position = [0.0; 2];
            size[axis] = f64::MAX;
            position[axis] = f64::MAX;
            overflowing.box_size = Some(size);
            overflowing.box_position = Some(position);
            assert!(source_property(&timeline(overflowing)).is_err());
        }
    }

    #[test]
    fn finite_derived_text_numbers_reject_auto_leading_overflow_only() {
        let mut automatic = document();
        automatic.font_size = f64::MAX;
        automatic.leading = None;
        assert!(timeline(automatic).validate().is_err());

        let mut explicit = document();
        explicit.font_size = f64::MAX;
        explicit.leading = Some(80.0);
        source_property(&timeline(explicit))
            .expect("explicit finite leading does not derive an overflowing line height");
    }

    fn document_with(font: String, text: String) -> TextDocumentSpec {
        TextDocumentSpec {
            font_postscript: font,
            text,
            ..document()
        }
    }

    fn keyed_timeline(
        documents: impl IntoIterator<Item = TextDocumentSpec>,
    ) -> TextDocumentTimeline {
        TextDocumentTimeline {
            keyed: true,
            keys: documents
                .into_iter()
                .enumerate()
                .map(|(index, document)| TextDocumentKey {
                    time_millis: i64::try_from(index).unwrap() * 100,
                    document,
                })
                .collect(),
        }
    }

    fn document_font_indices(cos: &str) -> Vec<usize> {
        const PREFIX: &str = "/6 << /0 [ << /0 << /0 << /6 << /0 ";
        cos.split(PREFIX)
            .skip(1)
            .map(|suffix| suffix.split_once(' ').unwrap().0.parse().unwrap())
            .collect()
    }

    fn utf16_hex(value: &str) -> String {
        let mut encoded = String::new();
        push_utf16_hex(&mut encoded, value);
        encoded
    }

    #[test]
    fn cos_font_table_keeps_first_seen_order_and_reuses_indices() {
        let timeline = keyed_timeline([
            document_with("Second-Seen".into(), "one".into()),
            document_with("First-Repeated".into(), "two".into()),
            document_with("Second-Seen".into(), "three".into()),
        ]);
        let cos = String::from_utf8(encode_cos(&timeline)).unwrap();

        assert!(
            cos.find(&utf16_hex("Second-Seen")).unwrap()
                < cos.find(&utf16_hex("First-Repeated")).unwrap()
        );
        assert_eq!(document_font_indices(&cos), [0, 1, 0]);
    }

    #[test]
    fn high_cardinality_cos_fonts_keep_stable_indices() {
        const FONT_COUNT: usize = 2_048;
        let mut documents = (0..FONT_COUNT)
            .map(|index| document_with(format!("Font-{index:04}"), format!("Text {index}")))
            .collect::<Vec<_>>();
        documents.push(document_with("Font-0000".into(), "repeat first".into()));
        documents.push(document_with(
            format!("Font-{:04}", FONT_COUNT - 1),
            "repeat last".into(),
        ));

        let timeline = keyed_timeline(documents);
        let first = encode_cos(&timeline);
        let second = encode_cos(&timeline);
        assert_eq!(first, second);
        let indices = document_font_indices(std::str::from_utf8(&first).unwrap());
        assert_eq!(indices.len(), FONT_COUNT + 2);
        assert_eq!(indices[0], 0);
        assert_eq!(indices[FONT_COUNT - 1], FONT_COUNT - 1);
        assert_eq!(indices[FONT_COUNT..], [0, FONT_COUNT - 1]);
    }

    #[test]
    fn source_text_payloads_and_keys_exceed_old_policy_boundaries() {
        let large = timeline(document_with(
            "F".repeat(1_025),
            "x".repeat(8 * 1024 * 1024 + 1),
        ));
        source_property(&large).expect("text and font bytes above the old policy limits");

        let keyed =
            keyed_timeline((0..=10_000).map(|index| document_with("F".into(), index.to_string())));
        source_property(&keyed).expect("documents above the old policy limit");
    }

    #[test]
    fn keyed_documents_bind_signed_times_to_cos_document_order() {
        let mut second = document();
        second.text = "Second".into();
        second.font_postscript = "Other-Bold".into();
        let timeline = TextDocumentTimeline {
            keyed: true,
            keys: vec![
                TextDocumentKey {
                    time_millis: -250,
                    document: document(),
                },
                TextDocumentKey {
                    time_millis: 500,
                    document: second,
                },
            ],
        };
        let property = source_property(&timeline).unwrap();
        let children = property.children().unwrap();
        let metadata = children[0].children().unwrap();
        let list = metadata
            .iter()
            .find(|chunk| chunk.list_kind() == Some(*b"list"))
            .unwrap();
        let data = list
            .children()
            .unwrap()
            .iter()
            .find(|chunk| chunk.id() == *b"ldat")
            .unwrap()
            .data_payload()
            .unwrap();
        assert_eq!(i32::from_be_bytes(data[0..4].try_into().unwrap()), -6_144);
        assert_eq!(i32::from_be_bytes(data[8..12].try_into().unwrap()), 12_288);
        let cos = children[1].opaque_payload().unwrap();
        let text = std::str::from_utf8(cos).unwrap();
        assert!(text.contains("/0 0 /1 72"));
        assert!(text.contains("/0 1 /1 72"));
    }
}
