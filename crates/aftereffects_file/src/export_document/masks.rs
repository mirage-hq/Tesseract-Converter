//! Lower current FX masks and text-path guides to fresh native mask DTOs.

mod held_rect;
mod inverted_compound;
mod positive_compound;
mod source_affine;

use std::collections::BTreeSet;

use fx_schema::{
    Layer, LayerId, Position, PropertyTarget, PropertyValue, ShapePath, ShapePathCommand,
    TextPathOptions, Transform,
    animator::{AnimatorData, PropertyKeyframeTrack},
    layer::{MaskMode, PathMask},
};

use crate::writer::{
    KeyframeEasing, NativeMaskMode, NativeMaskSpec, NumericKeyframe, NumericTrack,
};

use super::effective_constant;

#[derive(Clone, Copy)]
pub(crate) struct MaskOwner<'a> {
    /// None only for a proven source-local guide copied before its Group occurrence transform.
    pub coordinate_owner: Option<LayerId>,
    pub parent: Option<LayerId>,
    pub transform: &'a Transform,
    pub source_size: [u32; 2],
    /// Proven identity owner clock; the owner need not be in a child-guide stack.
    pub clock: Option<fx_schema::TimeRangeProperty>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LoweredMasks {
    pub masks: Vec<NativeMaskSpec>,
    /// One-based index in `masks`, suitable for native Text Path Options.
    pub text_path_index: Option<u16>,
    /// Static guide layers consumed into same-layer native masks.
    pub consumed_guides: BTreeSet<LayerId>,
    /// An omitted coverage gate must not become an unmasked Adjustment effect.
    pub has_omitted_gating_mask: bool,
    pub diagnostics: Vec<String>,
}

