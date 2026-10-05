//! Legacy Source Text: the UTF-16 JSON document that Premiere wrote before
//! its FlatBuffer.
//!
//! The payload keeps the frame's little-endian `u64` byte count, but that
//! many bytes of UTF-16LE text follow it in place of the magic and the
//! buffer, without a byte order mark or terminator. The text is one JSON
//! object, `{"mTextParam": {...}, "mVersion": 1}`. `mTextParam` holds the
//! paragraph fields and `mStyleSheet`, whose `mText` is the whole text and
//! whose other fields each hold one character style as
//! `{"mParamValues": [[start, value], ...]}`, one entry per style run that
//! starts at character `start`.
//!
//! Only version 1 reads, with every base field below present with its type and
//! no unknown field, so a field of unknown meaning fails closed instead of
//! being dropped. A second complete layout includes disabled background,
//! mask, underline and additional-stroke controls. No render measured this
//! form. Both layouts convert with
//! the meanings that the Premiere 26 encoding has for the same controls:
//!
//! - one style run: every style holds a single entry from character 0;
//! - point text: `mWidth` and `mHeight` 0, since a box without area holds no
//!   line, laid out from its first baseline, as Premiere 26 reads an absent
//!   box alignment (the legacy form stores none);
//! - `mAlignment` 0 or 2: left or centred lines, as Premiere 26 stores them;
//! - `mLeading` 0: the automatic line spacing;
//! - the PostScript `mFontName`, `mFontSize` in pixels and `mTracking` in
//!   1/1000 em;
//! - `mFillVisible` true with a gray `mFillColor` (equal channels, which no
//!   channel order changes), or false for no fill; a gray stroke of positive
//!   pixel width, with fill above stroke when both are enabled; shadow off.
//!
//! Any other alignment, leading or box size, several runs, a nonempty
//! `mDefaultRun`, right-to-left, vertical, Indic, Hindi-digit or ligature
//! text, faux bold or italic, a nonzero caps option, baseline option,
//! baseline shift, kerning or tsume (`mTsumi`), a tab (no tab stops are
//! modeled), a color beyond 24 bits, another fill color, an enabled nongray
//! stroke, stroke above fill or shadow fail closed. Source Text keys and
//! caption blocks do not read this form.

use super::{DecodedGraphicText, TextDocuments};
use crate::error::{ensure, unsupported, Result};
use crate::schema::text::{
    normalize_line_breaks, PrJustification, PrRgb, PrTextDocument, PrTextFrame, PrTextStroke,
    PrVerticalAlign,
};
use serde::de::{
    value::MapAccessDeserializer, Deserializer, Error as _, IgnoredAny, MapAccess, Visitor,
};
use serde::Deserialize;
use std::{fmt, marker::PhantomData};

/// How messages name a legacy Source Text payload.
const LEGACY: &str = "legacy UTF-16 JSON Source Text";

/// The first text unit of a legacy payload, `{` in UTF-16LE, where a
/// FlatBuffer frame holds its magic.
const JSON_START: &[u8] = b"{\0";

/// The legacy form stores no vertical alignment; a point text without one
/// sits on its first baseline.
const POINT_ALIGNMENT: PrVerticalAlign = PrVerticalAlign::Top;

/// Whether `payload` holds legacy JSON rather than a FlatBuffer frame.
pub(super) fn holds_json(payload: &[u8]) -> bool {
    payload.get(8..10) == Some(JSON_START)
}

/// A legacy Source Text value: its paragraph and version, and nothing else.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyText {
    #[serde(rename = "mTextParam", deserialize_with = "named_object")]
    paragraph: Paragraph,
    #[serde(rename = "mVersion")]
    version: u32,
}

