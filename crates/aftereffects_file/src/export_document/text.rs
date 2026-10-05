//! FX Text lowering for the fresh native writer.
//!
//! Registration and timeline dispatch are owned by the parent exporter.  This
//! module returns a typed `TextSpec` plus explicit approximation diagnostics.

use std::collections::BTreeSet;

use fx_schema::animator::{AnimatorData, PropertyKeyframeTrack};
use fx_schema::{
    FxItemId, LayerId, Position, PropType, PropertyKeyframeEasing, PropertyTarget, PropertyValue,
    TextDocument, TextLayer, Transform,
};

use crate::writer::{
    KeyframeEasing, NumericKeyframe, NumericTrack, SolidTransform,
    text::{
        NativeTextProfile, PropertyTracks, RangeSelectorSpec, TextAnchorOptionsSpec,
        TextAnimatorSpec, TextDocumentKey, TextDocumentSpec, TextDocumentTimeline,
        TextPathOptionsSpec, TextSpec, WigglySelectorSpec,
    },
};

use super::effective_constant;

/// A native Text leaf and all semantic approximations made while lowering it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct LoweredText {
    pub spec: TextSpec,
    pub diagnostics: Vec<String>,
}

/// Lowers one FX Text layer. Group transform selection and active-range wrapping
/// remain in the parent dispatcher so Text follows the same hierarchy policy as
/// Solid/vector leaves.
#[cfg(test)]
pub(super) fn lower(
    layer: &TextLayer,
    transform: &Transform,
    transform_id: LayerId,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    native_path_index: Option<u16>,
) -> Result<LoweredText, &'static str> {
    lower_with_native_3d(
        layer,
        transform,
        transform_id,
        dynamics,
        native_path_index,
        false,
        None,
    )
}

