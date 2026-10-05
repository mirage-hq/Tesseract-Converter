//! Import-only lowering of provable control links; never executes expression code.
//!
//! Supports identical-component Slider Scale vectors, complete Slider references
//! for pixel-unit properties, bounded equal-stretch Angle/Slider offsets for
//! Rotation/X Position, bounded Scale sums, one delayed-Position rig, the
//! Source Text Slider percent binding and pure same-effect scalar aliases.
//! A bounded posterizeTime/wiggle Position profile retains native base keys with
//! explicit jitter/sampling omissions. Other expressions retain captured-sample fallback.
//! A Slider reference names its value parameter or uses index `1`; text
//! animator expressions reuse this grammar in `text::expression_links`.

mod color_control;
mod cross_comp;
mod cross_comp_point;
mod cross_comp_slider;
mod delayed_position;
mod dimension_scale;
mod effect_alias;
mod indexed_position;
mod parent_scale;
mod point_control;
mod position_wiggle_base;
mod property_alias;
mod rotation;
mod rotation_offset_loop;
mod scale_offset;
mod sibling_rotation;
mod slider;

use std::collections::HashMap;

use crate::{
    properties::{self, NumericProperty, NumericValueKind, PropertyError, TransformProperty},
    rifx::Chunk,
    structure::{Composition, Layer, ProjectItem},
};

pub(super) fn read_transform(
    content: &[Chunk],
) -> Result<(Vec<TransformProperty>, Vec<String>), PropertyError> {
    let mut properties = properties::read_transform(content)?;
    let mut warnings = Vec::new();
    for property in &mut properties {
        if property.match_name != "ADBE Scale"
            || !property
                .numeric
                .as_ref()
                .is_ok_and(|value| value.expression_enabled)
        {
            continue;
        }
        match linked_scale(content) {
            Ok(scale) => {
                property.numeric = Ok(scale);
                warnings.push("ADBE Scale: pure same-layer Slider aliases lowered to independent editable Scale values/keys; original controller linkage is not retained".into());
            }
            Err(error) => warnings.push(format!(
                "ADBE Scale: control link not lowered ({error}); original expression retained"
            )),
        }
    }
    Ok((properties, warnings))
}

// Import-only component markers; never emitted as native AEP property names.
pub(super) const SCALE_X: &str = "__aep_import_scale_x";
pub(super) const SCALE_Y: &str = "__aep_import_scale_y";

pub(super) fn lower_sibling_rotation_expression(
    layer: &Layer,
    composition: &Composition,
    property: &mut NumericProperty,
    text: &str,
) -> Option<Result<(), PropertyError>> {
    sibling_rotation::lower_text(layer, composition, property, text)
}

/// Resolves an Effect Parade control whose enabled expression is exactly a
/// reference to another control of its own effect occurrence. Returns the
/// referenced control's match name; `None` means the expression is not such a
/// candidate and ordinary expression handling applies unchanged.
pub(super) fn resolve_same_effect_alias(
    content: &[Chunk],
    effect_index: usize,
    parameter: &str,
) -> Option<Result<String, PropertyError>> {
    effect_alias::resolve(content, effect_index, parameter)
}

/// Copies a static same-composition Point Control into independent pixel coordinates.
/// Only proven planar Shape controller storage is admitted.
pub(super) fn lower_static_point_control(
    property: &[Chunk],
    composition: &Composition,
    consumer_id: u32,
) -> Result<[f64; 2], PropertyError> {
    point_control::resolve(property, composition, consumer_id)
}

/// Copies one complete static Color Control alias into an independent editable color.
/// Animated controls and expressions on the referenced control are not evaluated.
pub(super) fn lower_static_color_control(
    property: &[Chunk],
    composition: &Composition,
    source_items: Option<&HashMap<u32, &ProjectItem>>,
) -> Result<[f64; 4], PropertyError> {
    color_control::resolve(property, composition, source_items)
}

pub(super) fn lower_slider_scalar(
    layer: &Layer,
    composition: &Composition,
    property: &[Chunk],
) -> Result<NumericProperty, PropertyError> {
    slider::lower_signed_scalar(layer, composition, expression(property)?)
}

pub(super) fn lower_slider_repeated_vector(
    layer: &Layer,
    composition: &Composition,
    property: &[Chunk],
) -> Result<NumericProperty, PropertyError> {
    slider::lower_repeated_vector(layer, composition, expression(property)?)
}

/// Resolves the owner's own Slider behind a complete Source Text percent
/// expression, `s = effect("…")("…"); Math.round(s).toLocaleString() + "%";`,
/// through pure same-layer aliases only. Formatting its values is the caller's.
pub(super) fn slider_percent(
    layer: &Layer,
    property: &[Chunk],
) -> Result<NumericProperty, PropertyError> {
    let reference = slider::percent_reference(expression(property)?).ok_or(
        PropertyError::Layout("not a complete Slider percent expression"),
    )?;
    let root = properties::root_runs(&layer.content)?;
    let parade = unique_run(&root, "ADBE Effect Parade")?;
    let effects = properties::runs(properties::unique_list(parade, *b"tdgp")?)?;
    resolve(&effects, reference)
}

