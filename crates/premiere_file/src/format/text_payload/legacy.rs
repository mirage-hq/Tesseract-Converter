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
    #[serde(rename = "mCapsOption", deserialize_with = "one_run")]
    caps_option: Run<u32>,
    #[serde(rename = "mFillColor", deserialize_with = "one_run")]
    fill_color: Run<u32>,
    #[serde(rename = "mFillOverStroke", deserialize_with = "one_run")]
    fill_over_stroke: Run<bool>,
    #[serde(rename = "mFillVisible", deserialize_with = "one_run")]
    fill_visible: Run<bool>,
    #[serde(rename = "mFontName", deserialize_with = "one_run")]
    font: Run<String>,
    #[serde(rename = "mFontSize", deserialize_with = "one_run")]
    size: Run<f32>,
    #[serde(rename = "mStrokeColor", deserialize_with = "one_run")]
    stroke_color: Run<u32>,
    #[serde(rename = "mStrokeVisible", deserialize_with = "one_run")]
    stroke_visible: Run<bool>,
    #[serde(rename = "mStrokeWidth", deserialize_with = "one_run")]
    stroke_width: Run<f32>,
    #[serde(rename = "mText")]
    text: String,
    #[serde(rename = "mTracking", deserialize_with = "one_run")]
    tracking: Run<f32>,
    #[serde(flatten)]
    controls: BTreeMap<String, Value>,
}

struct Run<T> {
    value: T,
    metadata: BTreeMap<String, Value>,
}

impl<T> Run<T> {
    fn into_value(self, field: &str, omitted: &mut Vec<OmittedTextFeature>) -> T {
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

/// The editable uniform document requires one actual character style starting
/// at zero. Unmapped character controls do not pass through this admission.
fn one_run<'de, D, T>(deserializer: D) -> std::result::Result<Run<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
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
    match <[(u64, T); 1]>::try_from(values) {
        Ok([(0, value)]) => Ok(Run { value, metadata }),
        Ok([(start, _)]) => Err(D::Error::custom(format!(
            "its one style run starts at character {start}, not 0"
        ))),
        Err(values) => Err(D::Error::custom(format!(
            "mixed text styles are unsupported: a style holds {} runs, not one",
            values.len()
        ))),
    }
}

pub(super) fn decode(payload: &[u8]) -> Result<DecodedGraphicText> {
    let json = json_text(payload)?;
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
    let mut omitted = Vec::new();
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
    let caps_option = caps_option.into_value("mCapsOption", &mut omitted);
    let fill_color = fill_color.into_value("mFillColor", &mut omitted);
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
    // PrTextDocument::validate bounds the target's line spacing at 0.8 em.
    // Keep supported leading unchanged; recover only below that capability.
    let leading = if f64::from(leading) >= -0.4 * f64::from(size) {
        leading
    } else {
        diagnose(
            &mut omitted,
            "mLeading",
            "line spacing below the editable 0.8 em minimum replaced with automatic leading",
        );
        0.0
    };
    let text = normalize_line_breaks(&text);
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
        stroke,
        shadow: None,
        all_caps,
        tracking,
        leading,
        justification,
        frame,
        background: None,
    };
    Ok(DecodedGraphicText {
        documents: TextDocuments::Uniform(document),
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
            | "mShadowVisible" | "mBackFillVisible" => value == &Value::Bool(false),
            "mDefaultRun" => value.as_array().is_some_and(Vec::is_empty),
            "mNumStrokes" => value == &Value::from(1),
            "mLineCapType" | "mLineJoinType" => !stroke_visible || value == &Value::from(0),
            "mMiterLimit" => !stroke_visible || value == &Value::from(2.5),
            "mBackFillColor" | "mBackFillOpacity" | "mBackFillSize" | "mShadowAngle"
            | "mShadowBlur" | "mShadowColor" | "mShadowOffset" | "mShadowOpacity"
            | "mShadowSize" | "mTabWidth" => true,
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

/// Gray paint is independent of the unverified legacy channel order. Other
/// colors use the existing white text default, not a guessed channel mapping.
fn paint(field: &str, value: u32, omitted: &mut Vec<OmittedTextFeature>) -> PrRgb {
    let [high, red, green, blue] = value.to_be_bytes();
    if high == 0 && red == green && green == blue {
        PrRgb([red, green, blue])
    } else {
        diagnose(omitted, field, "unmapped legacy color replaced with the white editable text default; legacy channel order is unverified");
        super::DEFAULT_FILL
    }
}

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

#[cfg(test)]
mod tests;
