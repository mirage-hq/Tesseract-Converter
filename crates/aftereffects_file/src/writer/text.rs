//! Fresh native AE Text layers built from typed FX text values.
//!
//! This writer deliberately has no constructor from imported chunks or COS
//! bytes. Adobe acceptance of newly generated nonempty projects remains a
//! separate proof obligation.

use std::collections::BTreeMap;

use fx_schema::text_animator::{SelectorBasis, SelectorMode, SelectorShape, SelectorUnits};
use fx_schema::{AnchorPointGrouping, RangeSelector, TextAnimator, WigglySelector};

use crate::{rifx::Chunk, schema::layer_records::LayerRecord, timing::Duration24};

pub(crate) use super::text_document::{
    FontFormat, TextDocumentKey, TextDocumentSpec, TextDocumentTimeline,
};

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

/// Native semantic defaults selected by the writer and its owner-local diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeTextProfile {
    Point,
    Box,
}

impl TextSpec {
    pub(crate) fn native_profile(&self) -> Option<NativeTextProfile> {
        self.documents.keys.first()?;
        if self
            .documents
            .keys
            .iter()
            .all(|key| key.document.box_size.is_none())
        {
            Some(NativeTextProfile::Point)
        } else if self
            .documents
            .keys
            .iter()
            .all(|key| key.document.box_size.is_some())
        {
            Some(NativeTextProfile::Box)
        } else {
            None
        }
    }
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
                text_properties_with_clock(text, id, clock)?,
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
    text_properties_with_clock(text, 0, super::keyframes::PropertyClock::DEFAULT)
}