/// Resolves a property whose enabled expression is exactly one complete
/// same-layer Slider reference, `effect("…")("…")`, or for a vector an array of
/// one such reference per component. Each reference follows the Scale links'
/// local aliases and at most one same-clock sibling hop. The curves keep the
/// Slider's own units: the Scale percentage conversion is not applied.
pub(super) fn slider_components(
    layer: &Layer,
    composition: &Composition,
    property: &[Chunk],
    dimensions: usize,
) -> Result<Vec<NumericProperty>, PropertyError> {
    let mut text = expression(property)?;
    let references = if dimensions == 1 {
        reference(&mut text)
            .filter(|_| finished(text))
            .map(|reference| vec![reference])
    } else {
        vector_references(text).filter(|references| references.len() == dimensions)
    }
    .ok_or(PropertyError::Layout(
        "not a complete Slider reference expression",
    ))?;
    let root = properties::root_runs(&layer.content)?;
    let parade = unique_run(&root, "ADBE Effect Parade")?;
    let effects = properties::runs(properties::unique_list(parade, *b"tdgp")?)?;
    references
        .into_iter()
        .map(|reference| resolve_scoped(&effects, reference, Some((layer, composition))))
        .collect()
}

pub(super) fn read_layer_transform(
    layer: &Layer,
    composition: &Composition,
) -> Result<(Vec<TransformProperty>, Vec<String>), PropertyError> {
    read_layer_transform_inner(layer, composition, None, &mut Vec::new(), None)
}

pub(super) fn read_layer_transform_with_sources(
    layer: &Layer,
    composition_id: u32,
    composition: &Composition,
    items: &HashMap<u32, &ProjectItem>,
) -> Result<(Vec<TransformProperty>, Vec<String>), PropertyError> {
    read_layer_transform_inner(
        layer,
        composition,
        Some(cross_comp::Context {
            composition_id,
            composition,
            items,
        }),
        &mut Vec::new(),
        None,
    )
}

