//! Editable import of AE Source Text and the destination-supported Text Animator subset.
//! Binary layout reference: MIT py-aep e12a451c35bacd3f34a080090265f9370e66162b.

use std::sync::Arc;

use fx_schema::animator::AnimationGraphEntry;
use fx_schema::{
    AnchorPointGrouping, FxItemId, GroupLayer, Justification, LayerData as FxLayer, LayerId,
    NonNegativeProperty, PercentageProperty, Position, PositiveProperty, PropertyTarget,
    RangeSelector, TextAnchorOptions, TextAnimator, TextDocument, TextLayer, TextPathOptions, Time,
    TimeRangeProperty, Transform, WigglySelector,
};

use crate::{
    properties::{NumericProperty, PropertyError, read_numeric, root_runs, runs, unique_list},
    rifx::Chunk,
    structure::Layer,
};

use super::{
    animation::{NumericAnimationClock, NumericAnimationTarget, numeric_entries},
    animation_budget::AnimationBudget,
    control_links,
};

mod alternating;
mod cos;
mod expression_links;

use cos::Value;
use expression_links::ExpressionLinks;

/// Editable TextLayers of one AE text layer and their keyed control tracks.
#[derive(Default)]
pub(super) struct TextImport {
    pub(super) layers: Vec<FxLayer>,
    pub(super) animations: Vec<AnimationGraphEntry>,
    pub(super) warnings: Vec<String>,
}

/// Test convenience for fixtures without native text-path masks.
#[cfg(test)]
fn import(
    layer: &Layer,
    occurrence: &GroupLayer,
    next_id: &mut u64,
) -> (Vec<FxLayer>, Vec<String>) {
    let imported = import_with_mask_guides(
        layer,
        occurrence,
        &[],
        next_id,
        &mut AnimationBudget::default(),
    );
    (imported.layers, imported.warnings)
}

/// Imports text and its keyed control tracks while binding AE's one-based
/// Path Options mask index to the corresponding guide id returned by the mask
/// importer. A lowered Slider percent expression replaces the authored
/// documents, and a lowered alternating Expression Selector expands its
/// animators, only if the whole expansion is admitted.
pub(super) fn import_with_mask_guides(
    layer: &Layer,
    occurrence: &GroupLayer,
    mask_guide_ids: &[(u32, LayerId)],
    next_id: &mut u64,
    budget: &mut AnimationBudget,
) -> TextImport {
    let mut warnings = Vec::new();
    let source = match read_source(layer) {
        Ok(Some(source)) => source,
        Ok(None) => return TextImport::default(),
        Err(error) => {
            warnings.push(format!("Source Text could not be decoded: {error}"));
            return TextImport {
                warnings,
                ..TextImport::default()
            };
        }
    };
    if source.documents.is_empty() {
        warnings.push("Source Text contains no text documents; text content omitted".into());
        return TextImport {
            warnings,
            ..TextImport::default()
        };
    }

    let mut rejected_expansion = None;
    if let Some(percent) = &source.percent {
        match import_percent_holds(
            &source,
            percent,
            occurrence,
            mask_guide_ids,
            next_id,
            budget,
        ) {
            Ok(imported) => return imported,
            Err(reason) => rejected_expansion = Some(expression_not_lowered(&reason)),
        }
    }
    let rejected_alternation = match alternating::candidate(&source) {
        Ok(None) => None,
        Ok(Some(candidate)) => {
            match import_alternating(&candidate, occurrence, mask_guide_ids, next_id, budget) {
                Ok(imported) => return imported,
                Err(reason) => Some(reason),
            }
        }
        Err(reason) => Some(reason),
    };
    let ranges = document_hold_ranges(
        source.documents.len(),
        &source.document_starts,
        &mut warnings,
    );
    let layers = held_segments(
        &source,
        None,
        ranges,
        occurrence,
        mask_guide_ids,
        next_id,
        &mut warnings,
    );
    if source.documents.len() > 1 {
        warnings.push(
            "timed Source Text is represented by hold-segment editable TextLayers; text-document interpolation and per-run transitions are not available in the destination"
                .into(),
        );
    }
    warnings.extend(rejected_expansion);
    warnings.extend(
        rejected_alternation
            .as_deref()
            .map(alternating::not_lowered),
    );
    let (animations, animation_warnings) = segment_animation_entries(&source, &layers, budget);
    warnings.extend(source.warnings);
    warnings.extend(animation_warnings);
    TextImport {
        layers,
        animations,
        warnings,
    }
}

/// Imports a lowered Slider percent expression only as a whole. Its Hold
/// segments take one checked reservation of generated identifiers, and all
/// their copied control tracks are admitted before either is committed. A
/// rejection leaves the identifier cursor unchanged and rolls back the
/// provisional track reservations.
fn import_percent_holds(
    source: &SourceText,
    percent: &PercentHolds,
    occurrence: &GroupLayer,
    mask_guide_ids: &[(u32, LayerId)],
    next_id: &mut u64,
    budget: &mut AnimationBudget,
) -> Result<TextImport, String> {
    let mut warnings = Vec::new();
    let ranges = document_hold_ranges(percent.texts.len(), &percent.starts, &mut warnings);
    let segments = ranges.len();
    let mut candidate_id = *next_id;
    let Some(mut segment_cursor) = segment_id_count(source, mask_guide_ids, segments)
        .and_then(|count| super::reserve_ids(&mut candidate_id, count))
    else {
        return Err(format!(
            "its {segments} Hold segments exceed the remaining generated identifier space"
        ));
    };
    let layers = held_segments(
        source,
        Some(percent),
        ranges,
        occurrence,
        mask_guide_ids,
        &mut segment_cursor,
        &mut warnings,
    );
    debug_assert_eq!(
        segment_cursor, candidate_id,
        "the Hold segments use exactly their reserved identifiers"
    );
    let checkpoint = budget.checkpoint();
    let denials = budget.denials();
    let (animations, animation_warnings) = segment_animation_entries(source, &layers, budget);
    if budget.denials() > denials {
        budget.rollback(checkpoint);
        return Err(format!(
            "the copied control tracks of its {segments} Hold segments exceed the generated-animation allowance"
        ));
    }
    *next_id = candidate_id;
    warnings.push(format!(
        "Source Text: enabled same-layer Slider percent expression lowered to {} independent editable Hold text segment(s); the live Slider linkage is not retained",
        percent.texts.len()
    ));
    warnings.extend(source.warnings.iter().cloned());
    warnings.extend(animation_warnings);
    Ok(TextImport {
        layers,
        animations,
        warnings,
    })
}

/// Imports an expanded alternating text only as a whole, like
/// [`import_percent_holds`]: its identifiers are reserved before any is
/// allocated, and all its tracks, including both copies of each expanded Range
/// Selector's keys, are admitted before any is committed. A rejection leaves
/// the identifier cursor unchanged and rolls back the provisional track
/// reservations, so the caller imports the unexpanded source.
fn import_alternating(
    candidate: &alternating::Candidate,
    occurrence: &GroupLayer,
    mask_guide_ids: &[(u32, LayerId)],
    next_id: &mut u64,
    budget: &mut AnimationBudget,
) -> Result<TextImport, String> {
    let source = &candidate.source;
    let mut warnings = Vec::new();
    let ranges = document_hold_ranges(
        source.documents.len(),
        &source.document_starts,
        &mut warnings,
    );
    let mut candidate_id = *next_id;
    let Some(mut cursor) = segment_id_count(source, mask_guide_ids, ranges.len())
        .and_then(|count| super::reserve_ids(&mut candidate_id, count))
    else {
        return Err(
            "its correction animators exceed the remaining generated identifier space".into(),
        );
    };
    let layers = held_segments(
        source,
        None,
        ranges,
        occurrence,
        mask_guide_ids,
        &mut cursor,
        &mut warnings,
    );
    debug_assert_eq!(
        cursor, candidate_id,
        "the expanded text uses exactly its reserved identifiers"
    );
    let checkpoint = budget.checkpoint();
    let denials = budget.denials();
    let (animations, animation_warnings) = segment_animation_entries(source, &layers, budget);
    let rejection = if budget.denials() > denials {
        Some("its copied Range Selector tracks exceed the generated-animation allowance")
    } else if !candidate.tracks_complete(&layers, &animations) {
        Some("a keyed Range Selector field has no editable track on both copies")
    } else {
        None
    };
    if let Some(reason) = rejection {
        budget.rollback(checkpoint);
        return Err(reason.into());
    }
    *next_id = candidate_id;
    warnings.extend(candidate.lowered());
    warnings.extend(source.warnings.iter().cloned());
    warnings.extend(animation_warnings);
    Ok(TextImport {
        layers,
        animations,
        warnings,
    })
}

/// Generated identifiers that `segments` held TextLayers take: one for each
/// layer and one for each of its text controls. The controls are converted
/// once on a scratch counter, so the count follows the allocating code.
fn segment_id_count(
    source: &SourceText,
    mask_guide_ids: &[(u32, LayerId)],
    segments: usize,
) -> Option<u64> {
    let mut control_ids = 0;
    let _ = convert_text_properties(source, mask_guide_ids, &mut control_ids);
    u64::try_from(segments)
        .ok()?
        .checked_mul(control_ids.checked_add(1)?)
}

/// One editable TextLayer per held range, showing the authored document or,
/// with `percent`, that range's lowered expression string.
fn held_segments(
    source: &SourceText,
    percent: Option<&PercentHolds>,
    ranges: Vec<(usize, f64, f64)>,
    occurrence: &GroupLayer,
    mask_guide_ids: &[(u32, LayerId)],
    next_id: &mut u64,
    warnings: &mut Vec<String>,
) -> Vec<FxLayer> {
    let count = ranges.len();
    let mut layers = Vec::with_capacity(count);
    for (segment_index, (index, start, end)) in ranges.into_iter().enumerate() {
        let Some(id) = allocate_layer_id(next_id) else {
            warnings.push(
                "Source Text content omitted: generated layer identifier space exhausted".into(),
            );
            break;
        };
        let (document, mut document_warnings) = source.held_document(percent, index);
        let mut transform = identity_transform();
        let native_document = &source.documents[if percent.is_some() { 0 } else { index }];
        if let Some(scale) = point_text_scale(source, native_document, &document) {
            transform.scale = [100.0 * scale[0], 100.0 * scale[1]];
            document_warnings.retain(|warning| !warning.starts_with("Source Text glyph scales"));
            document_warnings.push("Source Text uniform point-text glyph scale is approximated by an editable inner Text transform; tracking and raster stroke scale geometrically, while the authored occurrence position and clock remain unchanged".into());
        }
        warnings.append(&mut document_warnings);
        let (animators, anchor_options, path_options, mut property_warnings) =
            convert_text_properties(source, mask_guide_ids, next_id);
        warnings.append(&mut property_warnings);
        layers.push(FxLayer::Text(TextLayer {
            id,
            name: if count == 1 {
                occurrence.name.clone()
            } else {
                format!("{} (Source Text {})", occurrence.name, segment_index + 1)
            },
            description: "Editable AE Source Text; text style/layout imported without flattened media"
                .into(),
            is_hidden: false,
            parent: Some(occurrence.id),
            blend_mode: Default::default(),
            track_matte: None,
            masks: Vec::new(),
            active_range: hold_range(start, end),
            effects: Vec::new(),
            motion_blur: false,
            transform,
            source_text: document,
            animators,
            path_options,
            anchor_options,
        }));
    }
    layers
}

/// A held value's source-local range. Adjacent values share one boundary, so
/// no two overlap: the whole millisecond at or before the key, since FX time
/// cannot store a key between milliseconds (11/30 s is 366.67 ms). Rounding
/// such a key up to 367 ms hid its value at the key's own frame wherever time
/// is sampled exactly, as After Effects samples an export.
fn hold_range(start: f64, end: f64) -> TimeRangeProperty {
    let start = hold_boundary(start);
    TimeRangeProperty::new(start, hold_boundary(end).saturating_sub(start))
}

/// A product within float error of a whole millisecond is that millisecond
/// (tick 30030 of 30000 per second is 1000.9999999999999 ms); native keys are
/// whole ticks, never that close to one without being on it.
fn hold_boundary(seconds: f64) -> Time {
    let millis = seconds * 1000.0;
    let nearest = millis.round();
    Time::from_millis_f64(if (millis - nearest).abs() < 1e-6 {
        nearest
    } else {
        millis.floor()
    })
}

fn character_style(document: &Value) -> Option<&Value> {
    at(
        document,
        &[
            Key::Name("0"),
            Key::Name("6"),
            Key::Name("0"),
            Key::Index(0),
            Key::Name("0"),
            Key::Name("0"),
            Key::Name("6"),
        ],
    )
}

fn point_text_scale(
    source: &SourceText,
    document: &Value,
    converted: &TextDocument,
) -> Option<[f64; 2]> {
    if converted.box_text || !source.path_options.is_empty() || run_count(document, "6") != 1 {
        return None;
    }
    let raw_text = at(document, &[Key::Name("0"), Key::Name("0")])?.as_str()?;
    let runs = at(document, &[Key::Name("0"), Key::Name("6"), Key::Name("0")])?.as_array()?;
    if runs[0].get("1")?.as_f64()? != raw_text.encode_utf16().count() as f64 {
        return None;
    }
    let style = character_style(document)?;
    let scale = [
        number(style, "6").unwrap_or(1.0),
        number(style, "7").unwrap_or(1.0),
    ];
    if scale == [1.0, 1.0]
        || scale
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0 || !(*v * 100.0).is_finite())
    {
        return None;
    }
    for animator in &source.animators {
        for (name, property) in &animator.properties {
            let Ok(property) = property else {
                return None;
            };
            if property.expression_enabled
                || (property.animated && property.keyframes.is_empty())
                || (property.values.is_empty() && property.keyframes.is_empty())
            {
                return None;
            }
            let values = || {
                std::iter::once(property.values.as_slice())
                    .filter(|values| !values.is_empty())
                    .chain(property.keyframes.iter().map(|key| key.values.as_slice()))
            };
            let finite_scalar = || values().all(|v| v.len() == 1 && v[0].is_finite());
            match name.as_str() {
                "ADBE Text Opacity" | "ADBE Text Stroke Width" if finite_scalar() => (),
                "ADBE Text Position 3D"
                    if scale[1] == 1.0
                        && values().all(|v| {
                            v.len() == 3 && v[0] == 0.0 && v[1].is_finite() && v[2] == 0.0
                        })
                        && property.keyframes.iter().all(|key| {
                            key.spatial_in
                                .iter()
                                .chain(&key.spatial_out)
                                .all(|v| *v == 0.0)
                        }) => {}
                _ => return None,
            }
        }
    }
    Some(scale)
}

/// Fixed source-layout width for a deliberately narrow caption expression.
/// Vertical-only text animators and opacity cannot change its horizontal extent.
/// This is not a live sourceRectAtTime binding after text or font edits.
pub(super) fn cached_caption_width(layer: &Layer) -> Result<f64, &'static str> {
    let source = read_source(layer)
        .map_err(|_| "caption Source Text cache is malformed")?
        .ok_or("preceding layer has no Source Text")?;
    if source.documents.len() != 1
        || !source.document_is_static
        || !source.document_starts.is_empty()
    {
        return Err("caption width requires one static text document");
    }
    let document = &source.documents[0];
    let raw_text = at(document, &[Key::Name("0"), Key::Name("0")])
        .and_then(Value::as_str)
        .ok_or("caption text content is missing")?;
    let text = raw_text.trim_end_matches(['\r', '\n']);
    let paragraphs = at(document, &[Key::Name("0"), Key::Name("5"), Key::Name("0")])
        .and_then(Value::as_array)
        .ok_or("caption paragraph-style runs missing")?;
    // Repeated identical style runs for trailing empty paragraphs add no
    // horizontal geometry. Require their UTF-16 lengths and empty CR suffix
    // explicitly, rather than admitting mixed styles or visible extra lines.
    let equivalent_empty_paragraphs = paragraphs.first().is_some_and(|first| {
        paragraphs.iter().all(|run| run.get("0") == first.get("0"))
            && (paragraphs.len() == 1
                || (first.get("1").and_then(Value::as_f64)
                    == Some((text.encode_utf16().count() + 1) as f64)
                    && raw_text[text.len()..] == "\r".repeat(paragraphs.len())
                    && paragraphs[1..]
                        .iter()
                        .all(|run| run.get("1").and_then(Value::as_f64) == Some(1.0))))
    });
    if text.is_empty()
        || text.contains(['\r', '\n'])
        || run_count(document, "6") != 1
        || !equivalent_empty_paragraphs
        || !source.path_options.is_empty()
        || !source.more_options.is_empty()
        || !source.warnings.is_empty()
    {
        return Err("caption cached width requires one line/style and no text path/options");
    }
    for animator in &source.animators {
        if animator.properties.iter().any(|(name, numeric)| {
            let Ok(numeric) = numeric else {
                return true;
            };
            if numeric.expression_enabled || numeric.animated || !numeric.keyframes.is_empty() {
                return true;
            }
            match name.as_str() {
                "ADBE Text Position 3D" => {
                    numeric.values.len() != 3
                        || numeric.values[0] != 0.0
                        || numeric.values[2] != 0.0
                        || !numeric.values[1].is_finite()
                }
                "ADBE Text Opacity" => numeric.values.len() != 1 || !numeric.values[0].is_finite(),
                _ => true,
            }
        }) {
            return Err("caption animator may change cached horizontal bounds");
        }
    }
    let layout = document
        .get("1")
        .and_then(|value| value.get("2"))
        .ok_or("caption cached layout is missing")?;
    let mut lines = 0;
    let mut boxes = Vec::new();
    fn collect(value: &Value, lines: &mut usize, boxes: &mut Vec<[f64; 4]>) -> Option<()> {
        match value.get("99").and_then(Value::as_str) {
            Some("L") => *lines += 1,
            Some("G") => {
                let bounds = value.get("8")?.as_array()?;
                if bounds.len() != 4 {
                    return None;
                }
                let bounds = [
                    bounds[0].as_f64()?,
                    bounds[1].as_f64()?,
                    bounds[2].as_f64()?,
                    bounds[3].as_f64()?,
                ];
                if !bounds.into_iter().all(f64::is_finite)
                    || bounds[2] <= bounds[0]
                    || bounds[3] <= bounds[1]
                {
                    return None;
                }
                boxes.push(bounds);
            }
            _ => {}
        }
        match value {
            Value::Array(values) => {
                for value in values {
                    collect(value, lines, boxes)?;
                }
            }
            Value::Dict(values) => {
                for value in values.values() {
                    collect(value, lines, boxes)?;
                }
            }
            _ => {}
        }
        Some(())
    }
    collect(layout, &mut lines, &mut boxes).ok_or("caption cached glyph bounds are malformed")?;
    if lines != 1 || boxes.is_empty() {
        return Err("caption cached layout is not one glyph line");
    }
    let left = boxes
        .iter()
        .map(|bounds| bounds[0])
        .fold(f64::INFINITY, f64::min);
    let right = boxes
        .iter()
        .map(|bounds| bounds[2])
        .fold(f64::NEG_INFINITY, f64::max);
    let width = right - left;
    if !width.is_finite() || width <= 0.0 {
        return Err("caption cached width is invalid");
    }
    Ok(width)
}

