//! Legacy Source Text stores a byte-counted UTF-16LE JSON document. Known
//! text, paint and layout fields map to the existing editable text model;
//! unsupported controls are diagnosed independently of that document.
//! Required structure, consumed values and unconverted masks remain checked.
//! These mappings have structural proof, not legacy native-render fidelity.

use super::{DecodedGraphicText, OmittedTextFeature, TextDocuments};
use crate::error::{ensure, unsupported, Result};
use crate::schema::text::{
    normalize_line_breaks, PrJustification, PrRgb, PrTextDocument, PrTextFrame, PrTextStroke,
    PrVerticalAlign,
};
use crate::schema::text_shadow::PrTextShadow;
use serde::de::{value::MapAccessDeserializer, Deserializer, MapAccess, Visitor};
use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeMap, fmt, marker::PhantomData};

const LEGACY: &str = "legacy UTF-16 JSON Source Text";
const JSON_START: &[u8] = b"{\0";
/// Legacy text has no saved vertical alignment; use the existing default.
const DEFAULT_ALIGNMENT: PrVerticalAlign = PrVerticalAlign::Top;

pub(super) fn holds_json(payload: &[u8]) -> bool {
    payload.get(8..10) == Some(JSON_START)
}

#[derive(Deserialize)]
struct LegacyText {
    #[serde(rename = "mTextParam", deserialize_with = "named_object")]
    paragraph: Paragraph,
    #[serde(rename = "mVersion", default)]
    _version: Option<Value>,
    #[serde(rename = "mMask", default, deserialize_with = "present")]
    mask: Option<bool>,
    #[serde(flatten)]
    controls: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
struct Paragraph {
    #[serde(rename = "mAlignment")]
    alignment: u32,
    #[serde(rename = "mHeight")]
    height: f32,
    #[serde(rename = "mLeading")]
    leading: f32,
    // Do not ignore malformed or enabled concealment controls.
    #[serde(rename = "mIsMask", default, deserialize_with = "present")]
    is_mask: Option<bool>,
    #[serde(rename = "mIsMaskInverted", default, deserialize_with = "present")]
    is_mask_inverted: Option<bool>,
    #[serde(rename = "mStyleSheet", deserialize_with = "named_object")]
    style: Style,
    #[serde(rename = "mWidth")]
    width: f32,
    #[serde(flatten)]
    controls: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
struct Style {
    #[serde(
        rename = "mCapsOption",
        default,
        deserialize_with = "optional_first_run"
    )]
    caps_option: Option<Run<u32>>,
    #[serde(rename = "mFillColor", deserialize_with = "first_run")]
    fill_color: Run<u32>,
    #[serde(rename = "mFillOverStroke", deserialize_with = "first_run")]
    fill_over_stroke: Run<bool>,
    #[serde(rename = "mFillVisible", deserialize_with = "visibility_run")]
    fill_visible: Run<bool>,
    #[serde(rename = "mFontName", deserialize_with = "first_run")]
    font: Run<String>,
    #[serde(rename = "mFontSize", deserialize_with = "first_run")]
    size: Run<f32>,
    #[serde(rename = "mStrokeColor", deserialize_with = "first_run")]
    stroke_color: Run<u32>,
    #[serde(rename = "mStrokeVisible", deserialize_with = "visibility_run")]
    stroke_visible: Run<bool>,
    #[serde(rename = "mStrokeWidth", deserialize_with = "first_run")]
    stroke_width: Run<f32>,
    #[serde(rename = "mText")]
    text: String,
    #[serde(rename = "mTracking", deserialize_with = "first_run")]
    tracking: Run<f32>,
    #[serde(flatten)]
    controls: BTreeMap<String, Value>,
}

struct Run<T> {
    value: T,
    metadata: BTreeMap<String, Value>,
    additional_runs: usize,
    uniform: bool,
    later: Vec<(u64, T)>,
}

impl<T> Run<T> {
    fn into_value(self, field: &str, omitted: &mut Vec<OmittedTextFeature>) -> T {
        if self.additional_runs > 0 {
            diagnose(
                omitted,
                field,
                "multiple character runs replaced with the first style value for the whole editable text; later character styling and insertion defaults lost",
            );
        }
        for metadata in self.metadata.keys() {
            diagnose(
                omitted,
                &format!("{field}.{metadata}"),
                "unmapped style-run metadata omitted; actual text and supported styling retained",
            );
        }
        self.value
    }
}