pub(super) fn lower_with_native_3d(
    layer: &TextLayer,
    transform: &Transform,
    transform_id: LayerId,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    native_path_index: Option<u16>,
    native_3d: bool,
    fonts: Option<&super::fonts::ArchiveFonts>,
) -> Result<LoweredText, &'static str> {
    if layer.name.is_empty() || layer.name.len() > 255 || layer.name.contains('\0') {
        return Err("Native Text name must be 1..=255 UTF-8 bytes without NUL");
    }
    let Position::TwoD(position) = transform.position else {
        return Err("3D Text Transform export is not implemented");
    };
    if transform.skew != 0.0
        || transform.skew_axis != 0.0
        || transform.rotation_x != 0.0
        || transform.rotation_y != 0.0
        || transform.orientation != [0.0; 3]
    {
        return Err("Skew and 3D Text Transform export are not implemented");
    }

    let mut diagnostics = Vec::new();
    let text_item_ids = layer
        .animators
        .iter()
        .flat_map(|animator| {
            std::iter::once(animator.id)
                .chain(animator.selectors.iter().map(|selector| selector.id))
                .chain(animator.wiggly_selectors.iter().map(|selector| selector.id))
        })
        .chain(layer.anchor_options.iter().map(|options| options.id))
        .chain(layer.path_options.iter().map(|options| options.id))
        .collect::<BTreeSet<_>>();
    for entry in dynamics {
        if matches!(
            entry.animator.data(),
            AnimatorData::Keyframes { enabled: false, .. }
        ) && entry
            .target
            .fx_item_id()
            .is_some_and(|id| text_item_ids.contains(&id))
        {
            diagnostics.push(
                if effective_constant(&entry.animator).is_some() {
                    "Disabled Text sub-item key authoring was not retained; its runtime-visible disabledValue was normalized to a native constant"
                } else {
                    "Disabled Text sub-item keys have no runtime-visible disabledValue; mapped native export fails closed instead of using the typed static base"
                }
                .into(),
            );
        }
    }
    let document = lower_document(&layer.source_text, &mut diagnostics, fonts.is_none())?;
    let documents = lower_document_timeline(document, layer.id, dynamics, &mut diagnostics, fonts)?;
    // Source Text document controls are already consumed by the text timeline.
    // They are not 2D Transform controls and must not be rejected by its gate.
    let transform_dynamics = dynamics
        .iter()
        .filter(|entry| {
            !matches!(&entry.target, PropertyTarget::LayerProperty(target)
                if target.layer_id() == layer.id && is_document_property(target.property_type()))
        })
        .cloned()
        .collect::<Vec<_>>();
    let transform_index = super::AnimationIndex::new(&transform_dynamics);
    let transform_animations = super::transform_animations_partitioned(
        &transform_index,
        transform_id,
        transform,
        layer.id,
        native_3d,
        false,
    )?;
    let mut animators = Vec::with_capacity(layer.animators.len());
    for animator in &layer.animators {
        ensure_materialized_targets(
            dynamics,
            animator.id,
            &[
                ("anchorPoint", animator.anchor_point.is_some()),
                ("position", animator.position.is_some()),
                ("scale", animator.scale.is_some()),
                ("rotation", animator.rotation.is_some()),
                ("skew", animator.skew.is_some()),
                ("skewAxis", animator.skew_axis.is_some()),
                ("tracking", animator.tracking.is_some()),
                ("strokeWidth", animator.stroke_width.is_some()),
                ("blur", animator.blur.is_some()),
                ("opacity", animator.opacity.is_some()),
                ("fillColor", animator.fill_color.is_some()),
                ("strokeColor", animator.stroke_color.is_some()),
                ("lineSpacing", animator.line_spacing.is_some()),
                ("lineAnchor", animator.line_anchor.is_some()),
                ("characterOffset", animator.character_offset.is_some()),
                ("characterValue", animator.character_value.is_some()),
            ],
        )?;
        let mut animations = PropertyTracks::default();
        insert_track(
            &mut animations,
            "anchorPoint",
            fx_track(
                dynamics,
                animator.id,
                "anchorPoint",
                TrackEncoding::Spatial3,
            )?,
        );
        insert_track(
            &mut animations,
            "position",
            fx_track(dynamics, animator.id, "position", TrackEncoding::Spatial3)?,
        );
        insert_track(
            &mut animations,
            "scale",
            fx_track(dynamics, animator.id, "scale", TrackEncoding::Scale3)?,
        );
        for name in [
            "rotation",
            "skew",
            "skewAxis",
            "tracking",
            "strokeWidth",
            "opacity",
            "lineAnchor",
            "characterOffset",
            "characterValue",
        ] {
            insert_track(
                &mut animations,
                name,
                fx_track(dynamics, animator.id, name, TrackEncoding::Scalar(1.0))?,
            );
        }
        insert_track(
            &mut animations,
            "blur",
            fx_track(dynamics, animator.id, "blur", TrackEncoding::Vector2)?,
        );
        insert_track(
            &mut animations,
            "fillColor",
            fx_track(dynamics, animator.id, "fillColor", TrackEncoding::Color)?,
        );
        insert_track(
            &mut animations,
            "strokeColor",
            fx_track(dynamics, animator.id, "strokeColor", TrackEncoding::Color)?,
        );
        insert_track(
            &mut animations,
            "lineSpacing",
            fx_track(
                dynamics,
                animator.id,
                "lineSpacing",
                TrackEncoding::LineSpacing,
            )?,
        );

        let selectors = animator
            .selectors
            .iter()
            .map(|selector| lower_range_selector(selector, dynamics))
            .collect::<Result<Vec<_>, _>>()?;
        let wiggly_selectors = animator
            .wiggly_selectors
            .iter()
            .map(|selector| lower_wiggly_selector(selector, dynamics))
            .collect::<Result<Vec<_>, _>>()?;
        animators.push(TextAnimatorSpec {
            value: animator.clone(),
            animations,
            selectors,
            wiggly_selectors,
        });
    }

    let anchor_options = layer
        .anchor_options
        .as_ref()
        .map(|options| {
            ensure_materialized_targets(dynamics, options.id, &[("groupingAlignment", true)])?;
            let mut animations = PropertyTracks::default();
            insert_track(
                &mut animations,
                "groupingAlignment",
                fx_track(
                    dynamics,
                    options.id,
                    "groupingAlignment",
                    TrackEncoding::Vector2,
                )?,
            );
            Ok::<_, &'static str>(TextAnchorOptionsSpec {
                grouping: options.anchor_point_grouping,
                alignment: options.grouping_alignment,
                animations,
            })
        })
        .transpose()?;

    let path_options = match (&layer.path_options, native_path_index) {
        (Some(path), Some(path_index)) if path_index != 0 => {
            let mut animations = PropertyTracks::default();
            insert_track(
                &mut animations,
                "firstMargin",
                fx_track(dynamics, path.id, "firstMargin", TrackEncoding::Scalar(1.0))?,
            );
            insert_track(
                &mut animations,
                "lastMargin",
                fx_track(dynamics, path.id, "lastMargin", TrackEncoding::Scalar(1.0))?,
            );
            for entry in dynamics
                .iter()
                .filter(|entry| entry.target.fx_item_id() == Some(path.id))
            {
                let PropertyTarget::FxItemProperty(target) = &entry.target else {
                    continue;
                };
                if !matches!(target.property_name(), "firstMargin" | "lastMargin") {
                    diagnostics.push(format!(
                        "Text Path Options property {:?} has no established native mapping and its keys were omitted",
                        target.property_name()
                    ));
                }
            }
            if path.align != Default::default() || path.align_offset != 0.0 {
                diagnostics.push(
                    "Text path cap-band alignment/offset are FX extensions without established AE Path Options records and were omitted"
                        .into(),
                );
            }
            Some(TextPathOptionsSpec {
                path_index,
                first_margin: path.first_margin,
                last_margin: path.last_margin,
                perpendicular_to_path: path.perpendicular_to_path,
                reverse_path: path.reverse_path,
                force_alignment: path.force_alignment,
                animations,
            })
        }
        (Some(path), _) => {
            diagnostics.push(format!(
                "Text Path Options reference FX layer {}; native Text requires a proven one-based mask index, so path layout, margins, toggles, alignment and their keys were omitted",
                path.path_layer.value()
            ));
            None
        }
        (None, _) => None,
    };

    let spec = TextSpec {
        name: layer.name.clone(),
        transform: SolidTransform {
            anchor: transform.anchor_point,
            position,
            scale: transform.scale,
            rotation: transform.rotation,
            opacity: transform.opacity.value(),
        },
        transform_animations,
        documents,
        animators,
        anchor_options,
        path_options,
    };
    if let Some(message) = native_profile_diagnostic(&spec) {
        diagnostics.push(message.into());
    }
    Ok(LoweredText { spec, diagnostics })
}

/// Uses exactly the emitted Source Text timeline for bounds-only classification.
/// Diagnostics are emitted by ordinary lowering; this view never changes content.
pub(super) fn bounds_documents(
    layer: &TextLayer,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    fonts: &super::fonts::ArchiveFonts,
) -> Result<TextDocumentTimeline, &'static str> {
    let mut diagnostics = Vec::new();
    let document = lower_document(&layer.source_text, &mut diagnostics, false)?;
    lower_document_timeline(document, layer.id, dynamics, &mut diagnostics, Some(fonts))
}

fn native_profile_diagnostic(spec: &TextSpec) -> Option<&'static str> {
    match spec.native_profile()? {
        NativeTextProfile::Point => Some(
            "Experimental Point Text uses independent Adobe-native semantic defaults without inherited glyph/line caches. Native font-table defaults are not validated against FX fallback/custom-font resolution or actual font binaries. Bounded static native acceptance does not establish general styles, font-changing holds, native editable-control readback, alpha or full-project fidelity. Fractional Tracking rounds to native integers at document keys",
        ),
        NativeTextProfile::Box => Some(
            "Experimental Box Text uses cache-free independent Adobe-native semantic defaults and native automatic-leading storage. Unknown requested-font vendor metadata is omitted. Native font-table defaults are not validated against FX fallback/custom-font resolution or actual font binaries. Bounded native acceptance/readback does not establish full-project render, alpha, edited box reflow or actual-font fidelity. Fractional Tracking rounds to native integers at document keys",
        ),
    }
}

