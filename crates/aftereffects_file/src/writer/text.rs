//! Fresh native AE Text layers built from typed FX text values.
//!
//! This writer deliberately has no constructor from imported chunks or COS
//! bytes. Adobe acceptance of newly generated nonempty projects remains a
//! separate proof obligation.

use std::collections::BTreeMap;

use fx_schema::text_animator::{SelectorBasis, SelectorMode, SelectorShape, SelectorUnits};
use fx_schema::{AnchorPointGrouping, RangeSelector, TextAnimator, WigglySelector};

use crate::{rifx::Chunk, schema::layer_records::LayerRecord, timing::Duration24};

pub(crate) use super::text_document::{TextDocumentKey, TextDocumentSpec, TextDocumentTimeline};

use super::{
    AepWriteError, NumericTrack, SolidTransform, TransformAnimations, text_document,
    views::{self, ValueKind},
};

/// Native numeric tracks addressed by the FX property's stable camel-case name.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PropertyTracks {
    tracks: BTreeMap<&'static str, NumericTrack>,
}

impl PropertyTracks {
    pub(crate) fn insert(&mut self, name: &'static str, track: NumericTrack) {
        self.tracks.insert(name, track);
    }

    fn get(&self, name: &'static str) -> Option<&NumericTrack> {
        self.tracks.get(name)
    }
}

/// One typed Range Selector and its native numeric tracks.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RangeSelectorSpec {
    pub value: RangeSelector,
    pub animations: PropertyTracks,
}

/// One typed Wiggly Selector and its native numeric tracks.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WigglySelectorSpec {
    pub value: WigglySelector,
    pub animations: PropertyTracks,
}

/// One native Text Animator with explicitly materialized properties only.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextAnimatorSpec {
    pub value: TextAnimator,
    pub animations: PropertyTracks,
    pub selectors: Vec<RangeSelectorSpec>,
    pub wiggly_selectors: Vec<WigglySelectorSpec>,
}

/// AE Text > More Options fields represented by the FX model.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextAnchorOptionsSpec {
    pub grouping: AnchorPointGrouping,
    pub alignment: [f64; 2],
    pub animations: PropertyTracks,
}

/// AE Text > Path Options after integration resolves the sibling mask index.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextPathOptionsSpec {
    /// Native AE one-based mask index, not an FX layer id.
    pub path_index: u16,
    pub first_margin: f64,
    pub last_margin: f64,
    pub perpendicular_to_path: bool,
    pub reverse_path: bool,
    pub force_alignment: bool,
    pub animations: PropertyTracks,
}

/// One fresh source-less native Text layer.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextSpec {
    pub name: String,
    pub transform: SolidTransform,
    pub transform_animations: TransformAnimations,
    pub documents: TextDocumentTimeline,
    pub animators: Vec<TextAnimatorSpec>,
    pub anchor_options: Option<TextAnchorOptionsSpec>,
    pub path_options: Option<TextPathOptionsSpec>,
}

pub(super) fn validate(text: &TextSpec) -> Result<(), AepWriteError> {
    if text.name.is_empty() || text.name.len() > 255 || text.name.contains('\0') {
        return Err(AepWriteError::Invalid(
            "text layer name must be 1..=255 UTF-8 bytes without NUL",
        ));
    }
    text.documents.validate()?;
    let transform = &text.transform;
    if transform
        .anchor
        .iter()
        .chain(&transform.position)
        .chain(&transform.scale)
        .any(|value| !value.is_finite())
        || !transform.rotation.is_finite()
        || !transform.opacity.is_finite()
        || !(0.0..=100.0).contains(&transform.opacity)
    {
        return Err(AepWriteError::Invalid("invalid native Text Transform"));
    }
    if text.animators.len() > 1_024
        || text.animators.iter().any(|animator| {
            animator.selectors.len() > 1_024 || animator.wiggly_selectors.len() > 1_024
        })
    {
        return Err(AepWriteError::Invalid(
            "native Text Animator limit exceeded",
        ));
    }
    Ok(())
}

/// Encodes a fresh source-less Text `Layr` for owner C's timeline dispatcher.
#[expect(dead_code, reason = "default-clock wrapper; callers pass a clock")]
pub(crate) fn timeline_layer(
    text: &TextSpec,
    id: u32,
    duration: Duration24,
) -> Result<Chunk, AepWriteError> {
    timeline_layer_with_clock(text, id, duration, super::keyframes::PropertyClock::DEFAULT)
}