fn document_hold_ranges(
    document_count: usize,
    starts: &[f64],
    warnings: &mut Vec<String>,
) -> Vec<(usize, f64, f64)> {
    if document_count == 1 && starts.is_empty() {
        return vec![(0, 0.0, super::MAX_TIME_SECS)];
    }
    if starts.len() != document_count
        || !starts.iter().all(|value| value.is_finite())
        || !starts.windows(2).all(|pair| pair[0] <= pair[1])
    {
        warnings.push(
            "Source Text document/keyframe times are missing, unordered, or mismatched; only the first document was imported"
                .into(),
        );
        return (document_count != 0)
            .then_some((0, 0.0, super::MAX_TIME_SECS))
            .into_iter()
            .collect();
    }

    let mut ranges = Vec::with_capacity(document_count);
    for index in 0..document_count {
        // AE holds the first Source Text value backward before its first key.
        // Multiple keys at or before source-local zero collapse to the last one.
        let start = if index == 0 {
            0.0
        } else {
            starts[index].max(0.0)
        };
        let end = starts
            .get(index + 1)
            .copied()
            .unwrap_or(super::MAX_TIME_SECS)
            .max(0.0);
        if end <= start {
            warnings.push(format!(
                "Source Text document {} has no visible hold interval after source-local time clamping and was omitted",
                index + 1
            ));
        } else {
            ranges.push((index, start, end));
        }
    }
    ranges
}

/// Test convenience: [`segment_animation_entries`] for `imported` segments of
/// a native text layer.
#[cfg(test)]
pub(super) fn animation_entries(
    layer: &Layer,
    imported: &[FxLayer],
    budget: &mut AnimationBudget,
) -> (Vec<AnimationGraphEntry>, Vec<String>) {
    let source = match read_source(layer) {
        Ok(Some(source)) => source,
        Ok(None) => return (Vec::new(), Vec::new()),
        Err(error) => {
            return (
                Vec::new(),
                vec![format!(
                    "Text animation source could not be decoded: {error}"
                )],
            );
        }
    };
    segment_animation_entries(&source, imported, budget)
}

/// Builds keyed graph entries after import so targets reuse the already-minted FX item ids.
fn segment_animation_entries(
    source: &SourceText,
    imported: &[FxLayer],
    budget: &mut AnimationBudget,
) -> (Vec<AnimationGraphEntry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    for (segment_index, text) in imported
        .iter()
        .filter_map(|layer| match layer {
            FxLayer::Text(text) => Some(text),
            _ => None,
        })
        .enumerate()
    {
        let segment = format!("Text segment {}", segment_index + 1);
        let clock = NumericAnimationClock::source_local_rebased(text.active_range.start.as_secs());
        for (source_animator, animator) in source.animators.iter().zip(&text.animators) {
            append_animator_animation(
                source_animator,
                animator,
                &format!("{segment} {}", source_animator.name),
                clock,
                &mut entries,
                &mut warnings,
                budget,
            );
        }
        if source.animators.len() > text.animators.len()
            && source.animators[text.animators.len()..]
                .iter()
                .any(animator_has_keyframes)
        {
            warnings.push(format!(
                "{segment} has keyed Text Animators without imported target ids; those tracks were omitted"
            ));
        }
        match (&source.more_options[..], text.anchor_options.as_ref()) {
            (properties, Some(options)) => append_numeric_animation(
                properties,
                "ADBE Text Anchor Point Align",
                &format!("{segment} Text More Options"),
                NumericAnimationTarget::vector2(
                    PropertyTarget::fx_item(options.id, "groupingAlignment"),
                    [0, 1],
                    [1.0, 1.0],
                ),
                clock,
                TextAnimationOutput::new(&mut entries, &mut warnings, budget),
            ),
            (properties, None) if has_keyframes(properties, "ADBE Text Anchor Point Align") => {
                warnings.push(format!(
                    "{segment} Grouping Alignment animation has no imported TextAnchorOptions target and was omitted"
                ));
            }
            _ => {}
        }
        match (&source.path_options[..], text.path_options.as_ref()) {
            (properties, Some(options)) => {
                append_numeric_animation(
                    properties,
                    "ADBE Text First Margin",
                    &format!("{segment} Text Path Options"),
                    NumericAnimationTarget::float(
                        PropertyTarget::fx_item(options.id, "firstMargin"),
                        0,
                        1.0,
                    ),
                    clock,
                    TextAnimationOutput::new(&mut entries, &mut warnings, budget),
                );
                append_numeric_animation(
                    properties,
                    "ADBE Text Last Margin",
                    &format!("{segment} Text Path Options"),
                    NumericAnimationTarget::float(
                        PropertyTarget::fx_item(options.id, "lastMargin"),
                        0,
                        1.0,
                    ),
                    clock,
                    TextAnimationOutput::new(&mut entries, &mut warnings, budget),
                );
            }
            (properties, None)
                if ["ADBE Text First Margin", "ADBE Text Last Margin"]
                    .into_iter()
                    .any(|name| has_keyframes(properties, name)) =>
            {
                warnings.push(format!(
                    "{segment} has keyed Path Options margins, but no imported mask guide/path target; those tracks were omitted"
                ));
            }
            _ => {}
        }
    }
    (entries, warnings)
}

#[derive(Clone)]
struct SourceText {
    document_is_static: bool,
    fonts: Vec<Option<String>>,

    documents: Vec<Value>,
    document_starts: Vec<f64>,
    /// A lowered Slider percent expression, imported in place of the single
    /// cached document only if its whole expansion is admitted.
    percent: Option<PercentHolds>,
    frame: Option<Value>,
    animators: Vec<AnimatorSource>,
    more_options: Vec<(String, Result<NumericProperty, PropertyError>)>,
    path_options: Vec<(String, Result<NumericProperty, PropertyError>)>,
    warnings: Vec<String>,
}

/// The string a lowered Slider percent expression shows while the Slider
/// holds each value, and each value's source-local start (none when static).
#[derive(Clone)]
struct PercentHolds {
    starts: Vec<f64>,
    texts: Vec<String>,
}

impl SourceText {
    /// The editable document held from start `index`. A lowered expression
    /// string replaces the text of the single cached document, keeping the
    /// first-run style that the whole-layer destination document applies.
    fn held_document(
        &self,
        percent: Option<&PercentHolds>,
        index: usize,
    ) -> (TextDocument, Vec<String>) {
        let Some(percent) = percent else {
            return convert_document(&self.documents[index], &self.fonts, self.frame.as_ref());
        };
        let (mut document, warnings) =
            convert_document(&self.documents[0], &self.fonts, self.frame.as_ref());
        document.text.clone_from(&percent.texts[index]);
        (document, warnings)
    }
}

#[derive(Clone)]
struct AnimatorSource {
    /// The destination animator name: `Animator N` for the Nth decoded native
    /// animator, `Animator N alternating position` for its correction.
    /// Diagnostics use it, so a correction does not renumber later siblings.
    name: String,
    properties: Vec<(String, Result<NumericProperty, PropertyError>)>,
    selectors: Vec<SelectorSource>,
}

#[derive(Clone)]
enum SelectorSource {
    Range {
        properties: Vec<(String, Result<NumericProperty, PropertyError>)>,
    },
    Wiggly {
        properties: Vec<(String, Result<NumericProperty, PropertyError>)>,
    },
    /// An Expression Selector that alternates the preceding selection's sign
    /// (see [`alternating`]). It is omitted unless its animator is expanded.
    AlternatingSign {
        properties: Vec<(String, Result<NumericProperty, PropertyError>)>,
    },
    Unsupported {
        name: String,
        properties: Vec<(String, Result<NumericProperty, PropertyError>)>,
    },
}

fn read_source(layer: &Layer) -> Result<Option<SourceText>, TextError> {
    let mut text_groups = root_runs(&layer.content)?
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Text Properties");
    let Some((_, text_group)) = text_groups.next() else {
        return Ok(None);
    };
    if text_groups.next().is_some() {
        return Err(TextError::Layout("duplicate Text groups"));
    }
    let text_group = unique_list(text_group, *b"tdgp")?;
    let mut documents = runs(text_group)?
        .into_iter()
        .filter(|(name, _)| *name == "ADBE Text Document");
    let Some((_, document_run)) = documents.next() else {
        return Err(TextError::Layout("missing Source Text property"));
    };
    if documents.next().is_some() {
        return Err(TextError::Layout("duplicate Source Text properties"));
    }
    let wrapper = unique_list(document_run, *b"btds")?;
    let mut blobs = wrapper
        .iter()
        .filter(|chunk| chunk.list_kind() == Some(*b"btdk"));
    let payload = blobs
        .next()
        .and_then(Chunk::opaque_payload)
        .ok_or(TextError::Layout("missing opaque btdk payload"))?;
    if blobs.next().is_some() {
        return Err(TextError::Layout("duplicate btdk payload"));
    }
    let document_is_static = unique_list(wrapper, *b"tdbs")
        .ok()
        .and_then(|metadata| {
            let flags = only_data(metadata, *b"tdb4").ok()?;
            (flags.len() == 124).then(|| {
                let expression_present = flags[120] & 1 != 0
                    || metadata
                        .iter()
                        .any(|chunk| matches!(&chunk.id(), b"Utf8" | b"expr"));
                flags[68] == 0
                    && !metadata
                        .iter()
                        .any(|chunk| chunk.list_kind() == Some(*b"list"))
                    && (!expression_present || flags[119] & 1 != 0)
            })
        })
        .unwrap_or(false);
    let root = cos::parse(payload)?;
    let fonts = extract_fonts(&root);
    let documents = at(&root, &[Key::Name("1"), Key::Name("1")])
        .and_then(Value::as_array)
        .ok_or(TextError::Layout("missing COS text document array"))?
        .to_vec();
    let frame = at(
        &root,
        &[
            Key::Name("0"),
            Key::Name("8"),
            Key::Name("0"),
            Key::Index(0),
            Key::Name("0"),
        ],
    )
    .cloned();
    let (document_starts, mut warnings) = read_document_starts(wrapper, documents.len());
    let percent = lower_source_text_expression(layer, wrapper, documents.len(), &mut warnings);
    let links = ExpressionLinks::new(&layer.content, text_group);
    let (animators, more_options, path_options, mut property_warnings) =
        read_text_properties(text_group, &links);
    warnings.append(&mut property_warnings);
    Ok(Some(SourceText {
        document_is_static,
        fonts,
        documents,
        document_starts,
        percent,
        frame,
        animators,
        more_options,
        path_options,
        warnings,
    }))
}

type NumericProperties = Vec<(String, Result<NumericProperty, PropertyError>)>;

fn read_text_properties<'a>(
    text_group: &'a [Chunk],
    links: &ExpressionLinks<'a>,
) -> (
    Vec<AnimatorSource>,
    NumericProperties,
    NumericProperties,
    Vec<String>,
) {
    let mut warnings = Vec::new();
    let mut animators = Vec::new();
    let mut more_options = Vec::new();
    let mut path_options = Vec::new();
    let Ok(text_runs) = runs(text_group) else {
        warnings.push("Text property groups are malformed; animators/options omitted".into());
        return (animators, more_options, path_options, warnings);
    };
    for (name, run) in text_runs {
        if matches!(name, "ADBE Text Animator" | "ADBE Text Path Options")
            && !enabled_text_group(run, name, &mut warnings)
        {
            continue;
        }
        match name {
            "ADBE Text Animators" => match unique_list(run, *b"tdgp").and_then(runs) {
                Ok(runs) => {
                    for (_, animator_run) in runs
                        .into_iter()
                        .filter(|(name, _)| *name == "ADBE Text Animator")
                    {
                        if !enabled_text_group(animator_run, "ADBE Text Animator", &mut warnings) {
                            continue;
                        }
                        match read_animator(animator_run, animators.len(), links, &mut warnings) {
                            Ok(animator) => animators.push(animator),
                            Err(error) => warnings.push(format!(
                                "Text Animator {} could not be decoded: {error}",
                                animators.len() + 1
                            )),
                        }
                    }
                }
                Err(error) => warnings.push(format!("Text Animators omitted: {error}")),
            },
            // Retain tolerance for files that materialize Animator directly.
            "ADBE Text Animator" => match read_animator(run, animators.len(), links, &mut warnings)
            {
                Ok(animator) => animators.push(animator),
                Err(error) => warnings.push(format!(
                    "Text Animator {} could not be decoded: {error}",
                    animators.len() + 1
                )),
            },
            "ADBE Text More Options" => match unique_list(run, *b"tdgp").and_then(numeric_runs) {
                Ok(properties) => more_options = properties,
                Err(error) => warnings.push(format!("Text More Options omitted: {error}")),
            },
            "ADBE Text Path Options" => match unique_list(run, *b"tdgp").and_then(numeric_runs) {
                Ok(properties) => path_options = properties,
                Err(error) => warnings.push(format!("Text Path Options omitted: {error}")),
            },
            _ => {}
        }
    }
    (animators, more_options, path_options, warnings)
}

fn enabled_text_group(run: &[Chunk], name: &str, warnings: &mut Vec<String>) -> bool {
    let enabled = crate::properties::group_enabled_or_warn(run, name, warnings);
    if !enabled {
        warnings.push(format!(
            "disabled {name} omitted; destination has no matching native enable switch"
        ));
    }
    enabled
}