fn read_layer_transform_inner<'a>(
    layer: &'a Layer,
    composition: &'a Composition,
    source_context: Option<cross_comp::Context<'a>>,
    visited: &mut Vec<(u32, u32, cross_comp::Member)>,
    only_member: Option<cross_comp::Member>,
) -> Result<(Vec<TransformProperty>, Vec<String>), PropertyError> {
    let (mut properties, mut warnings) = read_transform(&layer.content)?;
    // Foreign member aliases can sample beyond the source composition's
    // interval. Do not advertise a finite prepared loop as a live alias curve.
    if only_member.is_none()
        && let Some(property) = properties.iter_mut().find(|property| {
            property.match_name == "ADBE Rotate Z"
                && property
                    .numeric
                    .as_ref()
                    .is_ok_and(|value| value.expression_enabled)
        })
        && let Ok(base) = &property.numeric
        && let Some(result) = rotation_offset_loop::lower(layer, composition, base)
    {
        match result {
            Ok(value) => {
                property.numeric = Ok(value);
                warnings.retain(|warning| {
                    !warning.starts_with("ADBE Rotate Z: scalar offset link not lowered")
                });
                warnings.push("ADBE Rotate Z: exact two-key Linear offset loop lowered to editable Linear keys through the composition interval; native clock rounding remains, and later FX key edits do not retain a live loop expression".into());
            }
            Err(error) => warnings.push(format!(
                "ADBE Rotate Z: offset loop not lowered ({error}); original expression retained"
            )),
        }
    }
    let mut axes = Vec::new();
    if only_member.is_none_or(|member| member == cross_comp::Member::Scale)
        && let Some(property) = properties.iter_mut().find(|property| {
            property.match_name == "ADBE Scale"
                && property
                    .numeric
                    .as_ref()
                    .is_ok_and(|value| value.expression_enabled)
        })
        && let Some(result) = dimension_scale::lower(layer, composition)
    {
        match result {
            Ok(value) => {
                property.numeric = Ok(value);
                warnings.retain(|warning| !warning.starts_with("ADBE Scale: control link not lowered"));
                warnings.push("ADBE Scale: bounded composition-dimension expression lowered to independent editable static values; live composition-resize linkage is not retained".into());
            }
            Err(error) => warnings.push(format!("ADBE Scale: composition-dimension expression not lowered ({error}); original expression retained")),
        }
    }
    if let Some(context) = source_context {
        for property in &mut properties {
            let Some(member) = cross_comp::Member::from_match_name(&property.match_name) else {
                continue;
            };
            if only_member.is_some_and(|requested| requested != member) {
                continue;
            }
            let Ok(base) = &property.numeric else {
                continue;
            };
            if !base.expression_enabled {
                continue;
            }
            if member == cross_comp::Member::Position
                && let Some(result) = cross_comp_point::lower(layer, context, base)
            {
                match result {
                    Ok(value) => {
                        property.numeric = Ok(value);
                        warnings.push("ADBE Position: static native Point2D binding lowered to independent editable Position3D using the user-approved Z=0 policy, not established Adobe coercion; live controller linkage is not retained".into());
                    }
                    Err(error) => warnings.push(format!(
                        "ADBE Position: static Point2D-to-Position3D policy binding not lowered ({error}); original expression retained"
                    )),
                }
                continue;
            }
            if member == cross_comp::Member::Scale
                && let Some(result) = cross_comp_slider::lower(layer, context, base)
            {
                match result {
                    Ok(value) => {
                        property.numeric = Ok(value);
                        warnings.retain(|warning| !warning.starts_with("ADBE Scale: control link not lowered"));
                        warnings.push("ADBE Scale: bounded static cross-composition Slider repeated XY binding lowered to independent editable values; live controller linkage and native Z Scale semantics are not retained".into());
                    }
                    Err(error) => warnings.push(format!("ADBE Scale: static cross-composition Slider binding not lowered ({error}); original expression retained")),
                }
                continue;
            }
            if let Some(result) = cross_comp::same_member_identity(layer, member, base) {
                match result {
                    Ok(value) => {
                        property.numeric = Ok(value);
                        warnings.push(format!(
                            "{}: exact same-member Transform identity lowered to native editable values/keys",
                            property.match_name
                        ));
                    }
                    Err(error) => warnings.push(format!(
                        "{}: same-member Transform identity not lowered ({error}); original expression retained",
                        property.match_name
                    )),
                }
                continue;
            }
            let Some(reference) = cross_comp::reference(layer, member) else {
                continue;
            };
            let lowered = reference.and_then(|reference| {
                lower_cross_comp_alias(layer, context, member, reference, base, visited)
            });
            match lowered {
                Ok(lowered) => {
                    property.numeric = Ok(lowered.numeric);
                    axes.extend(lowered.axes);
                    warnings.extend(lowered.warnings.into_iter().map(|warning| {
                        format!(
                            "{}: cross-composition source diagnostic: {warning}",
                            property.match_name
                        )
                    }));
                    if member == cross_comp::Member::Scale {
                        warnings.retain(|warning| {
                            !warning.starts_with("ADBE Scale: control link not lowered")
                        });
                    }
                    warnings.push(format!(
                        "{}: bounded direct cross-composition Transform alias lowered to independent editable values/keys; live linkage is not retained",
                        property.match_name
                    ));
                }
                Err(error) => warnings.push(format!(
                    "{}: cross-composition Transform alias not lowered ({error}); original expression retained",
                    property.match_name
                )),
            }
        }
    }
    if only_member.is_none_or(|member| member == cross_comp::Member::Scale)
        && let Some(property) = properties.iter_mut().find(|property| {
            property.match_name == "ADBE Scale"
                && property
                    .numeric
                    .as_ref()
                    .is_ok_and(|numeric| numeric.expression_enabled)
        })
    {
        let base = property
            .numeric
            .as_ref()
            .expect("filtered successful Scale property");
        if let Ok(value) = scale_offset::lower(layer, base) {
            property.numeric = Ok(value);
            warnings.retain(|warning| !warning.starts_with("ADBE Scale: control link not lowered"));
            warnings.push("ADBE Scale: bounded same-layer Slider addition lowered to independent editable keys; two animated inputs use a sparse approximation checked within 0.01 percentage points against the analytical model on a 1ms grid before FX clock quantization; controller edit linkage is not retained".into());
        }
    }
    if only_member.is_none_or(|member| member == cross_comp::Member::Scale)
        && let Some(property) = properties.iter_mut().find(|property| {
            property.match_name == "ADBE Scale"
                && property
                    .numeric
                    .as_ref()
                    .is_ok_and(|numeric| numeric.expression_enabled)
        })
    {
        let base = property
            .numeric
            .as_ref()
            .expect("filtered successful Scale property");
        let (resolved, binding) =
            if let Some(result) = parent_scale::lower(layer, composition, base) {
                (
                    result.map(|lowered| {
                        warnings.extend(lowered.warnings);
                        lowered.axes
                    }),
                    None,
                )
            } else {
                (split_scale(layer, composition), Some("Slider components"))
            };
        match resolved {
            Ok(values) => {
                property.numeric = Ok(NumericProperty {
                    values: vec![1.0, 1.0],
                    animated: false,
                    expression_enabled: false,
                    expression_present: false,
                    dimensions_separated: false,
                    keyframes: Vec::new(),
                    value_kind: NumericValueKind::Continuous,
                });
                axes.extend(
                    [SCALE_X, SCALE_Y]
                        .into_iter()
                        .zip(values)
                        .map(|(name, numeric)| TransformProperty {
                            match_name: name.into(),
                            numeric: Ok(numeric),
                        }),
                );
                warnings.retain(|w| !w.starts_with("ADBE Scale: control link not lowered"));
                if let Some(binding) = binding {
                    warnings.push(format!("ADBE Scale: {binding} lowered to independent editable X/Y values/keys; controller edit linkage is not retained"));
                }
            }
            Err(error) => warnings.push(format!(
                "ADBE Scale: {} not lowered ({error}); original expression retained",
                binding.unwrap_or("parent Scale cancellation")
            )),
        }
    }
    properties.extend(axes);
    if only_member.is_none_or(|member| member == cross_comp::Member::Position)
        && let Some(property) = properties.iter_mut().find(|property| {
            property.match_name == "ADBE Position"
                && property.numeric.as_ref().is_ok_and(|numeric| {
                    numeric.expression_enabled && numeric.animated && !numeric.keyframes.is_empty()
                })
        })
        && position_wiggle_base::source(layer).is_ok_and(position_wiggle_base::recognized)
        && let Ok(numeric) = &mut property.numeric
    {
        numeric.expression_enabled = false;
        numeric.expression_present = false;
        warnings.push("ADBE Position: native base Position keys retained as an editable approximation for a bounded posterizeTime/wiggle expression; jitter and posterized sampling omitted, native expression fidelity is not established".into());
    }
    if only_member.is_none_or(|member| member == cross_comp::Member::Position)
        && let Some(property) = properties.iter_mut().find(|property| {
            property.match_name == "ADBE Position"
                && property
                    .numeric
                    .as_ref()
                    .is_ok_and(|numeric| numeric.expression_enabled)
        })
    {
        let base = property
            .numeric
            .as_ref()
            .expect("filtered successful Position property");
        if let Some(result) = indexed_position::lower(layer, composition, base) {
            match result {
                Ok(value) => {
                    property.numeric = Ok(value);
                    warnings.push("ADBE Position: bounded native index times static Slider expression lowered to independent editable Position; layer-order/controller edit linkage is not retained".into());
                }
                Err(error) => warnings.push(format!(
                    "ADBE Position: indexed static Slider expression not lowered ({error}); original expression retained"
                )),
            }
        } else if let Some(result) = delayed_position::lower(layer, composition, base) {
            match result {
                Ok(value) => {
                    property.numeric = Ok(value);
                    warnings.push("ADBE Position: delayed Master rig approximated with sparse editable native keys; the analytical model uses native clocks/easing and a compatible easeOut approximation; fit error is checked within 0.01 pixel on a 1ms grid before FX clock quantization, not against Adobe output; live controller linkage is not retained".into());
                }
                Err(error) => warnings.push(format!(
                    "ADBE Position: delayed Master rig not lowered ({error}); original expression retained"
                )),
            }
        }
    }
    for property in &mut properties {
        let name = property.match_name.as_str();
        if !matches!(
            name,
            "ADBE Rotate Z" | "ADBE Position_0" | "ADBE Position_1"
        ) || only_member.is_some_and(|member| member.match_name() != name)
        {
            continue;
        }
        let Ok(base) = &property.numeric else {
            continue;
        };
        if !base.expression_enabled {
            continue;
        }
        if let Some(result) = property_alias::lower(layer, composition, name, base) {
            match result {
                Ok(value) => {
                    property.numeric = Ok(value);
                    warnings.push(format!("{name}: bounded signed sibling property alias lowered to independent editable values/keys; live linkage is not retained"));
                }
                Err(error) => warnings.push(format!(
                    "{name}: property alias not lowered ({error}); original expression retained"
                )),
            }
            continue;
        }
        if name == "ADBE Position_1" {
            continue;
        }
        let (resolved, binding) = if name == "ADBE Rotate Z" {
            let mut sibling = base.clone();
            match sibling_rotation::lower(layer, composition, &mut sibling) {
                Some(result) => (result.map(|()| sibling), "sibling Rotation offset"),
                None => (rotation::lower(layer, composition, base), "scalar offset"),
            }
        } else {
            rotation::lower_position_x(layer, composition, base)
        };
        match resolved {
            Ok(value) => {
                property.numeric = Ok(value);
                warnings.push(format!("{name}: equal-stretch {binding} lowered to independent editable values/keys; controller edit linkage is not retained"));
            }
            Err(error) => warnings.push(format!(
                "{name}: {binding} link not lowered ({error}); original expression retained"
            )),
        }
    }
    Ok((properties, warnings))
}