fn lower_document(
    document: &TextDocument,
    diagnostics: &mut Vec<String>,
    diagnose_candidate: bool,
) -> Result<DocumentState, &'static str> {
    if document.box_text && (document.box_size.is_none() || document.box_position.is_none()) {
        return Err("Box Text requires boxSize and boxPosition for native export");
    }
    if document.apply_stroke && document.stroke_color.is_none() {
        return Err("Enabled Text stroke has no color");
    }
    if document.font_variations.is_some() {
        diagnostics.push(
            "Variable-font axes have no established native Source Text writer grammar and were omitted"
                .into(),
        );
    }
    if document.scale_box_text_with_transform {
        diagnostics.push(
            "scaleBoxTextWithTransform is a renderer behavior without an AE Source Text field and was omitted"
                .into(),
        );
    }
    if document.box_first_baseline.is_some() {
        diagnostics.push(
            "boxFirstBaseline is derived PAG/renderer metadata, not an AE-authored Source Text control, and was omitted"
                .into(),
        );
    }
    if document.underline || document.strikethrough {
        diagnostics.push(
            "Underline/strikethrough have no established whole-document AE COS mapping and were omitted"
                .into(),
        );
    }
    if !document.box_text && document.vertical_align.is_some() {
        diagnostics
            .push("Box vertical alignment is not applicable to Point Text and was omitted".into());
    }
    let (font_postscript, approximated) =
        postscript_name(document.font_family.as_ref(), document.font_style.as_ref())?;
    if diagnose_candidate && approximated {
        diagnostics.push(format!(
            "FX stores font family/style rather than AE's PostScript identity; authored deterministic candidate {font_postscript:?}, so host font resolution remains unverified"
        ));
    }
    Ok(DocumentState {
        font_family: document.font_family.to_string(),
        font_style: document.font_style.to_string(),
        document: TextDocumentSpec {
            text: document.text.clone(),
            font_postscript,
            font_format: None,
            font_size: document.font_size.value(),
            apply_fill: document.apply_fill,
            fill_color: document.fill_color,
            apply_stroke: document.apply_stroke,
            stroke_color: document.stroke_color,
            stroke_width: document.stroke_width.value(),
            stroke_over_fill: document.stroke_over_fill,
            justification: document.justification,
            tracking: document.tracking,
            leading: document.leading.map(|value| value.value()),
            baseline_shift: document.baseline_shift,
            box_size: document.box_text.then_some(document.box_size).flatten(),
            box_position: document.box_text.then_some(document.box_position).flatten(),
            vertical_align: document
                .box_text
                .then_some(document.vertical_align)
                .flatten(),
            all_caps: document.all_caps,
        },
    })
}

#[derive(Clone)]
struct DocumentState {
    document: TextDocumentSpec,
    font_family: String,
    font_style: String,
}

fn lower_document_timeline(
    mut state: DocumentState,
    layer_id: LayerId,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    diagnostics: &mut Vec<String>,
    fonts: Option<&super::fonts::ArchiveFonts>,
) -> Result<TextDocumentTimeline, &'static str> {
    let mut seen = BTreeSet::new();
    let mut tracks = Vec::new();
    let mut changed_font = false;
    for entry in dynamics {
        let PropertyTarget::LayerProperty(target) = &entry.target else {
            continue;
        };
        let property = target.property_type();
        if target.layer_id() != layer_id || !is_document_property(property) {
            continue;
        }
        if !seen.insert(property) {
            return Err("Duplicate Source Text animation target");
        }
        if !entry.dependencies.is_empty()
            || entry.random_seed_target.is_some()
            || !entry.layer_refs.is_empty()
        {
            return Err(
                "Dependent Source Text animator cannot be represented without evaluation or baking",
            );
        }
        match entry.animator.data() {
            AnimatorData::Constant { value } => {
                apply_document_value(&mut state, property, value)?;
                changed_font |= matches!(property, PropType::FontFamily | PropType::FontStyle);
            }
            AnimatorData::Keyframes {
                track,
                enabled: false,
                ..
            } => {
                let Some(value) = effective_constant(&entry.animator) else {
                    return Err("Disabled Source Text animator has no runtime-visible constant");
                };
                apply_document_value(&mut state, property, value)?;
                changed_font |= matches!(property, PropType::FontFamily | PropType::FontStyle);
                diagnostics.push(format!(
                    "Disabled {property:?} Source Text keys ({} authored keys) were not retained; runtime-visible disabledValue normalized to a native constant",
                    track.keyframes().len()
                ));
            }
            AnimatorData::Keyframes {
                track,
                enabled: true,
                ..
            } => {
                for key in track.keyframes() {
                    validate_document_value(property, key.value())?;
                    if key.spatial_in_tangent().is_some() || key.spatial_out_tangent().is_some() {
                        return Err("Source Text document keys cannot carry spatial tangents");
                    }
                }
                if track
                    .keyframes()
                    .iter()
                    .skip(1)
                    .any(|key| key.easing() != PropertyKeyframeEasing::Hold)
                {
                    diagnostics.push(format!(
                        "{property:?} uses continuous interpolation; native Source Text documents are Hold-only, so its keys were omitted and the current typed static base remains active"
                    ));
                    continue;
                }
                changed_font |= matches!(property, PropType::FontFamily | PropType::FontStyle);
                tracks.push((property, track));
            }
            AnimatorData::JsScript { .. } => {
                return Err(
                    "JavaScript Source Text animators are never evaluated, translated, or baked",
                );
            }
        }
    }
    refresh_font(&mut state, fonts, diagnostics)?;
    if changed_font && fonts.is_none() {
        diagnostics.push("Source Text font changes use deterministic PostScript-name candidates; native font identity and host resolution remain unverified".into());
    }
    if tracks.is_empty() {
        return Ok(TextDocumentTimeline {
            keyed: false,
            keys: vec![TextDocumentKey {
                time_millis: 0,
                document: state.document,
            }],
        });
    }
    let times = tracks
        .iter()
        .flat_map(|(_, track)| track.keyframes())
        .map(|key| key.layer_time().as_millis())
        .collect::<BTreeSet<_>>();
    let mut keys = Vec::with_capacity(times.len());
    for time_millis in times {
        let mut current = state.clone();
        for (property, track) in &tracks {
            let position = track
                .keyframes()
                .partition_point(|key| key.layer_time().as_millis() <= time_millis);
            let key = &track.keyframes()[position.saturating_sub(1)];
            apply_document_value(&mut current, *property, key.value())?;
        }
        refresh_font(&mut current, fonts, diagnostics)?;
        keys.push(TextDocumentKey {
            time_millis,
            document: current.document,
        });
    }
    Ok(TextDocumentTimeline { keyed: true, keys })
}