pub(super) fn timeline_layer_with_clock(
    text: &TextSpec,
    id: u32,
    duration: Duration24,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    validate(text)?;
    let properties = views::group(
        1,
        "",
        vec![
            (
                "ADBE Transform Group",
                layer_transform_with_clock(&text.transform, &text.transform_animations, clock)?,
            ),
            (
                "ADBE Text Properties",
                text_properties_with_clock(text, clock)?,
            ),
        ],
    )?;

    // Text and Shape are both source-less visual layers. Start from the typed
    // canonical source-less envelope, then set the two independently observed
    // Text fields: native layer type 3 and the native fixture's label 1.
    let mut record = LayerRecord::shape_ae26(id, duration)?.encode();
    record[61] = 1;
    record[131] = 3;
    let record = LayerRecord::decode(&record)?;
    Ok(Chunk::list(
        *b"Layr",
        vec![
            Chunk::data(*b"ldta", record.encode())?,
            Chunk::data(*b"Utf8", text.name.as_bytes().to_vec())?,
            properties,
        ],
    ))
}

#[expect(dead_code, reason = "default-clock wrapper; callers pass a clock")]
fn text_properties(text: &TextSpec) -> Result<Chunk, AepWriteError> {
    text_properties_with_clock(text, super::keyframes::PropertyClock::DEFAULT)
}

fn text_properties_with_clock(
    text: &TextSpec,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let mut entries = vec![(
        "ADBE Text Document",
        text_document::source_property_with_clock(&text.documents, clock)?,
    )];
    if let Some(path) = &text.path_options {
        entries.push(("ADBE Text Path Options", path_options(path, clock)?));
    }
    entries.push((
        "ADBE Text More Options",
        more_options(text.anchor_options.as_ref(), clock)?,
    ));
    entries.push(("ADBE Text Animators", animators(&text.animators, clock)?));
    Ok(views::group(1, "Text", entries)?)
}

fn animators(
    values: &[TextAnimatorSpec],
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let mut entries = Vec::with_capacity(values.len());
    for (index, animator) in values.iter().enumerate() {
        entries.push((
            "ADBE Text Animator",
            animator_group(animator, index + 1, clock)?,
        ));
    }
    Ok(views::group(1, "Animators", entries)?)
}

fn animator_group(
    value: &TextAnimatorSpec,
    index: usize,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    if value.selectors.len() != value.value.selectors.len()
        || value.wiggly_selectors.len() != value.value.wiggly_selectors.len()
    {
        return Err(AepWriteError::Invalid(
            "Text Animator selector/value count mismatch",
        ));
    }
    let mut selector_entries = Vec::with_capacity(
        value
            .selectors
            .len()
            .saturating_add(value.wiggly_selectors.len()),
    );
    for (selector_index, selector) in value.selectors.iter().enumerate() {
        selector_entries.push((
            "ADBE Text Selector",
            range_selector(selector, selector_index + 1, clock)?,
        ));
    }
    for (selector_index, selector) in value.wiggly_selectors.iter().enumerate() {
        selector_entries.push((
            "ADBE Text Wiggly Selector",
            wiggly_selector(selector, selector_index + 1, clock)?,
        ));
    }
    let properties = animator_properties_with_clock(value, clock)?;
    let fallback_name;
    let name = if value.value.name.is_empty() {
        fallback_name = format!("Animator {index}");
        &fallback_name
    } else {
        &value.value.name
    };
    Ok(views::group(
        1,
        name,
        vec![
            (
                "ADBE Text Selectors",
                views::group(1, "Selectors", selector_entries)?,
            ),
            ("ADBE Text Animator Properties", properties),
        ],
    )?)
}

#[cfg(test)]
fn animator_properties(value: &TextAnimatorSpec) -> Result<Chunk, AepWriteError> {
    animator_properties_with_clock(value, super::keyframes::PropertyClock::DEFAULT)
}