/// Lowers masks independently so one unsupported guide does not discard its
/// convertible siblings. `siblings` is the owner's immediate structural stack.
pub(crate) fn lower(
    masks: &[PathMask],
    text_path: Option<&TextPathOptions>,
    owner: MaskOwner<'_>,
    siblings: &[Layer],
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> LoweredMasks {
    let mut output = LoweredMasks::default();
    if owner.source_size.contains(&0) {
        output.has_omitted_gating_mask = masks.iter().any(|mask| mask.mode != MaskMode::None);
        if !masks.is_empty() || text_path.is_some() {
            output
                .diagnostics
                .push("Masks omitted because the owning native source dimensions are zero".into());
        }
        return output;
    }
    for (position, mask) in masks.iter().enumerate() {
        match lower_mask(mask, position + 1, owner, siblings, dynamics) {
            Ok((spec, guide, diagnostics)) => {
                output.masks.push(spec);
                if let Some(guide) = guide {
                    output.consumed_guides.insert(guide);
                }
                output.diagnostics.extend(
                    diagnostics
                        .into_iter()
                        .map(|message| format!("Mask {}: {message}", position + 1)),
                );
            }
            Err(message) => {
                if let Ok(specs) =
                    inverted_compound::lower(mask, masks.len(), position + 1, owner, dynamics)
                {
                    output.masks.extend(specs);
                    output.diagnostics.push(format!("Mask {}: approximate inverted compound union exported as two editable noninverted Subtract masks; independent per-contour expansion/feather differs from shared whole-union processing (native 40px/15px control: 34134/262144 pixels differ, maximum channel difference 2/255); alpha fidelity and edits outside the admitted profile are unverified", position + 1));
                    continue;
                }
                match positive_compound::lower(
                    mask,
                    masks.len(),
                    position + 1,
                    owner,
                    siblings,
                    dynamics,
                ) {
                    Ok((specs, guide, diagnostic)) => {
                        output.masks.extend(specs);
                        output.consumed_guides.insert(guide);
                        output
                            .diagnostics
                            .push(format!("Mask {}: {diagnostic}", position + 1));
                    }
                    Err(reason) => {
                        output.has_omitted_gating_mask |= mask.mode != MaskMode::None;
                        output.diagnostics.push(format!(
                            "Mask {} omitted: {message}; positive compound fallback: {reason}",
                            position + 1
                        ));
                    }
                }
            }
        }
    }
    if let Some(options) = text_path {
        match animated_guide_path(options.path_layer, owner, siblings, dynamics) {
            Ok((path, path_track)) => {
                let next = output.masks.len() + 1;
                match u16::try_from(next) {
                    Ok(index) => {
                        let geometry = if path_track.is_some() {
                            "animated"
                        } else {
                            "static"
                        };
                        output.masks.push(NativeMaskSpec {
                            name: "Text Path Guide".into(),
                            path,
                            path_track,
                            // Native Text mask paths store layer-local pixels,
                            // unlike footage-normalized masks. Keep an identity
                            // divisor without changing FX coordinates or AV masks.
                            source_size: [1, 1],
                            mode: NativeMaskMode::None,
                            inverted: false,
                            feather: [0.0; 2],
                            opacity: 1.0,
                            expansion: 0.0,
                            feather_track: None,
                            opacity_track: None,
                            expansion_track: None,
                        });
                        output.text_path_index = Some(index);
                        output.consumed_guides.insert(options.path_layer);
                        output.diagnostics.push(format!(
                            "Text Path guide layer {} was copied as same-layer {geometry} native Mask {index}; the live cross-layer geometry link is not retained",
                            options.path_layer
                        ));
                    }
                    Err(_) => output.diagnostics.push(
                        "Text Path guide omitted because the native mask index exceeds u16".into(),
                    ),
                }
            }
            Err(message) => output
                .diagnostics
                .push(format!("Text Path guide omitted: {message}")),
        }
    }
    output
}

fn lower_mask(
    mask: &PathMask,
    index: usize,
    owner: MaskOwner<'_>,
    siblings: &[Layer],
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> Result<(NativeMaskSpec, Option<LayerId>, Vec<String>), &'static str> {
    let mut normalized_inline_loop = false;
    let (path, path_track, guide) = match (&mask.legacy_path, mask.layer) {
        (Some(path), None) => {
            let mut path = checked_path(path.clone())?;
            // Legacy filled masks may return exactly to MoveTo without an
            // explicit Close. Native AE treats that contour as open unless we
            // close it; keep genuine open paths and two-command loops intact.
            if path.commands.len() >= 3
                && matches!(
                    path.commands.last(),
                    Some(ShapePathCommand::LineTo { .. } | ShapePathCommand::CubicTo { .. })
                )
                && path.commands.first().and_then(ShapePathCommand::endpoint)
                    == path.commands.last().and_then(ShapePathCommand::endpoint)
            {
                path.commands.push(ShapePathCommand::Close);
                normalized_inline_loop = true;
            }
            (path, None, None)
        }
        (None, Some(id)) => {
            let (path, track) = animated_guide_path(id, owner, siblings, dynamics)?;
            (path, track, Some(id))
        }
        (Some(_), Some(_)) => return Err("mask has both legacy inline and guide-layer geometry"),
        (None, None) => return Err("mask has no geometry source"),
    };
    let (feather, feather_track, mut diagnostics) = mask_property(
        mask.id,
        "feather",
        PropertyValue::Vector2(mask.feather),
        TrackKind::Vector2,
        dynamics,
    );
    let base_opacity = mask.opacity.value().min(1.0);
    if base_opacity != mask.opacity.value() {
        diagnostics.push("Mask opacity exceeds the native 0..1 range; clamped to fully opaque (edge coverage may differ)".into());
    }
    let (opacity, opacity_track, opacity_diagnostics) = mask_property(
        mask.id,
        "opacity",
        PropertyValue::Float(base_opacity),
        TrackKind::Scalar {
            scale: 100.0,
            min: 0.0,
            max: 1.0,
        },
        dynamics,
    );
    let (expansion, expansion_track, expansion_diagnostics) = mask_property(
        mask.id,
        "expansion",
        PropertyValue::Float(mask.expansion),
        TrackKind::Scalar {
            scale: 1.0,
            min: f64::NEG_INFINITY,
            max: f64::INFINITY,
        },
        dynamics,
    );
    diagnostics.extend(opacity_diagnostics);
    diagnostics.extend(expansion_diagnostics);
    if normalized_inline_loop {
        diagnostics.push("duplicate-endpoint inline mask normalized to closed native contour; native path encoding merges the terminal vertex into the first while preserving its incoming handle".into());
    }
    if let Some(guide_id) = guide {
        diagnostics.push(format!(
            "guide layer {guide_id} was copied into same-layer native geometry and supported authored Path keys; the live cross-layer link is not retained"
        ));
    }
    let PropertyValue::Vector2(feather) = feather else {
        return Err("mask Feather constant has the wrong type");
    };
    let PropertyValue::Float(opacity) = opacity else {
        return Err("mask Opacity constant has the wrong type");
    };
    let PropertyValue::Float(expansion) = expansion else {
        return Err("mask Expansion constant has the wrong type");
    };
    let mode = match mask.mode {
        MaskMode::None => NativeMaskMode::None,
        MaskMode::Add => NativeMaskMode::Add,
        MaskMode::Subtract => NativeMaskMode::Subtract,
        MaskMode::Intersect => NativeMaskMode::Intersect,
        MaskMode::Lighten => NativeMaskMode::Lighten,
        MaskMode::Darken => NativeMaskMode::Darken,
        MaskMode::Difference => NativeMaskMode::Difference,
        MaskMode::Accum => {
            diagnostics.push("Accum has no distinct decoded native ordinal; Add was used".into());
            NativeMaskMode::Add
        }
    };
    Ok((
        NativeMaskSpec {
            name: format!("Mask {index}"),
            path,
            path_track,
            source_size: owner.source_size,
            mode,
            inverted: mask.inverted,
            feather,
            opacity,
            expansion,
            feather_track,
            opacity_track,
            expansion_track,
        },
        guide,
        diagnostics,
    ))
}

/// Certify only the static hard-Rect mask that the existing native lowerer
/// actually authors. Its path is copied into the same native layer and is a
/// pointwise alpha gate: it cannot sample pixels beyond the consumer's inverse
/// viewport. This is not a generic mask-bound or animated-guide certificate.
pub(crate) fn static_rect_add_crop_certificate(
    masks: &[PathMask],
    owner: MaskOwner<'_>,
    siblings: &[Layer],
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> bool {
    let [mask] = masks else {
        return false;
    };
    let Some(guide_id) = mask.layer else {
        return false;
    };
    let Some(clock) = owner.clock else {
        return false;
    };
    let Some(guide) = siblings.iter().find(|layer| layer.id() == guide_id) else {
        return false;
    };
    if !rectangular_crop_guide(guide)
        || guide.active_range() != clock
        || mask.legacy_path.is_some()
        || mask.mode != MaskMode::Add
        || mask.inverted
        || mask.feather != [0.0; 2]
        || mask.opacity.value() != 1.0
        || mask.expansion != 0.0
        || dynamics
            .iter()
            .any(|entry| entry.target.fx_item_id() == Some(mask.id))
    {
        return false;
    }
    let lowered = lower(masks, None, owner, siblings, dynamics);
    let [native] = lowered.masks.as_slice() else {
        return false;
    };
    matches!(native.mode, NativeMaskMode::Add)
        && native.path_track.is_none()
        && native.feather_track.is_none()
        && native.opacity_track.is_none()
        && native.expansion_track.is_none()
        && lowered.consumed_guides.contains(&guide_id)
        && matches!(lowered.diagnostics.as_slice(), [message] if message.contains("was copied into same-layer native geometry"))
}

/// The crop certificate depends on exact contour geometry, not its FX layer tag.
/// Keep general polygons, procedural modifiers and rounded paths outside it.
fn rectangular_crop_guide(guide: &Layer) -> bool {
    if let fx_schema::LayerData::Rect(rect) = guide.data() {
        return rect.rect.roundness == 0.0;
    }
    let Ok((path, _)) = checked_shape_guide(guide) else {
        return false;
    };
    if !path.is_finite() {
        return false;
    }
    let [
        ShapePathCommand::MoveTo {
            x: left, y: top, ..
        },
        ShapePathCommand::LineTo {
            x: right, y: top2, ..
        },
        ShapePathCommand::LineTo {
            x: right2,
            y: bottom,
            ..
        },
        ShapePathCommand::LineTo {
            x: left2,
            y: bottom2,
            ..
        },
        closure @ ..,
    ] = path.commands.as_slice()
    else {
        return false;
    };
    let closed = match closure {
        [ShapePathCommand::Close] => true,
        [
            ShapePathCommand::LineTo { x, y, .. },
            ShapePathCommand::Close,
        ] => x == left && y == top,
        _ => false,
    };
    // The caller's existing lower() still rejects mirror/corner controls,
    // coordinate animation, unsafe affine ownership and mask properties.
    closed
        && right > left
        && bottom > top
        && top2 == top
        && right2 == right
        && left2 == left
        && bottom2 == bottom
}

#[derive(Clone, Copy)]
enum TrackKind {
    Vector2,
    Scalar { scale: f64, min: f64, max: f64 },
}

fn mask_property(
    id: fx_schema::FxItemId,
    name: &str,
    base: PropertyValue,
    kind: TrackKind,
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> (PropertyValue, Option<NumericTrack>, Vec<String>) {
    let Some(entry) = dynamics.iter().find(|entry| {
        matches!(
            &entry.target,
            PropertyTarget::FxItemProperty(target)
                if target.item_id() == id && target.property_name() == name
        )
    }) else {
        return (base, None, Vec::new());
    };
    let mut diagnostics = Vec::new();
    if !entry.dependencies.is_empty()
        || entry.random_seed_target.is_some()
        || !entry.layer_refs.is_empty()
    {
        diagnostics.push(format!(
            "dependent {name} animator omitted; current typed value retained"
        ));
        return (base, None, diagnostics);
    }
    match entry.animator.data() {
        AnimatorData::Constant { value } => match checked_value(value, kind) {
            Ok(value) => (value.clone(), None, diagnostics),
            Err(message) => {
                diagnostics.push(format!("{name} constant omitted: {message}"));
                (base, None, diagnostics)
            }
        },
        AnimatorData::Keyframes {
            track,
            enabled: true,
            ..
        } => match numeric_track(track, kind) {
            Ok(track) => (base, Some(track), diagnostics),
            Err(message) => {
                diagnostics.push(format!("{name} keys omitted: {message}"));
                (base, None, diagnostics)
            }
        },
        AnimatorData::Keyframes { enabled: false, .. } => {
            let Some(value) = effective_constant(&entry.animator) else {
                diagnostics.push(format!(
                    "disabled {name} keys have no runtime-visible disabledValue; current typed value retained"
                ));
                return (base, None, diagnostics);
            };
            match checked_value(value, kind) {
                Ok(value) => {
                    diagnostics.push(format!(
                        "disabled {name} keys omitted; runtime-visible disabledValue normalized to a native constant"
                    ));
                    (value.clone(), None, diagnostics)
                }
                Err(message) => {
                    diagnostics.push(format!(
                        "disabled {name} runtime-visible value omitted: {message}; current typed value retained"
                    ));
                    (base, None, diagnostics)
                }
            }
        }
        AnimatorData::JsScript { .. } => {
            diagnostics.push(format!(
                "scripted {name} omitted; current typed value retained"
            ));
            (base, None, diagnostics)
        }
    }
}

fn checked_value(value: &PropertyValue, kind: TrackKind) -> Result<&PropertyValue, &'static str> {
    match (value, kind) {
        (PropertyValue::Vector2(values), TrackKind::Vector2)
            if values
                .iter()
                .all(|value| value.is_finite() && *value >= 0.0) =>
        {
            Ok(value)
        }
        (PropertyValue::Float(scalar), TrackKind::Scalar { min, max, .. })
            if scalar.is_finite() && (min..=max).contains(scalar) =>
        {
            Ok(value)
        }
        _ => Err("value type or range is not native-exportable"),
    }
}

fn numeric_track(
    track: &PropertyKeyframeTrack,
    kind: TrackKind,
) -> Result<NumericTrack, &'static str> {
    let keys = track
        .keyframes()
        .iter()
        .map(|key| {
            checked_value(key.value(), kind)?;
            let (values, dimensions) = match (key.value(), kind) {
                (PropertyValue::Vector2(value), TrackKind::Vector2) => (value.to_vec(), 2),
                (PropertyValue::Float(value), TrackKind::Scalar { scale, .. }) => {
                    (vec![value * scale], 1)
                }
                _ => return Err("mask key value has the wrong type"),
            };
            Ok(NumericKeyframe {
                time_millis: key.layer_time().as_millis(),
                values,
                easing: vec![native_easing(key.easing()); dimensions],
                spatial_in: Vec::new(),
                spatial_out: Vec::new(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(NumericTrack { keys })
}

fn native_easing(value: fx_schema::PropertyKeyframeEasing) -> KeyframeEasing {
    match value {
        fx_schema::PropertyKeyframeEasing::Hold => KeyframeEasing::Hold,
        fx_schema::PropertyKeyframeEasing::Linear => KeyframeEasing::Linear,
        fx_schema::PropertyKeyframeEasing::CubicBezier { x1, y1, x2, y2 } => {
            KeyframeEasing::CubicBezier { x1, y1, x2, y2 }
        }
    }
}

fn animated_guide_path(
    id: LayerId,
    owner: MaskOwner<'_>,
    siblings: &[Layer],
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> Result<(ShapePath, Option<crate::writer::PathTrack>), &'static str> {
    let path_source = super::track(dynamics, id, fx_schema::PropType::ShapePath)?;
    let Some(mut track) = super::path_animation::track(path_source)? else {
        return match guide_path(id, owner, siblings, dynamics) {
            Ok(path) => Ok((path, None)),
            Err(reason) if owner.coordinate_owner.is_some() => Err(reason),
            Err(_) => held_rect::guide_path(id, owner, siblings, dynamics).or_else(|reason| {
                if siblings.iter().any(|guide| {
                    guide.id() == id && matches!(guide.data(), fx_schema::LayerData::Shape(_))
                }) {
                    source_affine::guide_path(id, owner, siblings, dynamics)
                } else {
                    Err(reason)
                }
            }),
        };
    };
    let guide = checked_guide(id, owner, siblings, dynamics, true)?;
    let clock = owner
        .clock
        .ok_or("mask owner has an unproven source clock")?;
    let guide_range = guide.active_range();
    if guide_range.start != clock.start || guide_range.duration < clock.duration {
        return Err("animated mask guide and owner clocks are not proven equivalent");
    }
    let (_, transform) = checked_shape_guide(guide)?;
    let affine = relative_affine(owner.transform, transform)?;
    for key in &mut track.keyframes {
        key.path = checked_path(transform_path(key.path.clone(), affine))?;
    }
    crate::writer::validate_path_track(&track)
        .map_err(|_| "transformed mask Path keys exceed native bounds")?;
    Ok((track.keyframes[0].path.clone(), Some(track)))
}

fn guide_path(
    id: LayerId,
    owner: MaskOwner<'_>,
    siblings: &[Layer],
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> Result<ShapePath, &'static str> {
    guide_path_checked(id, owner, siblings, dynamics)
}

fn guide_path_checked(
    id: LayerId,
    owner: MaskOwner<'_>,
    siblings: &[Layer],
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> Result<ShapePath, &'static str> {
    let guide = checked_guide(id, owner, siblings, dynamics, false)?;
    match guide.data() {
        fx_schema::LayerData::Shape(_) => {
            let (path, transform) = checked_shape_guide(guide)?;
            checked_path(transform_path(
                path.clone(),
                relative_affine(owner.transform, transform)?,
            ))
        }
        fx_schema::LayerData::Rect(layer) if layer.rect.roundness == 0.0 => {
            checked_path(transform_path(
                rectangle_path(layer.rect.position, layer.rect.size),
                relative_affine(owner.transform, &layer.transform)?,
            ))
        }
        fx_schema::LayerData::BooleanOperation(_) => {
            Err("static Boolean guide flattening is not implemented")
        }
        _ => Err("guide is not a static unmodified Path or square Rect"),
    }
}

fn checked_guide<'a>(
    id: LayerId,
    owner: MaskOwner<'_>,
    siblings: &'a [Layer],
    dynamics: &crate::export_document::AnimationIndex<'_>,
    allow_path_keys: bool,
) -> Result<&'a Layer, &'static str> {
    let guide = siblings
        .iter()
        .find(|layer| layer.id() == id)
        .ok_or("guide is not in the owner's immediate sibling stack")?;
    if guide.parent_id() != owner.parent {
        return Err("guide and owner do not share the same coordinate parent");
    }
    if owner
        .coordinate_owner
        .is_some_and(|id| has_coordinate_animation(id, dynamics))
        || has_guide_geometry_animation(id, dynamics, allow_path_keys)
    {
        return Err(
            "owner/guide coordinate or guide geometry animation prevents an exact relative-coordinate copy",
        );
    }
    Ok(guide)
}

fn checked_shape_guide(guide: &Layer) -> Result<(&ShapePath, &Transform), &'static str> {
    match guide.data() {
        fx_schema::LayerData::Shape(layer)
            if layer.shape.ellipse.is_none()
                && layer.shape.poly_star.is_none()
                && layer.shape.round_corners.is_none()
                && layer.shape.offset_paths.is_none()
                && layer.shape.trim.is_none() =>
        {
            Ok((&layer.shape.path, &layer.transform))
        }
        _ => Err("guide is not an unmodified Path Shape"),
    }
}

fn has_coordinate_animation(
    id: LayerId,
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> bool {
    dynamics.for_layer(id).any(|entry| {
        entry.target.as_property().is_some_and(|property| {
            property.layer_id() == id
                && matches!(
                    property.property_type(),
                    fx_schema::PropType::AnchorPointX
                        | fx_schema::PropType::AnchorPointY
                        | fx_schema::PropType::PositionX
                        | fx_schema::PropType::PositionY
                        | fx_schema::PropType::PositionZ
                        | fx_schema::PropType::ScaleX
                        | fx_schema::PropType::ScaleY
                        | fx_schema::PropType::Rotation
                        | fx_schema::PropType::RotationX
                        | fx_schema::PropType::RotationY
                        | fx_schema::PropType::OrientationX
                        | fx_schema::PropType::OrientationY
                        | fx_schema::PropType::OrientationZ
                        | fx_schema::PropType::Skew
                        | fx_schema::PropType::SkewAxis
                )
        })
    })
}

fn has_guide_geometry_animation(
    id: LayerId,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    allow_path_keys: bool,
) -> bool {
    has_coordinate_animation(id, dynamics)
        || dynamics.for_layer(id).any(|entry| {
            entry.target.as_property().is_some_and(|property| {
                property.layer_id() == id
                    && matches!(
                        property.property_type(),
                        fx_schema::PropType::ShapePath
                            | fx_schema::PropType::RectSize
                            | fx_schema::PropType::RectRoundness
                            | fx_schema::PropType::RoundCornersRadius
                            | fx_schema::PropType::OffsetPathsAmount
                            | fx_schema::PropType::TrimStart
                            | fx_schema::PropType::TrimEnd
                            | fx_schema::PropType::TrimOffset
                            | fx_schema::PropType::PolyStarPoints
                            | fx_schema::PropType::PolyStarPosition
                            | fx_schema::PropType::PolyStarRotation
                            | fx_schema::PropType::PolyStarOuterRadius
                            | fx_schema::PropType::PolyStarInnerRadius
                            | fx_schema::PropType::PolyStarOuterRoundness
                            | fx_schema::PropType::PolyStarInnerRoundness
                            | fx_schema::PropType::EllipseSize
                            | fx_schema::PropType::EllipsePosition
                    )
                    && !(allow_path_keys
                        && property.property_type() == fx_schema::PropType::ShapePath)
            })
        })
}

fn rectangle_path(position: [f64; 2], size: [f64; 2]) -> ShapePath {
    let [x, y] = position;
    let [width, height] = size;
    ShapePath {
        commands: vec![
            anchor(x, y, true),
            anchor(x + width, y, false),
            anchor(x + width, y + height, false),
            anchor(x, y + height, false),
            ShapePathCommand::Close,
        ],
    }
}

fn anchor(x: f64, y: f64, first: bool) -> ShapePathCommand {
    if first {
        ShapePathCommand::MoveTo {
            x,
            y,
            mirror: None,
            corner_radius: None,
        }
    } else {
        ShapePathCommand::LineTo {
            x,
            y,
            mirror: None,
            corner_radius: None,
        }
    }
}

fn checked_path(path: ShapePath) -> Result<ShapePath, &'static str> {
    if !path.is_finite() || path.commands.len() < 2 {
        return Err("guide path is empty or non-finite");
    }
    let mut vertices = 0_usize;
    for (index, command) in path.commands.iter().enumerate() {
        match command {
            ShapePathCommand::MoveTo {
                mirror,
                corner_radius,
                ..
            } if index == 0 => {
                vertices += 1;
                if mirror.is_some() || corner_radius.is_some() {
                    return Err(
                        "guide path has editor mirror/corner controls without native mask equivalents",
                    );
                }
            }
            ShapePathCommand::LineTo {
                mirror,
                corner_radius,
                ..
            }
            | ShapePathCommand::CubicTo {
                mirror,
                corner_radius,
                ..
            } if vertices > 0 => {
                vertices += 1;
                if mirror.is_some() || corner_radius.is_some() {
                    return Err(
                        "guide path has editor mirror/corner controls without native mask equivalents",
                    );
                }
            }
            ShapePathCommand::Close if index + 1 == path.commands.len() && vertices > 1 => {}
            _ => return Err("guide path is not one native static contour"),
        }
    }
    if !(2..=21_845).contains(&vertices) {
        return Err("guide path vertex count exceeds native static contour bounds");
    }
    Ok(path)
}

#[derive(Clone, Copy)]
struct Affine {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    tx: f64,
    ty: f64,
}

fn relative_affine(owner: &Transform, source: &Transform) -> Result<Affine, &'static str> {
    let owner = affine(owner)?;
    let source = affine(source)?;
    Ok(owner.inverse()?.compose(source))
}

fn affine(value: &Transform) -> Result<Affine, &'static str> {
    let Position::TwoD(position) = value.position else {
        return Err("3D guide coordinates are not exported");
    };
    if value.skew != 0.0
        || value.skew_axis != 0.0
        || value.rotation_x != 0.0
        || value.rotation_y != 0.0
        || value.orientation != [0.0; 3]
    {
        return Err("skewed or 3D guide coordinates are not exported");
    }
    let radians = value.rotation.to_radians();
    let (sin, cos) = radians.sin_cos();
    let sx = value.scale[0] / 100.0;
    let sy = value.scale[1] / 100.0;
    let result = Affine {
        a: cos * sx,
        b: sin * sx,
        c: -sin * sy,
        d: cos * sy,
        tx: position[0] - cos * sx * value.anchor_point[0] + sin * sy * value.anchor_point[1],
        ty: position[1] - sin * sx * value.anchor_point[0] - cos * sy * value.anchor_point[1],
    };
    if [result.a, result.b, result.c, result.d, result.tx, result.ty]
        .iter()
        .all(|value| value.is_finite())
    {
        Ok(result)
    } else {
        Err("guide transform is non-finite")
    }
}

impl Affine {
    fn inverse(self) -> Result<Self, &'static str> {
        let determinant = self.a * self.d - self.b * self.c;
        if !determinant.is_finite() || determinant.abs() <= f64::EPSILON {
            return Err("owner transform is singular");
        }
        Ok(Self {
            a: self.d / determinant,
            b: -self.b / determinant,
            c: -self.c / determinant,
            d: self.a / determinant,
            tx: (self.c * self.ty - self.d * self.tx) / determinant,
            ty: (self.b * self.tx - self.a * self.ty) / determinant,
        })
    }

    fn compose(self, other: Self) -> Self {
        Self {
            a: self.a * other.a + self.c * other.b,
            b: self.b * other.a + self.d * other.b,
            c: self.a * other.c + self.c * other.d,
            d: self.b * other.c + self.d * other.d,
            tx: self.a * other.tx + self.c * other.ty + self.tx,
            ty: self.b * other.tx + self.d * other.ty + self.ty,
        }
    }

    fn apply(self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.tx,
            self.b * x + self.d * y + self.ty,
        )
    }
}