fn is_document_property(property: PropType) -> bool {
    matches!(
        property,
        PropType::TextContent
            | PropType::FontFamily
            | PropType::FontStyle
            | PropType::FontSize
            | PropType::Tracking
            | PropType::Leading
            | PropType::AllCaps
            | PropType::FillEnabled
            | PropType::FillColor
            | PropType::StrokeEnabled
            | PropType::StrokeColor
            | PropType::StrokeWidth
    )
}

fn validate_document_value(property: PropType, value: &PropertyValue) -> Result<(), &'static str> {
    match (property, value) {
        (PropType::TextContent, PropertyValue::String(value)) if !value.contains('\0') => Ok(()),
        (PropType::FontFamily, PropertyValue::String(value))
            if !value.is_empty() && !value.contains('\0') =>
        {
            Ok(())
        }
        // An empty style marks the family as the exact PostScript name.
        (PropType::FontStyle, PropertyValue::String(value)) if !value.contains('\0') => Ok(()),
        (PropType::FontSize | PropType::Leading, PropertyValue::Float(value))
            if value.is_finite() && *value > 0.0 =>
        {
            Ok(())
        }
        (PropType::Tracking, PropertyValue::Float(value)) if value.is_finite() => Ok(()),
        (PropType::StrokeWidth, PropertyValue::Float(value))
            if value.is_finite() && *value >= 0.0 =>
        {
            Ok(())
        }
        (
            PropType::AllCaps | PropType::FillEnabled | PropType::StrokeEnabled,
            PropertyValue::Bool(_),
        ) => Ok(()),
        (PropType::FillColor | PropType::StrokeColor, PropertyValue::Color(value))
            if value
                .iter()
                .all(|component| component.is_finite() && (0.0..=1.0).contains(component)) =>
        {
            Ok(())
        }
        _ => Err("Source Text animator has the wrong typed value or an invalid range"),
    }
}

fn apply_document_value(
    state: &mut DocumentState,
    property: PropType,
    value: &PropertyValue,
) -> Result<(), &'static str> {
    validate_document_value(property, value)?;
    match (property, value) {
        (PropType::TextContent, PropertyValue::String(value)) => {
            state.document.text.clone_from(value)
        }
        (PropType::FontFamily, PropertyValue::String(value)) => state.font_family.clone_from(value),
        (PropType::FontStyle, PropertyValue::String(value)) => state.font_style.clone_from(value),
        (PropType::FontSize, PropertyValue::Float(value)) => state.document.font_size = *value,
        (PropType::Tracking, PropertyValue::Float(value)) => state.document.tracking = *value,
        (PropType::Leading, PropertyValue::Float(value)) => state.document.leading = Some(*value),
        (PropType::AllCaps, PropertyValue::Bool(value)) => state.document.all_caps = *value,
        (PropType::FillEnabled, PropertyValue::Bool(value)) => state.document.apply_fill = *value,
        (PropType::FillColor, PropertyValue::Color(value)) => state.document.fill_color = *value,
        (PropType::StrokeEnabled, PropertyValue::Bool(value)) => {
            state.document.apply_stroke = *value
        }
        (PropType::StrokeColor, PropertyValue::Color(value)) => {
            state.document.stroke_color = Some(*value)
        }
        (PropType::StrokeWidth, PropertyValue::Float(value)) => {
            state.document.stroke_width = *value
        }
        _ => return Err("Source Text animator has the wrong typed value"),
    }
    Ok(())
}

fn refresh_font(
    state: &mut DocumentState,
    fonts: Option<&super::fonts::ArchiveFonts>,
    diagnostics: &mut Vec<String>,
) -> Result<(), &'static str> {
    let (candidate, _) = postscript_name(&state.font_family, &state.font_style)?;
    state.document.font_postscript = candidate;
    state.document.font_format = None;
    if let Some(fonts) = fonts {
        match fonts.resolve(&state.font_family, &state.font_style) {
            Ok((name, format)) => {
                state.document.font_postscript = name.to_owned();
                state.document.font_format = Some(format);
            }
            Err(reason) => {
                let message = format!(
                    "Font {:?}/{:?}: {reason}; retained candidate {:?} without a guessed native face format; host resolution remains unverified",
                    state.font_family, state.font_style, state.document.font_postscript
                );
                if !diagnostics.contains(&message) {
                    diagnostics.push(message);
                }
            }
        }
    }
    Ok(())
}

fn postscript_name(family: &str, style: &str) -> Result<(String, bool), &'static str> {
    if family.is_empty() || family.contains('\0') || style.contains('\0') {
        return Err("Text font family is empty or its family/style contains NUL");
    }
    // An empty style marks the family as the exact PostScript name, the FX
    // convention of the Premiere converter and of dash-less AE imports.
    if style.is_empty() {
        return Ok((family.to_owned(), false));
    }
    let compact_family = family
        .chars()
        .filter(|value| !value.is_whitespace())
        .collect::<String>();
    let compact_style = style
        .chars()
        .filter(|value| !value.is_whitespace())
        .collect::<String>();
    let already_postscript = family
        .rsplit_once('-')
        .is_some_and(|(_, suffix)| suffix == style);
    if already_postscript {
        Ok((family.to_owned(), false))
    } else {
        // Family/style does not prove the PostScript name even when removing
        // whitespace happens to reproduce common fonts such as Inter.
        Ok((format!("{compact_family}-{compact_style}"), true))
    }
}