/// The version 1 paragraph fields. Every field is required, so that no
/// switch reads as on or off by default; the disabled shadow's values and
/// the tab width, which lays out no tab here, are read only for their types.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Paragraph {
    #[serde(rename = "mAlignment")]
    alignment: u32,
    #[serde(rename = "mBackFillColor", default, deserialize_with = "present")]
    back_fill_color: Option<u32>,
    #[serde(rename = "mBackFillOpacity", default, deserialize_with = "present")]
    back_fill_opacity: Option<f64>,
    #[serde(rename = "mBackFillSize", default, deserialize_with = "present")]
    back_fill_size: Option<f64>,
    #[serde(rename = "mBackFillVisible", default, deserialize_with = "present")]
    back_fill_visible: Option<bool>,
    #[serde(rename = "mIsMask", default, deserialize_with = "present")]
    is_mask: Option<bool>,
    #[serde(rename = "mIsMaskInverted", default, deserialize_with = "present")]
    is_mask_inverted: Option<bool>,
    #[serde(rename = "mLineCapType", default, deserialize_with = "present")]
    line_cap_type: Option<u32>,
    #[serde(rename = "mLineJoinType", default, deserialize_with = "present")]
    line_join_type: Option<u32>,
    #[serde(rename = "mMiterLimit", default, deserialize_with = "present")]
    miter_limit: Option<f64>,
    #[serde(rename = "mNumStrokes", default, deserialize_with = "present")]
    num_strokes: Option<u32>,
    #[serde(rename = "mDefaultRun")]
    default_run: Vec<IgnoredAny>,
    #[serde(rename = "mHeight")]
    height: f64,
    #[serde(rename = "mHindiDigits")]
    hindi_digits: bool,
    #[serde(rename = "mIndic")]
    indic: bool,
    #[serde(rename = "mIsVerticalText")]
    vertical_text: bool,
    #[serde(rename = "mLeading")]
    leading: f64,
    #[serde(rename = "mLigatures")]
    ligatures: bool,
    #[serde(rename = "mRTL")]
    right_to_left: bool,
    #[serde(rename = "mShadowAngle")]
    _shadow_angle: f64,
    #[serde(rename = "mShadowBlur")]
    _shadow_blur: f64,
    #[serde(rename = "mShadowColor")]
    shadow_color: u32,
    #[serde(rename = "mShadowOffset")]
    _shadow_offset: f64,
    #[serde(rename = "mShadowOpacity")]
    _shadow_opacity: f64,
    #[serde(rename = "mShadowSize")]
    _shadow_size: f64,
    #[serde(rename = "mShadowVisible")]
    shadow_visible: bool,
    #[serde(rename = "mStyleSheet", deserialize_with = "named_object")]
    style: Style,
    #[serde(rename = "mTabWidth")]
    _tab_width: f64,
    #[serde(rename = "mWidth")]
    width: f64,
}

/// The text and its version 1 character styles, each of one run
/// ([`one_run`]). Only an enabled stroke uses its saved width and color.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Style {
    #[serde(
        rename = "mAdditionalStrokeColor",
        default,
        deserialize_with = "present"
    )]
    additional_stroke_color: Option<Vec<IgnoredAny>>,
    #[serde(
        rename = "mAdditionalStrokeVisible",
        default,
        deserialize_with = "present"
    )]
    additional_stroke_visible: Option<Vec<IgnoredAny>>,
    #[serde(
        rename = "mAdditionalStrokeWidth",
        default,
        deserialize_with = "present"
    )]
    additional_stroke_width: Option<Vec<IgnoredAny>>,
    #[serde(rename = "mUnderline", default, deserialize_with = "present_run")]
    underline: Option<bool>,
    #[serde(rename = "mBaselineOption", deserialize_with = "one_run")]
    baseline_option: u32,
    #[serde(rename = "mBaselineShift", deserialize_with = "one_run")]
    baseline_shift: f64,
    #[serde(rename = "mCapsOption", deserialize_with = "one_run")]
    caps_option: u32,
    #[serde(rename = "mFauxBold", deserialize_with = "one_run")]
    faux_bold: bool,
    #[serde(rename = "mFauxItalic", deserialize_with = "one_run")]
    faux_italic: bool,
    #[serde(rename = "mFillColor", deserialize_with = "one_run")]
    fill_color: u32,
    #[serde(rename = "mFillOverStroke", deserialize_with = "one_run")]
    fill_over_stroke: bool,
    #[serde(rename = "mFillVisible", deserialize_with = "one_run")]
    fill_visible: bool,
    #[serde(rename = "mFontName", deserialize_with = "one_run")]
    font: String,
    #[serde(rename = "mFontSize", deserialize_with = "one_run")]
    size: f32,
    #[serde(rename = "mKerning", deserialize_with = "one_run")]
    kerning: f64,
    #[serde(rename = "mStrokeColor", deserialize_with = "one_run")]
    stroke_color: u32,
    #[serde(rename = "mStrokeVisible", deserialize_with = "one_run")]
    stroke_visible: bool,
    #[serde(rename = "mStrokeWidth", deserialize_with = "one_run")]
    stroke_width: f32,
    #[serde(rename = "mText")]
    text: String,
    #[serde(rename = "mTracking", deserialize_with = "one_run")]
    tracking: f32,
    #[serde(rename = "mTsumi", deserialize_with = "one_run")]
    tsumi: f64,
}

/// An absent supplemental field is distinct from a present null value.
fn present<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

fn present_run<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    one_run(deserializer).map(Some)
}

/// Require named fields: Serde's struct derive also accepts positional
/// arrays. Delegating the original map preserves duplicate-field checks.
fn named_object<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Object<T>(PhantomData<T>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for Object<T> {
        type Value = T;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an object with named fields")
        }

        fn visit_map<M: MapAccess<'de>>(self, map: M) -> std::result::Result<T, M::Error> {
            T::deserialize(MapAccessDeserializer::new(map))
        }
    }
    deserializer.deserialize_map(Object(PhantomData))
}