fn read_animator<'a>(
    run: &'a [Chunk],
    index: usize,
    links: &ExpressionLinks<'a>,
    warnings: &mut Vec<String>,
) -> Result<AnimatorSource, PropertyError> {
    let context = format!("Text Animator {}", index + 1);
    let group = unique_list(run, *b"tdgp")?;
    let mut properties = Vec::new();
    let mut selectors = Vec::new();
    let mut range_count = 0;
    for (name, run) in runs(group)? {
        match name {
            "ADBE Text Animator Properties" => {
                let leaves = unique_list(run, *b"tdgp")?;
                properties = numeric_runs(leaves)?;
                lower_links(&[leaves], &mut properties, links, &context, warnings);
            }
            "ADBE Text Selectors" => {
                let selector_group = unique_list(run, *b"tdgp")?;
                for (selector_name, selector_run) in runs(selector_group)? {
                    if !enabled_text_group(selector_run, selector_name, warnings) {
                        continue;
                    }
                    let selector = unique_list(selector_run, *b"tdgp")?;
                    let mut values = numeric_runs(selector)?;
                    if selector_name == "ADBE Text Selector" {
                        let mut groups = vec![selector];
                        for (advanced_name, advanced_run) in runs(selector)? {
                            if advanced_name == "ADBE Text Range Advanced" {
                                let advanced = unique_list(advanced_run, *b"tdgp")?;
                                values.extend(numeric_runs(advanced)?);
                                groups.push(advanced);
                            }
                        }
                        range_count += 1;
                        let selector_context = format!("{context} Range Selector {range_count}");
                        lower_links(&groups, &mut values, links, &selector_context, warnings);
                        selectors.push(SelectorSource::Range { properties: values });
                    } else if selector_name == "ADBE Text Wiggly Selector" {
                        selectors.push(SelectorSource::Wiggly { properties: values });
                    } else if selector_name == alternating::EXPRESSION_SELECTOR
                        && alternating::alternates_sign(selector, &values)
                    {
                        selectors.push(SelectorSource::AlternatingSign { properties: values });
                    } else {
                        selectors.push(SelectorSource::Unsupported {
                            name: selector_name.to_owned(),
                            properties: values,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    Ok(AnimatorSource {
        name: format!("Animator {}", index + 1),
        properties,
        selectors,
    })
}

/// Replaces enabled expressions that resolve through bounded same-layer
/// control links with their editable values/keys; others keep their storage.
fn lower_links<'a>(
    groups: &[&'a [Chunk]],
    properties: &mut NumericProperties,
    links: &ExpressionLinks<'a>,
    context: &str,
    warnings: &mut Vec<String>,
) {
    for (name, numeric) in properties {
        let Ok(value) = numeric else {
            continue;
        };
        if !value.expression_enabled {
            continue;
        }
        match leaf_storage(groups, name).and_then(|storage| links.lower(storage, value)) {
            Ok(lowered) => {
                *value = lowered;
                warnings.push(format!("{context} {name}: bounded same-layer control expression lowered to independent editable values/keys; live control linkage is not retained"));
            }
            Err(error) => warnings.push(format!(
                "{context} {name}: control link not lowered ({error}); the stored/keyed value is used"
            )),
        }
    }
}

/// The first `tdbs` storage named `name`, as `numeric_runs` reads it.
fn leaf_storage<'a>(groups: &[&'a [Chunk]], name: &str) -> Result<&'a [Chunk], PropertyError> {
    for &group in groups {
        if let Some((_, run)) = runs(group)?
            .into_iter()
            .find(|(candidate, _)| *candidate == name)
        {
            return unique_list(run, *b"tdbs");
        }
    }
    Err(PropertyError::Layout("missing property storage"))
}

fn numeric_runs(group: &[Chunk]) -> Result<NumericProperties, PropertyError> {
    Ok(runs(group)?
        .into_iter()
        .filter_map(|(name, run)| {
            if name == "ADBE Text Range Advanced" && unique_list(run, *b"tdgp").is_ok() {
                return None;
            }
            let numeric = unique_list(run, *b"tdbs").and_then(read_numeric);
            Some((name.to_owned(), numeric))
        })
        .collect())
}

fn animator_has_keyframes(animator: &AnimatorSource) -> bool {
    animator
        .properties
        .iter()
        .any(|(_, value)| value.as_ref().is_ok_and(numeric_has_keyframes))
        || animator.selectors.iter().any(|selector| {
            selector_properties(selector)
                .iter()
                .any(|(_, value)| value.as_ref().is_ok_and(numeric_has_keyframes))
        })
}

fn numeric_has_keyframes(numeric: &NumericProperty) -> bool {
    !numeric.keyframes.is_empty()
}

fn selector_properties(
    selector: &SelectorSource,
) -> &[(String, Result<NumericProperty, PropertyError>)] {
    match selector {
        SelectorSource::Range { properties }
        | SelectorSource::Wiggly { properties }
        | SelectorSource::AlternatingSign { properties }
        | SelectorSource::Unsupported { properties, .. } => properties,
    }
}

fn has_keyframes(
    properties: &[(String, Result<NumericProperty, PropertyError>)],
    name: &str,
) -> bool {
    properties
        .iter()
        .find(|(candidate, _)| candidate == name)
        .and_then(|(_, value)| value.as_ref().ok())
        .is_some_and(numeric_has_keyframes)
}

struct TextAnimationOutput<'a> {
    entries: &'a mut Vec<AnimationGraphEntry>,
    warnings: &'a mut Vec<String>,
    budget: &'a mut AnimationBudget,
}

impl<'a> TextAnimationOutput<'a> {
    fn new(
        entries: &'a mut Vec<AnimationGraphEntry>,
        warnings: &'a mut Vec<String>,
        budget: &'a mut AnimationBudget,
    ) -> Self {
        Self {
            entries,
            warnings,
            budget,
        }
    }
}

fn append_numeric_animation(
    properties: &[(String, Result<NumericProperty, PropertyError>)],
    source_name: &str,
    context: &str,
    target: NumericAnimationTarget,
    clock: NumericAnimationClock,
    output: TextAnimationOutput<'_>,
) {
    let Some((_, numeric)) = properties
        .iter()
        .find(|(candidate, _)| candidate == source_name)
    else {
        return;
    };
    let Ok(numeric) = numeric else {
        return;
    };
    let name = format!("{context} {source_name}");
    let (mut property_entries, mut property_warnings) =
        numeric_entries(&name, numeric, &[target], clock, output.budget);
    output.entries.append(&mut property_entries);
    output.warnings.append(&mut property_warnings);
}

fn append_animator_animation(
    source: &AnimatorSource,
    animator: &TextAnimator,
    context: &str,
    clock: NumericAnimationClock,
    entries: &mut Vec<AnimationGraphEntry>,
    warnings: &mut Vec<String>,
    budget: &mut AnimationBudget,
) {
    let vector = |target_name| {
        NumericAnimationTarget::vector2(
            PropertyTarget::fx_item(animator.id, target_name),
            [0, 1],
            [1.0, 1.0],
        )
    };
    let scalar = |target_name| {
        NumericAnimationTarget::float(PropertyTarget::fx_item(animator.id, target_name), 0, 1.0)
    };
    for (source_name, target) in [
        ("ADBE Text Anchor Point 3D", vector("anchorPoint")),
        ("ADBE Text Position 3D", vector("position")),
        ("ADBE Text Scale 3D", vector("scale")),
        ("ADBE Text Blur", vector("blur")),
    ] {
        append_numeric_animation(
            &source.properties,
            source_name,
            context,
            target,
            clock,
            TextAnimationOutput::new(entries, warnings, budget),
        );
    }
    for (source_name, target_name) in [
        ("ADBE Text Rotation", "rotation"),
        ("ADBE Text Skew", "skew"),
        ("ADBE Text Skew Axis", "skewAxis"),
        ("ADBE Text Tracking Amount", "tracking"),
        ("ADBE Text Stroke Width", "strokeWidth"),
        ("ADBE Text Opacity", "opacity"),
        ("ADBE Text Line Anchor", "lineAnchor"),
        ("ADBE Text Character Offset", "characterOffset"),
        ("ADBE Text Character Replace", "characterValue"),
    ] {
        append_numeric_animation(
            &source.properties,
            source_name,
            context,
            scalar(target_name),
            clock,
            TextAnimationOutput::new(entries, warnings, budget),
        );
    }
    append_numeric_animation(
        &source.properties,
        "ADBE Text Line Spacing",
        context,
        NumericAnimationTarget::float(PropertyTarget::fx_item(animator.id, "lineSpacing"), 1, 1.0),
        clock,
        TextAnimationOutput::new(entries, warnings, budget),
    );
    for (source_name, target_name) in [
        ("ADBE Text Fill Color", "fillColor"),
        ("ADBE Text Stroke Color", "strokeColor"),
    ] {
        append_numeric_animation(
            &source.properties,
            source_name,
            context,
            NumericAnimationTarget::color(
                PropertyTarget::fx_item(animator.id, target_name),
                [0, 1, 2, 3],
                [1.0; 4],
            ),
            clock,
            TextAnimationOutput::new(entries, warnings, budget),
        );
    }

    let mut range_index = 0;
    let mut wiggly_index = 0;
    for (selector_index, selector_source) in source.selectors.iter().enumerate() {
        let selector_context = format!("{context} Selector {}", selector_index + 1);
        match selector_source {
            SelectorSource::Range { properties } => {
                let Some(selector) = animator.selectors.get(range_index) else {
                    if selector_properties(selector_source)
                        .iter()
                        .any(|(_, value)| value.as_ref().is_ok_and(numeric_has_keyframes))
                    {
                        warnings.push(format!(
                            "{selector_context} has native keys but no imported Range Selector target; animation omitted"
                        ));
                    }
                    range_index += 1;
                    continue;
                };
                range_index += 1;
                let percentage = match scalar_property_value(properties, "ADBE Text Range Units") {
                    Some(value) => match exact_integer(value) {
                        Some(2) => false,
                        Some(_) => true,
                        None => {
                            warnings.push(format!(
                                "{selector_context} ADBE Text Range Units value {value} is not an exact representable integer; percentage units used"
                            ));
                            true
                        }
                    },
                    None => true,
                };
                let (start, end, offset, scale) = if percentage {
                    (
                        "ADBE Text Percent Start",
                        "ADBE Text Percent End",
                        "ADBE Text Percent Offset",
                        0.01,
                    )
                } else {
                    (
                        "ADBE Text Index Start",
                        "ADBE Text Index End",
                        "ADBE Text Index Offset",
                        1.0,
                    )
                };
                for (source_name, target_name, target_scale) in [
                    (start, "start", scale),
                    (end, "end", scale),
                    (offset, "offset", scale),
                    ("ADBE Text Selector Max Amount", "amount", 0.01),
                    ("ADBE Text Levels Max Ease", "easeHigh", 0.01),
                    ("ADBE Text Levels Min Ease", "easeLow", 0.01),
                    ("ADBE Text Random Seed", "randomSeed", 1.0),
                ] {
                    append_numeric_animation(
                        properties,
                        source_name,
                        &selector_context,
                        NumericAnimationTarget::float(
                            PropertyTarget::fx_item(selector.id, target_name),
                            0,
                            target_scale,
                        ),
                        clock,
                        TextAnimationOutput::new(entries, warnings, budget),
                    );
                }
                warn_unmapped_keyframes(
                    properties,
                    &[
                        start,
                        end,
                        offset,
                        "ADBE Text Selector Max Amount",
                        "ADBE Text Levels Max Ease",
                        "ADBE Text Levels Min Ease",
                        "ADBE Text Random Seed",
                    ],
                    &selector_context,
                    warnings,
                );
            }
            SelectorSource::Wiggly { properties } => {
                let Some(selector) = animator.wiggly_selectors.get(wiggly_index) else {
                    if selector_properties(selector_source)
                        .iter()
                        .any(|(_, value)| value.as_ref().is_ok_and(numeric_has_keyframes))
                    {
                        warnings.push(format!(
                            "{selector_context} has native keys but no imported Wiggly Selector target; animation omitted"
                        ));
                    }
                    wiggly_index += 1;
                    continue;
                };
                wiggly_index += 1;
                for (source_name, target_name) in [
                    ("ADBE Text Temporal Freq", "speed"),
                    ("ADBE Text Wiggly Max Amount", "amount"),
                    ("ADBE Text Wiggly Random Seed", "seed"),
                ] {
                    append_numeric_animation(
                        properties,
                        source_name,
                        &selector_context,
                        NumericAnimationTarget::float(
                            PropertyTarget::fx_item(selector.id, target_name),
                            0,
                            1.0,
                        ),
                        clock,
                        TextAnimationOutput::new(entries, warnings, budget),
                    );
                }
                warn_unmapped_keyframes(
                    properties,
                    &[
                        "ADBE Text Temporal Freq",
                        "ADBE Text Wiggly Max Amount",
                        "ADBE Text Wiggly Random Seed",
                    ],
                    &selector_context,
                    warnings,
                );
            }
            SelectorSource::AlternatingSign { properties } => warn_omitted_selector_keys(
                properties,
                &selector_context,
                alternating::EXPRESSION_SELECTOR,
                warnings,
            ),
            SelectorSource::Unsupported { name, properties } => {
                warn_omitted_selector_keys(properties, &selector_context, name, warnings);
            }
        }
    }
}

fn warn_omitted_selector_keys(
    properties: &[(String, Result<NumericProperty, PropertyError>)],
    context: &str,
    name: &str,
    warnings: &mut Vec<String>,
) {
    if properties
        .iter()
        .any(|(_, value)| value.as_ref().is_ok_and(numeric_has_keyframes))
    {
        warnings.push(format!(
            "{context} {name} has no destination selector equivalent; its native animation was omitted"
        ));
    }
}

fn scalar_property_value(
    properties: &[(String, Result<NumericProperty, PropertyError>)],
    name: &str,
) -> Option<f64> {
    let numeric = properties
        .iter()
        .find(|(candidate, _)| candidate == name)?
        .1
        .as_ref()
        .ok()?;
    numeric
        .values
        .first()
        .or_else(|| numeric.keyframes.first()?.values.first())
        .copied()
}

fn warn_unmapped_keyframes(
    properties: &[(String, Result<NumericProperty, PropertyError>)],
    mapped: &[&str],
    context: &str,
    warnings: &mut Vec<String>,
) {
    for (name, numeric) in properties {
        if !mapped.contains(&name.as_str()) && numeric.as_ref().is_ok_and(numeric_has_keyframes) {
            warnings.push(format!(
                "{context} {name} is keyed but has no runtime-animatable destination field; animation omitted"
            ));
        }
    }
}

fn convert_text_properties(
    source: &SourceText,
    mask_guide_ids: &[(u32, LayerId)],
    next_id: &mut u64,
) -> (
    Vec<TextAnimator>,
    Option<TextAnchorOptions>,
    Option<TextPathOptions>,
    Vec<String>,
) {
    let mut warnings = Vec::new();
    let mut animators = Vec::with_capacity(source.animators.len());
    for source_animator in &source.animators {
        let prefix = format!("Text {}", source_animator.name);
        let Some(id) = allocate_item_id(next_id) else {
            warnings.push(format!(
                "{prefix} omitted: generated layer identifier space exhausted"
            ));
            break;
        };
        let mut animator = TextAnimator {
            id,
            name: source_animator.name.clone(),
            ..TextAnimator::default()
        };
        animator.anchor_point = vector2_property(
            &source_animator.properties,
            "ADBE Text Anchor Point 3D",
            &prefix,
            &mut warnings,
        );
        animator.position = vector2_property(
            &source_animator.properties,
            "ADBE Text Position 3D",
            &prefix,
            &mut warnings,
        );
        animator.scale = vector2_property(
            &source_animator.properties,
            "ADBE Text Scale 3D",
            &prefix,
            &mut warnings,
        );
        animator.rotation = scalar_property(
            &source_animator.properties,
            "ADBE Text Rotation",
            &prefix,
            &mut warnings,
        );
        animator.skew = scalar_property(
            &source_animator.properties,
            "ADBE Text Skew",
            &prefix,
            &mut warnings,
        );
        animator.skew_axis = scalar_property(
            &source_animator.properties,
            "ADBE Text Skew Axis",
            &prefix,
            &mut warnings,
        );
        animator.tracking = scalar_property(
            &source_animator.properties,
            "ADBE Text Tracking Amount",
            &prefix,
            &mut warnings,
        );
        animator.stroke_width = scalar_property(
            &source_animator.properties,
            "ADBE Text Stroke Width",
            &prefix,
            &mut warnings,
        );
        animator.blur = vector2_property(
            &source_animator.properties,
            "ADBE Text Blur",
            &prefix,
            &mut warnings,
        );
        animator.opacity = scalar_property(
            &source_animator.properties,
            "ADBE Text Opacity",
            &prefix,
            &mut warnings,
        );
        animator.fill_color = color_property(
            &source_animator.properties,
            "ADBE Text Fill Color",
            &prefix,
            &mut warnings,
        );
        animator.stroke_color = color_property(
            &source_animator.properties,
            "ADBE Text Stroke Color",
            &prefix,
            &mut warnings,
        );
        animator.line_spacing = vector2_property(
            &source_animator.properties,
            "ADBE Text Line Spacing",
            &prefix,
            &mut warnings,
        )
        .map(|value| value[1]);
        animator.line_anchor = scalar_property(
            &source_animator.properties,
            "ADBE Text Line Anchor",
            &prefix,
            &mut warnings,
        );
        animator.character_offset = scalar_property(
            &source_animator.properties,
            "ADBE Text Character Offset",
            &prefix,
            &mut warnings,
        );
        animator.character_value = scalar_property(
            &source_animator.properties,
            "ADBE Text Character Replace",
            &prefix,
            &mut warnings,
        );
        let mut saw_wiggly = false;
        let mut warned_mixed_order = false;
        for selector in &source_animator.selectors {
            match selector {
                SelectorSource::Range { properties } => {
                    if saw_wiggly && !warned_mixed_order {
                        warnings.push(format!(
                            "{prefix} interleaves Range and Wiggly Selectors; the destination evaluates all ranges before all wigglies, so authored selector order is approximated"
                        ));
                        warned_mixed_order = true;
                    }
                    let Some(selector_id) = allocate_item_id(next_id) else {
                        warnings.push(format!(
                            "{prefix} Range Selector omitted: generated layer identifier space exhausted"
                        ));
                        continue;
                    };
                    animator.selectors.push(convert_range_selector(
                        selector_id,
                        properties,
                        &prefix,
                        &mut warnings,
                    ));
                }
                SelectorSource::Wiggly { properties } => {
                    saw_wiggly = true;
                    let Some(selector_id) = allocate_item_id(next_id) else {
                        warnings.push(format!(
                            "{prefix} Wiggly Selector omitted: generated layer identifier space exhausted"
                        ));
                        continue;
                    };
                    animator.wiggly_selectors.push(convert_wiggly_selector(
                        selector_id,
                        properties,
                        &prefix,
                        &mut warnings,
                    ));
                }
                SelectorSource::AlternatingSign { .. } => warnings.push(format!(
                    "{prefix} selector {} has no existing destination selector equivalent and was omitted",
                    alternating::EXPRESSION_SELECTOR
                )),
                SelectorSource::Unsupported { name, .. } => warnings.push(format!(
                    "{prefix} selector {name} has no existing destination selector equivalent and was omitted"
                )),
            }
        }
        let unsupported = source_animator
            .properties
            .iter()
            .filter(|(name, _)| !supported_animator_property(name))
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        if !unsupported.is_empty() {
            warnings.push(format!(
                "{prefix} properties without an existing TextAnimator equivalent were omitted: {}",
                unsupported.join(", ")
            ));
        }
        animators.push(animator);
    }

    warn_unsupported_properties(
        &source.more_options,
        &[
            "ADBE Text Anchor Point Option",
            "ADBE Text Anchor Point Align",
        ],
        "Text More Options",
        &mut warnings,
    );
    warn_unsupported_properties(
        &source.path_options,
        &[
            "ADBE Text Path",
            "ADBE Text First Margin",
            "ADBE Text Last Margin",
            "ADBE Text Perpendicular To Path",
            "ADBE Text Reverse Path",
            "ADBE Text Force Align Path",
        ],
        "Text Path Options",
        &mut warnings,
    );

    let anchor_options = allocate_item_id(next_id).map(|id| {
        let grouping = exact_integer_property(
            &source.more_options,
            "ADBE Text Anchor Point Option",
            "Text More Options",
            &mut warnings,
        )
        .unwrap_or(1);
        let anchor_point_grouping = match grouping {
            1 => AnchorPointGrouping::Character,
            2 => AnchorPointGrouping::Word,
            3 => AnchorPointGrouping::Line,
            4 => AnchorPointGrouping::All,
            value => {
                warnings.push(format!(
                    "Text Anchor Point Grouping {value} is unknown; Character used"
                ));
                AnchorPointGrouping::Character
            }
        };
        let grouping_alignment = vector2_property(
            &source.more_options,
            "ADBE Text Anchor Point Align",
            "Text More Options",
            &mut warnings,
        )
        .unwrap_or([0.0, 0.0]);
        TextAnchorOptions {
            id,
            anchor_point_grouping,
            grouping_alignment,
        }
    });
    if anchor_options.is_none() {
        warnings
            .push("Text Anchor Options omitted: generated layer identifier space exhausted".into());
    }

    let path_index = exact_integer_property(
        &source.path_options,
        "ADBE Text Path",
        "Text Path Options",
        &mut warnings,
    )
    .unwrap_or(0);
    let path_options = if path_index <= 0 {
        None
    } else {
        let path_layer = u32::try_from(path_index).ok().and_then(|source_index| {
            mask_guide_ids
                .iter()
                .find(|(index, _)| *index == source_index)
                .map(|(_, guide_id)| *guide_id)
        });
        match path_layer {
            Some(path_layer) => allocate_item_id(next_id)
                .map(|id| TextPathOptions {
                    id,
                    path_layer,
                    first_margin: scalar_property(
                        &source.path_options,
                        "ADBE Text First Margin",
                        "Text Path Options",
                        &mut warnings,
                    )
                    .unwrap_or(0.0),
                    last_margin: scalar_property(
                        &source.path_options,
                        "ADBE Text Last Margin",
                        "Text Path Options",
                        &mut warnings,
                    )
                    .unwrap_or(0.0),
                    perpendicular_to_path: toggle_property(
                        &source.path_options,
                        "ADBE Text Perpendicular To Path",
                        true,
                        "Text Path Options",
                        &mut warnings,
                    ),
                    reverse_path: toggle_property(
                        &source.path_options,
                        "ADBE Text Reverse Path",
                        false,
                        "Text Path Options",
                        &mut warnings,
                    ),
                    force_alignment: toggle_property(
                        &source.path_options,
                        "ADBE Text Force Align Path",
                        false,
                        "Text Path Options",
                        &mut warnings,
                    ),
                    align: Default::default(),
                    align_offset: 0.0,
                })
                .or_else(|| {
                    warnings.push(
                        "Text Path Options omitted: generated layer identifier space exhausted"
                            .into(),
                    );
                    None
                }),
            None => {
                warnings.push(format!(
                    "Text Path references AE mask {path_index}, but no imported editable mask guide exists; path layout omitted"
                ));
                None
            }
        }
    };
    (animators, anchor_options, path_options, warnings)
}

fn warn_unsupported_properties(
    properties: &[(String, Result<NumericProperty, PropertyError>)],
    supported: &[&str],
    context: &str,
    warnings: &mut Vec<String>,
) {
    let unsupported = properties
        .iter()
        .map(|(name, _)| name.as_str())
        .filter(|name| !supported.contains(name))
        .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        warnings.push(format!(
            "{context} controls without an existing runtime destination equivalent were omitted: {}",
            unsupported.join(", ")
        ));
    }
}

fn convert_range_selector(
    id: FxItemId,
    properties: &[(String, Result<NumericProperty, PropertyError>)],
    prefix: &str,
    warnings: &mut Vec<String>,
) -> RangeSelector {
    let units = enum_code(properties, "ADBE Text Range Units", 1, prefix, warnings);
    let percentage = units != 2;
    let start_name = if percentage {
        "ADBE Text Percent Start"
    } else {
        "ADBE Text Index Start"
    };
    let end_name = if percentage {
        "ADBE Text Percent End"
    } else {
        "ADBE Text Index End"
    };
    let offset_name = if percentage {
        "ADBE Text Percent Offset"
    } else {
        "ADBE Text Index Offset"
    };
    let divisor = if percentage { 100.0 } else { 1.0 };
    warn_unsupported_properties(
        properties,
        &[
            "ADBE Text Percent Start",
            "ADBE Text Percent End",
            "ADBE Text Percent Offset",
            "ADBE Text Index Start",
            "ADBE Text Index End",
            "ADBE Text Index Offset",
            "ADBE Text Range Units",
            "ADBE Text Range Type2",
            "ADBE Text Selector Mode",
            "ADBE Text Selector Max Amount",
            "ADBE Text Range Shape",
            "ADBE Text Levels Max Ease",
            "ADBE Text Levels Min Ease",
            "ADBE Text Randomize Order",
            "ADBE Text Random Seed",
        ],
        &format!("{prefix} Range Selector"),
        warnings,
    );
    let value = serde_json::json!({
        "id": id.value(),
        "start": scalar_property(properties, start_name, prefix, warnings).unwrap_or(range_default(start_name)) / divisor,
        "end": scalar_property(properties, end_name, prefix, warnings).unwrap_or(range_default(end_name)) / divisor,
        "offset": scalar_property(properties, offset_name, prefix, warnings).unwrap_or(range_default(offset_name)) / divisor,
        "units": if percentage { "percentage" } else { "index" },
        "basedOn": selector_basis(enum_code(properties, "ADBE Text Range Type2", 1, prefix, warnings)),
        "mode": selector_mode(enum_code(properties, "ADBE Text Selector Mode", 1, prefix, warnings)),
        "amount": scalar_property(properties, "ADBE Text Selector Max Amount", prefix, warnings).unwrap_or(range_default("ADBE Text Selector Max Amount")) / 100.0,
        "shape": selector_shape(enum_code(properties, "ADBE Text Range Shape", 1, prefix, warnings)),
        "easeHigh": scalar_property(properties, "ADBE Text Levels Max Ease", prefix, warnings).unwrap_or(range_default("ADBE Text Levels Max Ease")) / 100.0,
        "easeLow": scalar_property(properties, "ADBE Text Levels Min Ease", prefix, warnings).unwrap_or(range_default("ADBE Text Levels Min Ease")) / 100.0,
        "randomizeOrder": toggle_property(
            properties,
            "ADBE Text Randomize Order",
            false,
            prefix,
            warnings,
        ),
        "randomSeed": scalar_property(properties, "ADBE Text Random Seed", prefix, warnings).unwrap_or(range_default("ADBE Text Random Seed")),
    });
    serde_json::from_value(value).unwrap_or_else(|error| {
        warnings.push(format!(
            "{prefix} Range Selector is invalid ({error}); AE defaults used"
        ));
        RangeSelector {
            id,
            ..RangeSelector::default()
        }
    })
}

/// AE's native value, in source units, for a Range Selector leaf that the
/// file does not materialize.
fn range_default(match_name: &str) -> f64 {
    match match_name {
        "ADBE Text Percent End" | "ADBE Text Selector Max Amount" => 100.0,
        _ => 0.0,
    }
}

fn convert_wiggly_selector(
    id: FxItemId,
    properties: &[(String, Result<NumericProperty, PropertyError>)],
    prefix: &str,
    warnings: &mut Vec<String>,
) -> WigglySelector {
    let max = scalar_property(properties, "ADBE Text Wiggly Max Amount", prefix, warnings)
        .unwrap_or(100.0);
    let min = scalar_property(properties, "ADBE Text Wiggly Min Amount", prefix, warnings)
        .unwrap_or(-100.0);
    if (max + min).abs() > f64::EPSILON {
        warnings.push(format!("{prefix} Wiggly Selector has asymmetric min/max [{min}, {max}]; destination stores one amount, so max amount is used"));
    }
    warn_unsupported_properties(
        properties,
        &[
            "ADBE Text Selector Mode",
            "ADBE Text Wiggly Max Amount",
            "ADBE Text Wiggly Min Amount",
            "ADBE Text Temporal Freq",
            "ADBE Text Wiggly Random Seed",
        ],
        &format!("{prefix} Wiggly Selector"),
        warnings,
    );
    let value = serde_json::json!({
        "id": id.value(),
        "mode": selector_mode(enum_code(properties, "ADBE Text Selector Mode", 1, prefix, warnings)),
        "speed": scalar_property(properties, "ADBE Text Temporal Freq", prefix, warnings).unwrap_or(2.0),
        "amount": max,
        "seed": scalar_property(properties, "ADBE Text Wiggly Random Seed", prefix, warnings).unwrap_or(0.0),
    });
    serde_json::from_value(value).unwrap_or_else(|error| {
        warnings.push(format!(
            "{prefix} Wiggly Selector is invalid ({error}); AE defaults used"
        ));
        WigglySelector {
            id,
            ..WigglySelector::default()
        }
    })
}

fn scalar_property(
    properties: &[(String, Result<NumericProperty, PropertyError>)],
    name: &str,
    prefix: &str,
    warnings: &mut Vec<String>,
) -> Option<f64> {
    numeric_property(properties, name, prefix, warnings)
        .and_then(|numeric| {
            numeric
                .values
                .first()
                .copied()
                .or_else(|| numeric.keyframes.first()?.values.first().copied())
        })
        .filter(|value| value.is_finite())
}

fn vector2_property(
    properties: &[(String, Result<NumericProperty, PropertyError>)],
    name: &str,
    prefix: &str,
    warnings: &mut Vec<String>,
) -> Option<[f64; 2]> {
    let numeric = numeric_property(properties, name, prefix, warnings)?;
    let values = if numeric.values.len() >= 2 {
        &numeric.values
    } else {
        &numeric.keyframes.first()?.values
    };
    let value = [*values.first()?, *values.get(1)?];
    value.into_iter().all(f64::is_finite).then_some(value)
}

fn color_property(
    properties: &[(String, Result<NumericProperty, PropertyError>)],
    name: &str,
    prefix: &str,
    warnings: &mut Vec<String>,
) -> Option<[f64; 4]> {
    let numeric = numeric_property(properties, name, prefix, warnings)?;
    let values = if numeric.values.len() >= 3 {
        &numeric.values
    } else {
        &numeric.keyframes.first()?.values
    };
    let value = [
        *values.first()?,
        *values.get(1)?,
        *values.get(2)?,
        values.get(3).copied().unwrap_or(1.0),
    ];
    value.into_iter().all(f64::is_finite).then_some(value)
}

fn numeric_property<'a>(
    properties: &'a [(String, Result<NumericProperty, PropertyError>)],
    name: &str,
    prefix: &str,
    warnings: &mut Vec<String>,
) -> Option<&'a NumericProperty> {
    let (_, result) = properties.iter().find(|(candidate, _)| candidate == name)?;
    match result {
        Ok(numeric) if numeric.expression_enabled => {
            warnings.push(format!("{prefix} {name} has an enabled AE expression; expression is not evaluated as FX script and the stored/keyed value is used"));
            Some(numeric)
        }
        Ok(numeric) => Some(numeric),
        Err(error) => {
            warnings.push(format!("{prefix} {name} omitted: {error}"));
            None
        }
    }
}