struct CrossCompLowering {
    numeric: NumericProperty,
    axes: Vec<TransformProperty>,
    warnings: Vec<String>,
}

fn lower_cross_comp_alias<'a>(
    owner: &'a Layer,
    context: cross_comp::Context<'a>,
    member: cross_comp::Member,
    reference: cross_comp::Reference<'_>,
    destination: &NumericProperty,
    visited: &mut Vec<(u32, u32, cross_comp::Member)>,
) -> Result<CrossCompLowering, PropertyError> {
    let owner_id = owner.record.id();
    if owner_id == 0
        || context
            .composition
            .layers
            .iter()
            .filter(|layer| layer.record.id() == owner_id)
            .count()
            != 1
    {
        return Err(PropertyError::Layout(
            "cross-composition Transform alias requires a unique destination layer identity",
        ));
    }
    let identity = (context.composition_id, owner_id, member);
    if visited.contains(&identity) {
        return Err(PropertyError::Layout(
            "cyclic cross-composition Transform alias",
        ));
    }
    if visited.len() >= 16 {
        return Err(PropertyError::Layout(
            "cross-composition Transform alias exceeds the bounded depth",
        ));
    }
    visited.push(identity);
    let result = (|| {
        let dimensions = cross_comp_dimensions(member, destination)?;
        cross_comp::validate_destination(destination, dimensions)?;
        let source = cross_comp::source(context, reference)?;
        if member == cross_comp::Member::AnchorPoint {
            cross_comp::validate_anchor_contract(context, owner, source)?;
        }
        let source_identity = (
            source.context.composition_id,
            source.layer.record.id(),
            reference.member,
        );
        if visited.contains(&source_identity) {
            return Err(PropertyError::Layout(
                "cyclic cross-composition Transform alias",
            ));
        }
        let (mut source_properties, source_warnings) = read_layer_transform_inner(
            source.layer,
            source.context.composition,
            Some(source.context),
            visited,
            Some(member),
        )?;
        if source_warnings
            .iter()
            .any(|warning| warning.contains("cyclic cross-composition Transform alias"))
        {
            return Err(PropertyError::Layout(
                "cyclic cross-composition Transform alias",
            ));
        }
        let mut source_warnings = source_warnings
            .into_iter()
            .filter(|warning| warning.starts_with(member.match_name()))
            .collect::<Vec<_>>();
        // Match the existing occurrence defaults only after the source group
        // parsed successfully. A malformed leaf is not an absent property.
        let source_default = match member {
            cross_comp::Member::Rotation => Some(vec![0.0]),
            cross_comp::Member::Position
                if source.layer.record.layer_type() == 4
                    && !source.layer.record.flags().three_d_layer
                    && !source.layer.record.flags().null_layer =>
            {
                let mut value = vec![0.0; dimensions];
                value[0] = f64::from(source.context.composition.width) / 2.0;
                value[1] = f64::from(source.context.composition.height) / 2.0;
                Some(value)
            }
            _ => None,
        };
        if !source_properties
            .iter()
            .any(|property| property.match_name == member.match_name())
            && let Some(values) = source_default
        {
            if member == cross_comp::Member::Position {
                source_warnings.push("ADBE Position: absent source Shape Position uses the same source-composition-center default as its occurrence; this is not captured expression/control evidence".into());
            }
            source_properties.push(TransformProperty {
                match_name: member.match_name().into(),
                numeric: Ok(NumericProperty {
                    values,
                    animated: false,
                    expression_enabled: false,
                    expression_present: false,
                    dimensions_separated: false,
                    keyframes: Vec::new(),
                    value_kind: NumericValueKind::Continuous,
                }),
            });
        }
        let source_property = source_properties
            .iter()
            .find(|property| property.match_name == member.match_name())
            .ok_or(PropertyError::Layout(
                "cross-composition Transform alias source property is missing",
            ))?;
        let source_numeric = source_property.numeric.as_ref().map_err(Clone::clone)?;

        let mut lowered_axes = Vec::new();
        if member == cross_comp::Member::Scale {
            let source_x = source_properties
                .iter()
                .find(|property| property.match_name == SCALE_X);
            let source_y = source_properties
                .iter()
                .find(|property| property.match_name == SCALE_Y);
            match (source_x, source_y) {
                (Some(source_x), Some(source_y)) => {
                    for (name, source_axis) in [(SCALE_X, source_x), (SCALE_Y, source_y)] {
                        let numeric = source_axis.numeric.as_ref().map_err(Clone::clone)?;
                        lowered_axes.push(TransformProperty {
                            match_name: name.into(),
                            numeric: Ok(cross_comp::rebase(
                                numeric.clone(),
                                source.layer,
                                owner,
                                1,
                            )?),
                        });
                    }
                    let mut numeric = destination.clone();
                    numeric.values = vec![1.0; dimensions];
                    numeric.animated = false;
                    numeric.keyframes.clear();
                    numeric.expression_enabled = false;
                    numeric.expression_present = false;
                    return Ok(CrossCompLowering {
                        numeric,
                        axes: lowered_axes,
                        warnings: source_warnings,
                    });
                }
                (None, None) => {}
                _ => {
                    return Err(PropertyError::Layout(
                        "cross-composition Scale alias source has incomplete independent axes",
                    ));
                }
            }
        }

        let rebase_dimensions = if member == cross_comp::Member::Scale {
            let source_dimensions = numeric_dimensions(source_numeric)?;
            if !matches!(source_dimensions, 2 | 3) {
                return Err(PropertyError::Layout(
                    "cross-composition Scale alias source dimensions are unsupported",
                ));
            }
            if source_dimensions != dimensions {
                source_warnings.push(format!(
                    "ADBE Scale: native {source_dimensions}D source mapped to {dimensions}D destination; editable FX preserves X/Y and has no destination Z Scale field"
                ));
            }
            source_dimensions
        } else {
            dimensions
        };
        Ok(CrossCompLowering {
            numeric: cross_comp::rebase(
                source_numeric.clone(),
                source.layer,
                owner,
                rebase_dimensions,
            )?,
            axes: lowered_axes,
            warnings: source_warnings,
        })
    })();
    let _ = visited.pop();
    result
}