fn lower_range_selector(
    selector: &fx_schema::RangeSelector,
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> Result<RangeSelectorSpec, &'static str> {
    ensure_materialized_targets(
        dynamics,
        selector.id,
        &[
            ("start", true),
            ("end", true),
            ("offset", true),
            ("amount", true),
            ("easeHigh", true),
            ("easeLow", true),
            ("randomSeed", true),
        ],
    )?;
    let percent = selector.units == fx_schema::text_animator::SelectorUnits::Percentage;
    let mut animations = PropertyTracks::default();
    for (name, scale) in [
        ("start", if percent { 100.0 } else { 1.0 }),
        ("end", if percent { 100.0 } else { 1.0 }),
        ("offset", if percent { 100.0 } else { 1.0 }),
        ("amount", 100.0),
        ("easeHigh", 100.0),
        ("easeLow", 100.0),
        ("randomSeed", 1.0),
    ] {
        insert_track(
            &mut animations,
            name,
            fx_track(dynamics, selector.id, name, TrackEncoding::Scalar(scale))?,
        );
    }
    Ok(RangeSelectorSpec {
        value: selector.clone(),
        animations,
    })
}

fn lower_wiggly_selector(
    selector: &fx_schema::WigglySelector,
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> Result<WigglySelectorSpec, &'static str> {
    ensure_materialized_targets(
        dynamics,
        selector.id,
        &[("speed", true), ("amount", true), ("seed", true)],
    )?;
    let mut animations = PropertyTracks::default();
    for name in ["speed", "amount", "seed"] {
        insert_track(
            &mut animations,
            name,
            fx_track(dynamics, selector.id, name, TrackEncoding::Scalar(1.0))?,
        );
    }
    Ok(WigglySelectorSpec {
        value: selector.clone(),
        animations,
    })
}

fn ensure_materialized_targets(
    dynamics: &crate::export_document::AnimationIndex<'_>,
    id: FxItemId,
    supported: &[(&str, bool)],
) -> Result<(), &'static str> {
    for entry in dynamics
        .iter()
        .filter(|entry| entry.target.fx_item_id() == Some(id))
    {
        let PropertyTarget::FxItemProperty(target) = &entry.target else {
            continue;
        };
        let Some((_, materialized)) = supported
            .iter()
            .find(|(name, _)| *name == target.property_name())
        else {
            return Err("Text FX item has an animator target outside native Text support");
        };
        if !materialized {
            return Err(
                "Text animation targets an absent Animator property; the native property pool is not materialized implicitly",
            );
        }
    }
    Ok(())
}

fn insert_track(tracks: &mut PropertyTracks, name: &'static str, track: Option<NumericTrack>) {
    if let Some(track) = track {
        tracks.insert(name, track);
    }
}

#[derive(Clone, Copy)]
enum TrackEncoding {
    Scalar(f64),
    Vector2,
    Spatial3,
    Scale3,
    LineSpacing,
    Color,
}

fn fx_track(
    entries: &crate::export_document::AnimationIndex<'_>,
    id: FxItemId,
    property_name: &str,
    encoding: TrackEncoding,
) -> Result<Option<NumericTrack>, &'static str> {
    let mut matches = entries.iter().filter(|entry| {
        matches!(
            &entry.target,
            PropertyTarget::FxItemProperty(target)
                if target.item_id() == id && target.property_name() == property_name
        )
    });
    let Some(entry) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err("Duplicate native Text animation target");
    }
    if !entry.dependencies.is_empty()
        || entry.random_seed_target.is_some()
        || !entry.layer_refs.is_empty()
    {
        return Err("Dependent Text animator cannot be represented as native numeric keys");
    }
    let track = match entry.animator.data() {
        AnimatorData::Keyframes {
            track,
            enabled: true,
            ..
        } => track,
        AnimatorData::Keyframes { enabled: false, .. } => {
            let value = effective_constant(&entry.animator)
                .ok_or("Disabled Text animator has no runtime-visible constant")?;
            return Ok(Some(constant_track(value, encoding)?));
        }
        AnimatorData::Constant { value } => {
            return Ok(Some(constant_track(value, encoding)?));
        }
        AnimatorData::JsScript { .. } => {
            return Err("JavaScript Text animators are never translated or baked");
        }
    };
    Ok(Some(encode_track(track, encoding)?))
}

fn constant_track(
    value: &PropertyValue,
    encoding: TrackEncoding,
) -> Result<NumericTrack, &'static str> {
    let (values, dimensions, spatial) = match encoding {
        TrackEncoding::Scalar(scale) => (vec![super::float_value(value)? * scale], 1, false),
        TrackEncoding::Vector2 => (vector_value(value)?.to_vec(), 2, false),
        TrackEncoding::Spatial3 => {
            let value = vector_value(value)?;
            (vec![value[0], value[1], 0.0], 1, true)
        }
        TrackEncoding::Scale3 => {
            let value = vector_value(value)?;
            (vec![value[0], value[1], 100.0], 3, false)
        }
        TrackEncoding::LineSpacing => (vec![0.0, super::float_value(value)?], 2, false),
        TrackEncoding::Color => {
            let PropertyValue::Color(value) = value else {
                return Err("Text color constant has a non-color value");
            };
            if value
                .iter()
                .any(|component| !component.is_finite() || !(0.0..=1.0).contains(component))
            {
                return Err("Text color constant lies outside 0..=1");
            }
            (
                vec![
                    value[3] * 255.0,
                    value[0] * 255.0,
                    value[1] * 255.0,
                    value[2] * 255.0,
                ],
                4,
                false,
            )
        }
    };
    Ok(NumericTrack {
        keys: vec![NumericKeyframe {
            time_millis: 0,
            values,
            easing: vec![KeyframeEasing::Hold; dimensions],
            spatial_in: if spatial { vec![0.0; 3] } else { Vec::new() },
            spatial_out: if spatial { vec![0.0; 3] } else { Vec::new() },
        }],
    })
}