pub(crate) fn translate(specs: &mut [NativeMaskSpec], offset: [f64; 2]) {
    let affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: offset[0],
        ty: offset[1],
    };
    for spec in specs {
        spec.path = transform_path(spec.path.clone(), affine);
        if let Some(track) = &mut spec.path_track {
            for key in &mut track.keyframes {
                key.path = transform_path(key.path.clone(), affine);
            }
        }
    }
}

fn transform_path(mut path: ShapePath, affine: Affine) -> ShapePath {
    for command in &mut path.commands {
        match command {
            ShapePathCommand::MoveTo { x, y, .. } | ShapePathCommand::LineTo { x, y, .. } => {
                (*x, *y) = affine.apply(*x, *y)
            }
            ShapePathCommand::CubicTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
                ..
            } => {
                (*c1x, *c1y) = affine.apply(*c1x, *c1y);
                (*c2x, *c2y) = affine.apply(*c2x, *c2y);
                (*x, *y) = affine.apply(*x, *y);
            }
            ShapePathCommand::Close => {}
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use fx_schema::animator::AnimationGraphEntry;
    use fx_schema::{
        PropertyAnimator, PropertyKeyframeEasing, TimeOffset,
        animator::{KeyframeId, PropertyKeyframe},
    };

    fn disabled_entry(
        id: fx_schema::FxItemId,
        name: &str,
        values: [PropertyValue; 2],
        disabled_value: PropertyValue,
    ) -> AnimationGraphEntry {
        let track = PropertyKeyframeTrack::new(
            values
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    PropertyKeyframe::new(
                        KeyframeId::new(format!("disabled-mask-{name}-{index}")),
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
            panic!("mask fixture is keyed")
        };
        *enabled = false;
        *stored_disabled_value = Some(disabled_value);
        AnimationGraphEntry {
            target: PropertyTarget::fx_item(id, name),
            animator: PropertyAnimator::from_data(&data).unwrap(),
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        }
    }

    #[test]
    fn disabled_mask_controls_use_runtime_visible_values() {
        let id = fx_schema::FxItemId::new(91);
        let entries = [
            disabled_entry(
                id,
                "opacity",
                [PropertyValue::Float(0.1), PropertyValue::Float(0.9)],
                PropertyValue::Float(0.25),
            ),
            disabled_entry(
                id,
                "feather",
                [
                    PropertyValue::Vector2([1.0, 2.0]),
                    PropertyValue::Vector2([3.0, 4.0]),
                ],
                PropertyValue::Vector2([9.0, 11.0]),
            ),
        ];

        let (opacity, opacity_track, opacity_diagnostics) = mask_property(
            id,
            "opacity",
            PropertyValue::Float(0.75),
            TrackKind::Scalar {
                scale: 100.0,
                min: 0.0,
                max: 1.0,
            },
            &crate::export_document::AnimationIndex::new(&entries),
        );
        assert_eq!(opacity, PropertyValue::Float(0.25));
        assert!(opacity_track.is_none());
        assert!(opacity_diagnostics.iter().any(|message| {
            message.contains("runtime-visible disabledValue") && message.contains("native constant")
        }));

        let (feather, feather_track, feather_diagnostics) = mask_property(
            id,
            "feather",
            PropertyValue::Vector2([4.0, 6.0]),
            TrackKind::Vector2,
            &crate::export_document::AnimationIndex::new(&entries),
        );
        assert_eq!(feather, PropertyValue::Vector2([9.0, 11.0]));
        assert!(feather_track.is_none());
        assert!(feather_diagnostics.iter().any(|message| {
            message.contains("runtime-visible disabledValue") && message.contains("native constant")
        }));
    }

    #[test]
    fn relative_affine_maps_source_pixels_into_owner_local_space() {
        let identity = Transform {
            anchor_point: [0.0; 2],
            position: Position::TwoD([0.0; 2]),
            scale: [100.0; 2],
            rotation: 0.0,
            skew: 0.0,
            skew_axis: 0.0,
            rotation_x: 0.0,
            rotation_y: 0.0,
            orientation: [0.0; 3],
            opacity: fx_schema::PercentageProperty::new(100.0).unwrap(),
        };
        let mut source = identity;
        source.position = Position::TwoD([12.0, -4.0]);
        assert_eq!(
            relative_affine(&identity, &source).unwrap().apply(3.0, 5.0),
            (15.0, 1.0)
        );
    }

    #[test]
    fn inline_mask_loop_closure_is_exact_and_does_not_change_other_topologies() {
        fn mask(commands: serde_json::Value) -> PathMask {
            serde_json::from_value(serde_json::json!({
                "id": 9001, "mode": "add", "inverted": false,
                "path": {"commands": commands},
                "feather": [0.0, 0.0], "expansion": 0.0, "opacity": 1.0
            }))
            .unwrap()
        }
        let identity = Transform {
            anchor_point: [0.0; 2],
            position: Position::TwoD([0.0; 2]),
            scale: [100.0; 2],
            rotation: 0.0,
            skew: 0.0,
            skew_axis: 0.0,
            rotation_x: 0.0,
            rotation_y: 0.0,
            orientation: [0.0; 3],
            opacity: fx_schema::PercentageProperty::new(100.0).unwrap(),
        };
        let owner = MaskOwner {
            coordinate_owner: Some(LayerId::new(1)),
            parent: None,
            transform: &identity,
            source_size: [320, 180],
            clock: None,
        };
        let start = serde_json::json!({"type":"moveTo", "x":10.0, "y":20.0});
        let next = serde_json::json!({"type":"lineTo", "x":40.0, "y":20.0});
        let return_line = serde_json::json!({"type":"lineTo", "x":10.0, "y":20.0});
        let cases = [
            (serde_json::json!([start, next, return_line]), true),
            (
                serde_json::json!([start, next, return_line, {"type":"close"}]),
                false,
            ),
            (
                serde_json::json!([start, next, {"type":"lineTo", "x":10.01, "y":20.0}]),
                false,
            ),
            (serde_json::json!([start, return_line]), false),
            (
                serde_json::json!([start, next, {"type":"cubicTo", "c1x":45.0, "c1y":30.0,
                "c2x":5.0, "c2y":24.0, "x":10.0, "y":20.0}]),
                true,
            ),
        ];
        for (commands, normalized) in cases {
            let input = mask(commands);
            let original = input.legacy_path.as_ref().unwrap();
            let (mut native, guide, diagnostics) = lower_mask(
                &input,
                1,
                owner,
                &[],
                &crate::export_document::AnimationIndex::new(&[]),
            )
            .unwrap();
            assert_eq!(guide, None);
            assert_eq!(native.mode, NativeMaskMode::Add);
            assert_eq!(native.source_size, [320, 180]);
            assert_eq!(native.path_track, None);
            assert_eq!(
                native.path.commands.len(),
                original.commands.len() + usize::from(normalized)
            );
            assert_eq!(
                diagnostics
                    .iter()
                    .any(|message| message.contains("duplicate-endpoint inline mask")),
                normalized
            );
            if normalized {
                assert_eq!(
                    &native.path.commands[..original.commands.len()],
                    &original.commands
                );
                assert_eq!(native.path.commands.last(), Some(&ShapePathCommand::Close));
            } else {
                assert_eq!(&native.path, original);
            }
            translate(std::slice::from_mut(&mut native), [-5.0, -7.0]);
            assert_eq!(native.path.commands[0].endpoint(), Some((5.0, 13.0)));
            assert_eq!(native.mode, NativeMaskMode::Add);
        }
    }

    #[test]
    fn inverted_inline_compound_subtract_pair() {
        let mut path = rectangle_path([40.0, 40.0], [200.0, 200.0]);
        path.commands
            .extend(rectangle_path([140.0, 140.0], [200.0, 200.0]).commands);
        let mut mask: PathMask = serde_json::from_value(serde_json::json!({
            "id":401,"mode":"add","inverted":true,"opacity":1.0,
            "feather":[15.0,15.0],"expansion":40.0
        }))
        .unwrap();
        mask.legacy_path = Some(path.clone());
        let identity = Transform {
            anchor_point: [0.0; 2],
            position: Position::TwoD([0.0; 2]),
            scale: [100.0; 2],
            rotation: 0.0,
            skew: 0.0,
            skew_axis: 0.0,
            rotation_x: 0.0,
            rotation_y: 0.0,
            orientation: [0.0; 3],
            opacity: fx_schema::PercentageProperty::new(100.0).unwrap(),
        };
        let owner = MaskOwner {
            coordinate_owner: Some(LayerId::new(1)),
            parent: None,
            transform: &identity,
            source_size: [512, 512],
            clock: None,
        };
        let dynamics = crate::export_document::AnimationIndex::new(&[]);
        let output = lower(std::slice::from_ref(&mask), None, owner, &[], &dynamics);
        assert_eq!(output.masks.len(), 2, "{:?}", output.diagnostics);
        for spec in &output.masks {
            assert_eq!(spec.mode, NativeMaskMode::Subtract);
            assert!(!spec.inverted);
            assert_eq!(spec.feather, [15.0; 2]);
            assert_eq!(spec.expansion, 40.0);
            assert_eq!(spec.opacity, 1.0);
            assert_eq!(spec.source_size, [512, 512]);
            assert!(spec.path_track.is_none());
        }
        assert_eq!(output.masks[0].path.commands, path.commands[..5]);
        assert_eq!(output.masks[1].path.commands, path.commands[5..]);
        assert!(
            output
                .diagnostics
                .iter()
                .any(|message| message.contains("approximate"))
        );
        for invalid in 0..7 {
            let mut rejected = mask.clone();
            match invalid {
                0 => rejected.inverted = false,
                1 => rejected.mode = MaskMode::Subtract,
                2 => rejected.opacity = serde_json::from_value(serde_json::json!(0.5)).unwrap(),
                3 => rejected.expansion = -1.0,
                4 => rejected.feather = [-1.0, 0.0],
                5 => rejected.legacy_path.as_mut().unwrap().commands.swap(6, 8),
                _ => {
                    rejected.legacy_path.as_mut().unwrap().commands.pop();
                }
            }
            assert!(
                lower(&[rejected], None, owner, &[], &dynamics)
                    .masks
                    .is_empty(),
                "profile {invalid}"
            );
        }
        assert!(
            lower(&[mask.clone(), mask.clone()], None, owner, &[], &dynamics)
                .masks
                .is_empty()
        );
        let entries = [disabled_entry(
            mask.id,
            "opacity",
            [PropertyValue::Float(1.0), PropertyValue::Float(1.0)],
            PropertyValue::Float(1.0),
        )];
        let animated = crate::export_document::AnimationIndex::new(&entries);
        assert!(lower(&[mask], None, owner, &[], &animated).masks.is_empty());
    }

    #[test]
    fn rectangle_uses_local_origin_and_size() {
        let path = rectangle_path([10.0, 20.0], [30.0, 40.0]);
        assert_eq!(path.commands[0].endpoint(), Some((10.0, 20.0)));
        assert_eq!(path.commands[2].endpoint(), Some((40.0, 60.0)));
    }
}