/// The value of a character style that every character shares: a single
/// `[start, value]` entry whose run starts at character 0.
fn one_run<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Runs<T> {
        #[serde(rename = "mParamValues")]
        values: Vec<(u64, T)>,
    }
    let Runs { values } = named_object(deserializer)?;
    match <[(u64, T); 1]>::try_from(values) {
        Ok([(0, value)]) => Ok(value),
        Ok([(start, _)]) => Err(D::Error::custom(format!(
            "its one style run starts at character {start}, not 0"
        ))),
        Err(values) => Err(D::Error::custom(format!(
            "mixed text styles are unsupported: a style holds {} runs, not one",
            values.len()
        ))),
    }
}

/// Decode a legacy Source Text value in the one profile that converts
/// (module docs).
///
/// # Errors
/// Rejects other framing, invalid UTF-16, JSON other than one object of the
/// version 1 fields with their types, other versions, and every form outside
/// the profile. `PrText::validate` checks the value ranges.
pub(super) fn decode(payload: &[u8]) -> Result<DecodedGraphicText> {
    let json = json_text(payload)?;
    let mut deserializer = serde_json::Deserializer::from_str(&json);
    let LegacyText { paragraph, version } = named_object(&mut deserializer)
        .and_then(|text| deserializer.end().map(|()| text))
        .map_err(|error| unsupported(format!("{LEGACY}: {error}")))?;
    ensure!(
        version == 1,
        "{LEGACY}: version {version} is unsupported; only version 1 converts"
    );
    validate_passive_decorations(&paragraph)?;
    let Paragraph {
        alignment,
        default_run,
        height,
        hindi_digits,
        indic,
        vertical_text,
        leading,
        ligatures,
        right_to_left,
        shadow_color,
        shadow_visible,
        style,
        width,
        ..
    } = paragraph;
    let justification = match alignment {
        0 => PrJustification::Left,
        2 => PrJustification::Center,
        other => {
            return Err(unsupported(format!(
                "{LEGACY}: alignment {other} is unsupported; only 0 (left) and 2 (centred) convert"
            )))
        }
    };
    ensure!(
        width == 0.0 && height == 0.0,
        "{LEGACY}: box text ({width} x {height}) is unsupported; only point text (0 x 0) converts"
    );
    ensure!(
        leading == 0.0,
        "{LEGACY}: leading {leading} is unsupported; only the automatic 0 converts"
    );
    ensure!(
        default_run.is_empty(),
        "{LEGACY}: a nonempty mDefaultRun is unsupported"
    );
    for (name, on) in [
        ("mRTL", right_to_left),
        ("mIsVerticalText", vertical_text),
        ("mIndic", indic),
        ("mHindiDigits", hindi_digits),
        ("mLigatures", ligatures),
        ("mFauxBold", style.faux_bold),
        ("mFauxItalic", style.faux_italic),
    ] {
        ensure!(!on, "{LEGACY}: {name} true is unsupported");
    }
    for (name, value) in [
        ("mCapsOption", style.caps_option),
        ("mBaselineOption", style.baseline_option),
    ] {
        ensure!(
            value == 0,
            "{LEGACY}: {name} {value} is unsupported; only 0 converts"
        );
    }
    for (name, value) in [
        ("mBaselineShift", style.baseline_shift),
        ("mKerning", style.kerning),
        ("mTsumi", style.tsumi),
    ] {
        ensure!(
            value == 0.0,
            "{LEGACY}: {name} {value} is unsupported; only 0 converts"
        );
    }
    let text = normalize_line_breaks(&style.text);
    ensure!(
        !text.contains('\t'),
        "{LEGACY}: a tab is unsupported, since no tab stops (mTabWidth) are modeled"
    );
    let fill_color = color("mFillColor", style.fill_color)?;
    let stroke_color = color("mStrokeColor", style.stroke_color)?;
    color("mShadowColor", shadow_color)?;
    let stroke = if style.stroke_visible {
        let [red, green, blue] = stroke_color;
        ensure!(
            red == green && green == blue,
            "{LEGACY}: only a gray stroke converts, since the channel order is unknown"
        );
        ensure!(
            !style.fill_visible || style.fill_over_stroke,
            "{LEGACY}: stroke over fill is unsupported"
        );
        ensure!(
            style.stroke_width.is_finite() && style.stroke_width > 0.0,
            "{LEGACY}: invalid enabled stroke width"
        );
        Some(PrTextStroke {
            color: PrRgb(stroke_color),
            width: style.stroke_width,
        })
    } else {
        None
    };
    ensure!(
        !shadow_visible,
        "{LEGACY}: an enabled shadow is unsupported"
    );
    let fill = if style.fill_visible {
        let [red, green, blue] = fill_color;
        ensure!(
            red == green && green == blue,
            "{LEGACY}: mFillColor {:#08x} is not gray; only a gray fill converts, since the channel order is unknown",
            style.fill_color
        );
        Some(PrRgb(fill_color))
    } else {
        None
    };
    let document = PrTextDocument {
        text,
        font: style.font,
        size: style.size,
        fill,
        stroke,
        shadow: None,
        all_caps: false,
        tracking: style.tracking,
        leading: 0.0,
        justification,
        frame: PrTextFrame::Point {
            vertical: POINT_ALIGNMENT,
        },
        background: None,
    };
    Ok(DecodedGraphicText {
        documents: TextDocuments::Uniform(document),
        omitted: Vec::new(),
        legacy_json: true,
        mask_source: None,
        box_alignment: POINT_ALIGNMENT,
    })
}