fn text_properties_with_clock(
    text: &TextSpec,
    id: u32,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let native = match text.native_profile() {
        Some(NativeTextProfile::Point) => Some(super::native_text::point_properties(
            &text.documents,
            id,
            clock,
        )?),
        Some(NativeTextProfile::Box) => Some(super::native_text::boxed_properties(
            &text.documents,
            id,
            clock,
        )?),
        None => None,
    };
    if let Some(mut group) = native {
        // Keep the complete native Source Text envelope and owner metadata.
        // Editable siblings must not switch its COS document to the legacy profile.
        let children = group.children_mut().ok_or(AepWriteError::Invalid(
            "native Text reference group changed",
        ))?;
        let mut replacements = Vec::new();
        if let Some(path) = &text.path_options {
            replacements.push(("ADBE Text Path Options", path_options(path, clock)?));
        }
        if let Some(anchor) = &text.anchor_options
            && (anchor.grouping != AnchorPointGrouping::Character
                || anchor.alignment != [0.0; 2]
                || !anchor.animations.tracks.is_empty())
        {
            replacements.push(("ADBE Text More Options", more_options(Some(anchor), clock)?));
        }
        if !text.animators.is_empty() {
            replacements.push(("ADBE Text Animators", animators(&text.animators, clock)?));
        }
        for (name, replacement) in replacements {
            if let Some(index) = children.windows(2).position(|pair| {
                pair[0].id() == *b"tdmn"
                    && pair[0]
                        .data_payload()
                        .is_some_and(|data| data.starts_with(name.as_bytes()))
            }) {
                children[index + 1] = replacement;
            } else {
                let entry = views::group(1, "", vec![(name, replacement)])?;
                let entry = entry
                    .children()
                    .ok_or(AepWriteError::Invalid("native Text entry group changed"))?;
                let end = children
                    .iter()
                    .position(|child| {
                        child.id() == *b"tdmn"
                            && child
                                .data_payload()
                                .is_some_and(|data| data.starts_with(b"ADBE Group End"))
                    })
                    .unwrap_or(children.len());
                children.splice(end..end, entry[2..4].iter().cloned());
            }
        }
        super::native_text_controls::normalize(&mut group, clock)?;
        return Ok(group);
    }
    let mut entries = vec![(
        "ADBE Text Document",
        text_document::source_property_with_clock(&text.documents, clock)?,
    )];
    let path = match &text.path_options {
        Some(path) => path_options(path, clock)?,
        None => views::group(1, "-_0_/-", vec![])?,
    };
    entries.push(("ADBE Text Path Options", path));
    entries.push((
        "ADBE Text More Options",
        more_options(text.anchor_options.as_ref(), clock)?,
    ));
    entries.push(("ADBE Text Animators", animators(&text.animators, clock)?));
    let mut group = views::group(1, "Text", entries)?;
    super::native_text_controls::normalize(&mut group, clock)?;
    Ok(group)
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
        // Animator Stroke Width is a signed additive delta, unlike the
        // nonnegative width in Source Text. AE 26.5 reports this control range.
        Some((-1000.0, 1000.0)),
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
    let native_enum = |value| property(ValueKind::VectorEnum, &[value], None, None);
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
                native_enum(enum_units(selector.units))?,
            ),
            (
                "ADBE Text Range Type2",
                native_enum(enum_basis(selector.based_on))?,
            ),
            (
                "ADBE Text Selector Mode",
                property(
                    ValueKind::TextSelectorMode,
                    &[enum_mode(selector.mode)],
                    None,
                    None,
                )?,
            ),
            (
                "ADBE Text Selector Max Amount",
                property(
                    ValueKind::EffectFloat,
                    &[selector.amount * 100.0],
                    None,
                    tracks.get("amount"),
                )?,
            ),
            (
                "ADBE Text Range Shape",
                native_enum(enum_shape(selector.shape))?,
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
                    ValueKind::EffectFloat,
                    &[selector.start * scale],
                    None,
                    tracks.get("start"),
                )?,
            ),
            (
                end,
                property(
                    ValueKind::EffectFloat,
                    &[selector.end * scale],
                    None,
                    tracks.get("end"),
                )?,
            ),
            (
                offset,
                property(
                    ValueKind::EffectFloat,
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
    // FX has one symmetric amount; keep native Min Amount the negated Max
    // Amount at every key so an animated amount (e.g. fading to 0) stays symmetric.
    let min_amount = tracks.get("amount").map(|track| NumericTrack {
        keys: track
            .keys
            .iter()
            .map(|key| super::NumericKeyframe {
                values: key.values.iter().map(|value| -value).collect(),
                ..key.clone()
            })
            .collect(),
    });
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
                property(
                    ValueKind::Scalar,
                    &[-selector.amount],
                    None,
                    min_amount.as_ref(),
                )?,
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
                property(ValueKind::VectorEnum, &[grouping], Some((1.0, 4.0)), None)?,
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
    fn native_signed_stroke_delta_preserves_adobe_range() {
        use crate::properties::{data, runs, unique_list};
        use crate::structure::ItemKind;
        use sha2::{Digest, Sha256};

        fn stroke_property(chunks: &[Chunk]) -> Option<&[Chunk]> {
            if let Ok(properties) = runs(chunks)
                && let Some((_, property)) = properties
                    .into_iter()
                    .find(|(name, _)| *name == "ADBE Text Stroke Width")
            {
                return Some(property);
            }
            chunks
                .iter()
                .find_map(|chunk| chunk.children().and_then(stroke_property))
        }

        let source = include_bytes!("../../tests/fixtures/text/stroke-delta/native.aep");
        let readback: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/text/stroke-delta/readback.json"
        ))
        .unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(source)),
            readback["source_sha256"].as_str().unwrap()
        );
        let project = crate::structure::read_project(source).unwrap();
        let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
            panic!("native stroke-delta composition")
        };
        let native = comp
            .layers
            .iter()
            .find_map(|layer| stroke_property(&layer.content))
            .unwrap();
        let native = unique_list(native, *b"tdbs").unwrap();
        let edited = readback["edited"].as_f64().unwrap();
        assert_eq!(
            crate::properties::read_numeric(native).unwrap().values,
            [edited]
        );
        let animator = TextAnimatorSpec {
            value: fx_schema::TextAnimator {
                stroke_width: Some(edited),
                ..Default::default()
            },
            animations: PropertyTracks::default(),
            selectors: Vec::new(),
            wiggly_selectors: Vec::new(),
        };
        let fresh = animator_properties(&animator).unwrap();
        let actual = stroke_property(fresh.children().unwrap()).unwrap();
        let actual = unique_list(actual, *b"tdbs").unwrap();
        assert_eq!(
            crate::properties::read_numeric(actual).unwrap().values,
            [edited]
        );
        // Native UI bounds are implicit in saved AEPs. Independent Adobe getter
        // readback, not our reader, establishes the signed additive range.
        for (tag, field) in [(*b"tdum", "min"), (*b"tduM", "max")] {
            let value = f64::from_be_bytes(data(actual, tag).unwrap().try_into().unwrap());
            assert_eq!(value, readback[field].as_f64().unwrap(), "{field}");
        }
    }

    #[test]
    fn native_range_selector_enums_match_adobe_source() {
        use crate::properties::{data, runs, unique_list};
        use crate::structure::ItemKind;
        use sha2::{Digest, Sha256};

        fn selector_property<'a>(chunks: &'a [Chunk], name: &str) -> Option<&'a [Chunk]> {
            if let Ok(properties) = runs(chunks)
                && let Some((_, property)) = properties.into_iter().find(|(key, _)| *key == name)
            {
                return Some(property);
            }
            chunks.iter().find_map(|chunk| {
                chunk
                    .children()
                    .and_then(|children| selector_property(children, name))
            })
        }

        let source = include_bytes!("../../tests/fixtures/text/selector-enums/source.aep");
        assert_eq!(
            format!("{:x}", Sha256::digest(source)),
            "af930a26ff398e33be705b2677bc977612a4887fd474976ef59513dd6188fa6d"
        );
        let project = crate::structure::read_project(source).unwrap();
        let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
            panic!("native selector composition")
        };
        let native_layer = &comp.layers[0];
        let spec = RangeSelectorSpec {
            value: RangeSelector {
                units: SelectorUnits::Index,
                based_on: SelectorBasis::Lines,
                mode: SelectorMode::Subtract,
                shape: SelectorShape::Triangle,
                ..RangeSelector::default()
            },
            animations: PropertyTracks::default(),
        };
        let thirty_fps = super::super::keyframes::PropertyClock::for_rate(
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        for clock in [super::super::keyframes::PropertyClock::DEFAULT, thirty_fps] {
            let fresh = range_selector(&spec, 1, clock).unwrap();
            for (name, ordinal) in [
                ("ADBE Text Range Units", 2.0),
                ("ADBE Text Range Type2", 4.0),
                ("ADBE Text Selector Mode", 2.0),
                ("ADBE Text Range Shape", 4.0),
            ] {
                let native = selector_property(&native_layer.content, name).unwrap();
                let native = unique_list(native, *b"tdbs").unwrap();
                let actual = selector_property(fresh.children().unwrap(), name).unwrap();
                let actual = unique_list(actual, *b"tdbs").unwrap();
                let mut descriptor = data(native, *b"tdb4").unwrap().to_vec();
                descriptor[12..16].copy_from_slice(&clock.ticks().to_be_bytes());
                assert_eq!(data(actual, *b"tdb4").unwrap(), descriptor, "{name}");
                for tag in [*b"tdsb", *b"cdat"] {
                    assert_eq!(
                        data(actual, tag).unwrap(),
                        data(native, tag).unwrap(),
                        "{name} {tag:?}"
                    );
                }
                assert_eq!(
                    crate::properties::read_numeric(actual).unwrap().values,
                    [ordinal]
                );
            }
        }
    }

    #[test]
    fn native_anchor_grouping_descriptor_matches_adobe_source() {
        use crate::properties::{data, runs, unique_list};
        use crate::structure::ItemKind;
        use sha2::{Digest, Sha256};

        fn anchor_property(chunks: &[Chunk]) -> Option<&[Chunk]> {
            if let Ok(properties) = runs(chunks)
                && let Some((_, property)) = properties
                    .into_iter()
                    .find(|(name, _)| *name == "ADBE Text Anchor Point Option")
            {
                return Some(property);
            }
            chunks
                .iter()
                .find_map(|chunk| chunk.children().and_then(anchor_property))
        }

        // Independently Adobe-authored source pinned in import_sources.json;
        // descriptor equality is storage evidence, not Adobe export acceptance.
        let source = include_bytes!("../../tests/fixtures/text/import_text_path_options.aep");
        assert_eq!(
            format!("{:x}", Sha256::digest(source)),
            "019db8c748e3b306591bdbade1cbb83ed466481abf860c3488db10a0a6d485ec"
        );
        let project = crate::structure::read_project(source).unwrap();
        let thirty_fps = super::super::keyframes::PropertyClock::for_rate(
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        // Character is the native default: composition92 has no stored option.
        // Compare the three explicitly authored enum values, not an absent record.
        for (composition_id, grouping, ordinal) in [
            (107, AnchorPointGrouping::Word, 2.0),
            (122, AnchorPointGrouping::Line, 3.0),
            (137, AnchorPointGrouping::All, 4.0),
        ] {
            let ItemKind::Composition(comp) = &project.item(composition_id).unwrap().kind else {
                panic!("native anchor-grouping composition")
            };
            let native = comp
                .layers
                .iter()
                .find_map(|layer| anchor_property(&layer.content))
                .unwrap();
            let native = unique_list(native, *b"tdbs").unwrap();
            let descriptor = data(native, *b"tdb4").unwrap();
            assert_eq!(descriptor.len(), 124);
            let options = TextAnchorOptionsSpec {
                grouping,
                alignment: [0.0, 0.0],
                animations: PropertyTracks::default(),
            };
            for clock in [super::super::keyframes::PropertyClock::DEFAULT, thirty_fps] {
                let fresh = more_options(Some(&options), clock).unwrap();
                let actual = anchor_property(fresh.children().unwrap()).unwrap();
                let actual = unique_list(actual, *b"tdbs").unwrap();
                let mut expected = descriptor.to_vec();
                // Only the owning composition's property clock may differ.
                expected[12..16].copy_from_slice(&clock.ticks().to_be_bytes());
                assert_eq!(
                    data(actual, *b"tdb4").unwrap(),
                    expected,
                    "composition {composition_id}"
                );
                // Native enum bounds are implicit; compare stored selection/value,
                // not optional min/max records absent from the native source.
                for tag in [*b"tdsb", *b"cdat"] {
                    assert_eq!(
                        data(actual, tag).unwrap(),
                        data(native, tag).unwrap(),
                        "{tag:?}"
                    );
                }
                assert_eq!(
                    crate::properties::read_numeric(actual).unwrap().values,
                    [ordinal]
                );
            }
        }
    }

    #[test]
    fn native_point_empty_path_group_matches_source() {
        use crate::properties::runs;
        use crate::structure::ItemKind;
        use sha2::{Digest, Sha256};

        fn path_group(chunks: &[Chunk]) -> Option<&Chunk> {
            if let Ok(properties) = runs(chunks)
                && let Some((_, property)) = properties
                    .into_iter()
                    .find(|(name, _)| *name == "ADBE Text Path Options")
            {
                return property
                    .iter()
                    .find(|chunk| chunk.list_kind() == Some(*b"tdgp"));
            }
            chunks
                .iter()
                .find_map(|chunk| chunk.children().and_then(path_group))
        }

        // Full native group equality is storage evidence, not Adobe acceptance.
        let source = include_bytes!("../../tests/fixtures/point_text_envelope/native_point_n.aep");
        assert_eq!(
            format!("{:x}", Sha256::digest(source)),
            "5e67c21a5c0b3ce9c7f08f33a27189ef1d5078858e4d22f4716f842afffe2d3e"
        );
        let project = crate::structure::read_project(source).unwrap();
        let native = project
            .items
            .iter()
            .find_map(|item| {
                let ItemKind::Composition(comp) = &item.kind else {
                    return None;
                };
                comp.layers
                    .iter()
                    .find_map(|layer| path_group(&layer.content))
            })
            .unwrap();
        let text = TextSpec {
            name: "Point".into(),
            transform: SolidTransform {
                anchor: [0.0; 2],
                position: [0.0; 2],
                scale: [100.0; 2],
                rotation: 0.0,
                opacity: 100.0,
            },
            transform_animations: TransformAnimations::default(),
            documents: TextDocumentTimeline {
                keyed: false,
                keys: vec![TextDocumentKey {
                    time_millis: 0,
                    document: TextDocumentSpec {
                        text: "N".into(),
                        font_postscript: "Inter-Regular".into(),
                        font_format: None,
                        font_size: 24.0,
                        apply_fill: true,
                        fill_color: [1.0; 4],
                        apply_stroke: false,
                        stroke_color: None,
                        stroke_width: 0.0,
                        stroke_over_fill: true,
                        justification: Justification::Left,
                        tracking: 0.0,
                        leading: None,
                        baseline_shift: 0.0,
                        box_size: None,
                        box_position: None,
                        vertical_align: None,
                        all_caps: false,
                    },
                }],
            },
            animators: vec![],
            anchor_options: None,
            path_options: None,
        };
        let thirty_fps = super::super::keyframes::PropertyClock::for_rate(
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        for clock in [super::super::keyframes::PropertyClock::DEFAULT, thirty_fps] {
            let fresh = text_properties_with_clock(&text, 0, clock).unwrap();
            let mut explicit_default = text.clone();
            explicit_default.anchor_options = Some(TextAnchorOptionsSpec {
                grouping: AnchorPointGrouping::Character,
                alignment: [0.0; 2],
                animations: PropertyTracks::default(),
            });
            assert_eq!(
                text_properties_with_clock(&explicit_default, 0, clock).unwrap(),
                fresh
            );
            let children = fresh.children().unwrap();
            assert_eq!(
                path_group(children).expect("empty Path Options group"),
                native
            );
            assert_eq!(
                runs(children)
                    .unwrap()
                    .iter()
                    .map(|(name, _)| *name)
                    .collect::<Vec<_>>(),
                [
                    "ADBE Text Document",
                    "ADBE Text Path Options",
                    "ADBE Text More Options"
                ]
            );
            let native_rifx =
                crate::rifx::Rifx::parse_with(source, |kind| kind == *b"btdk").unwrap();
            fn more_group(chunk: &Chunk) -> Option<&Chunk> {
                let children = chunk.children()?;
                if let Some(group) = children.windows(2).find_map(|pair| {
                    (pair[0]
                        .data_payload()?
                        .starts_with(b"ADBE Text More Options")
                        && pair[1].list_kind() == Some(*b"tdgp"))
                    .then_some(&pair[1])
                }) {
                    return Some(group);
                }
                children.iter().find_map(more_group)
            }
            let native_more = native_rifx.chunks().iter().find_map(more_group).unwrap();
            assert_eq!(more_group(&fresh).unwrap(), native_more);

            for boxed in [false, true] {
                let mut richer = text.clone();
                if boxed {
                    richer.documents.keys[0].document.box_size = Some([200.0, 100.0]);
                    richer.documents.keys[0].document.box_position = Some([0.0; 2]);
                }
                let baseline = text_properties_with_clock(&richer, 0, clock).unwrap();
                richer.animators.push(TextAnimatorSpec {
                    value: TextAnimator {
                        opacity: Some(50.0),
                        ..TextAnimator::default()
                    },
                    animations: PropertyTracks::default(),
                    selectors: vec![],
                    wiggly_selectors: vec![],
                });
                richer.anchor_options = Some(TextAnchorOptionsSpec {
                    grouping: AnchorPointGrouping::Word,
                    alignment: [10.0, 20.0],
                    animations: PropertyTracks::default(),
                });
                richer.path_options = Some(TextPathOptionsSpec {
                    path_index: 1,
                    first_margin: 12.0,
                    last_margin: 24.0,
                    perpendicular_to_path: true,
                    reverse_path: false,
                    force_alignment: false,
                    animations: PropertyTracks::default(),
                });
                let generated = text_properties_with_clock(&richer, 0, clock).unwrap();
                let before = runs(baseline.children().unwrap()).unwrap();
                let after = runs(generated.children().unwrap()).unwrap();
                assert_eq!(
                    before[0], after[0],
                    "richer controls must retain native Source Text"
                );
                for (name, expected) in [
                    (
                        "ADBE Text Path Options",
                        path_options(richer.path_options.as_ref().unwrap(), clock).unwrap(),
                    ),
                    (
                        "ADBE Text More Options",
                        more_options(richer.anchor_options.as_ref(), clock).unwrap(),
                    ),
                    (
                        "ADBE Text Animators",
                        animators(&richer.animators, clock).unwrap(),
                    ),
                ] {
                    let mut expected = expected;
                    super::super::native_text_controls::normalize(&mut expected, clock).unwrap();
                    let (_, property) = after
                        .iter()
                        .find(|(candidate, _)| *candidate == name)
                        .unwrap();
                    assert!(property.contains(&expected), "missing editable {name}");
                }
            }
        }
    }

    #[test]
    fn native_point_font_changes_follow_semantic_indices() {
        use crate::structure_document::text::cos;

        let first = TextDocumentSpec {
            text: "A😀".into(),
            font_postscript: "ArialMT".into(),
            font_format: None,
            font_size: 48.0,
            apply_fill: true,
            fill_color: [1.0; 4],
            apply_stroke: false,
            stroke_color: None,
            stroke_width: 1.0,
            stroke_over_fill: true,
            justification: Justification::Left,
            tracking: 0.0,
            leading: None,
            baseline_shift: 0.0,
            box_size: None,
            box_position: None,
            vertical_align: None,
            all_caps: false,
        };
        let mut second = first.clone();
        second.text = "B".into();
        second.font_postscript = "TimesNewRomanPSMT".into();
        let text = TextSpec {
            name: "Font holds".into(),
            transform: SolidTransform {
                anchor: [0.0; 2],
                position: [40.0, 90.0],
                scale: [100.0; 2],
                rotation: 0.0,
                opacity: 100.0,
            },
            transform_animations: TransformAnimations::default(),
            documents: TextDocumentTimeline {
                keyed: true,
                keys: vec![
                    TextDocumentKey {
                        time_millis: 0,
                        document: first,
                    },
                    TextDocumentKey {
                        time_millis: 500,
                        document: second,
                    },
                ],
            },
            animators: vec![],
            anchor_options: None,
            path_options: None,
        };
        let clock = super::super::keyframes::PropertyClock::for_rate(
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        let group = text_properties_with_clock(&text, 49, clock).unwrap();
        fn payload(chunk: &Chunk) -> Option<&[u8]> {
            chunk
                .opaque_payload()
                .or_else(|| chunk.children()?.iter().find_map(payload))
        }
        let parsed = cos::parse(payload(&group).unwrap()).unwrap();
        let fonts = parsed
            .get("0")
            .unwrap()
            .get("1")
            .unwrap()
            .get("0")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(fonts.len(), 5);
        for (index, font) in [(0, "ArialMT"), (2, "Helvetica"), (4, "TimesNewRomanPSMT")] {
            assert_eq!(
                fonts[index]
                    .get("0")
                    .unwrap()
                    .get("0")
                    .unwrap()
                    .get("0")
                    .unwrap()
                    .as_str(),
                Some(font)
            );
        }
        let documents = parsed
            .get("1")
            .unwrap()
            .get("1")
            .unwrap()
            .as_array()
            .unwrap();
        for (document, expected_font, units) in [(0, 0, 4), (1, 4, 2)] {
            let runs = documents[document]
                .get("0")
                .unwrap()
                .get("6")
                .unwrap()
                .get("0")
                .unwrap()
                .as_array()
                .unwrap();
            assert_eq!(runs[0].get("1").unwrap().as_i64(), Some(units));
            assert_eq!(
                runs[0]
                    .get("0")
                    .unwrap()
                    .get("0")
                    .unwrap()
                    .get("6")
                    .unwrap()
                    .get("0")
                    .unwrap()
                    .as_i64(),
                Some(expected_font)
            );
        }
    }

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
            font_format: None,
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
            vertical_align: None,
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
    fn keyed_range_float_envelopes_match_independent_native_source() {
        use crate::properties::{data, runs, unique_list};
        use crate::structure::ItemKind;
        use sha2::{Digest, Sha256};

        fn property<'a>(chunks: &'a [Chunk], name: &str) -> Option<&'a [Chunk]> {
            if let Ok(records) = runs(chunks)
                && let Some((_, record)) = records.into_iter().find(|(key, _)| *key == name)
            {
                return Some(record);
            }
            chunks.iter().find_map(|chunk| {
                chunk
                    .children()
                    .and_then(|children| property(children, name))
            })
        }
        let source = include_bytes!("../../tests/fixtures/text/import_selector_keyed_float.aep");
        assert_eq!(
            format!("{:x}", Sha256::digest(source)),
            "8bf318d14b4ef527822e7790cdb7934c13b913dc2f0c454f423a87b4053d9413"
        );
        let project = crate::structure::read_project(source).unwrap();
        let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
            panic!("native composition")
        };
        let clock = super::super::keyframes::PropertyClock::for_rate(
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        let mut animations = PropertyTracks::default();
        for (name, values) in [
            ("start", [0.0, 30.0]),
            ("end", [80.0, 100.0]),
            ("offset", [-20.0, 55.0]),
            ("amount", [25.0, 75.0]),
        ] {
            animations.insert(
                name,
                NumericTrack {
                    keys: values
                        .into_iter()
                        .zip([0, 500])
                        .map(|(value, time_millis)| super::super::NumericKeyframe {
                            time_millis,
                            values: vec![value],
                            easing: vec![super::super::KeyframeEasing::Linear],
                            spatial_in: Vec::new(),
                            spatial_out: Vec::new(),
                        })
                        .collect(),
                },
            );
        }
        let selector = RangeSelectorSpec {
            value: RangeSelector::default(),
            animations,
        };
        let fresh = range_selector(&selector, 1, clock).unwrap();
        for name in [
            "ADBE Text Percent Start",
            "ADBE Text Percent End",
            "ADBE Text Percent Offset",
            "ADBE Text Selector Max Amount",
        ] {
            let native = comp
                .layers
                .iter()
                .find_map(|layer| property(&layer.content, name))
                .unwrap();
            let native = unique_list(native, *b"tdbs").unwrap();
            let actual =
                unique_list(property(fresh.children().unwrap(), name).unwrap(), *b"tdbs").unwrap();
            for tag in [*b"tdsb", *b"tdb4"] {
                assert_eq!(
                    data(actual, tag).unwrap(),
                    data(native, tag).unwrap(),
                    "{name}: {tag:?}"
                );
            }
            let expected = crate::properties::read_numeric(native).unwrap();
            let generated = crate::properties::read_numeric(actual).unwrap();
            assert_eq!(generated.keyframes, expected.keyframes, "{name}");
        }
    }

    #[test]
    fn animated_wiggly_amount_keeps_native_min_and_max_symmetric() {
        use crate::properties::{read_numeric, runs, unique_list};

        let clock = super::super::keyframes::PropertyClock::for_rate(
            crate::timing::FrameRate::new(30.0).unwrap(),
        )
        .unwrap();
        let mut animations = PropertyTracks::default();
        animations.insert(
            "amount",
            NumericTrack {
                keys: [(0, 100.0), (500, 0.0), (1000, 50.0)]
                    .into_iter()
                    .map(|(time_millis, value)| super::super::NumericKeyframe {
                        time_millis,
                        values: vec![value],
                        easing: vec![super::super::KeyframeEasing::Linear],
                        spatial_in: Vec::new(),
                        spatial_out: Vec::new(),
                    })
                    .collect(),
            },
        );
        let selector = WigglySelectorSpec {
            value: WigglySelector {
                amount: 100.0,
                ..WigglySelector::default()
            },
            animations,
        };
        fn property<'a>(chunks: &'a [Chunk], name: &str) -> Option<&'a [Chunk]> {
            if let Ok(records) = runs(chunks)
                && let Some((_, record)) = records.into_iter().find(|(key, _)| *key == name)
            {
                return Some(record);
            }
            chunks.iter().find_map(|chunk| {
                chunk
                    .children()
                    .and_then(|children| property(children, name))
            })
        }
        let group = wiggly_selector(&selector, 1, clock).unwrap();
        let amount = |name: &str| {
            let record = property(std::slice::from_ref(&group), name).unwrap();
            read_numeric(unique_list(record, *b"tdbs").unwrap()).unwrap()
        };
        let (max, min) = (
            amount("ADBE Text Wiggly Max Amount"),
            amount("ADBE Text Wiggly Min Amount"),
        );
        assert_eq!(max.keyframes.len(), 3);
        assert_eq!(min.keyframes.len(), 3);
        for (max, min) in max.keyframes.iter().zip(&min.keyframes) {
            assert_eq!(max.time_secs, min.time_secs);
            assert_eq!(max.values[0], -min.values[0]);
        }
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