fn cross_comp_dimensions(
    member: cross_comp::Member,
    destination: &NumericProperty,
) -> Result<usize, PropertyError> {
    let dimensions = numeric_dimensions(destination)?;
    let valid = match member {
        cross_comp::Member::AnchorPoint
        | cross_comp::Member::Position
        | cross_comp::Member::Scale => matches!(dimensions, 2 | 3),
        cross_comp::Member::PositionX
        | cross_comp::Member::PositionY
        | cross_comp::Member::Rotation
        | cross_comp::Member::Opacity => dimensions == 1,
    };
    if !valid {
        return Err(PropertyError::Layout(
            "cross-composition Transform alias destination dimensions are unsupported",
        ));
    }
    Ok(dimensions)
}

fn numeric_dimensions(property: &NumericProperty) -> Result<usize, PropertyError> {
    if property.animated {
        property
            .keyframes
            .first()
            .map(|key| key.values.len())
            .ok_or(PropertyError::Layout(
                "cross-composition Transform alias animated source has no keys",
            ))
    } else {
        Ok(property.values.len())
    }
}

fn split_scale(
    layer: &Layer,
    composition: &Composition,
) -> Result<[NumericProperty; 2], PropertyError> {
    let root = properties::root_runs(&layer.content)?;
    let transform = unique_run(&root, "ADBE Transform Group")?;
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp")?)?;
    let scale = properties::unique_list(unique_run(&leaves, "ADBE Scale")?, *b"tdbs")?;
    let references = vector_references(expression(scale)?)
        .ok_or(PropertyError::Layout("not a direct Slider vector"))?;
    if references.len() != 2 {
        return Err(PropertyError::Layout(
            "independent Scale requires exactly two components",
        ));
    }
    let parade = unique_run(&root, "ADBE Effect Parade")?;
    let effects = properties::runs(properties::unique_list(parade, *b"tdgp")?)?;
    let x = resolve_scoped(&effects, references[0], Some((layer, composition)))?;
    let y = resolve_scoped(&effects, references[1], Some((layer, composition)))?;
    Ok([scale_vector(x, 1)?, scale_vector(y, 1)?])
}