fn encode_track(
    track: &PropertyKeyframeTrack,
    encoding: TrackEncoding,
) -> Result<NumericTrack, &'static str> {
    let keys = track
        .keyframes()
        .iter()
        .map(|key| {
            let easing = super::native_easing(key.easing());
            let (values, easing, spatial_in, spatial_out) = match encoding {
                TrackEncoding::Scalar(scale) => (
                    vec![super::float_value(key.value())? * scale],
                    vec![easing],
                    Vec::new(),
                    Vec::new(),
                ),
                TrackEncoding::Vector2 => {
                    let value = vector_value(key.value())?;
                    (value.to_vec(), vec![easing; 2], Vec::new(), Vec::new())
                }
                TrackEncoding::Spatial3 => {
                    let value = vector_value(key.value())?;
                    (
                        vec![value[0], value[1], 0.0],
                        vec![easing],
                        vec![0.0; 3],
                        vec![0.0; 3],
                    )
                }
                TrackEncoding::Scale3 => {
                    let value = vector_value(key.value())?;
                    (
                        vec![value[0], value[1], 100.0],
                        vec![easing; 3],
                        Vec::new(),
                        Vec::new(),
                    )
                }
                TrackEncoding::LineSpacing => (
                    vec![0.0, super::float_value(key.value())?],
                    vec![easing; 2],
                    Vec::new(),
                    Vec::new(),
                ),
                TrackEncoding::Color => {
                    let PropertyValue::Color(value) = key.value() else {
                        return Err("Text color keyframe has a non-color value");
                    };
                    if value
                        .iter()
                        .any(|component| !component.is_finite() || !(0.0..=1.0).contains(component))
                    {
                        return Err("Text color keyframe lies outside 0..=1");
                    }
                    (
                        vec![
                            value[3] * 255.0,
                            value[0] * 255.0,
                            value[1] * 255.0,
                            value[2] * 255.0,
                        ],
                        vec![easing; 4],
                        Vec::new(),
                        Vec::new(),
                    )
                }
            };
            Ok(NumericKeyframe {
                time_millis: key.layer_time().as_millis(),
                values,
                easing,
                spatial_in,
                spatial_out,
            })
        })
        .collect::<Result<Vec<_>, &'static str>>()?;
    Ok(NumericTrack { keys })
}

fn vector_value(value: &PropertyValue) -> Result<[f64; 2], &'static str> {
    let PropertyValue::Vector2(value) = value else {
        return Err("Text vector keyframe has a non-vector value");
    };
    value
        .iter()
        .copied()
        .all(f64::is_finite)
        .then_some(*value)
        .ok_or("Text vector keyframe has a non-finite value")
}

#[cfg(test)]
mod tests {
    use fx_schema::{
        PropertyAnimator, PropertyKeyframeEasing, TimeOffset,
        animator::{AnimationGraphEntry, KeyframeId, PropertyKeyframe},
    };

    use super::*;