fn animator_properties_with_clock(
    value: &TextAnimatorSpec,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let property = |kind: ValueKind,
                    values: &[f64],
                    bounds: Option<(f64, f64)>,
                    track: Option<&NumericTrack>|
     -> Result<Chunk, AepWriteError> {
        Ok(views::property_with_clock(
            kind, values, bounds, track, clock,
        )?)
    };
    let optional_scalar = |entries: &mut Vec<(&'static str, Chunk)>,
                           native_name: &'static str,
                           value: Option<f64>,
                           track_name: &'static str,
                           tracks: &PropertyTracks,
                           kind: ValueKind,
                           bounds: Option<(f64, f64)>|
     -> Result<(), AepWriteError> {
        if let Some(value) = value {
            entries.push((
                native_name,
                property(kind, &[value], bounds, tracks.get(track_name))?,
            ));
        }
        Ok(())
    };
    let animator = &value.value;
    let tracks = &value.animations;
    let mut entries = Vec::new();
    if let Some(current) = animator.anchor_point {
        entries.push((
            "ADBE Text Anchor Point 3D",
            property(
                ValueKind::Spatial,
                &[current[0], current[1], 0.0],
                None,
                tracks.get("anchorPoint"),
            )?,
        ));
    }
    if let Some(current) = animator.position {
        entries.push((
            "ADBE Text Position 3D",
            property(
                ValueKind::Spatial,
                &[current[0], current[1], 0.0],
                None,
                tracks.get("position"),
            )?,
        ));
    }
    if let Some(current) = animator.scale {
        entries.push((
            "ADBE Text Scale 3D",
            property(
                ValueKind::Scale,
                &[current[0], current[1], 100.0],
                None,
                tracks.get("scale"),
            )?,
        ));
    }
    optional_scalar(
        &mut entries,
        "ADBE Text Rotation",
        animator.rotation,
        "rotation",
        tracks,
        ValueKind::Angle,
        None,
    )?;
    optional_scalar(
        &mut entries,
        "ADBE Text Skew",
        animator.skew,
        "skew",
        tracks,
        ValueKind::Angle,
        None,
    )?;
    optional_scalar(
        &mut entries,
        "ADBE Text Skew Axis",
        animator.skew_axis,
        "skewAxis",
        tracks,
        ValueKind::Angle,
        None,
    )?;
    optional_scalar(
        &mut entries,
        "ADBE Text Tracking Amount",
        animator.tracking,
        "tracking",
        tracks,
        ValueKind::Scalar,
        None,
    )?;
    optional_scalar(
        &mut entries,
        "ADBE Text Stroke Width",
        animator.stroke_width,
        "strokeWidth",
        tracks,
        ValueKind::Scalar,
        Some((0.0, 100000.0)),
    )?;
    if let Some(current) = animator.blur {
        entries.push((
            "ADBE Text Blur",
            property(ValueKind::Pair, &current, None, tracks.get("blur"))?,
        ));
    }
    optional_scalar(
        &mut entries,
        "ADBE Text Opacity",
        animator.opacity,
        "opacity",
        tracks,
        ValueKind::Scalar,
        Some((0.0, 100.0)),
    )?;
    if let Some(color) = animator.fill_color {
        entries.push((
            "ADBE Text Fill Color",
            property(
                ValueKind::Color,
                &native_color(color),
                None,
                tracks.get("fillColor"),
            )?,
        ));
    }
    if let Some(color) = animator.stroke_color {
        entries.push((
            "ADBE Text Stroke Color",
            property(
                ValueKind::Color,
                &native_color(color),
                None,
                tracks.get("strokeColor"),
            )?,
        ));
    }
    if let Some(current) = animator.line_spacing {
        entries.push((
            "ADBE Text Line Spacing",
            property(
                ValueKind::Pair,
                &[0.0, current],
                None,
                tracks.get("lineSpacing"),
            )?,
        ));
    }
    optional_scalar(
        &mut entries,
        "ADBE Text Line Anchor",
        animator.line_anchor,
        "lineAnchor",
        tracks,
        ValueKind::Scalar,
        None,
    )?;
    optional_scalar(
        &mut entries,
        "ADBE Text Character Offset",
        animator.character_offset,
        "characterOffset",
        tracks,
        ValueKind::Scalar,
        None,
    )?;
    optional_scalar(
        &mut entries,
        "ADBE Text Character Replace",
        animator.character_value,
        "characterValue",
        tracks,
        ValueKind::Scalar,
        None,
    )?;
    Ok(views::group(1, "Animator Properties", entries)?)
}

fn range_selector(
    value: &RangeSelectorSpec,
    index: usize,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let property = |kind: ValueKind,
                    values: &[f64],
                    bounds: Option<(f64, f64)>,
                    track: Option<&NumericTrack>|
     -> Result<Chunk, AepWriteError> {
        Ok(views::property_with_clock(
            kind, values, bounds, track, clock,
        )?)
    };
    let scalar = |value, bounds| property(ValueKind::Scalar, &[value], bounds, None);
    let selector = &value.value;
    let percentage = selector.units == SelectorUnits::Percentage;
    let scale = if percentage { 100.0 } else { 1.0 };
    let (start, end, offset) = if percentage {
        (
            "ADBE Text Percent Start",
            "ADBE Text Percent End",
            "ADBE Text Percent Offset",
        )
    } else {
        (
            "ADBE Text Index Start",
            "ADBE Text Index End",
            "ADBE Text Index Offset",
        )
    };
    let tracks = &value.animations;
    let advanced = views::group(
        1,
        "Advanced",
        vec![
            (
                "ADBE Text Range Units",
                scalar(enum_units(selector.units), None)?,
            ),
            (
                "ADBE Text Range Type2",
                scalar(enum_basis(selector.based_on), None)?,
            ),
            (
                "ADBE Text Selector Mode",
                scalar(enum_mode(selector.mode), None)?,
            ),
            (
                "ADBE Text Selector Max Amount",
                property(
                    ValueKind::Scalar,
                    &[selector.amount * 100.0],
                    None,
                    tracks.get("amount"),
                )?,
            ),
            (
                "ADBE Text Range Shape",
                scalar(enum_shape(selector.shape), None)?,
            ),
            (
                "ADBE Text Levels Max Ease",
                property(
                    ValueKind::Scalar,
                    &[selector.ease_high * 100.0],
                    None,
                    tracks.get("easeHigh"),
                )?,
            ),
            (
                "ADBE Text Levels Min Ease",
                property(
                    ValueKind::Scalar,
                    &[selector.ease_low * 100.0],
                    None,
                    tracks.get("easeLow"),
                )?,
            ),
            (
                "ADBE Text Randomize Order",
                views::property_with_clock(
                    ValueKind::Toggle,
                    &[f64::from(u8::from(selector.randomize_order))],
                    None,
                    None,
                    clock,
                )?,
            ),
            (
                "ADBE Text Random Seed",
                property(
                    ValueKind::Scalar,
                    &[selector.random_seed],
                    None,
                    tracks.get("randomSeed"),
                )?,
            ),
        ],
    )?;
    Ok(views::group(
        1,
        &format!("Range Selector {index}"),
        vec![
            (
                start,
                property(
                    ValueKind::Scalar,
                    &[selector.start * scale],
                    None,
                    tracks.get("start"),
                )?,
            ),
            (
                end,
                property(
                    ValueKind::Scalar,
                    &[selector.end * scale],
                    None,
                    tracks.get("end"),
                )?,
            ),
            (
                offset,
                property(
                    ValueKind::Scalar,
                    &[selector.offset * scale],
                    None,
                    tracks.get("offset"),
                )?,
            ),
            ("ADBE Text Range Advanced", advanced),
        ],
    )?)
}

fn wiggly_selector(
    value: &WigglySelectorSpec,
    index: usize,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let property = |kind: ValueKind,
                    values: &[f64],
                    bounds: Option<(f64, f64)>,
                    track: Option<&NumericTrack>|
     -> Result<Chunk, AepWriteError> {
        Ok(views::property_with_clock(
            kind, values, bounds, track, clock,
        )?)
    };
    let scalar = |value, bounds| property(ValueKind::Scalar, &[value], bounds, None);
    let selector = &value.value;
    let tracks = &value.animations;
    Ok(views::group(
        1,
        &format!("Wiggly Selector {index}"),
        vec![
            (
                "ADBE Text Selector Mode",
                scalar(enum_mode(selector.mode), None)?,
            ),
            (
                "ADBE Text Wiggly Max Amount",
                property(
                    ValueKind::Scalar,
                    &[selector.amount],
                    None,
                    tracks.get("amount"),
                )?,
            ),
            (
                "ADBE Text Wiggly Min Amount",
                scalar(-selector.amount, None)?,
            ),
            (
                "ADBE Text Temporal Freq",
                property(
                    ValueKind::Scalar,
                    &[selector.speed],
                    None,
                    tracks.get("speed"),
                )?,
            ),
            (
                "ADBE Text Wiggly Random Seed",
                property(
                    ValueKind::Scalar,
                    &[selector.seed],
                    None,
                    tracks.get("seed"),
                )?,
            ),
        ],
    )?)
}

fn more_options(
    value: Option<&TextAnchorOptionsSpec>,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let property = |kind: ValueKind,
                    values: &[f64],
                    bounds: Option<(f64, f64)>,
                    track: Option<&NumericTrack>|
     -> Result<Chunk, AepWriteError> {
        Ok(views::property_with_clock(
            kind, values, bounds, track, clock,
        )?)
    };
    let scalar = |value, bounds| property(ValueKind::Scalar, &[value], bounds, None);
    let grouping = value.map_or(1.0, |value| match value.grouping {
        AnchorPointGrouping::Character => 1.0,
        AnchorPointGrouping::Word => 2.0,
        AnchorPointGrouping::Line => 3.0,
        AnchorPointGrouping::All => 4.0,
    });
    let alignment = value.map_or([0.0, 0.0], |value| value.alignment);
    Ok(views::group(
        1,
        "More Options",
        vec![
            (
                "ADBE Text Anchor Point Option",
                scalar(grouping, Some((1.0, 4.0)))?,
            ),
            (
                "ADBE Text Anchor Point Align",
                property(
                    ValueKind::Pair,
                    &alignment,
                    None,
                    value.and_then(|value| value.animations.get("groupingAlignment")),
                )?,
            ),
            ("ADBE Text Render Order", scalar(1.0, Some((1.0, 3.0)))?),
            (
                "ADBE Text Character Blend Mode",
                scalar(1.0, Some((1.0, 29.0)))?,
            ),
            (
                "ADBE Text Variable Font Spacing",
                scalar(1.0, Some((1.0, 3.0)))?,
            ),
        ],
    )?)
}

fn path_options(
    value: &TextPathOptionsSpec,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let property = |kind: ValueKind,
                    values: &[f64],
                    bounds: Option<(f64, f64)>,
                    track: Option<&NumericTrack>|
     -> Result<Chunk, AepWriteError> {
        Ok(views::property_with_clock(
            kind, values, bounds, track, clock,
        )?)
    };
    let scalar = |value, bounds| property(ValueKind::Scalar, &[value], bounds, None);
    let toggle =
        |value: bool| property(ValueKind::Toggle, &[f64::from(u8::from(value))], None, None);
    if value.path_index == 0 {
        return Err(AepWriteError::Invalid("Text Path mask index is zero"));
    }
    Ok(views::group(
        1,
        "Path Options",
        vec![
            ("ADBE Text Path", scalar(f64::from(value.path_index), None)?),
            ("ADBE Text Reverse Path", toggle(value.reverse_path)?),
            (
                "ADBE Text Perpendicular To Path",
                toggle(value.perpendicular_to_path)?,
            ),
            ("ADBE Text Force Align Path", toggle(value.force_alignment)?),
            (
                "ADBE Text First Margin",
                property(
                    ValueKind::Scalar,
                    &[value.first_margin],
                    None,
                    value.animations.get("firstMargin"),
                )?,
            ),
            (
                "ADBE Text Last Margin",
                property(
                    ValueKind::Scalar,
                    &[value.last_margin],
                    None,
                    value.animations.get("lastMargin"),
                )?,
            ),
        ],
    )?)
}

#[expect(dead_code, reason = "default-clock wrapper; callers pass a clock")]
fn layer_transform(
    transform: &SolidTransform,
    animations: &TransformAnimations,
) -> Result<Chunk, AepWriteError> {
    layer_transform_with_clock(
        transform,
        animations,
        super::keyframes::PropertyClock::DEFAULT,
    )
}

fn layer_transform_with_clock(
    transform: &SolidTransform,
    animations: &TransformAnimations,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let property = |kind: ValueKind,
                    values: &[f64],
                    bounds: Option<(f64, f64)>,
                    animation: Option<&NumericTrack>|
     -> Result<Chunk, AepWriteError> {
        Ok(views::property_with_clock(
            kind, values, bounds, animation, clock,
        )?)
    };
    Ok(views::group(
        1,
        "-_0_/-",
        vec![
            (
                "ADBE Anchor Point",
                property(
                    ValueKind::Spatial,
                    &[transform.anchor[0], transform.anchor[1], 0.0],
                    None,
                    animations.anchor.as_ref(),
                )?,
            ),
            (
                "ADBE Position",
                property(
                    ValueKind::Spatial,
                    &[transform.position[0], transform.position[1], 0.0],
                    None,
                    animations.position.as_ref(),
                )?,
            ),
            (
                "ADBE Scale",
                property(
                    ValueKind::Scale,
                    &[transform.scale[0] / 100.0, transform.scale[1] / 100.0, 1.0],
                    Some((0.0, 0.0)),
                    animations.scale.as_ref(),
                )?,
            ),
            (
                "ADBE Rotate Z",
                property(
                    ValueKind::Angle,
                    &[transform.rotation],
                    None,
                    animations.rotation.as_ref(),
                )?,
            ),
            (
                "ADBE Opacity",
                property(
                    ValueKind::Scalar,
                    &[transform.opacity / 100.0],
                    Some((0.0, 100.0)),
                    animations.opacity.as_ref(),
                )?,
            ),
        ],
    )?)
}

fn native_color(color: [f64; 4]) -> [f64; 4] {
    [
        color[3] * 255.0,
        color[0] * 255.0,
        color[1] * 255.0,
        color[2] * 255.0,
    ]
}

fn enum_units(value: SelectorUnits) -> f64 {
    match value {
        SelectorUnits::Percentage => 1.0,
        SelectorUnits::Index => 2.0,
    }
}

fn enum_basis(value: SelectorBasis) -> f64 {
    match value {
        SelectorBasis::Characters => 1.0,
        SelectorBasis::CharactersExcludingSpaces => 2.0,
        SelectorBasis::Words => 3.0,
        SelectorBasis::Lines => 4.0,
    }
}

fn enum_mode(value: SelectorMode) -> f64 {
    match value {
        SelectorMode::Add => 1.0,
        SelectorMode::Subtract => 2.0,
        SelectorMode::Intersect => 3.0,
        SelectorMode::Min => 4.0,
        SelectorMode::Max => 5.0,
        SelectorMode::Difference => 6.0,
    }
}

fn enum_shape(value: SelectorShape) -> f64 {
    match value {
        SelectorShape::Square => 1.0,
        SelectorShape::RampUp => 2.0,
        SelectorShape::RampDown => 3.0,
        SelectorShape::Triangle => 4.0,
        SelectorShape::Round => 5.0,
        SelectorShape::Smooth => 6.0,
    }
}

#[cfg(test)]
mod tests {
    use fx_schema::{FxItemId, Justification};

    use super::*;

    #[test]
    fn selector_enums_are_the_native_one_based_ordinals() {
        assert_eq!(enum_units(SelectorUnits::Index), 2.0);
        assert_eq!(enum_basis(SelectorBasis::Lines), 4.0);
        assert_eq!(enum_mode(SelectorMode::Difference), 6.0);
        assert_eq!(enum_shape(SelectorShape::Smooth), 6.0);
    }

    #[test]
    fn source_text_spec_rejects_a_missing_enabled_stroke() {
        let document = TextDocumentSpec {
            text: "Text".into(),
            font_postscript: "Inter-Regular".into(),
            font_size: 24.0,
            apply_fill: true,
            fill_color: [1.0; 4],
            apply_stroke: true,
            stroke_color: None,
            stroke_width: 1.0,
            stroke_over_fill: true,
            justification: Justification::Left,
            tracking: 0.0,
            leading: None,
            baseline_shift: 0.0,
            box_size: None,
            box_position: None,
            all_caps: false,
        };
        assert!(document.validate().is_err());
    }

    #[test]
    fn thirty_fps_text_path_controls_use_composition_clock() {
        fn inspect(chunk: &Chunk, descriptors: &mut Vec<u32>, keys: &mut Vec<i32>) {
            if chunk.id() == *b"tdb4" {
                let bytes = chunk.data_payload().unwrap();
                descriptors.push(u32::from_be_bytes(bytes[12..16].try_into().unwrap()));
            }
            if chunk.id() == *b"ldat" {
                let bytes = chunk.data_payload().unwrap();
                keys.push(i32::from_be_bytes(bytes[..4].try_into().unwrap()));
            }
            if let Some(children) = chunk.children() {
                for child in children {
                    inspect(child, descriptors, keys);
                }
            }
        }
        let mut animations = PropertyTracks::default();
        animations.insert(
            "firstMargin",
            NumericTrack {
                keys: vec![super::super::NumericKeyframe {
                    time_millis: 1_100,
                    values: vec![42.0],
                    easing: vec![super::super::KeyframeEasing::Hold],
                    spatial_in: Vec::new(),
                    spatial_out: Vec::new(),
                }],
            },
        );
        let options = TextPathOptionsSpec {
            path_index: 1,
            first_margin: 42.0,
            last_margin: 0.0,
            perpendicular_to_path: false,
            reverse_path: false,
            force_alignment: false,
            animations,
        };
        let clock = super::super::keyframes::PropertyClock::for_rate(
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        let native = path_options(&options, clock).unwrap();
        let mut descriptors = Vec::new();
        let mut keys = Vec::new();
        inspect(&native, &mut descriptors, &mut keys);
        assert_eq!(descriptors, vec![30_720; 6]);
        assert_eq!(keys, vec![33_792]);
    }

    #[test]
    fn thirty_fps_range_selector_and_animator_key_are_clocked() {
        fn inspect(chunk: &Chunk, clocks: &mut Vec<u32>, keys: &mut Vec<i32>) {
            if chunk.id() == *b"tdb4" {
                let bytes = chunk.data_payload().unwrap();
                clocks.push(u32::from_be_bytes(bytes[12..16].try_into().unwrap()));
            }
            if chunk.id() == *b"ldat" {
                let bytes = chunk.data_payload().unwrap();
                keys.push(i32::from_be_bytes(bytes[..4].try_into().unwrap()));
            }
            if let Some(children) = chunk.children() {
                for child in children {
                    inspect(child, clocks, keys);
                }
            }
        }
        let track = NumericTrack {
            keys: vec![super::super::NumericKeyframe {
                time_millis: 1_100,
                values: vec![0.5],
                easing: vec![super::super::KeyframeEasing::Hold],
                spatial_in: Vec::new(),
                spatial_out: Vec::new(),
            }],
        };
        let mut selector_tracks = PropertyTracks::default();
        selector_tracks.insert("start", track.clone());
        let selector = RangeSelectorSpec {
            value: RangeSelector::default(),
            animations: selector_tracks,
        };
        let mut animator_tracks = PropertyTracks::default();
        animator_tracks.insert("opacity", track);
        let animator = TextAnimatorSpec {
            value: TextAnimator {
                opacity: Some(50.0),
                selectors: vec![RangeSelector::default()],
                ..TextAnimator::default()
            },
            animations: animator_tracks,
            selectors: vec![selector],
            wiggly_selectors: Vec::new(),
        };
        let clock = super::super::keyframes::PropertyClock::for_rate(
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        let native = animator_group(&animator, 1, clock).unwrap();
        let (mut clocks, mut keys) = (Vec::new(), Vec::new());
        inspect(&native, &mut clocks, &mut keys);
        assert!(clocks.len() >= 12);
        assert!(clocks.iter().all(|value| *value == 30_720));
        assert_eq!(keys, vec![33_792, 33_792]);
    }

    #[test]
    fn empty_animator_does_not_materialize_the_property_pool() {
        let animator = TextAnimatorSpec {
            value: TextAnimator {
                id: FxItemId::new(1),
                name: "Animator 1".into(),
                selectors: Vec::new(),
                wiggly_selectors: Vec::new(),
                ..TextAnimator::default()
            },
            animations: PropertyTracks::default(),
            selectors: Vec::new(),
            wiggly_selectors: Vec::new(),
        };
        let group = animator_properties(&animator).unwrap();
        let children = group.children().unwrap();
        assert_eq!(children.len(), 3, "flags, display name and Group End only");
    }
}