fn linked_scale(content: &[Chunk]) -> Result<NumericProperty, PropertyError> {
    let root = properties::root_runs(content)?;
    let transform = unique_run(&root, "ADBE Transform Group")?;
    let leaves = properties::runs(properties::unique_list(transform, *b"tdgp")?)?;
    let scale = properties::unique_list(unique_run(&leaves, "ADBE Scale")?, *b"tdbs")?;
    let references = vector_references(expression(scale)?)
        .ok_or(PropertyError::Layout("not a direct Slider vector"))?;
    let parade = unique_run(&root, "ADBE Effect Parade")?;
    let effects = properties::runs(properties::unique_list(parade, *b"tdgp")?)?;
    let scalar = resolve(&effects, references[0])?;
    for reference in references.iter().skip(1) {
        if resolve(&effects, *reference)? != scalar {
            return Err(PropertyError::Layout(
                "independent component curves are not supported",
            ));
        }
    }
    scale_vector(scalar, references.len())
}

pub(super) fn unique_run<'a>(
    runs: &[(&str, &'a [Chunk])],
    name: &str,
) -> Result<&'a [Chunk], PropertyError> {
    let mut matches = runs.iter().filter(|(candidate, _)| *candidate == name);
    let (_, result) = matches
        .next()
        .ok_or(PropertyError::Layout("missing named control"))?;
    if matches.next().is_some() {
        return Err(PropertyError::Layout("ambiguous named control"));
    }
    Ok(result)
}

pub(super) fn display_name(chunks: &[Chunk]) -> Option<&str> {
    utf8_name(properties::data(chunks, *b"tdsn").ok()?)
}

/// The display name AE stores for an effect that keeps its default name.
const DEFAULT_EFFECT_NAME: &str = "-_0_/-";

/// The name `effect("…")` selects: the effect's display name, or the default
/// name in its plugin descriptor's `fnam` when AE stores the default-name
/// placeholder instead. A custom display name is never replaced.
pub(super) fn effect_name<'a>(descriptor: &'a [Chunk], controls: &'a [Chunk]) -> Option<&'a str> {
    match display_name(controls)? {
        DEFAULT_EFFECT_NAME => utf8_name(properties::data(descriptor, *b"fnam").ok()?),
        name => Some(name),
    }
}

/// A native `Utf8` name: length-prefixed UTF-8 and at most three zero pad bytes.
fn utf8_name(bytes: &[u8]) -> Option<&str> {
    if bytes.get(..4)? != b"Utf8" {
        return None;
    }
    let length = usize::try_from(u32::from_be_bytes(bytes.get(4..8)?.try_into().ok()?)).ok()?;
    let end = 8_usize.checked_add(length)?;
    let padding = bytes.get(end..)?;
    if padding.len() > 3 || padding.iter().any(|byte| *byte != 0) {
        return None;
    }
    std::str::from_utf8(bytes.get(8..end)?).ok()
}

/// The name AE expressions use for one Effect Parade occurrence: an explicit
/// instance rename in its controls' `tdsn`, or the plugin display name (`fnam`)
/// while the instance keeps AE's `-_0_/-` placeholder.
pub(super) fn effect_instance_name<'a>(
    descriptor: &'a [Chunk],
    controls: &'a [Chunk],
) -> Option<&'a str> {
    if controls.iter().any(|chunk| chunk.id() == *b"tdsn") {
        let name = display_name(controls)?;
        if name != "-_0_/-" {
            return Some(name);
        }
    }
    let bytes = properties::data(descriptor, *b"fnam").ok()?;
    if bytes.get(..4)? != b"Utf8" {
        return None;
    }
    let length = usize::try_from(u32::from_be_bytes(bytes.get(4..8)?.try_into().ok()?)).ok()?;
    let end = 8_usize.checked_add(length)?;
    let padding = bytes.get(end..)?;
    if padding.len() > 3 || padding.iter().any(|byte| *byte != 0) {
        return None;
    }
    std::str::from_utf8(bytes.get(8..end)?).ok()
}

pub(super) fn expression(chunks: &[Chunk]) -> Result<&str, PropertyError> {
    std::str::from_utf8(properties::data(chunks, *b"Utf8")?)
        .map_err(|_| PropertyError::Layout("invalid expression text"))
}

/// One `effect(name)(parameter)` reference to a same-layer effect control.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Reference<'a> {
    effect: &'a str,
    parameter: &'a str,
}

