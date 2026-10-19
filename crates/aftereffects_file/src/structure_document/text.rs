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
    expression_samples::{EvaluatedProperty, ExpressionSamples, PropertyIdentity},
    properties::{NumericProperty, PropertyError, read_numeric, root_runs, runs, unique_list},
    rifx::Chunk,
    structure::Layer,
};

use super::{
    animation::{
        NumericAnimationClock, NumericAnimationTarget, evaluated_numeric_entries, numeric_entries,
        rebased_samples_with,
    },
    animation_budget::AnimationBudget,
    control_links,
};

mod alternating;
pub(crate) mod cos;
mod expression_links;
mod path_reference;
mod source_alias;
#[cfg(test)]
mod source_alias_cases;
#[cfg(test)]
mod source_alias_tests;

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
#[cfg(test)]
pub(super) fn import_with_mask_guides(
    layer: &Layer,
    occurrence: &GroupLayer,
    mask_guide_ids: &[(u32, LayerId)],
    next_id: &mut u64,
    budget: &mut AnimationBudget,
) -> TextImport {
    import_text(
        layer,
        None,
        None,
        occurrence,
        mask_guide_ids,
        next_id,
        budget,
    )
}

/// Resolves direct Source Text content aliases against the effective, occurrence-local
/// composition. Unsupported animator expressions remain separately diagnosed.
pub(super) fn import_in_composition(
    layer: &Layer,
    composition: &crate::structure::Composition,
    evaluations: Option<(u32, &ExpressionSamples)>,
    occurrence: &GroupLayer,
    mask_guide_ids: &[(u32, LayerId)],
    next_id: &mut u64,
    budget: &mut AnimationBudget,
) -> TextImport {
    import_text(
        layer,
        Some(composition),
        evaluations,
        occurrence,
        mask_guide_ids,
        next_id,
        budget,
    )
}