/// An absent mask field is distinct from a present null value.
fn present<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Serde's struct derive also accepts positional arrays; these records require
/// named fields, including duplicate checks for the consumed properties.
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

/// Only absence uses the optional control's fallback; present nulls and invalid
/// runs still go through the same admission as required style controls.
fn optional_first_run<'de, D, T>(deserializer: D) -> std::result::Result<Option<Run<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + PartialEq,
{
    first_run(deserializer).map(Some)
}

/// Retain the first style for fields without a character mapping rather than
/// omit the text. Fill runs are lowered to character selectors separately.
fn first_run<'de, D, T>(deserializer: D) -> std::result::Result<Run<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + PartialEq,
{
    read_run(deserializer, false)
}

fn visibility_run<'de, D>(deserializer: D) -> std::result::Result<Run<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    // Flattening changing visibility could expose concealed characters.
    read_run(deserializer, true)
}

fn read_run<'de, D, T>(
    deserializer: D,
    require_uniform: bool,
) -> std::result::Result<Run<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + PartialEq,
{
    use serde::de::Error as _;
    #[derive(Deserialize)]
    struct Runs<T> {
        #[serde(rename = "mParamValues")]
        values: Vec<(u64, T)>,
        #[serde(flatten)]
        metadata: BTreeMap<String, Value>,
    }
    let Runs { values, metadata } = named_object(deserializer)?;
    if values.first().map(|(start, _)| *start) != Some(0)
        || !values.windows(2).all(|pair| pair[0].0 < pair[1].0)
    {
        return Err(D::Error::custom(
            "style runs must start at character 0 and have strictly increasing offsets",
        ));
    }
    let uniform = values.windows(2).all(|pair| pair[0].1 == pair[1].1);
    if require_uniform && !uniform {
        return Err(D::Error::custom(
            "changing character visibility is unsupported; text omitted to preserve concealment",
        ));
    }
    let additional_runs = values.len() - 1;
    let mut values = values.into_iter();
    let (_, value) = values.next().expect("nonempty runs checked above");
    Ok(Run {
        value,
        metadata,
        additional_runs,
        uniform,
        later: values.collect(),
    })
}