impl Reference<'_> {
    /// A built-in Slider value selector, even when AE omitted its default record.
    pub(super) fn selects_slider_value(self) -> bool {
        matches!(self.parameter, SLIDER_VALUE | "Slider")
    }
}

/// The Slider value's match name; `effect(name)(1)` selects it by index.
const SLIDER_VALUE: &str = "ADBE Slider Control-0001";

pub(super) fn token(text: &mut &str, expected: &str) -> Option<()> {
    *text = text.trim_start().strip_prefix(expected)?;
    Some(())
}

/// An ASCII JS identifier; `$` is accepted only as its first character.
pub(super) fn identifier<'a>(text: &mut &'a str) -> Option<&'a str> {
    *text = text.trim_start();
    let end = text
        .char_indices()
        .take_while(|(index, value)| {
            value.is_ascii_alphanumeric() || *value == '_' || (*index == 0 && *value == '$')
        })
        .map(|(index, value)| index + value.len_utf8())
        .last()?;
    let value = &text[..end];
    if value
        .chars()
        .next()
        .is_none_or(|value| !(value.is_ascii_alphabetic() || matches!(value, '_' | '$')))
    {
        return None;
    }
    *text = &text[end..];
    Some(value)
}

/// A finite optionally signed decimal literal with optional fraction and exponent.
pub(super) fn finite_number(text: &mut &str) -> Option<f64> {
    *text = text.trim_start();
    let bytes = text.as_bytes();
    let mut end = usize::from(matches!(bytes.first().copied(), Some(b'+' | b'-')));
    let integer_start = end;
    while bytes.get(end).is_some_and(|byte| byte.is_ascii_digit()) {
        end += 1;
    }
    let mut has_digits = end != integer_start;
    if bytes.get(end) == Some(&b'.') {
        end += 1;
        let fraction_start = end;
        while bytes.get(end).is_some_and(|byte| byte.is_ascii_digit()) {
            end += 1;
        }
        has_digits |= end != fraction_start;
    }
    if !has_digits {
        return None;
    }
    if matches!(bytes.get(end).copied(), Some(b'e' | b'E')) {
        end += 1;
        if matches!(bytes.get(end).copied(), Some(b'+' | b'-')) {
            end += 1;
        }
        let exponent_start = end;
        while bytes.get(end).is_some_and(|byte| byte.is_ascii_digit()) {
            end += 1;
        }
        if end == exponent_start {
            return None;
        }
    }
    let value = text.get(..end)?.parse::<f64>().ok()?;
    if !value.is_finite() {
        return None;
    }
    *text = &text[end..];
    Some(value)
}

pub(super) fn quoted<'a>(text: &mut &'a str) -> Option<&'a str> {
    *text = text.trim_start();
    let quote = text.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
    let remaining = text.get(1..)?;
    let end = remaining.find(quote)?;
    let value = &remaining[..end];
    // Escapes, line continuations and arbitrary JS syntax are deliberately not evaluated.
    if value.contains(['\\', '\n', '\r']) {
        return None;
    }
    *text = &remaining[end + 1..];
    Some(value)
}

/// `effect(name)(parameter)` with a quoted parameter or the index `1`.
///
/// Resolution still requires an `ADBE Slider Control` and its value
/// parameter, so index 1 never selects the hidden `-0000` control or the
/// Compositing Options group. Controls of other effects use
/// `named_reference`: their index 1 is not a Slider value.
pub(super) fn reference<'a>(text: &mut &'a str) -> Option<Reference<'a>> {
    reference_with(text, |text| {
        if token(text, "1").is_some() {
            Some(SLIDER_VALUE)
        } else {
            quoted(text)
        }
    })
}

/// `effect(name)(parameter)` whose parameter is a quoted match or display name.
fn named_reference<'a>(text: &mut &'a str) -> Option<Reference<'a>> {
    reference_with(text, quoted)
}

/// `effect(name)(parameter)`, with the parameter syntax read by `parameter`.
fn reference_with<'a>(
    text: &mut &'a str,
    parameter: impl FnOnce(&mut &'a str) -> Option<&'a str>,
) -> Option<Reference<'a>> {
    token(text, "effect")?;
    token(text, "(")?;
    let effect = quoted(text)?;
    token(text, ")")?;
    token(text, "(")?;
    let parameter = parameter(text)?;
    token(text, ")")?;
    Some(Reference { effect, parameter })
}

pub(super) fn finished(text: &str) -> bool {
    text.trim()
        .strip_suffix(';')
        .unwrap_or(text.trim())
        .trim()
        .is_empty()
}

fn vector_references(mut text: &str) -> Option<Vec<Reference<'_>>> {
    token(&mut text, "[")?;
    let mut references = vec![reference(&mut text)?];
    while text.trim_start().starts_with(',') && references.len() < 3 {
        token(&mut text, ",")?;
        references.push(reference(&mut text)?);
    }
    token(&mut text, "]")?;
    (references.len() >= 2 && finished(text)).then_some(references)
}