fn exact_integer(value: f64) -> Option<i64> {
    const I64_EXCLUSIVE_UPPER_BOUND: f64 = 9_223_372_036_854_775_808.0;

    (value.is_finite()
        && value.fract() == 0.0
        && value >= i64::MIN as f64
        && value < I64_EXCLUSIVE_UPPER_BOUND)
        .then_some(value as i64)
}

fn exact_integer_property(
    properties: &[(String, Result<NumericProperty, PropertyError>)],
    name: &str,
    prefix: &str,
    warnings: &mut Vec<String>,
) -> Option<i64> {
    let value = scalar_property(properties, name, prefix, warnings)?;
    let Some(value) = exact_integer(value) else {
        warnings.push(format!(
            "{prefix} {name} value {value} is not an exact representable integer; AE default used"
        ));
        return None;
    };
    Some(value)
}

fn enum_code(
    properties: &[(String, Result<NumericProperty, PropertyError>)],
    name: &str,
    default: i64,
    prefix: &str,
    warnings: &mut Vec<String>,
) -> i64 {
    exact_integer_property(properties, name, prefix, warnings).unwrap_or(default)
}

fn toggle_property(
    properties: &[(String, Result<NumericProperty, PropertyError>)],
    name: &str,
    default: bool,
    prefix: &str,
    warnings: &mut Vec<String>,
) -> bool {
    enum_code(
        properties,
        name,
        if default { 1 } else { 0 },
        prefix,
        warnings,
    ) != 0
}

fn selector_basis(value: i64) -> &'static str {
    match value {
        2 => "charactersExcludingSpaces",
        3 => "words",
        4 => "lines",
        _ => "characters",
    }
}

fn selector_mode(value: i64) -> &'static str {
    match value {
        2 => "subtract",
        3 => "intersect",
        4 => "min",
        5 => "max",
        6 => "difference",
        _ => "add",
    }
}

fn selector_shape(value: i64) -> &'static str {
    match value {
        2 => "rampUp",
        3 => "rampDown",
        4 => "triangle",
        5 => "round",
        6 => "smooth",
        _ => "square",
    }
}

fn supported_animator_property(name: &str) -> bool {
    matches!(
        name,
        "ADBE Text Anchor Point 3D"
            | "ADBE Text Position 3D"
            | "ADBE Text Scale 3D"
            | "ADBE Text Rotation"
            | "ADBE Text Skew"
            | "ADBE Text Skew Axis"
            | "ADBE Text Tracking Amount"
            | "ADBE Text Stroke Width"
            | "ADBE Text Blur"
            | "ADBE Text Opacity"
            | "ADBE Text Fill Color"
            | "ADBE Text Stroke Color"
            | "ADBE Text Line Spacing"
            | "ADBE Text Line Anchor"
            | "ADBE Text Character Offset"
            | "ADBE Text Character Replace"
    )
}

fn read_document_starts(wrapper: &[Chunk], document_count: usize) -> (Vec<f64>, Vec<String>) {
    if document_count <= 1 {
        return (Vec::new(), Vec::new());
    }
    let result = (|| -> Result<Vec<f64>, TextError> {
        let metadata = unique_list(wrapper, *b"tdbs")?;
        let tdb4 = only_data(metadata, *b"tdb4")?;
        if tdb4.len() != 124 {
            return Err(TextError::Layout("Source Text tdb4 layout"));
        }
        let timebase = u32::from_be_bytes(
            tdb4[12..16]
                .try_into()
                .map_err(|_| TextError::Layout("Source Text timebase"))?,
        );
        if timebase == 0 {
            return Err(TextError::Layout("zero Source Text timebase"));
        }
        let list = unique_list(metadata, *b"list")?;
        let header = only_data(list, *b"lhd3")?;
        if header.len() < 24 {
            return Err(TextError::Layout("Source Text keyframe header"));
        }
        let count = usize::from(u16::from_be_bytes([header[10], header[11]]));
        let stride = usize::from(u16::from_be_bytes([header[18], header[19]]));
        if count != document_count || stride < 8 {
            return Err(TextError::Layout("Source Text keyframe count/stride"));
        }
        let bytes = only_data(list, *b"ldat")?;
        let length = count
            .checked_mul(stride)
            .ok_or(TextError::Layout("Source Text keyframe length overflow"))?;
        if bytes.len() < length {
            return Err(TextError::Layout("truncated Source Text keyframes"));
        }
        let mut starts = Vec::with_capacity(count);
        for item in bytes[..length].chunks_exact(stride) {
            let units = i32::from_be_bytes(
                item[..4]
                    .try_into()
                    .map_err(|_| TextError::Layout("Source Text key time"))?,
            );
            starts.push(f64::from(units) / f64::from(timebase));
        }
        Ok(starts)
    })();
    match result {
        Ok(starts) => (starts, Vec::new()),
        Err(error) => (
            Vec::new(),
            vec![format!(
                "timed Source Text metadata could not be decoded: {error}"
            )],
        ),
    }
}

/// Lowers an enabled Source Text expression that shows a same-layer Slider as
/// a percentage into that Slider's key times and strings. Any other enabled
/// expression keeps the authored documents with a diagnostic; a disabled one
/// leaves them in effect, as in AE. No expression code is executed.
fn lower_source_text_expression(
    layer: &Layer,
    wrapper: &[Chunk],
    document_count: usize,
    warnings: &mut Vec<String>,
) -> Option<PercentHolds> {
    let metadata = match enabled_expression_descriptor(wrapper) {
        Ok(Some(metadata)) => metadata,
        Ok(None) => return None,
        Err(error) => {
            warnings.push(format!(
                "Source Text expression state is malformed ({error}); authored Source Text retained without evaluating any expression"
            ));
            return None;
        }
    };
    let lowered = match document_count {
        0 => return None,
        1 => control_links::slider_percent(layer, metadata)
            .map_err(|error| error.to_string())
            .and_then(|slider| held_percent_texts(&slider).map_err(str::to_owned)),
        _ => Err("keyed Source Text documents are not combined with an expression".to_owned()),
    };
    match lowered {
        Ok((starts, texts)) => Some(PercentHolds { starts, texts }),
        Err(reason) => {
            warnings.push(expression_not_lowered(&reason));
            None
        }
    }
}

fn expression_not_lowered(reason: &str) -> String {
    format!(
        "Source Text: enabled expression not lowered ({reason}); authored Source Text retained without evaluating it"
    )
}

/// Source Text's descriptor carries the numeric-leaf expression flags read by
/// `properties::read_numeric`: tdb4 byte 120 marks a stored expression and
/// byte 119 disables it. Returns the descriptor only for an enabled expression.
fn enabled_expression_descriptor(wrapper: &[Chunk]) -> Result<Option<&[Chunk]>, TextError> {
    if !wrapper
        .iter()
        .any(|chunk| chunk.list_kind() == Some(*b"tdbs"))
    {
        return Ok(None);
    }
    let metadata = unique_list(wrapper, *b"tdbs")?;
    let meta = only_data(metadata, *b"tdb4")?;
    if meta.len() != 124 || meta[..2] != [0xdb, 0x99] {
        return Err(TextError::Layout("Source Text tdb4 layout"));
    }
    let present = meta[120] & 1 != 0
        || metadata
            .iter()
            .any(|chunk| matches!(&chunk.id(), b"Utf8" | b"expr"));
    Ok((present && meta[119] & 1 == 0).then_some(metadata))
}