pub(super) fn decode(payload: &[u8]) -> Result<DecodedGraphicText> {
    let (json, stale_count) = json_text(payload)?;
    let mut deserializer = serde_json::Deserializer::from_str(&json);
    let LegacyText {
        paragraph,
        controls,
        mask,
        ..
    } = named_object(&mut deserializer)
        .and_then(|text| deserializer.end().map(|()| text))
        .map_err(|error| unsupported(format!("{LEGACY}: {error}")))?;
    ensure!(
        mask != Some(true)
            && paragraph.is_mask != Some(true)
            && paragraph.is_mask_inverted != Some(true),
        "{LEGACY}: active mask is unsupported"
    );
    // Without fill, a zero-width stroke can conceal a character span.
    ensure!(
        paragraph.style.fill_visible.value
            || !paragraph.style.stroke_visible.value
            || paragraph.style.stroke_width.uniform,
        "{LEGACY}: changing outline-only stroke width is unsupported; text omitted to preserve concealment"
    );
    let mut omitted = Vec::new();
    if let Some(count) = stale_count {
        diagnose(
            &mut omitted,
            "byte count",
            &format!("saved byte count {count} differs from the {} payload bytes; complete validated JSON retained instead of truncating editable text", payload.len() - 8),
        );
    }
    for field in controls.keys() {
        diagnose(&mut omitted, field, "unmapped root control omitted");
    }
    let Paragraph {
        alignment,
        height,
        leading,
        style,
        width,
        controls,
        ..
    } = paragraph;
    let shadow = legacy_shadow(&controls, &mut omitted);
    diagnose_paragraph(&controls, style.stroke_visible.value, &mut omitted);
    for (field, value) in &style.controls {
        let inactive = match field.as_str() {
            "mBaselineOption" | "mBaselineShift" | "mKerning" | "mTsumi" => {
                run_is(value, &Value::from(0))
            }
            "mFauxBold" | "mFauxItalic" | "mUnderline" => run_is(value, &Value::Bool(false)),
            "mAdditionalStrokeColor" | "mAdditionalStrokeWidth" => true,
            "mAdditionalStrokeVisible" => value.as_array().is_some_and(Vec::is_empty),
            _ => false,
        };
        if !inactive {
            diagnose(
                &mut omitted,
                field,
                "unmapped character control omitted; actual text and supported styling retained",
            );
        }
    }
    let Style {
        caps_option,
        fill_color,
        fill_over_stroke,
        fill_visible,
        font,
        size,
        stroke_color,
        stroke_visible,
        stroke_width,
        text,
        tracking,
        ..
    } = style;
    let caps_option = if let Some(caps_option) = caps_option {
        caps_option.into_value("mCapsOption", &mut omitted)
    } else {
        diagnose(
            &mut omitted,
            "mCapsOption",
            "absent caps control replaced with normal case; actual text and supported styling retained",
        );
        0
    };
    let fill_spans = fill_spans(&fill_color, &text, caps_option == 2, &mut omitted);
    let fill_color = Run {
        additional_runs: 0,
        ..fill_color
    }
    .into_value("mFillColor", &mut omitted);
    let fill_over_stroke = fill_over_stroke.into_value("mFillOverStroke", &mut omitted);
    let fill_visible = fill_visible.into_value("mFillVisible", &mut omitted);
    let font = font.into_value("mFontName", &mut omitted);
    let size = size.into_value("mFontSize", &mut omitted);
    let stroke_color = stroke_color.into_value("mStrokeColor", &mut omitted);
    let stroke_visible = stroke_visible.into_value("mStrokeVisible", &mut omitted);
    let stroke_width = stroke_width.into_value("mStrokeWidth", &mut omitted);
    let tracking = tracking.into_value("mTracking", &mut omitted);
    let justification = match alignment {
        0 => PrJustification::Left,
        1 => PrJustification::Right,
        2 => PrJustification::Center,
        3 => PrJustification::Justify,
        _ => {
            diagnose(
                &mut omitted,
                "mAlignment",
                "unmapped paragraph alignment replaced with left alignment",
            );
            PrJustification::Left
        }
    };
    ensure!(
        width.is_finite() && height.is_finite() && width >= 0.0 && height >= 0.0,
        "{LEGACY}: invalid text box dimensions"
    );
    let frame = if width == 0.0 && height == 0.0 {
        PrTextFrame::Point {
            vertical: DEFAULT_ALIGNMENT,
        }
    } else if width > 0.0 && height > 0.0 {
        PrTextFrame::Box {
            width,
            height,
            vertical: DEFAULT_ALIGNMENT,
        }
    } else {
        diagnose(
            &mut omitted,
            "mWidth/mHeight",
            "box without area retained as point text",
        );
        PrTextFrame::Point {
            vertical: DEFAULT_ALIGNMENT,
        }
    };
    ensure!(leading.is_finite(), "{LEGACY}: invalid text leading");
    let text = normalize_line_breaks(&text);
    let tight_point_lines = f64::from(leading) < -0.4 * f64::from(size)
        && f64::from(1.2_f32 * size) + f64::from(leading) > 0.0
        && matches!(frame, PrTextFrame::Point { .. })
        && text.contains('\n')
        && justification != PrJustification::Justify
        && fill_spans.is_empty();
    let leading = if tight_point_lines || f64::from(leading) >= -0.4 * f64::from(size) {
        leading
    } else {
        diagnose(
            &mut omitted,
            "mLeading",
            "line spacing below the editable 0.8 em minimum replaced with automatic leading",
        );
        0.0
    };
    if text.contains('\t') {
        diagnose(
            &mut omitted,
            "mTabWidth",
            "text retained with editable default tab spacing; saved tab stops not mapped",
        );
    }
    let fill = fill_visible.then(|| paint("mFillColor", fill_color, &mut omitted));
    let stroke = if stroke_visible {
        if stroke_width.is_finite() && stroke_width >= 0.0 {
            if fill_visible && !fill_over_stroke {
                diagnose(
                    &mut omitted,
                    "mFillOverStroke",
                    "stroke-over-fill replaced with the editable fill-over-stroke order",
                );
            }
            Some(PrTextStroke {
                color: paint("mStrokeColor", stroke_color, &mut omitted),
                width: stroke_width,
            })
        } else {
            diagnose(
                &mut omitted,
                "mStrokeWidth",
                "invalid enabled stroke omitted; actual text and fill retained",
            );
            None
        }
    } else {
        None
    };
    let all_caps = match caps_option {
        0 => false,
        2 => true,
        _ => {
            diagnose(
                &mut omitted,
                "mCapsOption",
                "unmapped caps option omitted; actual text retained",
            );
            false
        }
    };
    let document = PrTextDocument {
        text,
        font,
        size,
        fill,
        fill_spans: if fill_visible { fill_spans } else { Vec::new() },
        stroke,
        shadow,
        all_caps,
        tracking,
        leading,
        justification,
        frame,
        background: None,
    };
    let documents = if tight_point_lines {
        TextDocuments::Lines(
            document
                .text
                .split('\n')
                .map(|line| PrTextDocument {
                    text: line.to_owned(),
                    ..document.clone()
                })
                .collect(),
        )
    } else {
        TextDocuments::Uniform(document)
    };
    Ok(DecodedGraphicText {
        documents,
        omitted,
        legacy_json: true,
        mask_source: None,
        box_alignment: DEFAULT_ALIGNMENT,
    })
}