/// The referenced same-layer Slider value, through pure same-layer aliases.
pub(super) fn resolve<'a>(
    effects: &[(&str, &'a [Chunk])],
    link: Reference<'a>,
) -> Result<NumericProperty, PropertyError> {
    resolve_scoped(effects, link, None)
}

fn resolve_scoped<'a>(
    effects: &[(&str, &'a [Chunk])],
    mut link: Reference<'a>,
    scope: Option<(&Layer, &Composition)>,
) -> Result<NumericProperty, PropertyError> {
    let mut visited = Vec::new();
    loop {
        let mut matches = effects
            .iter()
            .enumerate()
            .filter_map(|(index, (kind, run))| {
                let plugin = properties::unique_list(run, *b"sspc").ok()?;
                let body = properties::unique_list(plugin, *b"tdgp").ok()?;
                (effect_name(plugin, body) == Some(link.effect)).then_some((index, *kind, body))
            });
        let (effect_index, kind, body) = matches
            .next()
            .ok_or(PropertyError::Layout("Slider not found"))?;
        if matches.next().is_some() || kind != "ADBE Slider Control" {
            return Err(PropertyError::Layout("ambiguous or non-Slider effect"));
        }
        let parameters = properties::runs(body)?;
        let mut matches = parameters
            .iter()
            .enumerate()
            .filter_map(|(index, (name, run))| {
                let body = properties::unique_list(run, *b"tdbs").ok()?;
                (*name == link.parameter || display_name(body) == Some(link.parameter))
                    .then_some((index, *name, body))
            });
        let (parameter_index, name, body) = matches
            .next()
            .ok_or(PropertyError::Layout("Slider parameter not found"))?;
        if matches.next().is_some() || name != SLIDER_VALUE {
            return Err(PropertyError::Layout(
                "ambiguous or non-value Slider parameter",
            ));
        }
        let identity = (effect_index, parameter_index);
        if visited.contains(&identity) {
            return Err(PropertyError::Layout("cyclic Slider alias"));
        }
        visited.push(identity);
        let numeric = properties::read_numeric(body)?;
        if !numeric.expression_enabled {
            return Ok(numeric);
        }
        let mut text = expression(body)?;
        let mut local = text;
        if let Some(next) = reference(&mut local).filter(|_| finished(local)) {
            link = next;
            continue;
        }
        let (layer, composition) = scope.ok_or(PropertyError::Layout(
            "Slider expression is not a pure alias",
        ))?;
        token(&mut text, "thisComp.layer")
            .and_then(|_| token(&mut text, "("))
            .ok_or(PropertyError::Layout("not a direct sibling Slider alias"))?;
        let name = quoted(&mut text).ok_or(PropertyError::Layout("invalid sibling layer name"))?;
        token(&mut text, ")")
            .and_then(|_| token(&mut text, "."))
            .ok_or(PropertyError::Layout("invalid sibling Slider access"))?;
        let next = reference(&mut text)
            .filter(|_| finished(text))
            .ok_or(PropertyError::Layout("not a complete sibling Slider alias"))?;
        let mut matches = composition
            .layers
            .iter()
            .filter(|candidate| candidate.name.as_ref() == name);
        let source = matches
            .next()
            .ok_or(PropertyError::Layout("sibling Slider layer missing"))?;
        if matches.next().is_some() {
            return Err(PropertyError::Layout("ambiguous sibling Slider layer"));
        }
        if layer.record.start_time().is_none_or(|t| !t.is_finite())
            || source.record.start_time() != layer.record.start_time()
            || source.record.stretch() != layer.record.stretch()
            || layer
                .record
                .stretch()
                .is_none_or(|s| !s.is_finite() || s <= 0.0)
        {
            return Err(PropertyError::Layout(
                "sibling Slider links require identical positive source clocks",
            ));
        }
        let root = properties::root_runs(&source.content)?;
        let parade = unique_run(&root, "ADBE Effect Parade")?;
        let effects = properties::runs(properties::unique_list(parade, *b"tdgp")?)?;
        // At most one cross-layer hop. The existing local alias cycle detector
        // remains active; a second cross-layer hop is explicitly unsupported.
        return resolve(&effects, next);
    }
}

fn scale_vector(
    mut scalar: NumericProperty,
    dimensions: usize,
) -> Result<NumericProperty, PropertyError> {
    if scalar.value_kind != NumericValueKind::Continuous || scalar.dimensions_separated {
        return Err(PropertyError::Layout("non-continuous Slider"));
    }
    fn repeat(
        values: &mut Vec<f64>,
        dimensions: usize,
        factor: f64,
        allow_empty: bool,
    ) -> Result<(), PropertyError> {
        if values.is_empty() && allow_empty {
            return Ok(());
        }
        let [value] = values.as_slice() else {
            return Err(PropertyError::Layout("non-scalar Slider value or easing"));
        };
        *values = vec![value * factor; dimensions];
        Ok(())
    }
    repeat(&mut scalar.values, dimensions, 0.01, scalar.animated)?;
    for key in &mut scalar.keyframes {
        if !key.spatial_in.is_empty() || !key.spatial_out.is_empty() {
            return Err(PropertyError::Layout("spatial Slider keys"));
        }
        repeat(&mut key.values, dimensions, 0.01, false)?;
        repeat(&mut key.in_speed, dimensions, 0.01, true)?;
        repeat(&mut key.out_speed, dimensions, 0.01, true)?;
        repeat(&mut key.in_influence, dimensions, 1.0, true)?;
        repeat(&mut key.out_influence, dimensions, 1.0, true)?;
    }
    scalar.expression_enabled = false;
    scalar.expression_present = false;
    Ok(scalar)
}

#[cfg(test)]
mod tests;