/// The strings `Math.round(s).toLocaleString() + "%"` shows while a Slider
/// holds each key, with the key times (none for a static Slider). Only finite
/// whole values 0..=100 are exact without guessing rounding, digit grouping or
/// a signed zero; only outgoing Hold keys keep one string until the next key.
fn held_percent_texts(slider: &NumericProperty) -> Result<(Vec<f64>, Vec<String>), &'static str> {
    let (starts, values) = if slider.animated {
        let keys = &slider.keyframes;
        let Some((_, held)) = keys.split_last() else {
            return Err("the keyed Slider has no decodable keys");
        };
        // The outgoing type alone selects Hold, as in `animation::easing_for_key`.
        if held.iter().any(|key| key.out_interpolation != 3) {
            return Err("Slider keys are not all Hold");
        }
        if keys.iter().any(|key| !key.time_secs.is_finite())
            || keys
                .windows(2)
                .any(|pair| pair[0].time_secs >= pair[1].time_secs)
        {
            return Err("Slider key times are not finite and strictly increasing");
        }
        let values = keys
            .iter()
            .map(|key| match key.values.as_slice() {
                [value] => Ok(*value),
                _ => Err("the Slider is not a scalar value"),
            })
            .collect::<Result<Vec<_>, _>>()?;
        (keys.iter().map(|key| key.time_secs).collect(), values)
    } else {
        let [value] = slider.values.as_slice() else {
            return Err("the Slider is not a scalar value");
        };
        (Vec::new(), vec![*value])
    };
    if values.iter().any(|value| {
        value.fract() != 0.0 || !(0.0..=100.0).contains(value) || value.is_sign_negative()
    }) {
        return Err("Slider values are not whole numbers from 0 to 100");
    }
    let texts = values.iter().map(|value| format!("{value}%")).collect();
    Ok((starts, texts))
}

fn extract_fonts(root: &Value) -> Vec<Option<String>> {
    at(root, &[Key::Name("0"), Key::Name("1"), Key::Name("0")])
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|entry| {
            // Adobe's CoolTypeFont wraps the name in another dictionary.
            // Legacy converter output uses a compact table with a direct string.
            let value = at(entry, &[Key::Name("0"), Key::Name("0")])?;
            value
                .as_str()
                .or_else(|| value.get("0").and_then(Value::as_str))
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
        })
        .collect()
}