fn import_text(
    layer: &Layer,
    composition: Option<&crate::structure::Composition>,
    evaluations: Option<(u32, &ExpressionSamples)>,
    occurrence: &GroupLayer,
    mask_guide_ids: &[(u32, LayerId)],
    next_id: &mut u64,
    budget: &mut AnimationBudget,
) -> TextImport {
    let mut warnings = Vec::new();
    let mut source = match read_source(layer) {
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

    let alias_rejected =
        composition.is_some_and(|composition| source_alias::lower(layer, composition, &mut source));
    if !alias_rejected
        && source.percent.is_none()
        && let Some((comp_id, samples)) = evaluations
        && let Some(evaluated) = samples
            .texts
            .iter()
            .find(|text| text.composition_id == comp_id && text.layer_id == layer.record.id())
    {
        match evaluated_text_holds(layer, evaluated) {
            Ok(mut holds) => {
                let copied_style =
                    evaluated
                        .style_source
                        .and_then(|[source_comp, source_layer]| {
                            if source_comp != comp_id {
                                return None;
                            }
                            let target = composition?
                                .layers
                                .iter()
                                .find(|target| target.record.id() == source_layer)?;
                            let target = read_source(target).ok()??;
                            if !target.document_is_static
                                || target.documents.len() != 1
                                || !source_alias::uniform_styles(&target.documents[0], "6")
                            {
                                return None;
                            }
                            Some(convert_document(&target.documents[0], &target.fonts, None))
                        });
                if let Some((style, warnings)) = copied_style {
                    holds.style = Some(style);
                    source.warnings.extend(warnings);
                }
                if evaluated.style_source.is_some() {
                    source.warnings.push(if holds.style.is_some() {
                        "Source Text setText copies static uniform character style; consumer paragraph/box geometry remains local and live linkage is lost".into()
                    } else {
                        "Source Text setText style not lowered: source must be same-composition, static and uniform; consumer style retained".into()
                    });
                }
                source
                    .warnings
                    .retain(|warning| !warning.contains("enabled expression not lowered"));
                source.warnings.push(format!("Source Text expression evaluated by the converter into {} held text segment(s) at source frames; live expression linkage is not retained",holds.texts.len()));
                source.percent = Some(holds);
            }
            Err(reason) => source.warnings.push(format!(
                "Source Text expression samples not lowered: {reason}"
            )),
        }
    }
    if let Some((comp_id, samples)) = evaluations {
        let clock = [
            layer.record.start_time().unwrap_or(f64::NAN),
            layer.record.stretch().unwrap_or(f64::NAN),
        ];
        for (index, animator) in source.animators.iter_mut().enumerate() {
            let Ok(native_index) = u32::try_from(index + 1) else {
                continue;
            };
            for (name, numeric) in &animator.properties {
                if !numeric
                    .as_ref()
                    .is_ok_and(|numeric| numeric.expression_enabled)
                {
                    continue;
                }
                if let Some(sample) = samples.lookup(
                    comp_id,
                    layer.record.id(),
                    &PropertyIdentity::TextAnimator {
                        animator: native_index,
                        match_name: name.clone(),
                    },
                ) {
                    animator
                        .evaluated
                        .push((name.clone(), sample.clone(), clock));
                }
            }
            for (selector_index, selector) in animator.selectors.iter().enumerate() {
                let SelectorSource::Range { properties } = selector else {
                    continue;
                };
                let Ok(selector) = u32::try_from(selector_index + 1) else {
                    continue;
                };
                for (name, numeric) in properties {
                    if !numeric
                        .as_ref()
                        .is_ok_and(|numeric| numeric.expression_enabled)
                    {
                        continue;
                    }
                    if let Some(sample) = samples.lookup(
                        comp_id,
                        layer.record.id(),
                        &PropertyIdentity::TextSelector {
                            animator: native_index,
                            selector,
                            match_name: name.clone(),
                        },
                    ) {
                        animator.evaluated.push((
                            selector_sample_name(selector_index, name),
                            sample.clone(),
                            clock,
                        ));
                    }
                }
            }
        }
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
        let (mut animators, anchor_options, path_options, mut property_warnings) =
            convert_text_properties(source, mask_guide_ids, next_id);
        // Native animator tracking is pixels; document tracking is already 1/1000 em.
        for animator in &mut animators {
            animator.tracking = animator.tracking.and_then(|value| {
                let scaled = value * (1000.0 / document.font_size.value());
                if scaled.is_finite() {
                    Some(scaled)
                } else {
                    property_warnings.push(
                        "Source Text animator tracking overflows em units; static tracking omitted"
                            .into(),
                    );
                    None
                }
            });
        }
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
    cached_glyph_width(document)
}

/// Average source glyph extent is only a diagnosed layout approximation for an
/// edited capsule string; it is not a sourceRectAtTime evaluation.
pub(super) fn saved_graphic_width(layer: &Layer, current: &TextDocument) -> Result<f64, TextError> {
    let source = read_source(layer)?.ok_or(TextError::Layout("missing Source Text layout"))?;
    let (original, _) =
        convert_document(&source.documents[0], &source.fonts, source.frame.as_ref());
    let count = original.text.trim_end_matches(['\r', '\n']).chars().count();
    if count == 0 || current.text.contains(['\r', '\n']) {
        return Err(TextError::Layout(
            "single-line cached advance approximation unavailable",
        ));
    }
    Ok(
        cached_glyph_width(&source.documents[0]).map_err(TextError::Layout)?
            * current.text.chars().count() as f64
            / count as f64
            * current.font_size.value()
            / original.font_size.value(),
    )
}

fn cached_glyph_width(document: &Value) -> Result<f64, &'static str> {
    let layout = document
        .get("1")
        .and_then(|value| value.get("2"))
        .ok_or("caption cached layout is missing")?;
    let mut lines = 0;
    let mut boxes = Vec::new();
    fn collect(value: &Value, lines: &mut usize, boxes: &mut Vec<[f64; 4]>) -> Option<()> {
        let mut pending = vec![value];
        while let Some(value) = pending.pop() {
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
                Value::Array(values) => pending.extend(values.iter().rev()),
                Value::Dict(values) => pending.extend(values.values().rev()),
                _ => {}
            }
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
                1000.0 / text.source_text.font_size.value(),
                TextAnimationOutput::new(&mut entries, &mut warnings, budget),
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
    style: Option<TextDocument>,
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
        if let Some(style) = &percent.style {
            let mut copied = style.clone();
            copied.justification = document.justification;
            copied.box_text = document.box_text;
            copied.box_size = document.box_size;
            copied.box_position = document.box_position;
            copied.box_first_baseline = document.box_first_baseline;
            copied.vertical_align = document.vertical_align;
            copied.scale_box_text_with_transform = document.scale_box_text_with_transform;
            document = copied;
        }
        document.text = editable_text(&percent.texts[index]);
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
    /// Expression samples of enabled animator/selector fields, in the parent
    /// composition clock, with the owner layer's start/stretch.
    evaluated: Vec<(String, EvaluatedProperty, [f64; 2])>,
}

/// Enabled animator and Range Selector leaves, in the same order as text import.
pub(crate) fn expression_property_leaves(layer: &Layer) -> Vec<(PropertyIdentity, &[Chunk])> {
    let mut ignored = Vec::new();
    let mut result = Vec::new();
    let Some(text_group) = root_runs(&layer.content).ok().and_then(|roots| {
        roots
            .into_iter()
            .find(|(name, _)| *name == "ADBE Text Properties")
            .and_then(|(_, run)| unique_list(run, *b"tdgp").ok())
    }) else {
        return result;
    };
    let Ok(text_runs) = runs(text_group) else {
        return result;
    };
    let mut animators = Vec::new();
    for (name, run) in text_runs {
        match name {
            "ADBE Text Animators" => {
                if let Ok(children) = unique_list(run, *b"tdgp").and_then(runs) {
                    animators.extend(
                        children
                            .into_iter()
                            .filter(|(name, _)| *name == "ADBE Text Animator")
                            .map(|(_, run)| run),
                    );
                }
            }
            "ADBE Text Animator" => animators.push(run),
            _ => {}
        }
    }
    let mut animator = 0;
    for run in animators {
        if !enabled_text_group(run, "ADBE Text Animator", &mut ignored) {
            continue;
        }
        animator += 1;
        let Ok(children) = unique_list(run, *b"tdgp").and_then(runs) else {
            continue;
        };
        for (name, run) in children {
            let Ok(leaves) = unique_list(run, *b"tdgp").and_then(runs) else {
                continue;
            };
            if name == "ADBE Text Animator Properties" {
                result.extend(leaves.into_iter().map(|(name, run)| {
                    (
                        PropertyIdentity::TextAnimator {
                            animator,
                            match_name: name.to_owned(),
                        },
                        run,
                    )
                }));
            } else if name == "ADBE Text Selectors" {
                let mut selector = 0;
                for (name, run) in leaves {
                    if !enabled_text_group(run, name, &mut ignored) {
                        continue;
                    }
                    selector += 1;
                    if name != "ADBE Text Selector" {
                        continue;
                    }
                    let Ok(group) = unique_list(run, *b"tdgp") else {
                        continue;
                    };
                    let Ok(mut fields) = runs(group) else {
                        continue;
                    };
                    if let Some((_, advanced)) = fields
                        .iter()
                        .find(|(name, _)| *name == "ADBE Text Range Advanced")
                        && let Ok(advanced) = unique_list(advanced, *b"tdgp").and_then(runs)
                    {
                        fields.extend(advanced);
                    }
                    result.extend(
                        fields
                            .into_iter()
                            .filter(|(name, _)| {
                                matches!(
                                    *name,
                                    "ADBE Text Percent Start"
                                        | "ADBE Text Percent End"
                                        | "ADBE Text Percent Offset"
                                        | "ADBE Text Index Start"
                                        | "ADBE Text Index End"
                                        | "ADBE Text Index Offset"
                                        | "ADBE Text Selector Max Amount"
                                        | "ADBE Text Levels Max Ease"
                                        | "ADBE Text Levels Min Ease"
                                        | "ADBE Text Random Seed"
                                )
                            })
                            .map(|(name, run)| {
                                (
                                    PropertyIdentity::TextSelector {
                                        animator,
                                        selector,
                                        match_name: name.to_owned(),
                                    },
                                    run,
                                )
                            }),
                    );
                }
            }
        }
    }
    result
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

/// Read the same static, uniform document used by ordinary AEP text import.
/// Capsule controllers bind native layer IDs before calling this decoder.
pub(crate) fn saved_graphic_document(
    layer: &Layer,
) -> Result<(TextDocument, String, Vec<String>), TextError> {
    let source =
        read_source(layer)?.ok_or(TextError::Layout("controller source has no Source Text"))?;
    if !source.document_is_static
        || source.documents.len() != 1
        || !source.document_starts.is_empty()
        || run_count(&source.documents[0], "6") != 1
    {
        return Err(TextError::Layout(
            "capsule Source Text requires one static character style",
        ));
    }
    let (document, mut warnings) =
        convert_document(&source.documents[0], &source.fonts, source.frame.as_ref());
    warnings.extend(source.warnings);
    if !source.animators.is_empty()
        || !source.path_options.is_empty()
        || !source.more_options.is_empty()
    {
        warnings.push("capsule static text snapshot does not reconstruct Text animators, path or More Options controllers".into());
    }
    let font = if document.font_style.as_ref().is_empty() {
        document.font_family.to_string()
    } else {
        format!("{}-{}", document.font_family, document.font_style)
    };
    Ok((document, font, warnings))
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
            (flags.len() == 124 && flags[..2] == [0xdb, 0x99]).then(|| {
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
        read_text_properties(text_group, &links, &layer.content);
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
    source_content: &[Chunk],
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
            "ADBE Text Path Options" => {
                match unique_list(run, *b"tdgp")
                    .and_then(|group| path_reference::read(group, source_content))
                {
                    Ok(properties) => path_options = properties,
                    Err(error) => warnings.push(format!("Text Path Options omitted: {error}")),
                }
            }
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
        evaluated: Vec::new(),
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
                "{context} {name}: control link not lowered ({error}); the stored/keyed value is the fallback unless converter expression evaluation is admitted"
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

/// Animator properties with converter expression samples use those samples on
/// the same editable target; everything else imports native keys/values.
fn append_animator_property(
    source: &AnimatorSource,
    source_name: &str,
    context: &str,
    target: NumericAnimationTarget,
    clock: NumericAnimationClock,
    output: TextAnimationOutput<'_>,
) {
    append_sampled_property(
        &source.properties,
        source
            .evaluated
            .iter()
            .find(|(name, _, _)| name == source_name),
        source_name,
        context,
        target,
        clock,
        output,
    );
}

fn selector_sample_name(index: usize, name: &str) -> String {
    format!("selector {} {name}", index + 1)
}

fn append_sampled_property(
    properties: &[(String, Result<NumericProperty, PropertyError>)],
    evaluated: Option<&(String, EvaluatedProperty, [f64; 2])>,
    source_name: &str,
    context: &str,
    target: NumericAnimationTarget,
    clock: NumericAnimationClock,
    output: TextAnimationOutput<'_>,
) {
    if let Some((_, samples, owner_clock)) = evaluated {
        let name = format!("{context} {source_name}");
        match rebased_samples_with(samples, *owner_clock, clock) {
            Ok(rebased) => {
                let (mut entries, mut warnings) = evaluated_numeric_entries(
                    &name,
                    &rebased,
                    std::slice::from_ref(&target),
                    &[],
                    output.budget,
                );
                output.warnings.append(&mut warnings);
                if !entries.is_empty() {
                    output.warnings.push(format!("{name}: numeric text expression evaluated once per layer, not per character"));
                    output.entries.append(&mut entries);
                    return;
                }
            }
            Err(error) => output
                .warnings
                .push(format!("{name}: {error}; expression samples not lowered")),
        }
    }
    append_numeric_animation(properties, source_name, context, target, clock, output);
}

fn append_animator_animation(
    source: &AnimatorSource,
    animator: &TextAnimator,
    context: &str,
    clock: NumericAnimationClock,
    tracking_scale: f64,
    output: TextAnimationOutput<'_>,
) {
    let TextAnimationOutput {
        entries,
        warnings,
        budget,
    } = output;
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
        append_animator_property(
            source,
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
        append_animator_property(
            source,
            source_name,
            context,
            scalar(target_name).scaled(if target_name == "tracking" {
                tracking_scale
            } else {
                1.0
            }),
            clock,
            TextAnimationOutput::new(entries, warnings, budget),
        );
    }
    append_animator_property(
        source,
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
        append_animator_property(
            source,
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
                let hard_reveal = hard_range_reveal(properties);
                if hard_reveal
                    && let Some((_, Ok(numeric))) =
                        properties.iter().find(|(name, _)| name == start)
                {
                    let targets = ["start", "end"].map(|name| {
                        NumericAnimationTarget::float(
                            PropertyTarget::fx_item(selector.id, name),
                            0,
                            scale,
                        )
                    });
                    let (mut keyed, mut diagnostics) = numeric_entries(
                        &format!("{selector_context} shared reveal boundary"),
                        numeric,
                        &targets,
                        clock,
                        budget,
                    );
                    entries.append(&mut keyed);
                    warnings.append(&mut diagnostics);
                }
                for (source_name, target_name, target_scale) in [
                    (start, "start", scale),
                    (end, "end", scale),
                    (offset, "offset", scale),
                    ("ADBE Text Selector Max Amount", "amount", 0.01),
                    ("ADBE Text Levels Max Ease", "easeHigh", 0.01),
                    ("ADBE Text Levels Min Ease", "easeLow", 0.01),
                    ("ADBE Text Random Seed", "randomSeed", 1.0),
                ] {
                    if hard_reveal && matches!(target_name, "start" | "end") {
                        continue;
                    }
                    let sample_name = selector_sample_name(selector_index, source_name);
                    append_sampled_property(
                        properties,
                        source
                            .evaluated
                            .iter()
                            .find(|(name, _, _)| name == &sample_name),
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
        for (name, neutral) in [
            ("ADBE Text Anchor Point 3D", 0.0),
            ("ADBE Text Position 3D", 0.0),
            ("ADBE Text Scale 3D", 100.0),
        ] {
            report_dropped_z(
                &source_animator.properties,
                name,
                neutral,
                &prefix,
                &mut warnings,
            );
        }
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
        if !source_animator.selectors.is_empty()
            && animator.selectors.is_empty()
            && animator.wiggly_selectors.is_empty()
        {
            let Some(selector_id) = allocate_item_id(next_id) else {
                warnings.push(format!(
                    "{prefix} omitted: no identifier remains for the unsupported-selector safety gate"
                ));
                break;
            };
            // An empty FX selector stack selects every glyph. Retain editable
            // values without globally applying operations whose native selection
            // could not be represented. Unsupported selector keys have no target.
            animator.selectors.push(RangeSelector {
                id: selector_id,
                amount: 0.0,
                ..RangeSelector::default()
            });
            warnings.push(format!(
                "{prefix} has no imported native selector; an editable zero-amount Range Selector bypasses its operations to preserve source text. Native selection/reveal animation is omitted, not reproduced"
            ));
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

// A collapsed Ramp Up selects whole units past its boundary, unlike Square's
// fractional overlap. Keep both boundaries linked by copying the native Start keys.
fn hard_range_reveal(properties: &[(String, Result<NumericProperty, PropertyError>)]) -> bool {
    let static_control = |name: &str, expected: f64, absent: bool| {
        properties
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map_or(absent, |(_, value)| {
                value.as_ref().is_ok_and(|value| {
                    !value.animated
                        && !value.expression_enabled
                        && value.keyframes.is_empty()
                        && value.values.as_slice() == [expected]
                })
            })
    };
    static_control("ADBE Text Selector Smoothness", 0.0, false)
        && static_control("ADBE Text Range Units", 1.0, true)
        && static_control("ADBE Text Range Shape", 1.0, true)
        && static_control("ADBE Text Percent End", 100.0, true)
        && static_control("ADBE Text Percent Offset", 0.0, true)
        && static_control("ADBE Text Levels Max Ease", 0.0, true)
        && static_control("ADBE Text Levels Min Ease", 0.0, true)
        && properties
            .iter()
            .filter(|(name, _)| name == "ADBE Text Percent Start")
            .all(|(_, value)| value.as_ref().is_ok_and(|value| !value.expression_enabled))
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
    let hard_reveal = hard_range_reveal(properties);
    let mut supported = vec![
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
    ];
    if hard_reveal {
        supported.push("ADBE Text Selector Smoothness");
        warnings.push(format!("{prefix} Range Selector zero Smoothness reveal approximated by a collapsed Ramp Up with shared Start/End keys; edit both boundaries together; other Smoothness profiles remain unsupported"));
    }
    warn_unsupported_properties(
        properties,
        &supported,
        &format!("{prefix} Range Selector"),
        warnings,
    );
    let mut value = serde_json::json!({
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
    if hard_reveal {
        value["shape"] = serde_json::json!("rampUp");
        value["end"] = value["start"].clone();
    }
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

/// Editable text animators are 2D: report a non-neutral static or keyed Z
/// component that the XY import necessarily omits.
fn report_dropped_z(
    properties: &[(String, Result<NumericProperty, PropertyError>)],
    name: &str,
    neutral: f64,
    prefix: &str,
    warnings: &mut Vec<String>,
) {
    let Some((_, Ok(numeric))) = properties.iter().find(|(candidate, _)| candidate == name) else {
        return;
    };
    let dropped = std::iter::once(&numeric.values)
        .chain(numeric.keyframes.iter().map(|key| &key.values))
        .filter_map(|values| values.get(2))
        .any(|z| *z != neutral);
    if dropped {
        warnings.push(format!(
            "{prefix} {name} Z component is not representable in 2D editable text animators; Z omitted and XY retained"
        ));
    }
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
            warnings.push(format!("{prefix} {name} has an enabled AE expression; no FX script is generated and the stored/keyed value is the static fallback (converter expression evaluation, when admitted, is diagnosed separately)"));
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
        Ok((starts, texts)) => Some(PercentHolds {
            style: None,
            starts,
            texts,
        }),
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

/// Native Source Text dependency snapshot. It is consumed only by the converter;
/// expression results still become ordinary editable text/keyframe content.
pub(crate) struct SourceTextSnapshot<'a> {
    pub documents: Vec<TextDocument>,
    pub starts: Vec<f64>,
    pub expression: Option<&'a [Chunk]>,
    pub bounds: Option<[f64; 4]>,
}

pub(crate) fn source_text_snapshot(
    layer: &Layer,
) -> Result<Option<SourceTextSnapshot<'_>>, TextError> {
    let Some(source) = read_source(layer)? else {
        return Ok(None);
    };
    let roots = root_runs(&layer.content)?;
    let text = roots
        .into_iter()
        .find(|(name, _)| *name == "ADBE Text Properties")
        .ok_or(TextError::Layout("missing Text properties"))?;
    let text = unique_list(text.1, *b"tdgp")?;
    let document = runs(text)?
        .into_iter()
        .find(|(name, _)| *name == "ADBE Text Document")
        .ok_or(TextError::Layout("missing Source Text"))?;
    let wrapper = unique_list(document.1, *b"btds")?;
    let expression = enabled_expression_descriptor(wrapper)?;
    if source.documents.is_empty()
        || (source.documents.len() > 1
            && (source.document_starts.len() != source.documents.len()
                || source.document_starts.iter().any(|t| !t.is_finite())
                || source
                    .document_starts
                    .windows(2)
                    .any(|pair| pair[0] >= pair[1])))
    {
        return Err(TextError::Layout(
            "Source Text dependency has invalid document timing",
        ));
    }
    let documents = source
        .documents
        .iter()
        .map(|document| convert_document(document, &source.fonts, source.frame.as_ref()).0)
        .collect();
    let bounds = (source.document_is_static
        && source.documents.len() == 1
        && source.animators.is_empty()
        && source.path_options.is_empty())
    .then(|| cached_text_bounds(&source.documents[0]))
    .flatten();
    Ok(Some(SourceTextSnapshot {
        documents,
        starts: source.document_starts,
        expression,
        bounds,
    }))
}

/// Saved glyph-cache union, not a fresh font layout or live expression bound.
fn cached_text_bounds(document: &Value) -> Option<[f64; 4]> {
    let layout = document.get("1")?.get("2")?;
    let mut pending = vec![layout];
    let mut bounds: Option<[f64; 4]> = None;
    while let Some(value) = pending.pop() {
        if value.get("99").and_then(Value::as_str) == Some("G") {
            let raw = value.get("8")?.as_array()?;
            let [x0, y0, x1, y1] = raw else {
                return None;
            };
            let glyph = [x0.as_f64()?, y0.as_f64()?, x1.as_f64()?, y1.as_f64()?];
            if !glyph.into_iter().all(f64::is_finite) || glyph[2] < glyph[0] || glyph[3] < glyph[1]
            {
                return None;
            }
            bounds = Some(match bounds {
                None => glyph,
                Some(b) => [
                    b[0].min(glyph[0]),
                    b[1].min(glyph[1]),
                    b[2].max(glyph[2]),
                    b[3].max(glyph[3]),
                ],
            });
        }
        match value {
            Value::Array(values) => pending.extend(values.iter().rev()),
            Value::Dict(values) => pending.extend(values.values().rev()),
            _ => {}
        }
    }
    let [left, top, right, bottom] = bounds?;
    let result = [left, top, right - left, bottom - top];
    result.into_iter().all(f64::is_finite).then_some(result)
}

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
    let justification = if justification == Justification::Right
        && cached_left_aligned(document, raw_text)
    {
        warnings.push("Source Text right paragraph code conflicts with a complete cached left-aligned glyph layout; editable Left alignment approximates the saved layout, without reproducing native paragraph-engine controls or verifying cache freshness".into());
        Justification::Left
    } else {
        justification
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
            // A finite but huge source size must not overflow the fallback.
            (font_size.value() * 1.2).min(f64::MAX),
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

// A saved layout can expose physical alignment that the paragraph code alone cannot express.
fn cached_left_aligned(document: &Value, raw_text: &str) -> bool {
    fn check(document: &Value, raw_text: &str) -> Option<bool> {
        if raw_text.is_empty() || !raw_text.is_ascii() {
            return None;
        }
        let paragraphs = document.get("0")?.get("5")?.get("0")?.as_array()?;
        if paragraphs.is_empty()
            || !paragraphs.iter().all(|run| {
                let Some(style) = run
                    .get("0")
                    .and_then(|v| v.get("0"))
                    .and_then(|v| v.get("5"))
                else {
                    return false;
                };
                style.get("0").and_then(Value::as_i64) == Some(1)
                    && ["1", "2", "3"]
                        .iter()
                        .all(|key| style.get(key).is_none_or(|v| v.as_f64() == Some(0.0)))
            })
        {
            return None;
        }
        let mut pending = vec![document.get("1")?.get("2")?];
        let mut units = 0_usize;
        let mut glyphs = 0_usize;
        while let Some(node) = pending.pop() {
            if let Some(nodes) = node.as_array() {
                pending.extend(nodes);
                continue;
            }
            let kind = node.get("99")?.as_str()?;
            if !matches!(kind, "PC" | "F" | "R" | "L" | "S" | "G") {
                return None;
            }
            if let Some(origin) = node.get("0") {
                let point = origin.get("0")?.as_array()?;
                if point.len() != 2 || point[0].as_f64()? != 0.0 || !point[1].as_f64()?.is_finite()
                {
                    return None;
                }
            }
            match kind {
                "F" if node.get("10")?.as_i64()? != 0 => return None,
                "S" => {
                    let count = usize::try_from(node.get("15")?.get("0")?.as_i64()?).ok()?;
                    units = units.checked_add(count)?;
                }
                "G" => {
                    let bounds = node.get("8")?.as_array()?;
                    if bounds.len() != 4
                        || bounds[0].as_f64()? != 0.0
                        || !bounds
                            .iter()
                            .all(|v| v.as_f64().is_some_and(f64::is_finite))
                        || bounds[2].as_f64()? <= 0.0
                    {
                        return None;
                    }
                    glyphs += 1;
                }
                _ => {}
            }
            if kind != "G" {
                pending.extend(node.get("6")?.as_array()?);
            }
        }
        Some(glyphs > 0 && units == raw_text.encode_utf16().count())
    }
    check(document, raw_text).unwrap_or(false)
}

fn first_layout_line_baseline(document: &Value) -> Option<f64> {
    let layout = document.get("1")?.get("2")?;
    first_named_line_baseline(layout)
}

fn first_named_line_baseline(value: &Value) -> Option<f64> {
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        if value.get("99").and_then(Value::as_str) == Some("L") {
            if let Some(baseline) = value
                .get("10")
                .and_then(Value::as_f64)
                .filter(|baseline| baseline.is_finite())
            {
                return Some(baseline);
            }
            // A named line is a leaf for this search even if its baseline is bad.
            continue;
        }
        match value {
            Value::Array(values) => pending.extend(values.iter().rev()),
            Value::Dict(values) => pending.extend(values.values().rev()),
            _ => {}
        }
    }
    None
}

fn run_count(document: &Value, key: &str) -> usize {
    at(document, &[Key::Name("0"), Key::Name(key), Key::Name("0")])
        .and_then(Value::as_array)
        .map_or(0, <[Value]>::len)
}

/// A dash-less name such as `ArialMT` has no style to split off: it is kept
/// whole with an empty style, the FX convention for an exact PostScript
/// identity, instead of a guessed `Regular` that export cannot tell apart.
pub(crate) fn split_font_identity(postscript: &str) -> (&str, &str) {
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
pub(crate) enum TextError {
    #[error("{0}")]
    Property(#[from] PropertyError),
    #[error("{0}")]
    Cos(#[from] cos::Error),
    #[error("unsupported or malformed text property: {0}")]
    Layout(&'static str),
}

/// Group composition-clock Source Text samples into held texts with
/// source-local start times, as keyed Source Text documents are held.
fn evaluated_text_holds(
    layer: &Layer,
    evaluated: &crate::expression_samples::EvaluatedText,
) -> Result<PercentHolds, String> {
    let (Some(start), Some(stretch)) = (layer.record.start_time(), layer.record.stretch()) else {
        return Err("invalid layer clock".into());
    };
    if !start.is_finite() || !stretch.is_finite() || stretch <= 0.0 {
        return Err(format!(
            "unsupported layer clock start={start} stretch={stretch}"
        ));
    }
    let mut starts = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    for (time, text) in evaluated.sample_times_seconds.iter().zip(&evaluated.texts) {
        let local = (time - start) / stretch;
        if local < 0.0 {
            continue;
        }
        if texts.last() != Some(text) {
            starts.push(local);
            texts.push(text.clone());
        }
    }
    if texts.is_empty() {
        return Err("no Source Text samples inside the layer's local clock".into());
    }
    Ok(PercentHolds {
        style: None,
        starts,
        texts,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn cached_left_layout_overrides_conflicting_right_paragraph_alignment() {
        let source = |offset: f64, units: usize| {
            format!(
                "<< /0 << /0 (AB\\r) /5 << /0 [ << /0 << /0 << /5 << /0 1 >> >> >> >> ] >> >> /1 << /2 [ << /99 /PC /6 [ << /99 /F /10 0 /6 [ << /99 /L /6 [ << /99 /S /0 << /0 [ {offset} 0 ] >> /15 << /0 {units} >> /6 [ << /99 /G /8 [ 0 -10 20 0 ] >> ] >> ] >> ] >> ] >> ] >> >>"
            )
        };
        let native = cos::parse(source(0.0, 3).as_bytes()).unwrap();
        let (text, warnings) = convert_document(&native, &[], None);
        assert_eq!(text.text, "AB");
        assert_eq!(text.justification, Justification::Left);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("cached left-aligned"))
        );
        for rejected in [
            source(-20.0, 3),
            source(0.0, 2),
            source(0.0, 3).replace("/8 [ 0 -10 20 0 ]", "/8 [ 1 -10 20 0 ]"),
            source(0.0, 3).replace("/5 << /0 1 >>", "/5 << /0 1 /1 5 >>"),
            source(0.0, 3).replace("/99 /F /10 0", "/99 /F /10 1"),
            source(0.0, 3).replace("/0 (AB", "/0 (éB"),
            source(0.0, 3).replace("/99 /G", "/99 /Unknown"),
            source(0.0, 3).replace("/1 << /2", "/1 << /missing"),
        ] {
            let native = cos::parse(rejected.as_bytes()).unwrap();
            let (text, warnings) = convert_document(&native, &[], None);
            assert_eq!(text.justification, Justification::Right, "{rejected}");
            assert!(
                !warnings
                    .iter()
                    .any(|warning| warning.contains("cached left-aligned"))
            );
        }
    }

    #[test]
    fn huge_finite_font_size_does_not_overflow_the_manual_leading_fallback() {
        // 1.7e308 is a finite, positive COS decimal; times 1.2 it overflows.
        let size = format!("17{}.0", "0".repeat(307));
        let source = format!(
            "<< /0 << /0 (A) /6 << /0 [ << /0 << /0 << /6 << /1 {size} /4 false >> >> >> >> ] >> >> >>"
        );
        let document = cos::parse(source.as_bytes()).unwrap();
        let (document, _) = super::convert_document(&document, &[], None);
        assert!(document.font_size.value().is_finite());
    }

    #[test]
    fn review_layout_baseline_search_keeps_native_preorder_and_line_leaves() {
        let layout = cos::parse(
            br#"[ << /99 /L /10 (bad) /child << /99 /L /10 999 >> >>
                << /a << /99 /L /10 12 >> /b << /99 /L /10 24 >> >> ]"#,
        )
        .unwrap();
        assert_eq!(first_named_line_baseline(&layout), Some(12.0));
    }

    #[test]
    fn review_source_text_deep_containers_survive_production_walks() {
        std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(|| {
                let project = read_project(include_bytes!(
                    "../../tests/fixtures/text/text_ranges.aep"
                ))
                .unwrap();
                let ItemKind::Composition(composition) = &project.item(1).unwrap().kind else {
                    panic!("native text composition")
                };
                let mut layer = composition
                    .layers
                    .iter()
                    .find(|layer| layer.record.id() == 14)
                    .unwrap()
                    .clone();
                // A native layer shell with supplemental COS, not Adobe-authored
                // deep nesting or independent native fidelity evidence.
                let depth = 10_000;
                let nested = |leaf: &str| {
                    format!("{}{}{}", "[ << /child ".repeat(depth), leaf, " >> ]".repeat(depth))
                };
                let style = nested("42");
                let layout = nested("[ << /99 /L /10 27 >> << /99 /G /8 [ 3 4 23 14 ] >> ]");
                let frame = nested("true");
                let payload = format!(
                    "<< /1 << /1 [ << /0 << /0 (A\\r\\r) /5 << /0 [ << /0 {style} /1 2 >> << /0 {style} /1 1 >> ] >> /6 << /0 [ << >> ] >> >> /1 << /2 {layout} >> >> ] >> /0 << /8 << /0 [ << /0 {frame} >> ] >> >> >>"
                );
                let mut flags = vec![0; 124];
                flags[..2].copy_from_slice(&[0xdb, 0x99]);
                let mut name = b"ADBE Text Document".to_vec();
                name.resize(40, 0);
                let document = vec![
                    Chunk::data(*b"tdmn", name).unwrap(),
                    Chunk::list(
                        *b"btds",
                        vec![
                            Chunk::opaque_list(*b"btdk", payload.into_bytes()),
                            Chunk::list(
                                *b"tdbs",
                                vec![Chunk::data(*b"tdb4", flags).unwrap()],
                            ),
                        ],
                    ),
                ];
                layer.content = vec![Chunk::list(
                    *b"tdgp",
                    text_run("ADBE Text Properties", document),
                )];
                let source = read_source(&layer).unwrap().unwrap();
                let cloned = source.documents[0].clone();
                assert!(source.documents[0] == cloned);
                let cloned_frame = source.frame.as_ref().unwrap().clone();
                assert_eq!(cloned_frame.as_array().unwrap().len(), 1);
                let layout = at(&cloned, &[Key::Name("1"), Key::Name("2")]).unwrap();
                assert_eq!(first_named_line_baseline(layout), Some(27.0));
                assert_eq!(cached_caption_width(&layer), Ok(20.0));
                let (document, _) = convert_document(&cloned, &source.fonts, source.frame.as_ref());
                assert_eq!(document.text, "A\n");
            })
            .unwrap()
            .join()
            .unwrap();
    }

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
        for (index, item) in items.as_chunks_mut::<8>().0.iter_mut().enumerate() {
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
            read_text_properties(text_group, &ExpressionLinks::new(&[], text_group), &[]);
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
    fn zero_smoothness_reveal_uses_a_collapsed_ramp_with_shared_boundary_keys() {
        let properties = vec![
            (
                "ADBE Text Selector Smoothness".into(),
                Ok(numeric(&[0.0], false)),
            ),
            property("ADBE Text Percent Start", &[25.0]),
        ];
        let mut warnings = Vec::new();
        let selector =
            convert_range_selector(FxItemId::new(101), &properties, "Reveal", &mut warnings);
        let wire = serde_json::to_value(&selector).unwrap();
        assert_eq!(wire["shape"], "rampUp");
        assert_eq!(wire["start"], wire["end"]);
        assert!(warnings.iter().any(|w| w.contains("zero Smoothness")));
        let source = AnimatorSource {
            name: "Reveal".into(),
            properties: vec![property("ADBE Text Opacity", &[0.0])],
            selectors: vec![SelectorSource::Range { properties }],
            evaluated: Vec::new(),
        };
        let animator = TextAnimator {
            id: FxItemId::new(100),
            selectors: vec![selector],
            ..TextAnimator::default()
        };
        let mut entries = Vec::new();
        let mut budget = AnimationBudget::default();
        append_animator_animation(
            &source,
            &animator,
            "Reveal",
            NumericAnimationClock::source_local_rebased(0.25),
            1.0,
            TextAnimationOutput::new(&mut entries, &mut warnings, &mut budget),
        );
        AnimationGraph::from_entries(entries.clone()).unwrap();
        let keys = |name| {
            entries
                .iter()
                .find(|entry| entry.target == PropertyTarget::fx_item(FxItemId::new(101), name))
                .unwrap()
                .animator
                .keyframe_track()
                .unwrap()
                .keyframes()
        };
        for (start, end) in keys("start").iter().zip(keys("end")) {
            let mut start = serde_json::to_value(start).unwrap();
            let mut end = serde_json::to_value(end).unwrap();
            start.as_object_mut().unwrap().remove("id");
            end.as_object_mut().unwrap().remove("id");
            assert_eq!(start, end);
        }
        assert_eq!(keys("end")[0].layer_time().as_millis(), 1000);
        assert_eq!(keys("end")[1].layer_time().as_millis(), 2000);
        assert_eq!(keys("end")[0].value(), &PropertyValue::Float(0.25));
        let mut limited = Vec::new();
        append_animator_animation(
            &source,
            &animator,
            "Reveal",
            NumericAnimationClock::source_local(),
            1.0,
            TextAnimationOutput::new(
                &mut limited,
                &mut warnings,
                &mut AnimationBudget::with_limit(1),
            ),
        );
        assert!(limited.is_empty());
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("coupled animation target set omitted"))
        );
    }

    #[test]
    fn zero_smoothness_reveal_declines_other_range_profiles() {
        let zero = (
            "ADBE Text Selector Smoothness".into(),
            Ok(numeric(&[0.0], false)),
        );
        for (name, value) in [
            ("ADBE Text Selector Smoothness", 50.0),
            ("ADBE Text Range Units", 2.0),
            ("ADBE Text Range Shape", 2.0),
            ("ADBE Text Percent End", 90.0),
            ("ADBE Text Percent Offset", -10.0),
            ("ADBE Text Levels Max Ease", 20.0),
            ("ADBE Text Levels Min Ease", -20.0),
        ] {
            let mut properties = vec![zero.clone()];
            properties.retain(|(candidate, _)| candidate != name);
            properties.push((name.into(), Ok(numeric(&[value], false))));
            assert!(!hard_range_reveal(&properties), "{name}");
        }
        for name in [
            "ADBE Text Selector Smoothness",
            "ADBE Text Percent End",
            "ADBE Text Percent Offset",
        ] {
            for expression in [false, true] {
                let mut control = numeric(&[0.0], !expression);
                control.expression_enabled = expression;
                let mut properties = vec![zero.clone()];
                properties.retain(|(candidate, _)| candidate != name);
                properties.push((name.into(), Ok(control)));
                assert!(!hard_range_reveal(&properties), "{name}");
            }
        }
        assert!(!hard_range_reveal(&[]));
        let mut warnings = Vec::new();
        let selector = convert_range_selector(FxItemId::new(1), &[zero], "Static", &mut warnings);
        let wire = serde_json::to_value(selector).unwrap();
        assert_eq!(wire["shape"], "rampUp");
        assert_eq!(wire["start"], 0.0);
        assert_eq!(wire["end"], 0.0);
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
            // The keyed helper doubles all components; keep coverage <= 1.
            property("ADBE Text Stroke Color", &[0.5, 0.6, 0.7, 0.4]),
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
            evaluated: Vec::new(),
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
            1.0,
            TextAnimationOutput::new(&mut entries, &mut warnings, &mut budget),
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
                1.0,
                TextAnimationOutput::new(&mut rebased, &mut rebased_warnings, &mut budget),
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
            evaluated: Vec::new(),
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
                evaluated: Vec::new(),
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
            evaluated: Vec::new(),
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
            1.0,
            TextAnimationOutput::new(&mut entries, &mut warnings, &mut budget),
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
            read_text_properties(&chunks, &ExpressionLinks::new(&[], &chunks), &[]);
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
    fn unsupported_only_text_selector_does_not_select_every_glyph() {
        fn numeric_leaf(name: &str, values: &[f64]) -> Vec<Chunk> {
            let mut match_name = name.as_bytes().to_vec();
            match_name.resize(40, 0);
            vec![
                Chunk::data(*b"tdmn", match_name).unwrap(),
                Chunk::list(*b"tdbs", numeric_storage(values, false)),
            ]
        }
        let text_group = text_run(
            "ADBE Text Animators",
            text_run(
                "ADBE Text Animator",
                [
                    text_run(
                        "ADBE Text Animator Properties",
                        [
                            numeric_leaf("ADBE Text Opacity", &[0.0]),
                            numeric_leaf("ADBE Text Scale 3D", &[0.0, 0.0, 0.0]),
                        ]
                        .concat(),
                    ),
                    text_run(
                        "ADBE Text Selectors",
                        text_run("ADBE Text Expressible Selector", Vec::new()),
                    ),
                ]
                .concat(),
            ),
        );
        let (animators, _, _, warnings) = convert_test_text(&text_group);
        assert_eq!(animators.len(), 1);
        assert_eq!(animators[0].opacity, Some(0.0));
        assert_eq!(animators[0].scale, Some([0.0, 0.0]));
        assert_eq!(animators[0].selectors.len(), 1);
        assert_eq!(animators[0].selectors[0].amount, 0.0);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("zero-amount"))
        );

        let (mut sources, _, _, _) =
            read_text_properties(&text_group, &ExpressionLinks::new(&[], &text_group), &[]);
        sources[0].properties[0] = property("ADBE Text Opacity", &[0.0]);
        let mut entries = Vec::new();
        append_animator_animation(
            &sources[0],
            &animators[0],
            "unsupported selector regression",
            NumericAnimationClock::source_local(),
            1.0,
            TextAnimationOutput::new(
                &mut entries,
                &mut Vec::new(),
                &mut AnimationBudget::default(),
            ),
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].target,
            PropertyTarget::fx_item(animators[0].id, "opacity")
        );
        assert!(entries.iter().all(|entry| {
            entry.target != PropertyTarget::fx_item(animators[0].selectors[0].id, "amount")
        }));
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
    fn animator_tracking_pixels_use_each_held_documents_em_size() {
        let document = |size| {
            cos::parse(format!(
                "<< /0 << /0 (AB) /6 << /0 [ << /0 << /0 << /6 << /0 0 /1 {size} /8 -10 >> >> >> >> ] >> >> >>"
            ).as_bytes()).unwrap()
        };
        let mut tracking = numeric(&[10.0], true);
        tracking.keyframes[0].out_interpolation = 2;
        tracking.keyframes[0].out_speed = vec![4.0];
        tracking.keyframes[0].out_influence = vec![30.0];
        tracking.keyframes[1].in_interpolation = 2;
        tracking.keyframes[1].in_speed = vec![4.0];
        tracking.keyframes[1].in_influence = vec![30.0];
        let source = SourceText {
            document_is_static: false,
            fonts: Vec::new(),
            documents: vec![document(50), document(100)],
            document_starts: vec![0.0, 1.0],
            percent: None,
            frame: None,
            animators: vec![
                AnimatorSource {
                    name: "Tracking".into(),
                    properties: vec![("ADBE Text Tracking Amount".into(), Ok(tracking))],
                    selectors: Vec::new(),
                    evaluated: Vec::new(),
                },
                AnimatorSource {
                    name: "Static tracking".into(),
                    properties: vec![(
                        "ADBE Text Tracking Amount".into(),
                        Ok(numeric(&[-5.0], false)),
                    )],
                    selectors: Vec::new(),
                    evaluated: Vec::new(),
                },
            ],
            more_options: Vec::new(),
            path_options: Vec::new(),
            warnings: Vec::new(),
        };
        let range = TimeRangeProperty::new(Time::ZERO, Duration::from_secs(3.0));
        let occurrence: GroupLayer = serde_json::from_value(serde_json::json!({
            "id": 9000, "name": "Text", "layers": [],
            "playback": fx_schema::LayerPlayback::linear(range, range, range, 0).unwrap(),
            "transform": identity_transform()
        }))
        .unwrap();
        let mut warnings = Vec::new();
        let layers = held_segments(
            &source,
            None,
            vec![(0, 0.0, 1.0), (1, 1.0, 3.0)],
            &occurrence,
            &[],
            &mut 1,
            &mut warnings,
        );
        let (entries, animation_warnings) =
            segment_animation_entries(&source, &layers, &mut AnimationBudget::default());
        assert!(animation_warnings.is_empty(), "{animation_warnings:?}");
        for (index, layer) in layers.iter().enumerate() {
            let FxLayer::Text(text) = layer else {
                panic!("editable Text expected")
            };
            let size = [50.0, 100.0][index];
            assert_eq!(text.source_text.font_size.value(), size);
            assert_eq!(text.source_text.text, "AB");
            assert_eq!(text.source_text.tracking, -10.0);
            assert_eq!(text.animators[1].tracking, Some(-5.0 * 1000.0 / size));
            let animator = &text.animators[0];
            assert_eq!(animator.tracking, Some(10.0 * 1000.0 / size));
            let target = PropertyTarget::fx_item(animator.id, "tracking");
            let keys = entries
                .iter()
                .find(|entry| entry.target == target)
                .unwrap()
                .animator
                .keyframe_track()
                .unwrap()
                .keyframes();
            assert_eq!(keys[0].layer_time().as_millis(), [1250, 250][index]);
            assert_eq!(keys[1].layer_time().as_millis(), [2250, 1250][index]);
            assert_eq!(keys[0].value(), &PropertyValue::Float(10.0 * 1000.0 / size));
            assert_eq!(keys[1].value(), &PropertyValue::Float(20.0 * 1000.0 / size));
            let fx_schema::animator::PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } =
                keys[1].easing()
            else {
                panic!("native Bezier tracking expected");
            };
            for (actual, expected) in [(x1, 0.3), (y1, 0.12), (x2, 0.7), (y2, 0.88)] {
                assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
            }
        }
    }

    #[test]
    fn animator_tracking_expression_samples_use_the_same_unit_conversion() {
        let name = "ADBE Text Tracking Amount";
        let source = AnimatorSource {
            name: "Sampled tracking".into(),
            properties: Vec::new(),
            selectors: Vec::new(),
            evaluated: vec![(
                name.into(),
                EvaluatedProperty {
                    composition_id: 1,
                    layer_id: 2,
                    property: PropertyIdentity::TextAnimator {
                        animator: 1,
                        match_name: name.into(),
                    },
                    start_ms: 1250,
                    sample_times_seconds: vec![1.25, 2.25],
                    values: vec![vec![-10.0], vec![20.0]],
                    frame_sampled: true,
                },
                [0.0, 1.0],
            )],
        };
        let animator = TextAnimator {
            id: FxItemId::new(3),
            ..TextAnimator::default()
        };
        let mut entries = Vec::new();
        let mut warnings = Vec::new();
        append_animator_animation(
            &source,
            &animator,
            "Sampled tracking",
            NumericAnimationClock::source_local_rebased(1.0),
            1000.0 / 50.0,
            TextAnimationOutput::new(&mut entries, &mut warnings, &mut AnimationBudget::default()),
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].target,
            PropertyTarget::fx_item(animator.id, "tracking")
        );
        let keys = entries[0].animator.keyframe_track().unwrap().keyframes();
        assert_eq!(keys[0].layer_time().as_millis(), 250);
        assert_eq!(keys[1].layer_time().as_millis(), 1250);
        assert_eq!(keys[0].value(), &PropertyValue::Float(-200.0));
        assert_eq!(keys[1].value(), &PropertyValue::Float(400.0));
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("once per layer"))
        );
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
            render_domain: None,
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
            style: None,
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
