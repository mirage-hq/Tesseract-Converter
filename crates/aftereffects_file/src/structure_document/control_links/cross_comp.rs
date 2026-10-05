//! Strict lookup and clock handling for direct cross-composition Transform aliases.

use std::collections::HashMap;

use crate::{
    properties::{self, NumericProperty, NumericValueKind, PropertyError},
    structure::{Composition, ItemKind, Layer, ProjectItem},
};

use super::{expression, finished, quoted, token, unique_run};

#[derive(Clone, Copy)]
pub(super) struct Context<'a> {
    pub(super) composition_id: u32,
    pub(super) composition: &'a Composition,
    pub(super) items: &'a HashMap<u32, &'a ProjectItem>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Member {
    AnchorPoint,
    Position,
    PositionX,
    PositionY,
    Scale,
    Rotation,
    Opacity,
}

impl Member {
    pub(super) fn from_match_name(name: &str) -> Option<Self> {
        match name {
            "ADBE Anchor Point" => Some(Self::AnchorPoint),
            "ADBE Position" => Some(Self::Position),
            "ADBE Position_0" => Some(Self::PositionX),
            "ADBE Position_1" => Some(Self::PositionY),
            "ADBE Scale" => Some(Self::Scale),
            "ADBE Rotate Z" => Some(Self::Rotation),
            "ADBE Opacity" => Some(Self::Opacity),
            _ => None,
        }
    }

    pub(super) const fn match_name(self) -> &'static str {
        match self {
            Self::AnchorPoint => "ADBE Anchor Point",
            Self::Position => "ADBE Position",
            Self::PositionX => "ADBE Position_0",
            Self::PositionY => "ADBE Position_1",
            Self::Scale => "ADBE Scale",
            Self::Rotation => "ADBE Rotate Z",
            Self::Opacity => "ADBE Opacity",
        }
    }

    const fn expression_name(self) -> &'static str {
        match self {
            Self::AnchorPoint => "anchorPoint",
            Self::Position => "position",
            Self::PositionX => "xPosition",
            Self::PositionY => "yPosition",
            Self::Scale => "scale",
            Self::Rotation => "rotation",
            Self::Opacity => "opacity",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Reference<'a> {
    pub(super) composition: &'a str,
    pub(super) layer: &'a str,
    pub(super) member: Member,
}

#[derive(Clone, Copy)]
pub(super) struct Source<'a> {
    pub(super) context: Context<'a>,
    pub(super) layer: &'a Layer,
}

pub(super) fn same_member_identity(
    layer: &Layer,
    member: Member,
    base: &NumericProperty,
) -> Option<Result<NumericProperty, PropertyError>> {
    let mut text = match property_expression(layer, member) {
        Ok(expression) => expression,
        Err(error) => return Some(Err(error)),
    };
    token(&mut text, "transform")?;
    token(&mut text, ".")?;
    token(&mut text, member.expression_name())?;
    if !finished(text) {
        return None;
    }
    let mut lowered = base.clone();
    lowered.expression_enabled = false;
    lowered.expression_present = false;
    Some(Ok(lowered))
}

pub(super) fn reference<'a>(
    layer: &'a Layer,
    destination: Member,
) -> Option<Result<Reference<'a>, PropertyError>> {
    let expression = match property_expression(layer, destination) {
        Ok(expression) => expression,
        Err(error) => return Some(Err(error)),
    };
    parse(expression).map(|reference| {
        if reference.member != destination {
            Err(PropertyError::Layout(
                "cross-composition Transform alias changes property member",
            ))
        } else {
            Ok(reference)
        }
    })
}

fn parse(mut text: &str) -> Option<Reference<'_>> {
    token(&mut text, "comp")?;
    token(&mut text, "(")?;
    let composition = quoted(&mut text)?;
    if composition.is_empty() {
        return None;
    }
    token(&mut text, ")")?;
    token(&mut text, ".layer")?;
    token(&mut text, "(")?;
    let layer = quoted(&mut text)?;
    if layer.is_empty() {
        return None;
    }
    token(&mut text, ")")?;
    token(&mut text, ".transform.")?;
    let member = [
        Member::AnchorPoint,
        Member::PositionX,
        Member::PositionY,
        Member::Position,
        Member::Scale,
        Member::Rotation,
        Member::Opacity,
    ]
    .into_iter()
    .find(|member| token(&mut text, member.expression_name()).is_some())?;
    finished(text).then_some(Reference {
        composition,
        layer,
        member,
    })
}