fn convert_document(
    document: &Value,
    fonts: &[Option<String>],
    frame: Option<&Value>,
) -> (TextDocument, Vec<String>) {
    let mut warnings = Vec::new();
    let raw_text = at(document, &[Key::Name("0"), Key::Name("0")])
        .and_then(Value::as_str)
        .unwrap_or_default();
    let text = editable_text(raw_text);
    let character = at(
        document,
        &[
            Key::Name("0"),
            Key::Name("6"),
            Key::Name("0"),
            Key::Index(0),
            Key::Name("0"),
            Key::Name("0"),
            Key::Name("6"),
        ],
    );
    let paragraph = at(
        document,
        &[
            Key::Name("0"),
            Key::Name("5"),
            Key::Name("0"),
            Key::Index(0),
            Key::Name("0"),
            Key::Name("0"),
            Key::Name("5"),
        ],
    );
    let font_index = character
        .and_then(|style| style.get("0"))
        .and_then(Value::as_i64)
        .and_then(|value| usize::try_from(value).ok());
    let font = font_index
        .and_then(|index| fonts.get(index))
        .and_then(Option::as_ref)
        .cloned()
        .unwrap_or_else(|| {
            warnings.push("Source Text font identity is absent or out of range; sans-serif fallback name used".into());
            "sans-serif".into()
        });
    let (font_family, font_style) = split_font_identity(&font);
    warnings.push(format!(
        "AE stores PostScript font identity {font:?}, not host-resolved family/style; imported as family {font_family:?}, style {font_style:?} without bundling or system-font guarantees"
    ));
    let font_size = positive(
        character.and_then(|style| number(style, "1")),
        12.0,
        "font size",
        &mut warnings,
    );
    let apply_fill = character
        .and_then(|style| boolean(style, "56"))
        .unwrap_or(true);
    let fill_color = color(character.and_then(|style| style.get("53"))).unwrap_or_else(|| {
        warnings.push("Source Text fill color missing/malformed; white used".into());
        [1.0, 1.0, 1.0, 1.0]
    });
    let apply_stroke = character
        .and_then(|style| boolean(style, "57"))
        .unwrap_or(false);
    let stroke_color = color(character.and_then(|style| style.get("54")));
    if apply_stroke && stroke_color.is_none() {
        warnings.push("Source Text stroke enabled without a usable color; stroke disabled".into());
    }
    let stroke_width = non_negative(
        character.and_then(|style| number(style, "63")),
        1.0,
        "stroke width",
        &mut warnings,
    );
    let stroke_over_fill = character
        .and_then(|style| boolean(style, "58"))
        .unwrap_or(true);
    let justification_code = paragraph
        .and_then(|style| style.get("0"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let justification = match justification_code {
        0 => Justification::Left,
        1 => Justification::Right,
        2 => Justification::Center,
        3 => Justification::Justify,
        4..=6 => {
            let final_line_behavior = match justification_code {
                4 => "right-aligns the final line",
                5 => "centers the final line",
                _ => "fully justifies the final line",
            };
            warnings.push(format!(
                "Source Text first paragraph-style run field 0 justification code {justification_code} {final_line_behavior}, but the destination Justify mode leaves the final line left-aligned; Justify used"
            ));
            Justification::Justify
        }
        value => {
            warnings.push(format!(
                "Source Text justification code {value} is unknown; Left used"
            ));
            Justification::Left
        }
    };
    let tracking = finite_or_default(
        character.and_then(|style| number(style, "8")),
        0.0,
        "tracking",
        &mut warnings,
    );
    let auto_leading = character
        .and_then(|style| boolean(style, "4"))
        .unwrap_or(true);
    let leading = if auto_leading {
        None
    } else {
        Some(positive(
            character.and_then(|style| number(style, "5")),
            font_size.value() * 1.2,
            "leading",
            &mut warnings,
        ))
    };
    let baseline_shift = finite_or_default(
        character.and_then(|style| number(style, "9")),
        0.0,
        "baseline shift",
        &mut warnings,
    );
    // Independently AE-authored sources store caps code 0 for normal text and
    // 2 for All Caps. No other code has native evidence, so none is guessed.
    let all_caps = match character.and_then(|style| style.get("12")) {
        None => false,
        Some(value) => match value.as_i64() {
            Some(0) => false,
            Some(2) => true,
            Some(code) => {
                warnings.push(format!(
                    "Source Text first character-style run field 12 caps code {code} is unrecognized (only 0 normal and 2 All Caps are mapped); normal caps used"
                ));
                false
            }
            None => {
                warnings.push(
                    "Source Text first character-style run field 12 caps mode is malformed; normal caps used"
                        .into(),
                );
                false
            }
        },
    };
    let geometry = box_geometry(document, frame, &mut warnings);

    // The cached baseline is already a placed first line. Explicit alignment
    // prevents the destination's legacy leading-based box centering from
    // adding a second vertical placement offset.
    let vertical_align = geometry
        .as_ref()
        .and_then(|geometry| geometry.first_baseline)
        .map(|_| fx_schema::VerticalAlign::Top);
    let run_counts = (run_count(document, "6"), run_count(document, "5"));
    if run_counts.0 > 1 || run_counts.1 > 1 {
        warnings.push(format!(
            "Source Text has {} character-style and {} paragraph-style runs; destination TextDocument is whole-layer, so the first run is applied to all text",
            run_counts.0, run_counts.1
        ));
    }
    if character.and_then(|style| boolean(style, "2")) == Some(true)
        || character.and_then(|style| boolean(style, "3")) == Some(true)
    {
        warnings
            .push("Source Text faux bold/italic has no TextDocument field and is omitted".into());
    }
    let horizontal_scale = character
        .and_then(|style| number(style, "6"))
        .unwrap_or(1.0);
    let vertical_scale = character
        .and_then(|style| number(style, "7"))
        .unwrap_or(1.0);
    if (horizontal_scale - 1.0).abs() > f64::EPSILON || (vertical_scale - 1.0).abs() > f64::EPSILON
    {
        warnings.push(format!(
            "Source Text glyph scales [{horizontal_scale}, {vertical_scale}] have no whole-document destination field and are omitted"
        ));
    }

    (
        TextDocument {
            text,
            font_family: Arc::from(font_family),
            font_style: Arc::from(font_style),
            font_size,
            font_variations: None,
            apply_fill,
            fill_color,
            apply_stroke: apply_stroke && stroke_color.is_some(),
            stroke_color,
            stroke_width,
            stroke_over_fill,
            justification,
            tracking,
            leading,
            baseline_shift,
            box_text: geometry.is_some(),
            scale_box_text_with_transform: false,
            box_size: geometry.as_ref().map(|geometry| geometry.size),
            box_position: geometry.as_ref().map(|geometry| geometry.position),
            box_first_baseline: geometry.and_then(|geometry| geometry.first_baseline),
            all_caps,
            underline: false,
            strikethrough: false,
            vertical_align,
        },
        warnings,
    )
}

/// AE's native text without its terminal paragraph return, with every other
/// return as a destination line break.
fn editable_text(raw_text: &str) -> String {
    raw_text
        .strip_suffix('\r')
        .unwrap_or(raw_text)
        .replace('\r', "\n")
}

struct BoxGeometry {
    size: [f64; 2],
    position: [f64; 2],
    first_baseline: Option<f64>,
}

fn box_geometry(
    document: &Value,
    frame: Option<&Value>,
    warnings: &mut Vec<String>,
) -> Option<BoxGeometry> {
    let coords = frame
        .and_then(|value| value.get("1"))
        .and_then(|value| value.get("0"))
        .and_then(Value::as_array)?;
    if coords.len() < 14 {
        warnings.push("Source Text box outline is truncated; point text used".into());
        return None;
    }
    let [Some(left), Some(top), Some(left_bottom_x), Some(bottom)] =
        [0usize, 1, 12, 13].map(|index| coords[index].as_f64())
    else {
        warnings.push("Source Text box outline is non-numeric; point text used".into());
        return None;
    };
    let width = (left_bottom_x - left).abs();
    let height = (bottom - top).abs();
    if ![left, top, width, height].into_iter().all(f64::is_finite) || width <= 0.0 || height <= 0.0
    {
        warnings.push("Source Text box geometry is invalid; point text used".into());
        return None;
    }
    let first_baseline = first_layout_line_baseline(document)
        .map(|offset| top + offset)
        .filter(|baseline| baseline.is_finite());
    if first_baseline.is_some() {
        warnings.push(
            "Source Text box first-line baseline is imported from AE's cached source layout as a fixed value; destination text or font-size edits/animation and line-layout changes do not recompute it automatically"
                .into(),
        );
    } else {
        warnings.push(
            "Source Text box cached first-line baseline is absent or malformed; destination font-size approximation used"
                .into(),
        );
    }
    Some(BoxGeometry {
        size: [width, height],
        position: [left, top],
        first_baseline,
    })
}

fn first_layout_line_baseline(document: &Value) -> Option<f64> {
    let layout = document.get("1")?.get("2")?;
    first_named_line_baseline(layout)
}

fn first_named_line_baseline(value: &Value) -> Option<f64> {
    if value.get("99").and_then(Value::as_str) == Some("L") {
        return value
            .get("10")
            .and_then(Value::as_f64)
            .filter(|baseline| baseline.is_finite());
    }
    match value {
        Value::Array(values) => values.iter().find_map(first_named_line_baseline),
        Value::Dict(values) => values.values().find_map(first_named_line_baseline),
        _ => None,
    }
}

fn run_count(document: &Value, key: &str) -> usize {
    at(document, &[Key::Name("0"), Key::Name(key), Key::Name("0")])
        .and_then(Value::as_array)
        .map_or(0, <[Value]>::len)
}

/// A dash-less name such as `ArialMT` has no style to split off: it is kept
/// whole with an empty style, the FX convention for an exact PostScript
/// identity, instead of a guessed `Regular` that export cannot tell apart.
fn split_font_identity(postscript: &str) -> (&str, &str) {
    postscript
        .rsplit_once('-')
        .filter(|(family, style)| !family.is_empty() && !style.is_empty())
        .unwrap_or((postscript, ""))
}

fn color(value: Option<&Value>) -> Option<[f64; 4]> {
    let channels = value?.get("0")?.get("1")?.as_array()?;
    if channels.len() < 4 {
        return None;
    }
    let color = [
        channels[1].as_f64()?,
        channels[2].as_f64()?,
        channels[3].as_f64()?,
        channels[0].as_f64()?,
    ];
    color
        .into_iter()
        .all(|channel| channel.is_finite())
        .then_some(color)
}

fn number(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

fn boolean(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(|value| {
        value.as_bool().or_else(|| {
            value.as_i64().and_then(|number| match number {
                0 => Some(false),
                1 => Some(true),
                _ => None,
            })
        })
    })
}

fn positive(
    value: Option<f64>,
    default: f64,
    name: &str,
    warnings: &mut Vec<String>,
) -> PositiveProperty {
    value.and_then(PositiveProperty::new).unwrap_or_else(|| {
        if value.is_some() {
            warnings.push(format!("Source Text {name} is invalid; {default} used"));
        }
        PositiveProperty::new(default).expect("positive text default")
    })
}

fn non_negative(
    value: Option<f64>,
    default: f64,
    name: &str,
    warnings: &mut Vec<String>,
) -> NonNegativeProperty {
    value.and_then(NonNegativeProperty::new).unwrap_or_else(|| {
        if value.is_some() {
            warnings.push(format!("Source Text {name} is invalid; {default} used"));
        }
        NonNegativeProperty::new(default).expect("non-negative text default")
    })
}

fn finite_or_default(
    value: Option<f64>,
    default: f64,
    name: &str,
    warnings: &mut Vec<String>,
) -> f64 {
    value.filter(|value| value.is_finite()).unwrap_or_else(|| {
        if value.is_some() {
            warnings.push(format!("Source Text {name} is non-finite; {default} used"));
        }
        default
    })
}

fn identity_transform() -> Transform {
    Transform {
        anchor_point: [0.0, 0.0],
        position: Position::TwoD([0.0, 0.0]),
        scale: [100.0, 100.0],
        rotation: 0.0,
        skew: 0.0,
        skew_axis: 0.0,
        rotation_x: 0.0,
        rotation_y: 0.0,
        orientation: [0.0, 0.0, 0.0],
        opacity: PercentageProperty::new(100.0).expect("100 is a valid percentage"),
    }
}

fn allocate_layer_id(next_id: &mut u64) -> Option<LayerId> {
    super::reserve_ids(next_id, 1).map(LayerId::new)
}

#[allow(dead_code)]
fn allocate_item_id(next_id: &mut u64) -> Option<FxItemId> {
    super::reserve_ids(next_id, 1).map(FxItemId::new)
}

fn only_data(children: &[Chunk], id: [u8; 4]) -> Result<&[u8], TextError> {
    let mut matches = children.iter().filter(|chunk| chunk.id() == id);
    let first = matches
        .next()
        .and_then(Chunk::data_payload)
        .ok_or(TextError::Layout("missing data chunk"))?;
    if matches.next().is_some() {
        return Err(TextError::Layout("duplicate data chunk"));
    }
    Ok(first)
}

enum Key<'a> {
    Name(&'a str),
    Index(usize),
}

fn at<'a>(value: &'a Value, path: &[Key<'_>]) -> Option<&'a Value> {
    path.iter().try_fold(value, |value, key| match key {
        Key::Name(key) => value.get(key),
        Key::Index(index) => value.index(*index),
    })
}

#[derive(Debug, thiserror::Error)]
enum TextError {
    #[error("{0}")]
    Property(#[from] PropertyError),
    #[error("{0}")]
    Cos(#[from] cos::Error),
    #[error("unsupported or malformed text property: {0}")]
    Layout(&'static str),
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_cached_box_baseline_consumes_placement_without_legacy_centering() {
        let project =
            read_project(include_bytes!("../../tests/fixtures/text/text_ranges.aep")).unwrap();
        let ItemKind::Composition(composition) = &project.item(1).unwrap().kind else {
            panic!("native text composition")
        };
        let boxed = composition
            .layers
            .iter()
            .find(|layer| layer.record.id() == 15)
            .unwrap();
        let source = read_source(boxed).unwrap().unwrap();
        let (document, _) =
            convert_document(&source.documents[0], &source.fonts, source.frame.as_ref());
        assert!(document.box_first_baseline.is_some());
        assert_eq!(document.vertical_align, Some(fx_schema::VerticalAlign::Top));
        assert_eq!(document.box_size, Some([180.0, 300.0]));
        assert_eq!(document.box_position, Some([-90.0, -150.0]));

        // Supplemental positive-leading and malformed-cache controls retain the
        // native box/layout; only these fields are changed for boundary coverage.
        let mut with_leading = source.documents[0].clone();
        fn edit_character(value: &mut Value) {
            if let Value::Dict(values) = value {
                if values.contains_key("53") && values.contains_key("54") {
                    values.insert("4".into(), Value::Bool(false));
                    values.insert("5".into(), Value::Number(65.0));
                } else {
                    for value in values.values_mut() {
                        edit_character(value);
                    }
                }
            } else if let Value::Array(values) = value {
                for value in values {
                    edit_character(value);
                }
            }
        }
        edit_character(&mut with_leading);
        let (document, _) = convert_document(&with_leading, &source.fonts, source.frame.as_ref());
        assert_eq!(document.leading.unwrap().value(), 65.0);
        assert_eq!(document.vertical_align, Some(fx_schema::VerticalAlign::Top));

        for baseline in [
            None,
            Some(Value::Number(f64::NAN)),
            Some(Value::String("bad".into())),
        ] {
            let mut malformed = source.documents[0].clone();
            if let Value::Dict(values) = &mut malformed {
                values.insert("1".into(), baseline.unwrap_or(Value::Null));
            }
            let (document, warnings) =
                convert_document(&malformed, &source.fonts, source.frame.as_ref());
            assert_eq!(document.box_first_baseline, None);
            assert_eq!(document.vertical_align, None);
            assert!(
                warnings
                    .iter()
                    .any(|warning| warning.contains("absent or malformed"))
            );
        }
        let point = composition
            .layers
            .iter()
            .find(|layer| layer.record.id() == 14)
            .unwrap();
        let source = read_source(point).unwrap().unwrap();
        let (document, _) =
            convert_document(&source.documents[0], &source.fonts, source.frame.as_ref());
        assert!(!document.box_text);
        assert_eq!(document.box_first_baseline, None);
        assert_eq!(document.vertical_align, None);
    }

    #[test]
    fn source_text_key_times_above_former_count_limit_are_retained() {
        let count = 10_001_u16;
        let mut descriptor = vec![0; 124];
        descriptor[12..16].copy_from_slice(&1_000_u32.to_be_bytes());
        let mut header = vec![0; 24];
        header[10..12].copy_from_slice(&count.to_be_bytes());
        header[18..20].copy_from_slice(&8_u16.to_be_bytes());
        let mut items = vec![0; usize::from(count) * 8];
        for (index, item) in items.chunks_exact_mut(8).enumerate() {
            item[..4].copy_from_slice(&i32::try_from(index).unwrap().to_be_bytes());
        }
        let wrapper = [Chunk::list(
            *b"tdbs",
            vec![
                Chunk::data(*b"tdb4", descriptor).unwrap(),
                Chunk::list(
                    *b"list",
                    vec![
                        Chunk::data(*b"lhd3", header).unwrap(),
                        Chunk::data(*b"ldat", items).unwrap(),
                    ],
                ),
            ],
        )];
        let (starts, warnings) = read_document_starts(&wrapper, usize::from(count));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(starts.len(), usize::from(count));
        assert_eq!(starts.last(), Some(&10.0));
        assert!(
            !read_document_starts(&wrapper, usize::from(count) + 1)
                .1
                .is_empty()
        );
    }

    #[test]
    fn text_helpers_share_generated_id_counter_range() {
        let mut next_id = u64::MAX - 1;
        assert!(super::allocate_layer_id(&mut next_id).is_some());
        assert!(super::allocate_item_id(&mut next_id).is_none());
        assert!(super::allocate_layer_id(&mut next_id).is_none());
        assert_eq!(next_id, u64::MAX);
        next_id -= 1;
        assert!(super::allocate_item_id(&mut next_id).is_some());
        assert_eq!(next_id, u64::MAX);
    }
    use fx_schema::text_animator::{SelectorMode, SelectorShape, SelectorUnits};
    use fx_schema::{AnimationGraph, Duration, PropertyValue};

    use super::*;
    use crate::{
        properties::{NumericKeyframe, NumericValueKind},
        structure::{ItemKind, read_project},
    };

    fn numeric(values: &[f64], keyed: bool) -> NumericProperty {
        let keyframe = |time_secs: f64, multiplier: f64| NumericKeyframe {
            time_secs,
            values: values.iter().map(|value| value * multiplier).collect(),
            in_interpolation: 1,
            out_interpolation: 1,
            in_speed: vec![0.0; values.len()],
            in_influence: vec![0.0; values.len()],
            out_speed: vec![0.0; values.len()],
            out_influence: vec![0.0; values.len()],
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        };
        NumericProperty {
            values: values.to_vec(),
            animated: keyed,
            expression_enabled: false,
            expression_present: false,
            dimensions_separated: false,
            keyframes: if keyed {
                vec![keyframe(1.25, 1.0), keyframe(2.25, 2.0)]
            } else {
                Vec::new()
            },
            value_kind: NumericValueKind::Continuous,
        }
    }

    fn property(name: &str, values: &[f64]) -> (String, Result<NumericProperty, PropertyError>) {
        (name.into(), Ok(numeric(values, true)))
    }

    fn text_run(name: &str, children: Vec<Chunk>) -> Vec<Chunk> {
        let mut match_name = name.as_bytes().to_vec();
        match_name.resize(40, 0);
        vec![
            Chunk::data(*b"tdmn", match_name).unwrap(),
            Chunk::list(*b"tdgp", children),
        ]
    }

    fn animator_with_position_storage(storage: Vec<Chunk>) -> Vec<Chunk> {
        let mut position_name = b"ADBE Text Position 3D".to_vec();
        position_name.resize(40, 0);
        let mut position = vec![Chunk::data(*b"tdmn", position_name).unwrap()];
        position.extend(storage);
        let properties = text_run("ADBE Text Animator Properties", position);
        let mut animator_children = vec![Chunk::data(*b"tdsb", [0, 0, 0, 3]).unwrap()];
        animator_children.extend(properties);
        text_run("ADBE Text Animator", animator_children)
    }

    fn numeric_storage(values: &[f64], integer: bool) -> Vec<Chunk> {
        let mut meta = vec![0; 124];
        meta[..2].copy_from_slice(&[0xdb, 0x99]);
        meta[3] = u8::try_from(values.len()).unwrap();
        if integer {
            meta[59] |= 4;
        }
        vec![
            Chunk::data(*b"tdb4", meta).unwrap(),
            Chunk::data(*b"tdsb", [0, 0, 0, 1]).unwrap(),
            Chunk::data(
                *b"cdat",
                values
                    .iter()
                    .flat_map(|value| value.to_be_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        ]
    }

    fn text_numeric_run(name: &str, storage: Vec<Chunk>) -> Vec<Chunk> {
        text_run(name, vec![Chunk::list(*b"tdbs", storage)])
    }

    #[test]
    fn text_identifiers_cross_former_occurrence_limit_without_wrapping() {
        let mut next_id = 10_000;
        assert_eq!(allocate_layer_id(&mut next_id), Some(LayerId::new(10_000)));
        assert_eq!(allocate_item_id(&mut next_id), Some(FxItemId::new(10_001)));
        assert_eq!(next_id, 10_002);

        let mut exhausted = u64::MAX;
        assert_eq!(allocate_layer_id(&mut exhausted), None);
        assert_eq!(allocate_item_id(&mut exhausted), None);
        assert_eq!(exhausted, u64::MAX);
    }

    fn convert_test_text(
        text_group: &[Chunk],
    ) -> (
        Vec<TextAnimator>,
        Option<TextAnchorOptions>,
        Option<TextPathOptions>,
        Vec<String>,
    ) {
        let (animators, more_options, path_options, mut warnings) =
            read_text_properties(text_group, &ExpressionLinks::new(&[], text_group));
        let source = SourceText {
            document_is_static: true,
            fonts: Vec::new(),
            documents: Vec::new(),
            document_starts: Vec::new(),
            percent: None,
            frame: None,
            animators,
            more_options,
            path_options,
            warnings: Vec::new(),
        };
        let (animators, anchor_options, path_options, conversion_warnings) =
            convert_text_properties(&source, &[], &mut 1);
        warnings.extend(conversion_warnings);
        (animators, anchor_options, path_options, warnings)
    }

    fn first_key_value<'a>(
        entries: &'a [AnimationGraphEntry],
        target: &PropertyTarget,
    ) -> &'a PropertyValue {
        let entry = entries
            .iter()
            .find(|entry| &entry.target == target)
            .unwrap();
        let fx_schema::animator::AnimatorData::Keyframes { track, .. } = entry.animator.data()
        else {
            panic!("native numeric channel must emit keyframes")
        };
        assert_eq!(track.keyframes()[0].layer_time().as_millis(), 1_250);
        track.keyframes()[0].value()
    }

    #[test]
    fn held_percent_texts_require_whole_hold_values_from_0_to_100() {
        let key = |time_secs: f64, value: f64, out_interpolation: u8| NumericKeyframe {
            time_secs,
            values: vec![value],
            in_interpolation: 3,
            out_interpolation,
            in_speed: vec![0.0],
            in_influence: vec![16.666666667],
            out_speed: vec![0.0],
            out_influence: vec![16.666666667],
            spatial_in: Vec::new(),
            spatial_out: Vec::new(),
        };
        let keyed = |keyframes| NumericProperty {
            values: Vec::new(),
            animated: true,
            expression_enabled: false,
            expression_present: false,
            dimensions_separated: false,
            keyframes,
            value_kind: NumericValueKind::Continuous,
        };
        // The last key's outgoing type has no following segment to affect.
        assert_eq!(
            held_percent_texts(&keyed(vec![
                key(0.0, 0.0, 3),
                key(0.1, 15.0, 3),
                key(0.8, 100.0, 1),
            ])),
            Ok((
                vec![0.0, 0.1, 0.8],
                vec!["0%".to_owned(), "15%".to_owned(), "100%".to_owned()]
            ))
        );
        let mut fixed = numeric(&[42.0], false);
        assert_eq!(
            held_percent_texts(&fixed),
            Ok((Vec::new(), vec!["42%".to_owned()]))
        );
        fixed.values = vec![1.0, 2.0];
        assert_eq!(
            held_percent_texts(&fixed),
            Err("the Slider is not a scalar value")
        );
        let mut vector_key = key(0.0, 1.0, 3);
        vector_key.values.push(2.0);
        for (keys, reason) in [
            (vec![key(0.0, 0.0, 1), key(1.0, 50.0, 3)], "not all Hold"),
            (vec![key(0.0, 0.0, 2), key(1.0, 50.0, 3)], "not all Hold"),
            (vec![key(0.0, 15.5, 3)], "whole numbers"),
            (vec![key(0.0, 101.0, 3)], "whole numbers"),
            (vec![key(0.0, -1.0, 3)], "whole numbers"),
            (vec![key(0.0, -0.0, 3)], "whole numbers"),
            (
                vec![key(1.0, 0.0, 3), key(1.0, 50.0, 3)],
                "strictly increasing",
            ),
            (
                vec![key(1.0, 0.0, 3), key(0.5, 50.0, 3)],
                "strictly increasing",
            ),
            (vec![key(f64::NAN, 0.0, 3)], "strictly increasing"),
            (vec![vector_key], "not a scalar"),
            (Vec::new(), "no decodable keys"),
        ] {
            let error = held_percent_texts(&keyed(keys.clone())).unwrap_err();
            assert!(error.contains(reason), "{keys:?}: {error}");
        }
    }

    #[test]
    fn adjacent_hold_ranges_share_the_millisecond_at_or_before_each_key() {
        // Native 30720-per-second Slider ticks that are not whole milliseconds.
        let [a, b, c] = [11264.0, 16384.0, 21504.0].map(|ticks| ticks / 30720.0);
        let bounds = |range: TimeRangeProperty| (range.start.as_millis(), range.end().as_millis());
        assert_eq!(bounds(hold_range(a, b)), (366, 533));
        assert_eq!(bounds(hold_range(b, c)), (533, 700));
        assert_eq!(
            bounds(hold_range(0.8, super::super::MAX_TIME_SECS)),
            (800, 1_000_000_000_000)
        );
        // Float error below an exact millisecond does not move it.
        for (seconds, millis) in [
            (30_030.0 / 30_000.0, 1_001),
            (603.0 / 600.0, 1_005),
            (1.003, 1_003),
        ] {
            assert!(
                seconds * 1000.0 < millis as f64,
                "{seconds} s is a float error case"
            );
            assert_eq!(hold_boundary(seconds).as_millis(), millis);
        }
        assert_eq!(hold_boundary(2.0 / 3.0).as_millis(), 666);
        assert_eq!(hold_boundary(1.0 / 3.0).as_millis(), 333);
    }

    #[test]
    fn keys_before_source_zero_collapse_and_later_keys_start_at_their_millisecond() {
        let mut warnings = Vec::new();
        // Frames -3 and 11 of 30 fps, as native 30720-per-second ticks.
        let starts = [-3_072.0, 11_264.0].map(|ticks: f64| ticks / 30_720.0);
        let ranges = document_hold_ranges(2, &starts, &mut warnings)
            .into_iter()
            .map(|(index, start, end)| {
                let range = hold_range(start, end);
                (index, range.start.as_millis(), range.end().as_millis())
            })
            .collect::<Vec<_>>();
        assert_eq!(ranges, [(0, 0, 366), (1, 366, 1_000_000_000_000)]);
        assert!(warnings.is_empty());
    }

    #[test]
    fn document_hold_ranges_cover_pre_key_time_and_collapse_non_visible_keys() {
        let mut warnings = Vec::new();
        assert_eq!(
            document_hold_ranges(1, &[], &mut warnings),
            vec![(0, 0.0, super::super::MAX_TIME_SECS)]
        );
        assert!(warnings.is_empty());

        assert_eq!(
            document_hold_ranges(4, &[-2.0, -1.0, 2.0, 5.0], &mut warnings),
            vec![
                (1, 0.0, 2.0),
                (2, 2.0, 5.0),
                (3, 5.0, super::super::MAX_TIME_SECS),
            ]
        );
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("document 1"))
        );
    }

    #[test]
    fn all_runtime_supported_text_numeric_channels_emit_source_local_tracks() {
        let animator_properties = vec![
            property("ADBE Text Anchor Point 3D", &[1.0, 2.0, 3.0]),
            property("ADBE Text Position 3D", &[4.0, 5.0, 6.0]),
            property("ADBE Text Scale 3D", &[100.0, 80.0, 100.0]),
            property("ADBE Text Rotation", &[7.0]),
            property("ADBE Text Skew", &[8.0]),
            property("ADBE Text Skew Axis", &[9.0]),
            property("ADBE Text Tracking Amount", &[10.0]),
            property("ADBE Text Stroke Width", &[11.0]),
            property("ADBE Text Blur", &[12.0, 13.0]),
            property("ADBE Text Opacity", &[75.0]),
            property("ADBE Text Fill Color", &[0.1, 0.2, 0.3, 0.4]),
            property("ADBE Text Stroke Color", &[0.5, 0.6, 0.7, 0.8]),
            property("ADBE Text Line Spacing", &[0.0, 14.0]),
            property("ADBE Text Line Anchor", &[15.0]),
            property("ADBE Text Character Offset", &[16.0]),
            property("ADBE Text Character Replace", &[65.0]),
        ];
        let range_properties = vec![
            ("ADBE Text Range Units".into(), Ok(numeric(&[1.0], false))),
            property("ADBE Text Percent Start", &[10.0]),
            property("ADBE Text Percent End", &[90.0]),
            property("ADBE Text Percent Offset", &[20.0]),
            property("ADBE Text Selector Max Amount", &[50.0]),
            property("ADBE Text Levels Max Ease", &[25.0]),
            property("ADBE Text Levels Min Ease", &[-25.0]),
            property("ADBE Text Random Seed", &[17.0]),
        ];
        let wiggly_properties = vec![
            property("ADBE Text Temporal Freq", &[2.0]),
            property("ADBE Text Wiggly Max Amount", &[60.0]),
            property("ADBE Text Wiggly Random Seed", &[18.0]),
        ];
        let source = AnimatorSource {
            name: "Animator 1".into(),
            properties: animator_properties,
            selectors: vec![
                SelectorSource::Range {
                    properties: range_properties,
                },
                SelectorSource::Wiggly {
                    properties: wiggly_properties,
                },
            ],
        };
        let animator = TextAnimator {
            id: FxItemId::new(100),
            selectors: vec![RangeSelector {
                id: FxItemId::new(101),
                ..RangeSelector::default()
            }],
            wiggly_selectors: vec![WigglySelector {
                id: FxItemId::new(102),
                ..WigglySelector::default()
            }],
            ..TextAnimator::default()
        };
        let mut entries = Vec::new();
        let mut warnings = Vec::new();
        let mut budget = AnimationBudget::default();
        append_animator_animation(
            &source,
            &animator,
            "Text segment 1 Animator 1",
            NumericAnimationClock::source_local(),
            &mut entries,
            &mut warnings,
            &mut budget,
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(entries.len(), 26);
        AnimationGraph::from_entries(entries.clone()).unwrap();

        for property_name in TextAnimator::ANIMATABLE_PROPERTIES {
            assert!(
                entries.iter().any(|entry| {
                    entry.target == PropertyTarget::fx_item(animator.id, *property_name)
                }),
                "missing animator target {property_name}"
            );
        }
        for property_name in RangeSelector::ANIMATABLE_PROPERTIES {
            assert!(entries.iter().any(|entry| {
                entry.target == PropertyTarget::fx_item(FxItemId::new(101), *property_name)
            }));
        }
        for property_name in WigglySelector::ANIMATABLE_PROPERTIES {
            assert!(entries.iter().any(|entry| {
                entry.target == PropertyTarget::fx_item(FxItemId::new(102), *property_name)
            }));
        }
        assert_eq!(
            first_key_value(
                &entries,
                &PropertyTarget::fx_item(FxItemId::new(101), "start")
            ),
            &PropertyValue::Float(0.1)
        );
        assert_eq!(
            first_key_value(
                &entries,
                &PropertyTarget::fx_item(animator.id, "lineSpacing")
            ),
            &PropertyValue::Float(14.0)
        );
        assert_eq!(
            first_key_value(&entries, &PropertyTarget::fx_item(animator.id, "fillColor")),
            &PropertyValue::Color([0.1, 0.2, 0.3, 0.4])
        );

        for segment_start_ms in [999_i32, 1_000, 1_001, 2_000] {
            let mut rebased = Vec::new();
            let mut rebased_warnings = Vec::new();
            let mut budget = AnimationBudget::default();
            append_animator_animation(
                &source,
                &animator,
                "Timed Text Animator",
                NumericAnimationClock::source_local_rebased(f64::from(segment_start_ms) / 1_000.0),
                &mut rebased,
                &mut rebased_warnings,
                &mut budget,
            );
            assert!(rebased_warnings.is_empty(), "{rebased_warnings:?}");
            assert_eq!(rebased.len(), entries.len());
            for entry in &rebased {
                let keys = entry.animator.keyframe_track().unwrap().keyframes();
                assert_eq!(
                    keys[0].layer_time().as_millis(),
                    1_250 - i64::from(segment_start_ms)
                );
                assert_eq!(
                    keys[1].layer_time().as_millis(),
                    2_250 - i64::from(segment_start_ms)
                );
            }
        }
    }

    #[test]
    fn budget_rejection_keeps_static_text_numeric_and_later_affordable_sibling() {
        let source_animator = AnimatorSource {
            name: "Budgeted Animator".into(),
            properties: vec![
                property("ADBE Text Rotation", &[7.0]),
                property("ADBE Text Skew", &[8.0]),
            ],
            selectors: Vec::new(),
        };
        let source = SourceText {
            document_is_static: true,
            fonts: Vec::new(),
            documents: Vec::new(),
            document_starts: Vec::new(),
            percent: None,
            frame: None,
            animators: vec![source_animator],
            more_options: Vec::new(),
            path_options: Vec::new(),
            warnings: Vec::new(),
        };
        let (animators, _, _, conversion_warnings) = convert_text_properties(&source, &[], &mut 1);
        assert!(conversion_warnings.is_empty(), "{conversion_warnings:?}");
        assert_eq!(animators[0].rotation, Some(7.0));
        assert_eq!(animators[0].skew, Some(8.0));

        let target = PropertyTarget::fx_item(animators[0].id, "rotation");
        let mut sizing_budget = AnimationBudget::default();
        let mut sized_entries = Vec::new();
        let mut sized_warnings = Vec::new();
        append_numeric_animation(
            &source.animators[0].properties,
            "ADBE Text Rotation",
            "affordable later text sibling",
            NumericAnimationTarget::float(target.clone(), 0, 1.0),
            NumericAnimationClock::source_local(),
            TextAnimationOutput::new(&mut sized_entries, &mut sized_warnings, &mut sizing_budget),
        );
        assert!(sized_warnings.is_empty(), "{sized_warnings:?}");
        assert_eq!(sized_entries.len(), 1);
        let serialized_bytes: usize = sized_entries
            .iter()
            .map(super::super::animation_budget::committed_entry_serialized_bytes)
            .sum::<Result<_, _>>()
            .unwrap();
        assert!(serialized_bytes <= sizing_budget.used());

        let numeric = source.animators[0].properties[0].1.as_ref().unwrap();
        let mut budget = AnimationBudget::with_limit(sizing_budget.used());
        let (rejected, rejection_warnings) = numeric_entries(
            "oversized earlier text track",
            numeric,
            &[
                NumericAnimationTarget::float(target.clone(), 0, 1.0),
                NumericAnimationTarget::float(
                    PropertyTarget::fx_item(animators[0].id, "skew"),
                    0,
                    1.0,
                ),
            ],
            NumericAnimationClock::source_local(),
            &mut budget,
        );
        assert!(rejected.is_empty());
        assert_eq!(
            budget.used(),
            0,
            "failed reservation must consume zero bytes"
        );
        assert!(
            rejection_warnings.iter().any(|warning| {
                warning.contains("animation budget") && warning.contains("static values retained")
            }),
            "warnings: {rejection_warnings:?}"
        );

        let mut later_entries = Vec::new();
        let mut later_warnings = Vec::new();
        append_numeric_animation(
            &source.animators[0].properties,
            "ADBE Text Rotation",
            "affordable later text sibling",
            NumericAnimationTarget::float(target, 0, 1.0),
            NumericAnimationClock::source_local(),
            TextAnimationOutput::new(&mut later_entries, &mut later_warnings, &mut budget),
        );
        assert!(later_warnings.is_empty(), "{later_warnings:?}");
        assert_eq!(later_entries.len(), 1);
        assert_eq!(animators[0].rotation, Some(7.0));
        assert_eq!(animators[0].skew, Some(8.0));
    }

    #[test]
    fn text_option_tracks_and_mask_path_bind_use_actual_item_ids() {
        let anchor = [(
            "ADBE Text Anchor Point Align".into(),
            Ok(numeric(&[5.0, 10.0], true)),
        )];
        let path = [
            ("ADBE Text Path".into(), Ok(numeric(&[2.0], false))),
            property("ADBE Text First Margin", &[20.0]),
            property("ADBE Text Last Margin", &[30.0]),
        ];
        let source = SourceText {
            document_is_static: true,
            fonts: Vec::new(),
            documents: Vec::new(),
            document_starts: Vec::new(),
            percent: None,
            frame: None,
            animators: Vec::new(),
            more_options: anchor.to_vec(),
            path_options: path.to_vec(),
            warnings: Vec::new(),
        };
        let mut next_id = 200;
        let (_, anchor_options, path_options, warnings) = convert_text_properties(
            &source,
            &[(1, LayerId::new(700)), (2, LayerId::new(701))],
            &mut next_id,
        );
        assert!(
            warnings
                .iter()
                .all(|warning| !warning.contains("no imported editable mask")),
            "{warnings:?}"
        );
        let anchor_options = anchor_options.unwrap();
        let path_options = path_options.unwrap();
        assert_eq!(path_options.path_layer, LayerId::new(701));

        let mut entries = Vec::new();
        let mut animation_warnings = Vec::new();
        let mut budget = AnimationBudget::default();
        append_numeric_animation(
            &source.more_options,
            "ADBE Text Anchor Point Align",
            "Text More Options",
            NumericAnimationTarget::vector2(
                PropertyTarget::fx_item(anchor_options.id, "groupingAlignment"),
                [0, 1],
                [1.0, 1.0],
            ),
            NumericAnimationClock::source_local(),
            TextAnimationOutput::new(&mut entries, &mut animation_warnings, &mut budget),
        );
        for (source_name, target_name) in [
            ("ADBE Text First Margin", "firstMargin"),
            ("ADBE Text Last Margin", "lastMargin"),
        ] {
            append_numeric_animation(
                &source.path_options,
                source_name,
                "Text Path Options",
                NumericAnimationTarget::float(
                    PropertyTarget::fx_item(path_options.id, target_name),
                    0,
                    1.0,
                ),
                NumericAnimationClock::source_local(),
                TextAnimationOutput::new(&mut entries, &mut animation_warnings, &mut budget),
            );
        }
        assert!(animation_warnings.is_empty(), "{animation_warnings:?}");
        assert_eq!(entries.len(), 3);

        for segment_start_ms in [999_i32, 1_000, 1_001, 2_000] {
            let clock =
                NumericAnimationClock::source_local_rebased(f64::from(segment_start_ms) / 1_000.0);
            let mut rebased = Vec::new();
            let mut rebased_warnings = Vec::new();
            let mut budget = AnimationBudget::default();
            append_numeric_animation(
                &source.more_options,
                "ADBE Text Anchor Point Align",
                "Timed Text More Options",
                NumericAnimationTarget::vector2(
                    PropertyTarget::fx_item(anchor_options.id, "groupingAlignment"),
                    [0, 1],
                    [1.0, 1.0],
                ),
                clock,
                TextAnimationOutput::new(&mut rebased, &mut rebased_warnings, &mut budget),
            );
            for (source_name, target_name) in [
                ("ADBE Text First Margin", "firstMargin"),
                ("ADBE Text Last Margin", "lastMargin"),
            ] {
                append_numeric_animation(
                    &source.path_options,
                    source_name,
                    "Timed Text Path Options",
                    NumericAnimationTarget::float(
                        PropertyTarget::fx_item(path_options.id, target_name),
                        0,
                        1.0,
                    ),
                    clock,
                    TextAnimationOutput::new(&mut rebased, &mut rebased_warnings, &mut budget),
                );
            }
            assert!(rebased_warnings.is_empty(), "{rebased_warnings:?}");
            assert_eq!(rebased.len(), 3);
            for entry in &rebased {
                let keys = entry.animator.keyframe_track().unwrap().keyframes();
                assert_eq!(
                    keys[0].layer_time().as_millis(),
                    1_250 - i64::from(segment_start_ms)
                );
                assert_eq!(
                    keys[1].layer_time().as_millis(),
                    2_250 - i64::from(segment_start_ms)
                );
            }
        }
    }

    #[test]
    fn malformed_animated_text_numeric_does_not_fall_back_to_static_cdat() {
        let valid_integer = read_numeric(&numeric_storage(&[2.0], true))
            .expect("integer-classified text numeric is supported by the shared decoder");
        assert_eq!(valid_integer.value_kind, NumericValueKind::Integer);
        assert_eq!(valid_integer.values, [2.0]);

        let mut malformed = numeric_storage(&[12.0, 34.0], false);
        let mut meta = malformed[0].data_payload().unwrap().to_vec();
        meta[68] = 1;
        malformed[0] = Chunk::data(*b"tdb4", meta).unwrap();
        malformed.push(Chunk::list(*b"list", Vec::new()));
        let (animators, _, _, warnings) =
            convert_test_text(&animator_with_position_storage(vec![Chunk::list(
                *b"tdbs", malformed,
            )]));
        assert_eq!(animators.len(), 1);
        assert_eq!(animators[0].position, None);
        assert!(
            warnings.iter().any(|warning| {
                warning.contains("ADBE Text Position 3D")
                    && warning.contains("missing numeric record")
            }),
            "warnings: {warnings:?}"
        );
    }

    #[test]
    fn valid_range_advanced_container_is_not_reported_as_a_numeric_leaf() {
        let units = text_numeric_run("ADBE Text Range Units", numeric_storage(&[1.0], true));
        let advanced = text_run("ADBE Text Range Advanced", units);
        let selector = text_run("ADBE Text Selector", advanced);
        let selectors = text_run("ADBE Text Selectors", selector);
        let animator = text_run("ADBE Text Animator", selectors);

        let (animators, _, _, warnings) = convert_test_text(&animator);
        assert_eq!(animators.len(), 1);
        assert_eq!(animators[0].selectors.len(), 1);
        assert!(
            warnings
                .iter()
                .all(|warning| !warning.contains("ADBE Text Range Advanced")),
            "warnings: {warnings:?}"
        );
    }

    #[test]
    fn fractional_text_ordinals_use_explicit_defaults_without_rounding() {
        let source = SourceText {
            document_is_static: true,
            fonts: Vec::new(),
            documents: Vec::new(),
            document_starts: Vec::new(),
            percent: None,
            frame: None,
            animators: vec![AnimatorSource {
                name: "Animator 1".into(),
                properties: Vec::new(),
                selectors: vec![SelectorSource::Range {
                    properties: vec![
                        ("ADBE Text Range Units".into(), Ok(numeric(&[1.6], false))),
                        ("ADBE Text Selector Mode".into(), Ok(numeric(&[1.6], false))),
                        (
                            "ADBE Text Range Shape".into(),
                            Ok(numeric(&[9_223_372_036_854_775_808.0], false)),
                        ),
                        (
                            "ADBE Text Randomize Order".into(),
                            Ok(numeric(&[0.6], false)),
                        ),
                    ],
                }],
            }],
            more_options: vec![(
                "ADBE Text Anchor Point Option".into(),
                Ok(numeric(&[1.6], false)),
            )],
            path_options: vec![("ADBE Text Path".into(), Ok(numeric(&[1.5], false)))],
            warnings: Vec::new(),
        };

        let (animators, anchor, path, warnings) =
            convert_text_properties(&source, &[(2, LayerId::new(700))], &mut 1);
        let range = &animators[0].selectors[0];
        assert_eq!(range.units, SelectorUnits::Percentage);
        assert_eq!(range.mode, SelectorMode::Add);
        assert_eq!(range.shape, SelectorShape::Square);
        assert!(!range.randomize_order);
        assert_eq!(
            anchor.unwrap().anchor_point_grouping,
            AnchorPointGrouping::Character
        );
        assert!(
            path.is_none(),
            "fractional mask indices must not bind a mask"
        );
        for name in [
            "ADBE Text Range Units",
            "ADBE Text Selector Mode",
            "ADBE Text Range Shape",
            "ADBE Text Randomize Order",
            "ADBE Text Anchor Point Option",
            "ADBE Text Path",
        ] {
            assert!(
                warnings.iter().any(|warning| {
                    warning.contains(name) && warning.contains("exact representable integer")
                }),
                "missing {name} warning in {warnings:?}"
            );
        }
    }

    #[test]
    fn fractional_range_units_route_animation_through_percentage_defaults() {
        let source = AnimatorSource {
            name: "Animator 1".into(),
            properties: Vec::new(),
            selectors: vec![SelectorSource::Range {
                properties: vec![
                    ("ADBE Text Range Units".into(), Ok(numeric(&[1.6], false))),
                    property("ADBE Text Percent Start", &[10.0]),
                ],
            }],
        };
        let animator = TextAnimator {
            id: FxItemId::new(10),
            selectors: vec![RangeSelector {
                id: FxItemId::new(11),
                ..RangeSelector::default()
            }],
            ..TextAnimator::default()
        };
        let mut entries = Vec::new();
        let mut warnings = Vec::new();
        let mut budget = AnimationBudget::default();
        append_animator_animation(
            &source,
            &animator,
            "Text segment 1 Animator 1",
            NumericAnimationClock::source_local(),
            &mut entries,
            &mut warnings,
            &mut budget,
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(
            first_key_value(
                &entries,
                &PropertyTarget::fx_item(FxItemId::new(11), "start")
            ),
            &PropertyValue::Float(0.1)
        );
        assert!(budget.used() > 0);
        assert!(
            warnings.iter().any(|warning| {
                warning.contains("ADBE Text Range Units")
                    && warning.contains("percentage units used")
            }),
            "warnings: {warnings:?}"
        );
    }

    #[test]
    fn review_text_malformed_numeric_run_framing_is_diagnosed() {
        let malformed = text_run(
            "ADBE Text More Options",
            vec![Chunk::data(*b"tdmn", vec![0]).unwrap()],
        );
        let (_, anchor_options, _, warnings) = convert_test_text(&malformed);
        assert!(
            anchor_options.is_some(),
            "malformed optional metadata must not remove editable text options"
        );
        assert!(
            warnings.iter().any(|warning| {
                warning.contains("Text More Options") && warning.contains("match-name length")
            }),
            "warnings: {warnings:?}"
        );
    }

    #[test]
    fn review_text_missing_numeric_tdbs_is_diagnosed() {
        let (animators, _, _, warnings) =
            convert_test_text(&animator_with_position_storage(Vec::new()));
        assert_eq!(
            animators.len(),
            1,
            "the editable animator sibling must survive"
        );
        assert!(
            warnings.iter().any(|warning| {
                warning.contains("Text Animator 1")
                    && warning.contains("ADBE Text Position 3D")
                    && warning.contains("missing property LIST")
            }),
            "warnings: {warnings:?}"
        );
    }

    #[test]
    fn review_text_duplicate_numeric_tdbs_is_diagnosed() {
        let storage = vec![
            Chunk::list(*b"tdbs", Vec::new()),
            Chunk::list(*b"tdbs", Vec::new()),
        ];
        let (animators, _, _, warnings) =
            convert_test_text(&animator_with_position_storage(storage));
        assert_eq!(
            animators.len(),
            1,
            "the editable animator sibling must survive"
        );
        assert!(
            warnings.iter().any(|warning| {
                warning.contains("Text Animator 1")
                    && warning.contains("ADBE Text Position 3D")
                    && warning.contains("duplicate property LIST")
            }),
            "warnings: {warnings:?}"
        );
    }

    #[test]
    fn disabled_text_groups_are_filtered_before_animation_target_assignment() {
        fn group(name: &str, enabled: bool, mut children: Vec<Chunk>) -> Vec<Chunk> {
            let mut name = name.as_bytes().to_vec();
            name.resize(40, 0);
            children.insert(
                0,
                Chunk::data(*b"tdsb", vec![0, 0, 0, if enabled { 3 } else { 2 }]).unwrap(),
            );
            vec![
                Chunk::data(*b"tdmn", name).unwrap(),
                Chunk::list(*b"tdgp", children),
            ]
        }
        let selectors = [
            group("ADBE Text Selector", false, vec![]),
            group("ADBE Text Selector", true, vec![]),
        ]
        .concat();
        let animator = group(
            "ADBE Text Animator",
            true,
            group("ADBE Text Selectors", true, selectors),
        );
        let chunks = [
            group("ADBE Text Animator", false, vec![]),
            animator,
            group("ADBE Text Path Options", false, vec![]),
        ]
        .concat();
        let (animators, _, path, warnings) =
            read_text_properties(&chunks, &ExpressionLinks::new(&[], &chunks));
        assert_eq!(animators.len(), 1);
        assert_eq!(animators[0].selectors.len(), 1);
        assert!(path.is_empty());
        assert_eq!(
            warnings
                .iter()
                .filter(|warning| warning.starts_with("disabled "))
                .count(),
            3
        );
    }

    #[test]
    fn native_empty_animator_does_not_enable_unmaterialized_property_pool() {
        let project = read_project(include_bytes!(
            "../../tests/fixtures/text/text_animator.aep"
        ))
        .unwrap();
        let layer = project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(composition) => composition
                    .layers
                    .iter()
                    .find(|layer| layer.name.as_ref() == "Animate Me"),
                _ => None,
            })
            .unwrap();
        let source = read_source(layer).unwrap().unwrap();
        let mut next_id = 1;
        let (animators, _, _, warnings) = convert_text_properties(&source, &[], &mut next_id);
        assert_eq!(animators.len(), 1);
        let animator = &animators[0];
        // This native fixture materializes an empty Animator Properties group.
        // py-aep's synthesized property pool describes available controls, not
        // enabled operations: synthesizing its red fill would recolor the text.
        assert!(source.animators[0].properties.is_empty());
        assert_eq!(animator.position, None);
        assert_eq!(animator.scale, None);
        assert_eq!(animator.rotation, None);
        assert_eq!(animator.opacity, None);
        assert_eq!(animator.fill_color, None);
        assert_eq!(animator.selectors.len(), 1);
        assert_eq!(animator.selectors[0].start, 0.0);
        assert_eq!(animator.selectors[0].end, 1.0);
        assert!(
            warnings
                .iter()
                .all(|warning| !warning.contains("could not be decoded")),
            "{warnings:?}"
        );
    }

    #[test]
    fn source_text_font_table_keeps_indices_across_unreadable_entries() {
        let root = cos::parse(
            br#"<< /0 << /1 << /0 [
                << /0 << /0 42 >> >>
                << /0 << /99 /CoolTypeFont /0 << /0 (MyriadPro-Regular) >> >> >>
                << /0 << /0 () >> >>
                << /0 << /0 (Bungee-Regular) >> >>
            ] >> >> >>"#,
        )
        .unwrap();
        let fonts = extract_fonts(&root);
        assert_eq!(
            fonts,
            [
                None,
                Some("MyriadPro-Regular".into()),
                None,
                Some("Bungee-Regular".into())
            ]
        );
        for (index, family) in [(0, "sans"), (1, "MyriadPro"), (2, "sans"), (3, "Bungee")] {
            let document = cos::parse(
                format!(
                    "<< /0 << /6 << /0 [ << /0 << /0 << /6 << /0 {index} >> >> >> >> ] >> >> >>"
                )
                .as_bytes(),
            )
            .unwrap();
            let (text, warnings) = convert_document(&document, &fonts, None);
            assert_eq!(text.font_family.as_ref(), family);
            assert_eq!(
                warnings.iter().any(|warning| warning.contains("fallback")),
                index == 0 || index == 2
            );
        }
    }

    #[test]
    fn compact_source_text_font_table_preserves_editable_identity() {
        // The compact font table tests compatibility with legacy converter output,
        // not independent Adobe-native behavior.
        let root = cos::parse(
            br#"<< /0 << /1 << /0 [
                << /0 << /0 (Bungee-Regular) >> >>
            ] >> >> /1 << /1 [ << /0 << /0 (B)
                /6 << /0 [ << /0 << /0 << /6 << /0 0 /1 210 >> >> >> >> ] >>
            >> >> ] >> >>"#,
        )
        .unwrap();
        let fonts = extract_fonts(&root);
        let document = at(&root, &[Key::Name("1"), Key::Name("1"), Key::Index(0)]).unwrap();
        let (text, warnings) = convert_document(document, &fonts, None);
        assert_eq!(text.font_family.as_ref(), "Bungee");
        assert_eq!(text.font_style.as_ref(), "Regular");
        assert_eq!(text.text, "B");
        assert_eq!(text.font_size.value(), 210.0);
        assert!(!warnings.iter().any(|warning| warning.contains("fallback")));
    }

    #[test]
    fn native_source_text_becomes_editable_with_point_and_box_layout() {
        let project =
            read_project(include_bytes!("../../tests/fixtures/text/text_ranges.aep")).unwrap();
        let composition = project
            .items
            .iter()
            .find_map(|item| match &item.kind {
                ItemKind::Composition(comp) if item.id == 1 => Some(comp),
                _ => None,
            })
            .unwrap();
        let parent = GroupLayer {
            id: LayerId::new(9000),
            name: "inner".into(),
            description: String::new(),
            is_hidden: false,
            parent: None,
            blend_mode: Default::default(),
            track_matte: None,
            masks: Vec::new(),
            playback: {
                let range = TimeRangeProperty::new(Time::ZERO, Duration::from_secs(10.0));
                fx_schema::LayerPlayback::linear(range, range, range, 0).unwrap()
            },
            effects: Vec::new(),
            motion_blur: false,
            padding_top: Default::default(),
            padding_right: Default::default(),
            padding_bottom: Default::default(),
            padding_left: Default::default(),
            fills: Vec::new(),
            corner_radius_top_left: Default::default(),
            corner_radius_top_right: Default::default(),
            corner_radius_bottom_right: Default::default(),
            corner_radius_bottom_left: Default::default(),
            transform: identity_transform(),
            layers: Vec::new(),
        };
        let mut next_id = 1;
        let mut imported = composition
            .layers
            .iter()
            .map(|layer| {
                let (layers, warnings) = import(layer, &parent, &mut next_id);
                assert!(
                    !warnings
                        .iter()
                        .any(|warning| warning.contains("could not be decoded")),
                    "{warnings:?}"
                );
                let [FxLayer::Text(text)] = layers.as_slice() else {
                    panic!(
                        "layer {} was not imported as one TextLayer",
                        layer.record.id()
                    );
                };
                (layer.record.id(), text.clone())
            })
            .collect::<std::collections::BTreeMap<_, _>>();

        let point = imported.remove(&14).unwrap();
        assert_eq!(point.source_text.text, "Hello World\nSecond Paragraph\nEnd");
        assert_eq!(point.source_text.font_family.as_ref(), "MyriadPro");
        assert_eq!(point.source_text.font_style.as_ref(), "Regular");
        assert_eq!(point.source_text.font_size.value(), 72.0);
        assert_eq!(point.source_text.fill_color, [1.0, 0.0, 0.0, 1.0]);
        assert!(!point.source_text.box_text);

        let boxed = imported.remove(&15).unwrap();
        assert!(boxed.source_text.box_text);
        assert_eq!(boxed.source_text.box_size, Some([180.0, 300.0]));
        assert_eq!(boxed.source_text.box_position, Some([-90.0, -150.0]));

        let explicit_leading = imported.remove(&16).unwrap();
        assert_eq!(explicit_leading.source_text.leading.unwrap().value(), 48.0);
        let reset = imported.remove(&21).unwrap();
        assert_eq!(reset.source_text.text, "New longer\nText here");
        assert_eq!(reset.source_text.justification, Justification::Left);
    }

    #[test]
    fn source_text_caps_field_distinguishes_normal_all_caps_and_unrecognized_modes() {
        // Native evidence covers only 0 (normal) and 2 (All Caps); an absent
        // field keeps the destination default like its sibling style fields.
        let fonts = [Some("Inter-Regular".to_owned())];
        for (field, all_caps, diagnostic) in [
            ("", false, None),
            (" /12 0", false, None),
            (" /12 2", true, None),
            (" /12 1", false, Some("caps code 1 is unrecognized")),
            (" /12 3", false, Some("caps code 3 is unrecognized")),
            (" /12 2.5", false, Some("caps mode is malformed")),
            (" /12 true", false, Some("caps mode is malformed")),
        ] {
            let native = format!(
                "<< /0 << /0 (Mixed Case\\r) /6 << /0 [ << /0 << /0 << /6 << /0 0{field} >> >> >> >> ] >> >> >>"
            );
            let native = cos::parse(native.as_bytes()).unwrap();
            let (document, warnings) = convert_document(&native, &fonts, None);
            assert_eq!(document.text, "Mixed Case", "{field:?}");
            assert_eq!(document.all_caps, all_caps, "{field:?}");
            let caps_warnings = warnings
                .iter()
                .filter(|warning| warning.contains("field 12"))
                .collect::<Vec<_>>();
            match diagnostic {
                None => assert!(caps_warnings.is_empty(), "{field:?}: {caps_warnings:?}"),
                Some(expected) => assert!(
                    caps_warnings.len() == 1 && caps_warnings[0].contains(expected),
                    "{field:?}: {caps_warnings:?}"
                ),
            }
        }
    }

    #[test]
    fn percent_hold_segments_reuse_cached_native_point_text_scale() {
        let project =
            read_project(include_bytes!("../../tests/fixtures/text/text_ranges.aep")).unwrap();
        let ItemKind::Composition(composition) = &project.item(1).unwrap().kind else {
            panic!()
        };
        let point = composition
            .layers
            .iter()
            .find(|layer| layer.record.id() == 14)
            .unwrap();
        let mut source = read_source(point).unwrap().unwrap();
        source.animators.clear();
        source.path_options.clear();
        source.more_options.clear();
        source.documents = vec![cos::parse(br#"<< /0 << /0 (ABC) /6 << /0 [<< /1 3 /0 << /0 << /6 << /0 0 /1 100 /6 .91 /7 1 >> >> >> >>] >> >> >>"#).unwrap()];
        let percent = PercentHolds {
            starts: vec![0.0, 1.0],
            texts: vec!["0".into(), "100".into()],
        };
        let occurrence = super::super::group(
            LayerId::new(9000),
            "Scaled loading text".into(),
            None,
            TimeRangeProperty::new(Time::ZERO, Duration::from_secs(2.0)),
        );
        let mut next_id = 1;
        let imported = import_percent_holds(
            &source,
            &percent,
            &occurrence,
            &[],
            &mut next_id,
            &mut AnimationBudget::default(),
        )
        .unwrap();
        assert_eq!(imported.layers.len(), 2);
        for (layer, expected) in imported.layers.iter().zip(["0", "100"]) {
            let FxLayer::Text(layer) = layer else {
                panic!()
            };
            assert_eq!(layer.source_text.text, expected);
            assert_eq!(layer.transform.scale, [91.0, 100.0]);
            assert_eq!(layer.parent, Some(occurrence.id));
        }
        let FxLayer::Text(first) = &imported.layers[0] else {
            panic!()
        };
        assert_eq!(
            first.active_range,
            TimeRangeProperty::new(Time::ZERO, Duration::from_secs(1.0))
        );
        assert!(
            !imported
                .warnings
                .iter()
                .any(|warning| warning.starts_with("Source Text glyph scales"))
        );
    }

    #[test]
    fn uniform_point_text_scale_uses_native_axes_and_rejects_other_owners() {
        let project =
            read_project(include_bytes!("../../tests/fixtures/text/text_ranges.aep")).unwrap();
        let ItemKind::Composition(composition) = &project.item(1).unwrap().kind else {
            panic!()
        };
        let point = composition
            .layers
            .iter()
            .find(|l| l.record.id() == 14)
            .unwrap();
        let mut source = read_source(point).unwrap().unwrap();
        source.animators.clear();
        source.path_options.clear();
        source.more_options.clear();
        let document = cos::parse(br#"<< /0 << /0 (ABC) /6 << /0 [<< /1 3 /0 << /0 << /6 << /0 0 /1 100 /6 .91 /7 1 >> >> >> >>] >> >> >>"#).unwrap();
        let (converted, _) = convert_document(&document, &source.fonts, None);
        assert_eq!(
            point_text_scale(&source, &document, &converted),
            Some([0.91, 1.0])
        );
        let mut boxed = converted.clone();
        boxed.box_text = true;
        assert_eq!(point_text_scale(&source, &document, &boxed), None);
        for axes in ["/6 1 /7 1", "/6 0 /7 1", "/6 -1 /7 1", "/6 1 /7 0"] {
            let text = std::str::from_utf8(br#"<< /0 << /0 (ABC) /6 << /0 [<< /1 3 /0 << /0 << /6 << /0 0 /1 100 /6 .91 /7 1 >> >> >> >>] >> >> >>"#).unwrap().replace("/6 .91 /7 1", axes);
            let document = cos::parse(text.as_bytes()).unwrap();
            assert_eq!(point_text_scale(&source, &document, &converted), None);
        }
        let partial = cos::parse(br#"<< /0 << /0 (ABC) /6 << /0 [<< /1 1 /0 << /0 << /6 << /6 .91 /7 1 >> >> >> >>] >> >> >>"#).unwrap();
        assert_eq!(point_text_scale(&source, &partial, &converted), None);
        // No outer placement is changed by the inner text geometry operation.
        let outer = identity_transform();
        let original = serde_json::to_value(outer).unwrap();
        let mut inner = identity_transform();
        let scale = point_text_scale(&source, &document, &converted).unwrap();
        inner.scale = [scale[0] * 100.0, scale[1] * 100.0];
        assert_eq!(inner.scale, [91.0, 100.0]);
        assert_eq!(serde_json::to_value(outer).unwrap(), original);
    }
    fn native_point_scale_layers() -> Vec<Layer> {
        let p = read_project(include_bytes!("../../tests/fixtures/text/text_ranges.aep")).unwrap();
        let ItemKind::Composition(comp) = &p.item(1).unwrap().kind else {
            panic!()
        };
        let template = &comp.layers[0];
        let parsed = crate::rifx::Rifx::parse_with(
            include_bytes!("../../tests/fixtures/text/point-scale-vertical-animators.rifx"),
            |kind| kind == *b"btdk",
        )
        .unwrap();
        parsed
            .chunks()
            .iter()
            .map(|chunk| {
                let mut layer = template.clone();
                layer.content = chunk.children().unwrap().to_vec();
                layer.record = crate::schema::layer_records::LayerRecord::decode(
                    crate::properties::data(&layer.content, *b"ldta").unwrap(),
                )
                .unwrap();
                layer.name = "Independent animated point text".into();
                layer
            })
            .collect()
    }

    #[test]
    fn native_point_scale_keeps_vertical_motion_and_stroke_animation_editable() {
        for layer in native_point_scale_layers() {
            let source = read_source(&layer).unwrap().unwrap();
            let (document, _) =
                convert_document(&source.documents[0], &source.fonts, source.frame.as_ref());
            assert_eq!(
                point_text_scale(&source, &source.documents[0], &document),
                Some([0.91, 1.])
            );
            let occurrence = super::super::group(
                LayerId::new(1000),
                "Different text owner".into(),
                None,
                TimeRangeProperty::new(Time::ZERO, Duration::from_secs(6.)),
            );
            let (layers, warnings) = import(&layer, &occurrence, &mut 1001);
            let [FxLayer::Text(text)] = layers.as_slice() else {
                panic!("{warnings:?}")
            };
            assert_eq!(text.transform.scale, [91., 100.]);
            assert_eq!(text.transform.position, Position::xy(0., 0.));
            assert_eq!(text.source_text.font_family, document.font_family);
            assert_eq!(text.animators.len(), source.animators.len());
            let (entries, animation_warnings) =
                animation_entries(&layer, &layers, &mut AnimationBudget::default());
            assert!(!entries.is_empty(), "{animation_warnings:?}");
            for (index, native) in source.animators.iter().enumerate() {
                for (name, param) in [
                    ("ADBE Text Position 3D", "position"),
                    ("ADBE Text Stroke Width", "strokeWidth"),
                ] {
                    let Some((_, numeric)) = native.properties.iter().find(|(n, _)| n == name)
                    else {
                        continue;
                    };
                    let numeric = numeric.as_ref().unwrap();
                    if numeric.keyframes.is_empty() {
                        continue;
                    }
                    let target = PropertyTarget::fx_item(text.animators[index].id, param);
                    let entry = entries.iter().find(|e| e.target == target).unwrap();
                    assert_eq!(
                        entry.animator.keyframe_track().unwrap().keyframes().len(),
                        numeric.keyframes.len()
                    );
                    if param == "position" {
                        let easing =
                            entry.animator.keyframe_track().unwrap().keyframes()[1].easing();
                        let fx_schema::animator::PropertyKeyframeEasing::CubicBezier {
                            x1,
                            y1,
                            x2,
                            y2,
                        } = easing
                        else {
                            panic!("{easing:?}")
                        };
                        for (actual, expected) in [
                            (x1, 0.36167874301437036),
                            (y1, 0.16195255474452552),
                            (x2, 0.12),
                            (y2, 0.86),
                        ] {
                            assert!(
                                (actual - expected).abs() < 1e-9,
                                "mirrored native spatial motion must share positive progress: {easing:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn point_scale_animator_admission_is_axis_and_control_based() {
        let layer = native_point_scale_layers().remove(0);
        let mut source = read_source(&layer).unwrap().unwrap();
        let document = cos::parse(br#"<< /0 << /0 (DIFFERENT) /6 << /0 [<< /1 9 /0 << /0 << /6 << /0 0 /1 100 /6 .73 /7 1 >> >> >> >>] >> >> >>"#).unwrap();
        let (converted, _) = convert_document(&document, &source.fonts, None);
        source.documents = vec![document.clone()];
        assert_eq!(
            point_text_scale(&source, &document, &converted),
            Some([0.73, 1.])
        );
        for case in 0..5 {
            let mut source = read_source(&layer).unwrap().unwrap();
            let numeric = source
                .animators
                .iter_mut()
                .flat_map(|a| &mut a.properties)
                .find(|(name, _)| name == "ADBE Text Position 3D")
                .unwrap()
                .1
                .as_mut()
                .unwrap();
            match case {
                0 => numeric.keyframes[0].values[0] = 1.,
                1 => numeric.keyframes[0].values[2] = 1.,
                2 => numeric.keyframes[0].values[1] = f64::NAN,
                3 => numeric.expression_enabled = true,
                _ => numeric.keyframes[0].spatial_out = vec![1., 0., 0.],
            }
            assert_eq!(
                point_text_scale(&source, &document, &converted),
                None,
                "case {case}"
            );
        }
        source.animators[0]
            .properties
            .push(("ADBE Text Rotation".into(), Ok(numeric(&[10.], false))));
        assert_eq!(point_text_scale(&source, &document, &converted), None);
    }
}