    fn keyed(id: FxItemId, name: &str, values: [f64; 2]) -> AnimationGraphEntry {
        let keys = values
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                PropertyKeyframe::new(
                    KeyframeId::new(format!("text-{index}")),
                    TimeOffset::from_millis(i64::try_from(index).unwrap() * 500),
                    PropertyValue::Float(value),
                    PropertyKeyframeEasing::Linear,
                )
            })
            .collect();
        AnimationGraphEntry {
            target: PropertyTarget::fx_item(id, name),
            animator: PropertyAnimator::keyframes(PropertyKeyframeTrack::new(keys).unwrap()),
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        }
    }

    #[test]
    fn percentage_selector_keys_are_scaled_to_native_percent() {
        let id = FxItemId::new(7);
        let entry = keyed(id, "start", [0.25, 0.75]);
        let track = fx_track(
            &crate::export_document::AnimationIndex::new(&[entry]),
            id,
            "start",
            TrackEncoding::Scalar(100.0),
        )
        .unwrap()
        .unwrap();
        assert_eq!(track.keys[0].values, vec![25.0]);
        assert_eq!(track.keys[1].values, vec![75.0]);
    }

    #[test]
    fn postscript_identity_is_deterministic_and_diagnoses_family_names() {
        // A different native fixture's face is not the user's resolved font.
        // Keep the existing diagnostic candidate, not a hard-coded alias.
        assert_eq!(
            postscript_name("Arial", "Regular").unwrap(),
            ("Arial-Regular".into(), true)
        );
        assert_eq!(
            postscript_name("MyriadPro", "Regular").unwrap(),
            ("MyriadPro-Regular".into(), true)
        );
        assert_eq!(
            postscript_name("Open Sans", "Semi Bold").unwrap(),
            ("OpenSans-SemiBold".into(), true)
        );
    }

    #[test]
    fn empty_style_marks_an_exact_postscript_name() {
        assert_eq!(
            postscript_name("ArialMT", "").unwrap(),
            ("ArialMT".into(), false)
        );
        assert_eq!(
            postscript_name("AvenirNext-DemiBold", "").unwrap(),
            ("AvenirNext-DemiBold".into(), false)
        );
        for (family, style) in [
            ("", ""),
            ("", "Regular"),
            ("Arial\0MT", ""),
            ("ArialMT", "\0"),
        ] {
            assert!(
                postscript_name(family, style).is_err(),
                "{family:?}/{style:?}"
            );
        }
        let style = |value: &str| PropertyValue::String(value.into());
        assert!(validate_document_value(PropType::FontStyle, &style("")).is_ok());
        assert!(validate_document_value(PropType::FontFamily, &style("")).is_err());
        assert!(validate_document_value(PropType::FontStyle, &style("\0")).is_err());
    }

    #[test]
    fn absent_animator_property_cannot_be_created_only_by_a_key_target() {
        let id = FxItemId::new(11);
        let entry = keyed(id, "rotation", [0.0, 90.0]);
        assert!(
            ensure_materialized_targets(
                &crate::export_document::AnimationIndex::new(&[entry]),
                id,
                &[("rotation", false)]
            )
            .is_err()
        );
    }

    fn document_state() -> DocumentState {
        DocumentState {
            font_family: "Inter".into(),
            font_style: "Regular".into(),
            document: TextDocumentSpec {
                text: "base".into(),
                font_postscript: "Inter-Regular".into(),
                font_format: None,
                font_size: 24.0,
                apply_fill: true,
                fill_color: [1.0; 4],
                apply_stroke: false,
                stroke_color: None,
                stroke_width: 0.0,
                stroke_over_fill: true,
                justification: fx_schema::Justification::Left,
                tracking: 0.0,
                leading: None,
                baseline_shift: 0.0,
                box_size: None,
                box_position: None,
                vertical_align: None,
                all_caps: false,
            },
        }
    }

    #[test]
    fn native_profile_diagnostics_match_writer_eligibility() {
        let mut spec = TextSpec {
            name: "Point".into(),
            transform: SolidTransform {
                anchor: [0.0; 2],
                position: [0.0; 2],
                scale: [100.0; 2],
                rotation: 0.0,
                opacity: 100.0,
            },
            transform_animations: crate::writer::TransformAnimations::default(),
            documents: TextDocumentTimeline {
                keyed: false,
                keys: vec![TextDocumentKey {
                    time_millis: 0,
                    document: document_state().document,
                }],
            },
            animators: vec![],
            anchor_options: None,
            path_options: None,
        };
        let point = native_profile_diagnostic(&spec).unwrap();
        assert!(point.contains("Point Text") && point.contains("without inherited"));
        assert!(!point.contains("including cached"));
        spec.anchor_options = Some(TextAnchorOptionsSpec {
            grouping: fx_schema::AnchorPointGrouping::Character,
            alignment: [0.0; 2],
            animations: PropertyTracks::default(),
        });
        assert_eq!(native_profile_diagnostic(&spec), Some(point));
        spec.documents.keys[0].document.box_size = Some([400.0, 180.0]);
        spec.documents.keys[0].document.box_position = Some([0.0; 2]);
        let boxed = native_profile_diagnostic(&spec).unwrap();
        assert!(boxed.contains("Box Text") && boxed.contains("cache-free"));
        spec.anchor_options.as_mut().unwrap().grouping = fx_schema::AnchorPointGrouping::Word;
        assert_eq!(native_profile_diagnostic(&spec), Some(boxed));
        spec.anchor_options = None;
        spec.path_options = Some(TextPathOptionsSpec {
            path_index: 1,
            first_margin: 0.0,
            last_margin: 0.0,
            perpendicular_to_path: false,
            reverse_path: false,
            force_alignment: false,
            animations: PropertyTracks::default(),
        });
        assert_eq!(native_profile_diagnostic(&spec), Some(boxed));
    }

    fn disabled_entry(
        target: PropertyTarget,
        values: [PropertyValue; 2],
        disabled_value: PropertyValue,
    ) -> AnimationGraphEntry {
        let track = PropertyKeyframeTrack::new(
            values
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    PropertyKeyframe::new(
                        KeyframeId::new(format!("disabled-text-{index}")),
                        TimeOffset::from_millis(i64::try_from(index).unwrap() * 500),
                        value,
                        PropertyKeyframeEasing::Linear,
                    )
                })
                .collect(),
        )
        .unwrap();
        let animator = PropertyAnimator::keyframes(track);
        let mut data = animator.data().clone();
        let AnimatorData::Keyframes {
            enabled,
            disabled_value: stored_disabled_value,
            ..
        } = &mut data
        else {
            panic!("Text fixture is keyed")
        };
        *enabled = false;
        *stored_disabled_value = Some(disabled_value);
        AnimationGraphEntry {
            target,
            animator: PropertyAnimator::from_data(&data).unwrap(),
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        }
    }

    #[test]
    fn disabled_source_text_and_animator_controls_use_runtime_visible_values() {
        let layer_id = LayerId::new(42);
        let leading = disabled_entry(
            PropertyTarget::layer(layer_id, PropType::Leading),
            [PropertyValue::Float(60.0), PropertyValue::Float(90.0)],
            PropertyValue::Float(72.0),
        );
        let mut state = document_state();
        state.document.leading = Some(54.0);
        let mut diagnostics = Vec::new();
        let timeline = lower_document_timeline(
            state,
            layer_id,
            &crate::export_document::AnimationIndex::new(&[leading]),
            &mut diagnostics,
            None,
        )
        .unwrap();
        assert!(!timeline.keyed);
        assert_eq!(timeline.keys.len(), 1);
        assert_eq!(timeline.keys[0].document.leading, Some(72.0));
        assert!(diagnostics.iter().any(|message| {
            message.contains("runtime-visible disabledValue") && message.contains("native constant")
        }));

        let animator_id = FxItemId::new(43);
        let tracking = disabled_entry(
            PropertyTarget::fx_item(animator_id, "tracking"),
            [PropertyValue::Float(10.0), PropertyValue::Float(20.0)],
            PropertyValue::Float(33.0),
        );
        let track = fx_track(
            &crate::export_document::AnimationIndex::new(&[tracking]),
            animator_id,
            "tracking",
            TrackEncoding::Scalar(1.0),
        )
        .unwrap()
        .expect("disabled Text animator becomes a native constant");
        assert_eq!(track.keys.len(), 1);
        assert_eq!(track.keys[0].time_millis, 0);
        assert_eq!(track.keys[0].values, [33.0]);
        assert_eq!(track.keys[0].easing, [KeyframeEasing::Hold]);
    }

    #[test]
    fn source_stroke_constants_are_order_independent() {
        let layer_id = LayerId::new(42);
        let make_entry = |property, value| AnimationGraphEntry {
            target: PropertyTarget::layer(layer_id, property),
            animator: PropertyAnimator::constant(value).unwrap(),
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        };
        let entries = vec![
            make_entry(PropType::StrokeEnabled, PropertyValue::Bool(true)),
            make_entry(
                PropType::StrokeColor,
                PropertyValue::Color([0.1, 0.7, 0.9, 1.0]),
            ),
            make_entry(PropType::StrokeWidth, PropertyValue::Float(11.0)),
        ];
        for ordered in [entries.clone(), entries.into_iter().rev().collect()] {
            let timeline = lower_document_timeline(
                document_state(),
                layer_id,
                &crate::export_document::AnimationIndex::new(&ordered),
                &mut Vec::new(),
                None,
            )
            .unwrap();
            assert!(!timeline.keyed);
            let document = &timeline.keys[0].document;
            assert!(document.apply_stroke);
            assert_eq!(document.stroke_color, Some([0.1, 0.7, 0.9, 1.0]));
            assert_eq!(document.stroke_width, 11.0);
            assert!(document.stroke_over_fill);
        }
    }

    #[test]
    fn source_stroke_keeps_dependency_and_continuous_interpolation_guards() {
        let layer_id = LayerId::new(42);
        let target = PropertyTarget::layer(layer_id, PropType::StrokeWidth);
        let mut entry = AnimationGraphEntry {
            target: target.clone(),
            animator: PropertyAnimator::keyframes(
                PropertyKeyframeTrack::new(vec![
                    PropertyKeyframe::new(
                        KeyframeId::new("stroke-start"),
                        TimeOffset::from_millis(0),
                        PropertyValue::Float(3.0),
                        PropertyKeyframeEasing::Linear,
                    ),
                    PropertyKeyframe::new(
                        KeyframeId::new("stroke-end"),
                        TimeOffset::from_millis(500),
                        PropertyValue::Float(9.0),
                        PropertyKeyframeEasing::Linear,
                    ),
                ])
                .unwrap(),
            ),
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        };
        let mut diagnostics = Vec::new();
        let timeline = lower_document_timeline(
            document_state(),
            layer_id,
            &crate::export_document::AnimationIndex::new(&[entry.clone()]),
            &mut diagnostics,
            None,
        )
        .unwrap();
        assert!(!timeline.keyed);
        assert_eq!(timeline.keys[0].document.stroke_width, 0.0);
        assert!(
            diagnostics
                .iter()
                .any(|message| message.contains("continuous interpolation"))
        );
        entry.random_seed_target = Some(target);
        assert_eq!(
            lower_document_timeline(
                document_state(),
                layer_id,
                &crate::export_document::AnimationIndex::new(&[entry]),
                &mut Vec::new(),
                None,
            )
            .unwrap_err(),
            "Dependent Source Text animator cannot be represented without evaluation or baking"
        );
    }

    #[test]
    fn source_stroke_value_guards_preserve_typed_ranges() {
        for value in [-1.0, f64::INFINITY, f64::NAN] {
            assert!(
                validate_document_value(PropType::StrokeWidth, &PropertyValue::Float(value))
                    .is_err()
            );
        }
        assert!(validate_document_value(PropType::StrokeWidth, &PropertyValue::Float(0.0)).is_ok());
        assert!(
            validate_document_value(PropType::StrokeEnabled, &PropertyValue::Float(1.0)).is_err()
        );
        for color in [
            [-0.1, 0.0, 0.0, 1.0],
            [0.0, 1.1, 0.0, 1.0],
            [0.0, 0.0, f64::NAN, 1.0],
        ] {
            assert!(
                validate_document_value(PropType::StrokeColor, &PropertyValue::Color(color))
                    .is_err()
            );
        }
    }

    #[test]
    fn source_stroke_disabled_tracks_use_the_runtime_visible_constant() {
        let layer_id = LayerId::new(42);
        let entries = [disabled_entry(
            PropertyTarget::layer(layer_id, PropType::StrokeWidth),
            [PropertyValue::Float(3.0), PropertyValue::Float(9.0)],
            PropertyValue::Float(11.0),
        )];
        let timeline = lower_document_timeline(
            document_state(),
            layer_id,
            &crate::export_document::AnimationIndex::new(&entries),
            &mut Vec::new(),
            None,
        )
        .unwrap();
        assert!(!timeline.keyed);
        assert_eq!(timeline.keys[0].document.stroke_width, 11.0);
        assert!(!timeline.keys[0].document.apply_stroke);
    }

    #[test]
    fn source_text_uses_the_union_of_signed_authored_hold_times() {
        let layer_id = LayerId::new(42);
        let make_entry = |property, values: [(i64, PropertyValue); 2]| {
            let keys = values
                .into_iter()
                .enumerate()
                .map(|(index, (time, value))| {
                    PropertyKeyframe::new(
                        KeyframeId::new(format!("document-{property:?}-{index}")),
                        TimeOffset::from_millis(time),
                        value,
                        PropertyKeyframeEasing::Hold,
                    )
                })
                .collect();
            AnimationGraphEntry {
                target: PropertyTarget::layer(layer_id, property),
                animator: PropertyAnimator::keyframes(PropertyKeyframeTrack::new(keys).unwrap()),
                dependencies: Vec::new(),
                random_seed_target: None,
                layer_refs: Default::default(),
            }
        };
        let entries = vec![
            make_entry(
                PropType::TextContent,
                [
                    (-250, PropertyValue::String("one".into())),
                    (500, PropertyValue::String("two".into())),
                ],
            ),
            make_entry(
                PropType::AllCaps,
                [
                    (0, PropertyValue::Bool(true)),
                    (750, PropertyValue::Bool(false)),
                ],
            ),
        ];
        let mut diagnostics = Vec::new();
        let timeline = lower_document_timeline(
            document_state(),
            layer_id,
            &crate::export_document::AnimationIndex::new(&entries),
            &mut diagnostics,
            None,
        )
        .unwrap();
        assert_eq!(
            timeline
                .keys
                .iter()
                .map(|key| key.time_millis)
                .collect::<Vec<_>>(),
            vec![-250, 0, 500, 750]
        );
        assert_eq!(timeline.keys[0].document.text, "one");
        assert!(timeline.keys[0].document.all_caps);
        assert_eq!(timeline.keys[2].document.text, "two");
        assert!(diagnostics.is_empty());
    }
}