pub(super) fn source<'a>(
    context: Context<'a>,
    reference: Reference<'_>,
) -> Result<Source<'a>, PropertyError> {
    let mut compositions = context.items.values().copied().filter(|item| {
        item.name == reference.composition && matches!(item.kind, ItemKind::Composition(_))
    });
    let item = compositions.next().ok_or(PropertyError::Layout(
        "cross-composition Transform alias composition is missing",
    ))?;
    if compositions.next().is_some() {
        return Err(PropertyError::Layout(
            "cross-composition Transform alias composition name is ambiguous",
        ));
    }
    let ItemKind::Composition(composition) = &item.kind else {
        return Err(PropertyError::Layout(
            "cross-composition Transform alias target is not a composition",
        ));
    };
    let mut layers = composition
        .layers
        .iter()
        .filter(|layer| layer.name.as_ref() == reference.layer);
    let layer = layers.next().ok_or(PropertyError::Layout(
        "cross-composition Transform alias layer is missing",
    ))?;
    if layers.next().is_some() {
        return Err(PropertyError::Layout(
            "cross-composition Transform alias layer name is ambiguous",
        ));
    }
    if layer.record.id() == 0 {
        return Err(PropertyError::Layout(
            "cross-composition Transform alias layer identity is invalid",
        ));
    }
    Ok(Source {
        context: Context {
            composition_id: item.id,
            composition,
            items: context.items,
        },
        layer,
    })
}

pub(super) fn validate_anchor_contract(
    owner_context: Context<'_>,
    owner: &Layer,
    source: Source<'_>,
) -> Result<(), PropertyError> {
    let owner_dimensions = source_relative_anchor_dimensions(owner_context, owner)?;
    let source_dimensions = source_relative_anchor_dimensions(source.context, source.layer)?;
    match (owner_dimensions, source_dimensions) {
        (None, None) => Ok(()),
        (Some(owner), Some(source)) if owner == source => Ok(()),
        _ => Err(PropertyError::Layout(
            "cross-composition Anchor Point alias has incompatible source-relative dimensions",
        )),
    }
}

fn source_relative_anchor_dimensions(
    context: Context<'_>,
    layer: &Layer,
) -> Result<Option<[u16; 2]>, PropertyError> {
    let source_id = layer.record.source_id();
    if source_id == 0 {
        return Ok(None);
    }
    let item = context.items.get(&source_id).ok_or(PropertyError::Layout(
        "cross-composition Anchor Point alias source item is missing",
    ))?;
    if matches!(item.solid, Some(Err(_))) {
        return Err(PropertyError::Layout(
            "cross-composition Anchor Point alias source dimensions are unavailable",
        ));
    }
    if !super::super::has_source_relative_anchor(item) {
        return Ok(None);
    }
    let dimensions = super::super::source_anchor_dimensions(Some(item), layer);
    if dimensions.contains(&0) {
        return Err(PropertyError::Layout(
            "cross-composition Anchor Point alias source dimensions are unavailable",
        ));
    }
    Ok(Some(dimensions))
}

pub(super) fn validate_destination(
    property: &NumericProperty,
    dimensions: usize,
) -> Result<(), PropertyError> {
    if !property.expression_enabled || property.dimensions_separated {
        return Err(PropertyError::Layout(
            "cross-composition Transform alias destination has incompatible dimensions or values",
        ));
    }
    // The exact enabled alias supplies the evaluated curve; keyed destination
    // values are authored fallback data, not the expression's result. Still
    // validate their native shape before replacing them with source keys.
    let mut native = property.clone();
    native.expression_enabled = false;
    native.expression_present = false;
    validate_curve(&native, dimensions).map_err(|_| {
        PropertyError::Layout(
            "cross-composition Transform alias destination has incompatible dimensions or values",
        )
    })
}