fn diagnose(omitted: &mut Vec<OmittedTextFeature>, field: &str, replacement: &str) {
    omitted.push(OmittedTextFeature::LegacyControl(format!(
        "{LEGACY}: {field}: {replacement}"
    )));
}

/// Unconsumed values are not admission requirements. Inactive decoration values
/// do not affect the picture; enabled/unidentified controls get local diagnostics.
fn diagnose_paragraph(
    controls: &BTreeMap<String, Value>,
    stroke_visible: bool,
    omitted: &mut Vec<OmittedTextFeature>,
) {
    for (field, value) in controls {
        let inactive = match field.as_str() {
            "mHindiDigits" | "mIndic" | "mIsVerticalText" | "mLigatures" | "mRTL"
            | "mBackFillVisible" => value == &Value::Bool(false),
            "mShadowVisible" | "mShadowColor" | "mShadowOpacity" | "mShadowAngle"
            | "mShadowOffset" | "mShadowSize" | "mShadowBlur" => true,
            "mDefaultRun" => value.as_array().is_some_and(Vec::is_empty),
            "mNumStrokes" => value == &Value::from(1),
            "mLineCapType" | "mLineJoinType" => !stroke_visible || value == &Value::from(0),
            "mMiterLimit" => !stroke_visible || value == &Value::from(2.5),
            "mBackFillColor" | "mBackFillOpacity" | "mBackFillSize" | "mTabWidth" => true,
            _ => false,
        };
        if !inactive {
            diagnose(
                omitted,
                field,
                "unmapped paragraph control omitted; actual text and supported layout retained",
            );
        }
    }
}

fn legacy_shadow(
    controls: &BTreeMap<String, Value>,
    omitted: &mut Vec<OmittedTextFeature>,
) -> Option<PrTextShadow> {
    match controls.get("mShadowVisible") {
        None | Some(Value::Bool(false)) => return None,
        Some(Value::Bool(true)) => {}
        _ => {
            diagnose(
                omitted,
                "mShadowVisible",
                "invalid shadow flag; shadow omitted, text retained",
            );
            return None;
        }
    }
    let decoded = (|| -> Result<PrTextShadow> {
        let number = |field: &str| -> Result<f32> {
            controls
                .get(field)
                .and_then(Value::as_f64)
                // Native shadow parameters use f32; validate rejects narrowing overflow.
                .map(|value| value as f32)
                .ok_or_else(|| unsupported(format!("missing or invalid {field}")))
        };
        let color = controls
            .get("mShadowColor")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| unsupported("missing or invalid mShadowColor"))?;
        let [high, red, green, blue] = color.to_be_bytes();
        ensure!(
            high == 0 && red == green && green == blue,
            "non-gray mShadowColor has unverified legacy channel order"
        );
        let shadow = PrTextShadow {
            color: PrRgb([red, green, blue]),
            opacity: number("mShadowOpacity")?,
            angle: number("mShadowAngle")?,
            distance: number("mShadowOffset")?,
            size: number("mShadowSize")?,
            blur: number("mShadowBlur")?,
        };
        shadow
            .validate()
            .map_err(|error| unsupported(error.to_string()))?;
        Ok(shadow)
    })();
    match decoded {
        Ok(shadow) => {
            diagnose(omitted, "mShadowVisible", "legacy shadow uses the existing calibrated text DropShadow mapping; legacy blur/size and native edge fidelity are unmeasured");
            Some(shadow)
        }
        Err(error) => {
            diagnose(
                omitted,
                "mShadowVisible",
                &format!("{error}; shadow omitted, text retained"),
            );
            None
        }
    }
}