/// Admit only the complete additional layout, with its drawing switches off.
/// Partial layouts and active decorations have no supported legacy mapping.
fn validate_passive_decorations(paragraph: &Paragraph) -> Result<()> {
    let style = &paragraph.style;
    let present = [
        paragraph.back_fill_color.is_some(),
        paragraph.back_fill_opacity.is_some(),
        paragraph.back_fill_size.is_some(),
        paragraph.back_fill_visible.is_some(),
        paragraph.is_mask.is_some(),
        paragraph.is_mask_inverted.is_some(),
        paragraph.line_cap_type.is_some(),
        paragraph.line_join_type.is_some(),
        paragraph.miter_limit.is_some(),
        paragraph.num_strokes.is_some(),
        style.additional_stroke_color.is_some(),
        style.additional_stroke_visible.is_some(),
        style.additional_stroke_width.is_some(),
        style.underline.is_some(),
    ];
    if present.iter().all(|present| !present) {
        return Ok(());
    }
    ensure!(
        present.iter().all(|present| *present),
        "{LEGACY}: incomplete passive decoration layout"
    );
    if let Some(value) = paragraph.back_fill_color {
        color("mBackFillColor", value)?;
    }
    ensure!(
        paragraph
            .back_fill_opacity
            .is_some_and(|value| (0.0..=100.0).contains(&value))
            && paragraph.back_fill_size.is_some_and(|value| value >= 0.0),
        "{LEGACY}: invalid disabled background opacity or size"
    );
    ensure!(
        paragraph.back_fill_visible == Some(false)
            && paragraph.is_mask == Some(false)
            && paragraph.is_mask_inverted == Some(false)
            && style.underline == Some(false),
        "{LEGACY}: active background, mask or underline is unsupported"
    );
    ensure!(
        paragraph.line_cap_type == Some(0)
            && paragraph.line_join_type == Some(0)
            && paragraph.miter_limit == Some(2.5)
            && paragraph.num_strokes == Some(1),
        "{LEGACY}: nondefault passive stroke controls are unsupported"
    );
    ensure!(
        [
            &style.additional_stroke_color,
            &style.additional_stroke_visible,
            &style.additional_stroke_width,
        ]
        .into_iter()
        .all(|values| values.as_ref().is_some_and(Vec::is_empty)),
        "{LEGACY}: additional strokes are unsupported"
    );
    Ok(())
}

/// The JSON text of a legacy payload: the UTF-16LE text that its byte count
/// covers, which is at most half again as long in UTF-8.
fn json_text(payload: &[u8]) -> Result<String> {
    let Some((count, text)) = payload.split_first_chunk::<8>() else {
        return Err(unsupported(format!("truncated {LEGACY}")));
    };
    let count = u64::from_le_bytes(*count);
    ensure!(
        u64::try_from(text.len()).is_ok_and(|bytes| bytes == count),
        "{LEGACY}: its byte count {count} does not match the {} bytes after it",
        text.len()
    );
    ensure!(
        text.len() % 2 == 0,
        "{LEGACY}: an odd byte count {} is not UTF-16",
        text.len()
    );
    char::decode_utf16(
        text.chunks_exact(2)
            .map(|unit| u16::from_le_bytes([unit[0], unit[1]])),
    )
    .collect::<std::result::Result<String, _>>()
    .map_err(|error| unsupported(format!("{LEGACY}: invalid UTF-16: {error}")))
}

/// The three low bytes of the legacy color `value` of the field `name`, in
/// an unknown channel order. The saved colors fit 24 bits; a higher byte has
/// no known meaning.
fn color(name: &str, value: u32) -> Result<[u8; 3]> {
    let [high, low @ ..] = value.to_be_bytes();
    ensure!(
        high == 0,
        "{LEGACY}: {name} {value:#08x} is outside the 24-bit color form"
    );
    Ok(low)
}

#[cfg(test)]
mod tests;