pub(super) fn rebase(
    mut curve: NumericProperty,
    source: &Layer,
    owner: &Layer,
    dimensions: usize,
) -> Result<NumericProperty, PropertyError> {
    validate_curve(&curve, dimensions)?;
    let (Some(source_start), Some(source_stretch)) =
        (source.record.start_time(), source.record.stretch())
    else {
        return Err(PropertyError::Layout(
            "invalid cross-composition Transform alias source clock",
        ));
    };
    let (Some(owner_start), Some(owner_stretch)) =
        (owner.record.start_time(), owner.record.stretch())
    else {
        return Err(PropertyError::Layout(
            "invalid cross-composition Transform alias destination clock",
        ));
    };
    if !source_start.is_finite()
        || !owner_start.is_finite()
        || !source_stretch.is_finite()
        || !owner_stretch.is_finite()
        || source_stretch <= 0.0
        || owner_stretch <= 0.0
    {
        return Err(PropertyError::Layout(
            "cross-composition Transform alias requires finite positive layer clocks",
        ));
    }
    let scale = source_stretch / owner_stretch;
    let offset = (source_start - owner_start) / owner_stretch;
    if !scale.is_finite() || !offset.is_finite() {
        return Err(PropertyError::Layout(
            "invalid cross-composition Transform alias clock transform",
        ));
    }
    for key in &mut curve.keyframes {
        key.time_secs = offset + key.time_secs * scale;
        for speed in key.in_speed.iter_mut().chain(&mut key.out_speed) {
            *speed /= scale;
        }
        if !key.time_secs.is_finite()
            || !key
                .in_speed
                .iter()
                .chain(&key.out_speed)
                .all(|speed| speed.is_finite())
        {
            return Err(PropertyError::Layout(
                "invalid rebased cross-composition Transform alias key",
            ));
        }
    }
    curve.expression_enabled = false;
    curve.expression_present = false;
    Ok(curve)
}

fn property_expression(layer: &Layer, member: Member) -> Result<&str, PropertyError> {
    let root = properties::root_runs(&layer.content)?;
    let transform = unique_run(&root, "ADBE Transform Group")?;
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp")?)?;
    let body = properties::unique_list(unique_run(&leaves, member.match_name())?, *b"tdbs")?;
    expression(body)
}

fn validate_curve(curve: &NumericProperty, dimensions: usize) -> Result<(), PropertyError> {
    let valid_values = curve.values.is_empty()
        || (curve.values.len() == dimensions && curve.values.iter().all(|value| value.is_finite()));
    let valid_key_order = curve
        .keyframes
        .windows(2)
        .all(|pair| pair[0].time_secs < pair[1].time_secs);
    let valid_keys = curve.keyframes.iter().all(|key| {
        key.time_secs.is_finite()
            && finite_components(&key.values, dimensions, false)
            && finite_components(&key.in_speed, dimensions, false)
            && finite_components(&key.out_speed, dimensions, false)
            && finite_components(&key.in_influence, dimensions, true)
            && finite_components(&key.out_influence, dimensions, true)
            && (key.spatial_in.is_empty() || finite_components(&key.spatial_in, dimensions, false))
            && (key.spatial_out.is_empty()
                || finite_components(&key.spatial_out, dimensions, false))
            && matches!(key.in_interpolation, 1..=3)
            && matches!(key.out_interpolation, 1..=3)
    });
    let valid_interpolation_pairs = curve.keyframes.windows(2).all(|pair| {
        if pair[0].out_interpolation == 3 {
            // Hold is controlled by the outgoing endpoint; incoming ease is unused.
            true
        } else {
            matches!(pair[0].out_interpolation, 1 | 2) && matches!(pair[1].in_interpolation, 1 | 2)
        }
    });
    let valid_shape = if curve.animated {
        !curve.keyframes.is_empty()
    } else {
        curve.values.len() == dimensions && curve.keyframes.is_empty()
    };
    if curve.expression_enabled
        || curve.expression_present
        || curve.dimensions_separated
        || curve.value_kind != NumericValueKind::Continuous
        || !valid_values
        || !valid_key_order
        || !valid_keys
        || !valid_interpolation_pairs
        || !valid_shape
    {
        return Err(PropertyError::Layout(
            "cross-composition Transform alias source is not a finite native curve",
        ));
    }
    Ok(())
}

fn finite_components(values: &[f64], dimensions: usize, influence: bool) -> bool {
    values.len() == dimensions
        && values
            .iter()
            .all(|value| value.is_finite() && (!influence || (0.0..=100.0).contains(value)))
}

#[cfg(test)]
mod tests;