fn run_is(value: &Value, default: &Value) -> bool {
    value
        .get("mParamValues")
        .and_then(Value::as_array)
        .is_some_and(|runs| {
            !runs.is_empty()
                && runs.iter().all(|run| {
                    run.as_array().is_some_and(|pair| {
                        pair.len() == 2
                            && (&pair[1] == default
                                || pair[1]
                                    .as_f64()
                                    .zip(default.as_f64())
                                    .is_some_and(|(value, default)| value == default))
                    })
                })
        })
}

fn fill_spans(
    runs: &Run<u32>,
    text: &str,
    all_caps: bool,
    omitted: &mut Vec<OmittedTextFeature>,
) -> Vec<crate::schema::text::PrTextFillSpan> {
    use crate::schema::text::PrTextFillSpan;
    // Native offsets count UTF-16 units; FX selectors count rendered Unicode
    // scalars, after paragraph normalization and the all-caps transform.
    let mut boundaries = BTreeMap::from([(0_u64, 0_usize)]);
    let mut units = 0;
    let mut index = 0;
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        units += character.len_utf16() as u64;
        if character == '\r' && chars.peek() == Some(&'\n') {
            chars.next();
            units += 1;
        }
        index += if all_caps {
            character.to_uppercase().count()
        } else {
            1
        };
        boundaries.insert(units, index);
    }
    let mut values: Vec<_> = std::iter::once((0, runs.value))
        .chain(runs.later.iter().copied())
        .collect();
    // Redundant per-code-unit values are not color boundaries, including
    // repeated values inside a surrogate pair or CRLF.
    values.dedup_by(|later, earlier| later.1 == earlier.1);
    let mut spans: Vec<PrTextFillSpan> = Vec::new();
    for (run, &(start, value)) in values.iter().enumerate() {
        if start >= units {
            diagnose(
                omitted,
                "mFillColor",
                "insertion defaults omitted; they do not paint existing characters",
            );
            break;
        }
        let end = values.get(run + 1).map_or(units, |next| next.0.min(units));
        let (Some(&start), Some(&end)) = (boundaries.get(&start), boundaries.get(&end)) else {
            diagnose(omitted, "mFillColor", "color boundary splits a surrogate pair or CRLF; existing first-style paint retained");
            return Vec::new();
        };
        if value == runs.value {
            continue;
        }
        let color = paint("mFillColor", value, omitted);
        if let Some(previous) = spans
            .last_mut()
            .filter(|span| span.range.end == start && span.color == color)
        {
            previous.range.end = end;
        } else {
            spans.push(PrTextFillSpan {
                range: start..end,
                color,
            });
        }
    }
    spans
}

/// Legacy text paint packs opaque RGB as 0xRRGGBB, not a COLORREF.
fn paint(field: &str, value: u32, omitted: &mut Vec<OmittedTextFeature>) -> PrRgb {
    let [high, red, green, blue] = value.to_be_bytes();
    if high == 0 {
        PrRgb([red, green, blue])
    } else {
        diagnose(omitted, field, "unmapped legacy color outside the 24-bit RGB form replaced with the white editable text default");
        super::DEFAULT_FILL
    }
}

fn json_text(payload: &[u8]) -> Result<(String, Option<u64>)> {
    let Some((count, text)) = payload.split_first_chunk::<8>() else {
        return Err(unsupported(format!("truncated {LEGACY}")));
    };
    let count = u64::from_le_bytes(*count);
    // Native saves can leave a stale length after editing the document. Never
    // slice by it: decode the bounded payload and require one complete JSON
    // object (including end-of-input) in decode before retaining any content.
    let stale_count =
        (!u64::try_from(text.len()).is_ok_and(|bytes| bytes == count)).then_some(count);
    ensure!(
        text.len() % 2 == 0,
        "{LEGACY}: an odd byte count {} is not UTF-16",
        text.len()
    );
    char::decode_utf16(
        text.as_chunks::<2>()
            .0
            .iter()
            .map(|unit| u16::from_le_bytes([unit[0], unit[1]])),
    )
    .collect::<std::result::Result<String, _>>()
    .map(|json| (json, stale_count))
    .map_err(|error| unsupported(format!("{LEGACY}: invalid UTF-16: {error}")))
}

#[cfg(test)]
mod tests;
