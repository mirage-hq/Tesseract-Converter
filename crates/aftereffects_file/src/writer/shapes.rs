//! Fresh native AE26 Ellipse and Star/Polygon shape geometry.
//! Properties are constructed from current FX values, never source records.

#[cfg(test)]
mod gradient_tests;
#[cfg(test)]
mod tests;

use std::borrow::Cow;

use fx_schema::{
    BlendMode,
    layer::{
        BooleanOp, ShapeEllipse, ShapeFillRule, ShapeGradientStop, ShapeGradientType, ShapeLineCap,
        ShapeLineJoin, ShapeOffsetPaths, ShapePaint, ShapePath, ShapePolyStar, ShapePolyStarType,
        ShapeRoundCorners, ShapeTrimMode, ShapeTrimPaths,
    },
};

use super::{
    AepWriteError, Duration24, NumericTrack, TransformAnimations, VectorAppearance,
    views::{self, ValueKind},
};
use crate::rifx::Chunk;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum VectorGeometry {
    Path(ShapePath),
    Ellipse(ShapeEllipse),
    PolyStar(ShapePolyStar),
    /// An FX origin-authored Rectangle. Native AE stores its center position.
    Rect {
        size: [f64; 2],
        position: [f64; 2],
        roundness: f64,
    },
    // Retained for direct-geometry writer compatibility tests.
    #[cfg_attr(not(test), allow(dead_code))]
    Boolean {
        op: BooleanOp,
        operands: Vec<VectorGeometry>,
    },
}

/// A fresh, editable native vector program. Entries are kept in authored order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct VectorLayerSpec {
    pub name: String,
    pub transform: super::SolidTransform,
    pub transform_animations: TransformAnimations,
    pub contents: Vec<VectorContent>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum VectorContent {
    Geometry {
        geometry: VectorGeometry,
        animations: GeometryAnimations,
    },
    Group(VectorGroupSpec),
    /// A native vector group whose Transform properties own their key tracks.
    AnimatedGroup(VectorGroupSpec, VectorGroupAnimations),
    Paint(VectorPaintSpec),
    Modifier(VectorModifierSpec),
    Merge(BooleanOp),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct VectorGroupSpec {
    pub name: String,
    pub blend_mode: BlendMode,
    pub transform: VectorGroupTransform,
    pub contents: Vec<VectorContent>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct VectorGroupTransform {
    pub anchor: [f64; 2],
    pub position: [f64; 2],
    pub scale: [f64; 2],
    pub skew: f64,
    pub skew_axis: f64,
    pub rotation: f64,
    pub opacity: f64,
}

impl Default for VectorGroupTransform {
    fn default() -> Self {
        Self {
            anchor: [0.0; 2],
            position: [0.0; 2],
            scale: [100.0; 2],
            skew: 0.0,
            skew_axis: 0.0,
            rotation: 0.0,
            opacity: 100.0,
        }
    }
}

/// Key tracks scoped to one native vector group's Transform operator.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct VectorGroupAnimations {
    pub anchor: Option<NumericTrack>,
    pub position: Option<NumericTrack>,
    pub scale: Option<NumericTrack>,
    pub rotation: Option<NumericTrack>,
    pub skew: Option<NumericTrack>,
    pub skew_axis: Option<NumericTrack>,
    pub opacity: Option<NumericTrack>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum VectorPaintSpec {
    Fill {
        paint: ShapePaint,
        fill_rule: ShapeFillRule,
        blend_mode: BlendMode,
        opacity: f64,
        animations: VectorPaintAnimations,
    },
    Stroke {
        paint: ShapePaint,
        blend_mode: BlendMode,
        opacity: f64,
        width: f64,
        cap: ShapeLineCap,
        join: ShapeLineJoin,
        miter_limit: f64,
        dashes: super::StrokeDashes,
        animations: VectorPaintAnimations,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct VectorPaintAnimations {
    pub color: Option<NumericTrack>,
    pub opacity: Option<NumericTrack>,
    pub width: Option<NumericTrack>,
    pub miter_limit: Option<NumericTrack>,
    pub join: Option<NumericTrack>,
    pub dash_offset: Option<NumericTrack>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum VectorModifierSpec {
    RoundCorners {
        value: ShapeRoundCorners,
        radius: Option<NumericTrack>,
    },
    OffsetPaths {
        value: ShapeOffsetPaths,
        amount: Option<NumericTrack>,
    },
    TrimPaths {
        value: ShapeTrimPaths,
        start: Option<NumericTrack>,
        end: Option<NumericTrack>,
        offset: Option<NumericTrack>,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct GeometryAnimations {
    pub path: Option<super::PathTrack>,
    pub rect_size: Option<NumericTrack>,
    pub rect_position: Option<NumericTrack>,
    pub rect_roundness: Option<NumericTrack>,
    pub ellipse_size: Option<NumericTrack>,
    pub ellipse_position: Option<NumericTrack>,
    pub star_position: Option<NumericTrack>,
    pub star_points: Option<NumericTrack>,
    pub star_rotation: Option<NumericTrack>,
    pub star_outer_radius: Option<NumericTrack>,
    pub star_inner_radius: Option<NumericTrack>,
    pub star_outer_roundness: Option<NumericTrack>,
    pub star_inner_roundness: Option<NumericTrack>,
}

impl GeometryAnimations {
    pub(crate) fn has_parametric_tracks(&self) -> bool {
        [
            &self.rect_size,
            &self.rect_position,
            &self.rect_roundness,
            &self.ellipse_size,
            &self.ellipse_position,
            &self.star_position,
            &self.star_points,
            &self.star_rotation,
            &self.star_outer_radius,
            &self.star_inner_radius,
            &self.star_outer_roundness,
            &self.star_inner_roundness,
        ]
        .iter()
        .any(|track| track.is_some())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct VectorShapeSpec {
    pub appearance: VectorAppearance,
    pub geometry: VectorGeometry,
    pub animations: TransformAnimations,
    pub geometry_animations: GeometryAnimations,
    pub stroke_animations: super::StrokeAnimations,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct VectorBooleanSpec {
    pub appearance: VectorAppearance,
    pub op: BooleanOp,
    pub operands: Vec<VectorGeometry>,
    pub animations: TransformAnimations,
    pub stroke_animations: super::StrokeAnimations,
}

fn validate_appearance(paint: &VectorAppearance) -> Result<(), AepWriteError> {
    if paint.name.is_empty() || paint.name.len() > 255 || paint.name.contains('\0') {
        return Err(AepWriteError::Invalid(
            "native shape name must be 1..=255 UTF-8 bytes without NUL",
        ));
    }
    if paint.fill_color.is_some() == paint.stroke_color.is_some() {
        return Err(AepWriteError::Invalid(
            "native shape requires exactly one solid Fill or Stroke",
        ));
    }
    if paint
        .fill_color
        .iter()
        .chain(paint.stroke_color.iter())
        .flatten()
        .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
        || !paint.paint_opacity.is_finite()
        || !(0.0..=100.0).contains(&paint.paint_opacity)
        || !paint.stroke_width.is_finite()
        || paint.stroke_width < 0.0
        || !paint.stroke_miter_limit.is_finite()
        || paint.stroke_miter_limit < 0.0
        || paint
            .transform
            .anchor
            .iter()
            .chain(&paint.transform.position)
            .chain(&paint.transform.scale)
            .any(|value| !value.is_finite())
        || !paint.transform.rotation.is_finite()
        || !paint.transform.opacity.is_finite()
        || !(0.0..=100.0).contains(&paint.transform.opacity)
    {
        return Err(AepWriteError::Invalid(
            "invalid native shape paint or transform",
        ));
    }
    Ok(())
}

pub(super) fn validate(spec: &VectorShapeSpec) -> Result<(), AepWriteError> {
    validate_appearance(&spec.appearance)?;
    match &spec.geometry {
        VectorGeometry::Path(path) => {
            // AE stores one contour per native Path property. Validation splits
            // compound geometry without changing its paint/modifier scope.
            super::path_geometry::validated_contours(path)?;
        }
        VectorGeometry::Ellipse(value) => {
            if value
                .size
                .iter()
                .any(|size| !size.is_finite() || *size <= 0.0 || *size > 65535.0)
                || value
                    .position
                    .iter()
                    .any(|coordinate| !coordinate.is_finite())
            {
                return Err(AepWriteError::Invalid(
                    "invalid editable native Ellipse geometry",
                ));
            }
        }
        VectorGeometry::PolyStar(value) => {
            if !value.points.is_finite()
                || value.points.fract() != 0.0
                || !(3.0..=1000.0).contains(&value.points)
                || value
                    .position
                    .iter()
                    .any(|coordinate| !coordinate.is_finite())
                || [
                    value.rotation,
                    value.outer_radius,
                    value.inner_radius,
                    value.outer_roundness,
                    value.inner_roundness,
                ]
                .iter()
                .any(|parameter| !parameter.is_finite())
                || value.outer_radius < 0.0
                || value.inner_radius < 0.0
                || !(0.0..=100.0).contains(&value.outer_roundness)
                || !(0.0..=100.0).contains(&value.inner_roundness)
            {
                return Err(AepWriteError::Invalid(
                    "invalid editable native Star/Polygon geometry",
                ));
            }
        }
        VectorGeometry::Rect { .. } | VectorGeometry::Boolean { .. } => {
            return Err(AepWriteError::Invalid(
                "Rectangle and Boolean geometry require their dedicated native layer specs",
            ));
        }
    }
    let a = &spec.geometry_animations;
    if let Some(track) = &a.path {
        if !matches!(spec.geometry, VectorGeometry::Path(_)) {
            return Err(AepWriteError::Invalid(
                "Path keys require native Path geometry",
            ));
        }
        super::split_path_track(track)?;
    }
    let valid = match &spec.geometry {
        VectorGeometry::Path(_) => !a.has_parametric_tracks(),
        VectorGeometry::Ellipse(_) => {
            a.rect_size.is_none()
                && a.rect_position.is_none()
                && a.rect_roundness.is_none()
                && a.star_position.is_none()
                && a.star_points.is_none()
                && a.star_rotation.is_none()
                && a.star_outer_radius.is_none()
                && a.star_inner_radius.is_none()
                && a.star_outer_roundness.is_none()
                && a.star_inner_roundness.is_none()
                && keys_within(a.ellipse_size.as_ref(), |value| {
                    value > 0.0 && value <= 65535.0
                })
                && keys_within(a.ellipse_position.as_ref(), f64::is_finite)
        }
        VectorGeometry::PolyStar(_) => {
            a.rect_size.is_none()
                && a.rect_position.is_none()
                && a.rect_roundness.is_none()
                && a.ellipse_size.is_none()
                && a.ellipse_position.is_none()
                && keys_within(a.star_position.as_ref(), f64::is_finite)
                && keys_within(a.star_rotation.as_ref(), f64::is_finite)
                && keys_within(a.star_points.as_ref(), |value| {
                    value.fract() == 0.0 && (3.0..=1000.0).contains(&value)
                })
                && keys_within(a.star_inner_radius.as_ref(), |value| value >= 0.0)
                && keys_within(a.star_outer_radius.as_ref(), |value| value >= 0.0)
                && keys_within(a.star_inner_roundness.as_ref(), |value| {
                    (0.0..=100.0).contains(&value)
                })
                && keys_within(a.star_outer_roundness.as_ref(), |value| {
                    (0.0..=100.0).contains(&value)
                })
        }
        VectorGeometry::Rect { .. } | VectorGeometry::Boolean { .. } => false,
    };
    if !valid {
        return Err(AepWriteError::Invalid(
            "animated native geometry exceeds supported bounds or belongs to another shape kind",
        ));
    }
    Ok(())
}

pub(super) fn validate_boolean(spec: &VectorBooleanSpec) -> Result<(), AepWriteError> {
    validate_appearance(&spec.appearance)?;
    if spec.operands.is_empty() {
        return Err(AepWriteError::Invalid(
            "native Boolean requires at least one geometry operand",
        ));
    }
    let mut count = 0usize;
    validate_operands(&spec.operands, &mut count)
}

fn validate_operands(operands: &[VectorGeometry], count: &mut usize) -> Result<(), AepWriteError> {
    if operands.is_empty() {
        return Err(AepWriteError::Invalid(
            "nested native Boolean requires at least one geometry operand",
        ));
    }
    for operand in operands {
        *count = count.checked_add(1).ok_or(AepWriteError::Invalid(
            "native Boolean operand count overflow",
        ))?;
        match operand {
            VectorGeometry::Path(path) => {
                let contour_count = super::path_geometry::validated_contours(path)?.len();
                if contour_count > 1 {
                    *count = count
                        .checked_add(contour_count)
                        .ok_or(AepWriteError::Invalid(
                            "native Boolean operand count overflow",
                        ))?;
                }
            }
            VectorGeometry::Ellipse(value) => {
                if value
                    .size
                    .iter()
                    .any(|size| !size.is_finite() || *size <= 0.0 || *size > 65535.0)
                    || value.position.iter().any(|value| !value.is_finite())
                {
                    return Err(AepWriteError::Invalid(
                        "invalid native Boolean Ellipse operand",
                    ));
                }
            }
            VectorGeometry::PolyStar(value) => {
                if !value.points.is_finite()
                    || value.points.fract() != 0.0
                    || !(3.0..=1000.0).contains(&value.points)
                    || value.position.iter().any(|value| !value.is_finite())
                    || [
                        value.rotation,
                        value.outer_radius,
                        value.inner_radius,
                        value.outer_roundness,
                        value.inner_roundness,
                    ]
                    .iter()
                    .any(|value| !value.is_finite())
                    || value.outer_radius < 0.0
                    || value.inner_radius < 0.0
                    || !(0.0..=100.0).contains(&value.outer_roundness)
                    || !(0.0..=100.0).contains(&value.inner_roundness)
                {
                    return Err(AepWriteError::Invalid(
                        "invalid native Boolean Star/Polygon operand",
                    ));
                }
            }
            VectorGeometry::Rect {
                size,
                position,
                roundness,
            } => {
                let center = [position[0] + size[0] / 2.0, position[1] + size[1] / 2.0];
                if size
                    .iter()
                    .any(|value| !value.is_finite() || *value <= 0.0 || *value > 65535.0)
                    || center.iter().any(|value| !value.is_finite())
                    || !roundness.is_finite()
                    || !(0.0..=100000.0).contains(roundness)
                {
                    return Err(AepWriteError::Invalid(
                        "invalid native Boolean Rectangle operand",
                    ));
                }
            }
            VectorGeometry::Boolean { operands, .. } => {
                validate_operands(operands, count)?;
            }
        }
    }
    Ok(())
}

fn keys_within(track: Option<&NumericTrack>, accepts: impl Fn(f64) -> bool) -> bool {
    track.is_none_or(|track| {
        track
            .keys
            .iter()
            .flat_map(|key| &key.values)
            .all(|value| accepts(*value))
    })
}

#[expect(dead_code, reason = "default-clock wrapper; callers pass a clock")]
pub(super) fn timeline_shape(
    spec: &VectorShapeSpec,
    id: u32,
    duration: Duration24,
) -> Result<Chunk, AepWriteError> {
    timeline_shape_with_clock(spec, id, duration, super::keyframes::PropertyClock::DEFAULT)
}

pub(super) fn timeline_shape_with_clock(
    spec: &VectorShapeSpec,
    id: u32,
    duration: Duration24,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let contents = geometry_entries(&spec.geometry, 1, Some(&spec.geometry_animations), clock)?;
    super::rects::timeline_vector_contents_with_clock(
        &spec.appearance,
        id,
        duration,
        contents,
        Some(&spec.animations),
        Some(&spec.stroke_animations),
        clock,
    )
}

#[expect(dead_code, reason = "default-clock wrapper; callers pass a clock")]
pub(super) fn timeline_boolean(
    spec: &VectorBooleanSpec,
    id: u32,
    duration: Duration24,
) -> Result<Chunk, AepWriteError> {
    timeline_boolean_with_clock(spec, id, duration, super::keyframes::PropertyClock::DEFAULT)
}

pub(super) fn timeline_boolean_with_clock(
    spec: &VectorBooleanSpec,
    id: u32,
    duration: Duration24,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let contents = boolean_entries(spec.op, &spec.operands, clock)?;
    super::rects::timeline_vector_contents_with_clock(
        &spec.appearance,
        id,
        duration,
        contents,
        Some(&spec.animations),
        Some(&spec.stroke_animations),
        clock,
    )
}

pub(super) fn validate_program(spec: &VectorLayerSpec) -> Result<(), AepWriteError> {
    if spec.name.is_empty() || spec.name.len() > 255 || spec.name.contains('\0') {
        return Err(AepWriteError::Invalid(
            "native vector layer name must be 1..=255 UTF-8 bytes without NUL",
        ));
    }
    if spec.contents.is_empty() {
        return Err(AepWriteError::Invalid("native vector program is empty"));
    }
    let transform = &spec.transform;
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
        return Err(AepWriteError::Invalid(
            "invalid native vector layer Transform",
        ));
    }
    let mut count = 0usize;
    validate_contents(&spec.contents, 0, &mut count)
}

fn validate_contents(
    contents: &[VectorContent],
    depth: usize,
    count: &mut usize,
) -> Result<(), AepWriteError> {
    if depth > 48 {
        return Err(AepWriteError::Invalid(
            "native vector group depth exceeds 48",
        ));
    }
    for content in contents {
        *count = count
            .checked_add(1)
            .ok_or(AepWriteError::Invalid("native vector entry count overflow"))?;
        match content {
            VectorContent::Geometry {
                geometry,
                animations,
            } => match geometry {
                VectorGeometry::Path(_)
                | VectorGeometry::Ellipse(_)
                | VectorGeometry::PolyStar(_) => {
                    let contour_count = match geometry {
                        VectorGeometry::Path(path) => {
                            super::path_geometry::validated_contours(path)?.len()
                        }
                        _ => 1,
                    };
                    *count = count
                        .checked_add(contour_count - 1)
                        .ok_or(AepWriteError::Invalid("native vector entry count overflow"))?;
                    let probe = VectorShapeSpec {
                        appearance: VectorAppearance {
                            name: "validation".into(),
                            stroke_dashes: Default::default(),
                            paint_opacity: 100.0,
                            fill_color: Some([0.0, 0.0, 0.0, 1.0]),
                            fill_rule: ShapeFillRule::NonZeroWinding,
                            stroke_color: None,
                            stroke_cap: ShapeLineCap::Butt,
                            stroke_width: 0.0,
                            stroke_join: ShapeLineJoin::Miter,
                            stroke_miter_limit: 4.0,
                            transform: super::SolidTransform {
                                anchor: [0.0; 2],
                                position: [0.0; 2],
                                scale: [100.0; 2],
                                rotation: 0.0,
                                opacity: 100.0,
                            },
                        },
                        geometry: geometry.clone(),
                        animations: TransformAnimations::default(),
                        geometry_animations: animations.clone(),
                        stroke_animations: Default::default(),
                    };
                    validate(&probe)?;
                }
                VectorGeometry::Rect { .. } => {
                    if animations.path.is_some()
                        || animations.ellipse_size.is_some()
                        || animations.ellipse_position.is_some()
                        || animations.star_position.is_some()
                        || animations.star_points.is_some()
                        || animations.star_rotation.is_some()
                        || animations.star_outer_radius.is_some()
                        || animations.star_inner_radius.is_some()
                        || animations.star_outer_roundness.is_some()
                        || animations.star_inner_roundness.is_some()
                        || !keys_within(animations.rect_size.as_ref(), |value| {
                            value > 0.0 && value <= 65535.0
                        })
                        || !keys_within(animations.rect_position.as_ref(), f64::is_finite)
                        || !keys_within(animations.rect_roundness.as_ref(), |value| {
                            (0.0..=100000.0).contains(&value)
                        })
                    {
                        return Err(AepWriteError::Invalid(
                            "Rectangle geometry keys exceed native bounds or belong to another shape kind",
                        ));
                    }
                    validate_operands(std::slice::from_ref(geometry), count)?;
                }
                VectorGeometry::Boolean { .. } => {
                    if animations != &GeometryAnimations::default() {
                        return Err(AepWriteError::Invalid(
                            "Boolean vector-program geometry cannot carry parametric keys",
                        ));
                    }
                    validate_operands(std::slice::from_ref(geometry), count)?;
                }
            },
            VectorContent::Group(group) => {
                validate_group(group, None, depth, count)?;
            }
            VectorContent::AnimatedGroup(group, animations) => {
                validate_group(group, Some(animations), depth, count)?;
            }
            VectorContent::Paint(paint) => validate_paint(paint)?,
            VectorContent::Modifier(modifier) => validate_modifier(modifier)?,
            VectorContent::Merge(_) => {}
        }
    }
    Ok(())
}

fn validate_group(
    group: &VectorGroupSpec,
    animations: Option<&VectorGroupAnimations>,
    depth: usize,
    count: &mut usize,
) -> Result<(), AepWriteError> {
    if group.name.is_empty() || group.name.len() > 255 || group.name.contains('\0') {
        return Err(AepWriteError::Invalid("invalid native vector group name"));
    }
    let transform = &group.transform;
    if transform
        .anchor
        .iter()
        .chain(&transform.position)
        .chain(&transform.scale)
        .any(|value| !value.is_finite())
        || [
            transform.skew,
            transform.skew_axis,
            transform.rotation,
            transform.opacity,
        ]
        .iter()
        .any(|value| !value.is_finite())
        || !(0.0..=100.0).contains(&transform.opacity)
    {
        return Err(AepWriteError::Invalid(
            "invalid native vector group Transform",
        ));
    }
    if let Some(animations) = animations
        && (!valid_group_track(animations.anchor.as_ref(), 2, f64::is_finite)
            || !valid_group_track(animations.position.as_ref(), 2, f64::is_finite)
            || !valid_group_track(animations.scale.as_ref(), 2, f64::is_finite)
            || !valid_group_track(animations.rotation.as_ref(), 1, f64::is_finite)
            || !valid_group_track(animations.skew.as_ref(), 1, f64::is_finite)
            || !valid_group_track(animations.skew_axis.as_ref(), 1, f64::is_finite)
            || !valid_group_track(animations.opacity.as_ref(), 1, |value| {
                (0.0..=100.0).contains(&value)
            }))
    {
        return Err(AepWriteError::Invalid(
            "invalid native vector group Transform animation",
        ));
    }
    validate_contents(&group.contents, depth + 1, count)
}

fn valid_group_track(
    track: Option<&NumericTrack>,
    dimensions: usize,
    accepts: impl Fn(f64) -> bool,
) -> bool {
    track.is_none_or(|track| {
        !track.keys.is_empty()
            && u16::try_from(track.keys.len()).is_ok()
            && track.keys.iter().all(|key| {
                key.values.len() == dimensions && key.values.iter().all(|value| accepts(*value))
            })
    })
}

fn validate_paint(spec: &VectorPaintSpec) -> Result<(), AepWriteError> {
    let (paint, opacity) = match spec {
        VectorPaintSpec::Fill { paint, opacity, .. }
        | VectorPaintSpec::Stroke { paint, opacity, .. } => (paint, *opacity),
    };
    if !paint.has_valid_values() || !opacity.is_finite() || !(0.0..=100.0).contains(&opacity) {
        return Err(AepWriteError::Invalid("invalid native vector paint"));
    }
    if let VectorPaintSpec::Stroke {
        width,
        miter_limit,
        dashes,
        animations,
        ..
    } = spec
    {
        if !width.is_finite() || *width < 0.0 || !miter_limit.is_finite() || *miter_limit < 0.0 {
            return Err(AepWriteError::Invalid("invalid native vector stroke"));
        }
        super::StrokeAnimations {
            width: animations.width.clone(),
            miter_limit: animations.miter_limit.clone(),
            join: animations.join.clone(),
        }
        .validate(true)?;
        dashes.native_group_with_offset(animations.dash_offset.as_ref())?;
    }
    let animations = match spec {
        VectorPaintSpec::Fill { animations, .. } | VectorPaintSpec::Stroke { animations, .. } => {
            animations
        }
    };
    let valid_opacity = animations.opacity.as_ref().is_none_or(|track| {
        track.keys.iter().all(|key| {
            key.values.len() == 1 && key.values.iter().all(|value| (0.0..=100.0).contains(value))
        })
    });
    if !keys_within(animations.color.as_ref(), |value| {
        (0.0..=1.0).contains(&value)
    }) || !valid_opacity
        || !keys_within(animations.dash_offset.as_ref(), f64::is_finite)
    {
        return Err(AepWriteError::Invalid(
            "vector paint animation exceeds native bounds",
        ));
    }
    if animations
        .color
        .as_ref()
        .is_some_and(|track| track.keys.iter().any(|key| key.values.len() != 4))
    {
        return Err(AepWriteError::Invalid(
            "vector color keys require RGBA values",
        ));
    }
    Ok(())
}

fn validate_modifier(modifier: &VectorModifierSpec) -> Result<(), AepWriteError> {
    let valid_track =
        |track: Option<&NumericTrack>, accepts: fn(f64) -> bool| keys_within(track, accepts);
    let valid = match modifier {
        VectorModifierSpec::RoundCorners { value, radius } => {
            value.radius.value().is_finite()
                && valid_track(radius.as_ref(), |value| value.is_finite() && value >= 0.0)
        }
        VectorModifierSpec::OffsetPaths { value, amount } => {
            value.amount.is_finite()
                && value.miter_limit.is_finite()
                && value.miter_limit >= 0.0
                && valid_track(amount.as_ref(), f64::is_finite)
        }
        VectorModifierSpec::TrimPaths {
            value,
            start,
            end,
            offset,
        } => {
            [value.start, value.end, value.offset]
                .iter()
                .all(|value| value.is_finite())
                && valid_track(start.as_ref(), |value| (0.0..=100.0).contains(&value))
                && valid_track(end.as_ref(), |value| (0.0..=100.0).contains(&value))
                && valid_track(offset.as_ref(), f64::is_finite)
        }
    };
    if valid {
        Ok(())
    } else {
        Err(AepWriteError::Invalid("invalid native vector modifier"))
    }
}

#[cfg(test)]
pub(super) fn timeline_program(
    spec: &VectorLayerSpec,
    id: u32,
    duration: Duration24,
) -> Result<Chunk, AepWriteError> {
    timeline_program_with_clock(spec, id, duration, super::keyframes::PropertyClock::DEFAULT)
}

pub(super) fn timeline_program_with_clock(
    spec: &VectorLayerSpec,
    id: u32,
    duration: Duration24,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let mut ordinal = 1usize;
    let contents = encode_contents(&spec.contents, &mut ordinal, clock)?;
    super::rects::timeline_vector_program_with_clock(
        &spec.name,
        &spec.transform,
        id,
        duration,
        contents,
        Some(&spec.transform_animations),
        clock,
    )
}

fn encode_contents(
    contents: &[VectorContent],
    ordinal: &mut usize,
    clock: super::keyframes::PropertyClock,
) -> Result<Vec<(&'static str, Chunk)>, AepWriteError> {
    let mut encoded = Vec::with_capacity(contents.len());
    // Each native group is a separate draw in its parent's stacking order.
    // Do not let a later paint's Above Previous jump across a rendered group.
    let mut preceding_paint = false;
    for content in contents {
        match content {
            VectorContent::Geometry {
                geometry,
                animations,
            } => {
                let entries = geometry_entries(geometry, *ordinal, Some(animations), clock)?;
                *ordinal = ordinal
                    .checked_add(entries.len())
                    .ok_or(AepWriteError::Invalid("native vector ordinal overflow"))?;
                encoded.extend(entries);
            }
            VectorContent::Group(group) => {
                encoded.push(encode_group(group, None, ordinal, clock)?);
                preceding_paint = false;
            }
            VectorContent::AnimatedGroup(group, animations) => {
                encoded.push(encode_group(group, Some(animations), ordinal, clock)?);
                preceding_paint = false;
            }
            VectorContent::Paint(paint) => {
                encoded.push(encode_paint(paint, preceding_paint, clock)?);
                preceding_paint = true;
            }
            VectorContent::Modifier(modifier) => encoded.push(encode_modifier(modifier, clock)?),
            VectorContent::Merge(op) => encoded.push((
                "ADBE Vector Filter - Merge",
                views::group(
                    1,
                    "Merge Paths",
                    vec![(
                        "ADBE Vector Merge Type",
                        views::property_with_clock(
                            ValueKind::VectorEnum,
                            &[boolean_ordinal(*op)],
                            None,
                            None,
                            clock,
                        )?,
                    )],
                )?,
            )),
        }
    }
    Ok(encoded)
}

fn encode_group(
    group: &VectorGroupSpec,
    animations: Option<&VectorGroupAnimations>,
    ordinal: &mut usize,
    clock: super::keyframes::PropertyClock,
) -> Result<(&'static str, Chunk), AepWriteError> {
    let contents = views::indexed_group(
        "Contents",
        encode_contents(&group.contents, ordinal, clock)?,
    )?;
    let entries = vec![
        (
            "ADBE Vector Blend Mode",
            views::property_with_clock(
                ValueKind::VectorEnum,
                &[blend_ordinal(group.blend_mode)?],
                None,
                None,
                clock,
            )?,
        ),
        ("ADBE Vectors Group", contents),
        (
            "ADBE Vector Transform Group",
            vector_group_transform(&group.transform, animations, clock)?,
        ),
    ];
    Ok(("ADBE Vector Group", views::group(1, &group.name, entries)?))
}

fn blend_ordinal(mode: BlendMode) -> Result<f64, AepWriteError> {
    Ok(match mode {
        BlendMode::Normal => 1.0,
        BlendMode::Darken => 3.0,
        BlendMode::Multiply => 4.0,
        BlendMode::ColorBurn => 5.0,
        BlendMode::LinearBurn => 6.0,
        BlendMode::DarkerColor => 7.0,
        BlendMode::Lighten => 9.0,
        BlendMode::Screen => 10.0,
        BlendMode::ColorDodge => 11.0,
        BlendMode::Add => 12.0,
        BlendMode::LighterColor => 13.0,
        BlendMode::Overlay => 15.0,
        BlendMode::SoftLight => 16.0,
        BlendMode::HardLight => 17.0,
        BlendMode::LinearLight => 18.0,
        BlendMode::VividLight => 19.0,
        BlendMode::PinLight => 20.0,
        BlendMode::HardMix => 21.0,
        BlendMode::Difference => 23.0,
        BlendMode::Exclusion => 24.0,
        BlendMode::Hue => 26.0,
        BlendMode::Saturation => 27.0,
        BlendMode::Color => 28.0,
        BlendMode::Luminosity => 29.0,
        BlendMode::ClassicColorBurn
        | BlendMode::ClassicColorDodge
        | BlendMode::ClassicDifference
        | BlendMode::Subtract
        | BlendMode::Divide => {
            return Err(AepWriteError::Invalid(
                "FX paint blend has no source-backed native shape ordinal",
            ));
        }
    })
}

fn encode_paint(
    paint: &VectorPaintSpec,
    above_previous: bool,
    clock: super::keyframes::PropertyClock,
) -> Result<(&'static str, Chunk), AepWriteError> {
    // AE's native ordinals are 1=Below Previous, 2=Above Previous.
    // Geometry and modifiers remain at their original positions in the scope.
    let composite_order = if above_previous { 2.0 } else { 1.0 };
    let property = |kind, values: &[f64], bounds| {
        views::property_with_clock(kind, values, bounds, None, clock)
    };
    let animated_property = |kind, values: &[f64], bounds, animation: Option<&NumericTrack>| {
        views::property_with_clock(kind, values, bounds, animation, clock)
    };
    let paint = native_static_alpha(paint);
    match paint.as_ref() {
        VectorPaintSpec::Fill {
            paint,
            fill_rule,
            blend_mode,
            opacity,
            animations,
        } => {
            let mut entries = vec![
                (
                    "ADBE Vector Blend Mode",
                    property(ValueKind::VectorEnum, &[blend_ordinal(*blend_mode)?], None)?,
                ),
                (
                    "ADBE Vector Composite Order",
                    property(ValueKind::VectorEnum, &[composite_order], None)?,
                ),
                (
                    "ADBE Vector Fill Rule",
                    property(
                        ValueKind::VectorEnum,
                        &[match fill_rule {
                            ShapeFillRule::NonZeroWinding => 1.0,
                            ShapeFillRule::EvenOdd => 2.0,
                        }],
                        None,
                    )?,
                ),
            ];
            let name = match paint {
                ShapePaint::Solid { color } => {
                    let color_track = animations.color.as_ref().map(native_color_track);
                    entries.push((
                        "ADBE Vector Fill Color",
                        animated_property(
                            ValueKind::VectorColor,
                            &super::rects::native_color(*color),
                            None,
                            color_track.as_ref(),
                        )?,
                    ));
                    "ADBE Vector Graphic - Fill"
                }
                ShapePaint::Gradient {
                    gradient_type,
                    start,
                    end,
                    stops,
                } => {
                    entries.extend(gradient_entries(
                        *gradient_type,
                        *start,
                        *end,
                        stops,
                        clock,
                    )?);
                    "ADBE Vector Graphic - G-Fill"
                }
            };
            entries.push((
                "ADBE Vector Fill Opacity",
                animated_property(
                    ValueKind::VectorScalar,
                    &[*opacity],
                    Some((0.0, 100.0)),
                    animations.opacity.as_ref(),
                )?,
            ));
            Ok((name, views::group(1, "Fill", entries)?))
        }
        VectorPaintSpec::Stroke {
            paint,
            blend_mode,
            opacity,
            width,
            cap,
            join,
            miter_limit,
            dashes,
            animations,
        } => {
            let mut entries = vec![
                (
                    "ADBE Vector Blend Mode",
                    property(ValueKind::VectorEnum, &[blend_ordinal(*blend_mode)?], None)?,
                ),
                (
                    "ADBE Vector Composite Order",
                    property(ValueKind::VectorEnum, &[composite_order], None)?,
                ),
            ];
            let name = match paint {
                ShapePaint::Solid { color } => {
                    let color_track = animations.color.as_ref().map(native_color_track);
                    entries.push((
                        "ADBE Vector Stroke Color",
                        animated_property(
                            ValueKind::VectorColor,
                            &super::rects::native_color(*color),
                            None,
                            color_track.as_ref(),
                        )?,
                    ));
                    "ADBE Vector Graphic - Stroke"
                }
                ShapePaint::Gradient {
                    gradient_type,
                    start,
                    end,
                    stops,
                } => {
                    entries.extend(gradient_entries(
                        *gradient_type,
                        *start,
                        *end,
                        stops,
                        clock,
                    )?);
                    "ADBE Vector Graphic - G-Stroke"
                }
            };
            entries.extend([
                (
                    "ADBE Vector Stroke Opacity",
                    animated_property(
                        ValueKind::VectorScalar,
                        &[*opacity],
                        Some((0.0, 100.0)),
                        animations.opacity.as_ref(),
                    )?,
                ),
                (
                    "ADBE Vector Stroke Width",
                    animated_property(
                        ValueKind::VectorScalar,
                        &[*width],
                        Some((0.0, 100.0)),
                        animations.width.as_ref(),
                    )?,
                ),
                (
                    "ADBE Vector Stroke Line Cap",
                    property(
                        ValueKind::VectorEnum,
                        &[match cap {
                            ShapeLineCap::Butt => 1.0,
                            ShapeLineCap::Round => 2.0,
                            ShapeLineCap::Square => 3.0,
                        }],
                        None,
                    )?,
                ),
                (
                    "ADBE Vector Stroke Line Join",
                    animated_property(
                        ValueKind::VectorEnum,
                        &[match join {
                            ShapeLineJoin::Miter => 1.0,
                            ShapeLineJoin::Round => 2.0,
                            ShapeLineJoin::Bevel => 3.0,
                        }],
                        None,
                        animations.join.as_ref(),
                    )?,
                ),
                (
                    "ADBE Vector Stroke Miter Limit",
                    animated_property(
                        ValueKind::VectorScalar,
                        &[*miter_limit],
                        Some((0.0, 100.0)),
                        animations.miter_limit.as_ref(),
                    )?,
                ),
            ]);
            if let Some(group) =
                dashes.native_group_with_offset_and_clock(animations.dash_offset.as_ref(), clock)?
            {
                entries.push(("ADBE Vector Stroke Dashes", group));
            }
            Ok((name, views::group(1, "Stroke", entries)?))
        }
    }
}

/// Native solid vector Color does not draw its stored alpha. Carry a static
/// alpha on that paint's Opacity instead, including paint-presence keys, without
/// moving the layer/group compositing gate. Gradient alpha stays on its stops.
/// Keyed colors require a separate varying-alpha product and are not normalized
/// here; their unused static base must not scale the live opacity controls.
fn native_static_alpha(paint: &VectorPaintSpec) -> Cow<'_, VectorPaintSpec> {
    let (source, animations) = match paint {
        VectorPaintSpec::Fill {
            paint, animations, ..
        }
        | VectorPaintSpec::Stroke {
            paint, animations, ..
        } => (paint, animations),
    };
    let ShapePaint::Solid { color } = source else {
        return Cow::Borrowed(paint);
    };
    if color[3] == 1.0 || animations.color.is_some() {
        return Cow::Borrowed(paint);
    }
    let mut native = paint.clone();
    let (source, opacity, animations) = match &mut native {
        VectorPaintSpec::Fill {
            paint,
            opacity,
            animations,
            ..
        }
        | VectorPaintSpec::Stroke {
            paint,
            opacity,
            animations,
            ..
        } => (paint, opacity, animations),
    };
    let ShapePaint::Solid { color } = source else {
        unreachable!("cloned solid paint retains its kind");
    };
    let alpha = color[3];
    color[3] = 1.0;
    *opacity *= alpha;
    if let Some(track) = &mut animations.opacity {
        for key in &mut track.keys {
            for value in &mut key.values {
                *value *= alpha;
            }
        }
    }
    Cow::Owned(native)
}

fn native_color_track(track: &NumericTrack) -> NumericTrack {
    NumericTrack {
        keys: track
            .keys
            .iter()
            .map(|key| {
                let mut key = key.clone();
                if let [red, green, blue, alpha] = key.values.as_slice() {
                    key.values = vec![alpha * 255.0, red * 255.0, green * 255.0, blue * 255.0];
                    if key.easing.len() == 4 {
                        key.easing.rotate_right(1);
                    }
                }
                key
            })
            .collect(),
    }
}

fn gradient_entries(
    gradient_type: ShapeGradientType,
    start: [f64; 2],
    end: [f64; 2],
    stops: &[ShapeGradientStop],
    clock: super::keyframes::PropertyClock,
) -> Result<Vec<(&'static str, Chunk)>, AepWriteError> {
    let property = |kind, values: &[f64], bounds| {
        views::property_with_clock(kind, values, bounds, None, clock)
    };
    let mut native_start = start;
    let mut mirrored_stops = None;
    let kind = match gradient_type {
        ShapeGradientType::Linear => 1.0,
        ShapeGradientType::Radial => 2.0,
        ShapeGradientType::Reflected => {
            native_start = [2.0 * start[0] - end[0], 2.0 * start[1] - end[1]];
            if native_start.iter().any(|value| !value.is_finite()) {
                return Err(AepWriteError::Invalid(
                    "reflected gradient mirrored axis is not finite",
                ));
            }
            mirrored_stops = Some(
                stops
                    .iter()
                    .rev()
                    .map(|stop| ShapeGradientStop {
                        offset: (1.0 - stop.offset) / 2.0,
                        color: stop.color,
                    })
                    .chain(stops.iter().map(|stop| ShapeGradientStop {
                        offset: (1.0 + stop.offset) / 2.0,
                        color: stop.color,
                    }))
                    .collect::<Vec<_>>(),
            );
            1.0
        }
        ShapeGradientType::Conic => {
            return Err(AepWriteError::Invalid(
                "conic gradient has no exact source-backed native AE shape mapping",
            ));
        }
    };
    let stops = mirrored_stops.as_deref().unwrap_or(stops);
    Ok(vec![
        (
            "ADBE Vector Grad Type",
            property(ValueKind::VectorEnum, &[kind], None)?,
        ),
        (
            "ADBE Vector Grad Start Pt",
            property(
                ValueKind::VectorSpatial,
                &[native_start[0], native_start[1]],
                None,
            )?,
        ),
        (
            "ADBE Vector Grad End Pt",
            property(ValueKind::VectorSpatial, &[end[0], end[1]], None)?,
        ),
        (
            "ADBE Vector Grad HiLite Length",
            property(ValueKind::VectorScalar, &[0.0], Some((-100.0, 100.0)))?,
        ),
        (
            "ADBE Vector Grad HiLite Angle",
            property(ValueKind::VectorAngle, &[0.0], None)?,
        ),
        ("ADBE Vector Grad Colors", gradient_colors(stops)?),
    ])
}

pub(super) fn gradient_colors(stops: &[ShapeGradientStop]) -> Result<Chunk, AepWriteError> {
    use std::fmt::Write as _;

    // Native prop.map v4 uses paired lists and typed arrays, not whitespace-
    // separated numbers. Keep alpha separate from RGB; the color array's sixth
    // component is the native constant 1, not the editable stop opacity.
    let mut xml = String::from(
        "<?xml version='1.0'?><prop.map version='4'><prop.list><prop.pair>\
         <key>Gradient Color Data</key><prop.list>",
    );
    for channel in ["Alpha", "Color"] {
        write!(
            xml,
            "<prop.pair><key>{channel} Stops</key><prop.list>\
            <prop.pair><key>Stops List</key><prop.list>"
        )
        .map_err(|_| AepWriteError::Invalid("gradient XML formatting failed"))?;
        for (index, stop) in stops.iter().enumerate() {
            write!(
                xml,
                "<prop.pair><key>Stop-{index}</key><prop.list>\
                <prop.pair><key>Stops {channel}</key><array><array.type><float/></array.type>"
            )
            .map_err(|_| AepWriteError::Invalid("gradient XML formatting failed"))?;
            let values: &[f64] = if channel == "Alpha" {
                &[stop.offset, 0.5, stop.color[3]]
            } else {
                &[
                    stop.offset,
                    0.5,
                    stop.color[0],
                    stop.color[1],
                    stop.color[2],
                    1.0,
                ]
            };
            for value in values {
                write!(xml, "<float>{value}</float>")
                    .map_err(|_| AepWriteError::Invalid("gradient XML formatting failed"))?;
            }
            xml.push_str("</array></prop.pair></prop.list></prop.pair>");
        }
        write!(
            xml,
            "</prop.list></prop.pair><prop.pair><key>Stops Size</key>\
            <int type='unsigned' size='32'>{}</int></prop.pair></prop.list></prop.pair>",
            stops.len()
        )
        .map_err(|_| AepWriteError::Invalid("gradient XML formatting failed"))?;
    }
    xml.push_str(
        "</prop.list></prop.pair><prop.pair><key>Gradient Colors</key>\
        <string>1.0</string></prop.pair></prop.list></prop.map>",
    );
    // Both independent native gradient Fill/Stroke streams carry this custom
    // value descriptor. Omitting it leaves AE's GradientColorsStream without
    // its initialization metadata when inserting the default key.
    let mut descriptor = crate::schema::view_records::StaticPropertyRecord::new(
        1,
        7,
        0,
        u32::MAX,
        0x10008,
        0,
        false,
    );
    descriptor.set_initialized();
    Ok(Chunk::list(
        *b"GCst",
        vec![
            Chunk::list(
                *b"tdbs",
                vec![
                    Chunk::data(*b"tdsb", 1_u32.to_be_bytes())?,
                    views::name_payload("-_0_/-")?,
                    Chunk::data(*b"tdb4", descriptor.encode())?,
                    Chunk::data(*b"cdat", [0_u8; 4])?,
                ],
            ),
            Chunk::list(*b"GCky", vec![Chunk::data(*b"Utf8", xml.into_bytes())?]),
        ],
    ))
}

fn encode_modifier(
    modifier: &VectorModifierSpec,
    clock: super::keyframes::PropertyClock,
) -> Result<(&'static str, Chunk), AepWriteError> {
    let property = |kind, values: &[f64], bounds| {
        views::property_with_clock(kind, values, bounds, None, clock)
    };
    let animated_property = |kind, values: &[f64], bounds, animation: Option<&NumericTrack>| {
        views::property_with_clock(kind, values, bounds, animation, clock)
    };
    Ok(match modifier {
        VectorModifierSpec::RoundCorners { value, radius } => (
            "ADBE Vector Filter - RC",
            views::group(
                1,
                "Round Corners",
                vec![(
                    "ADBE Vector RoundCorner Radius",
                    animated_property(
                        ValueKind::VectorScalar,
                        &[value.radius.value()],
                        Some((0.0, 100.0)),
                        radius.as_ref(),
                    )?,
                )],
            )?,
        ),
        VectorModifierSpec::OffsetPaths { value, amount } => (
            "ADBE Vector Filter - Offset",
            views::group(
                1,
                "Offset Paths",
                vec![
                    (
                        "ADBE Vector Offset Amount",
                        animated_property(
                            ValueKind::VectorScalar,
                            &[value.amount],
                            Some((0.0, 100.0)),
                            amount.as_ref(),
                        )?,
                    ),
                    (
                        "ADBE Vector Offset Line Join",
                        property(
                            ValueKind::VectorEnum,
                            &[match value.line_join {
                                ShapeLineJoin::Miter => 1.0,
                                ShapeLineJoin::Round => 2.0,
                                ShapeLineJoin::Bevel => 3.0,
                            }],
                            None,
                        )?,
                    ),
                    (
                        "ADBE Vector Offset Miter Limit",
                        property(
                            ValueKind::VectorScalar,
                            &[value.miter_limit],
                            Some((0.0, 100.0)),
                        )?,
                    ),
                    (
                        "ADBE Vector Offset Copies",
                        property(ValueKind::VectorScalar, &[1.0], Some((0.0, 100.0)))?,
                    ),
                    (
                        "ADBE Vector Offset Copy Offset",
                        property(ValueKind::VectorScalar, &[1.0], Some((0.0, 100.0)))?,
                    ),
                ],
            )?,
        ),
        VectorModifierSpec::TrimPaths {
            value,
            start,
            end,
            offset,
        } => (
            "ADBE Vector Filter - Trim",
            views::group(
                1,
                "Trim Paths",
                vec![
                    (
                        "ADBE Vector Trim Start",
                        animated_property(
                            ValueKind::VectorScalar,
                            &[value.start],
                            Some((0.0, 100.0)),
                            start.as_ref(),
                        )?,
                    ),
                    (
                        "ADBE Vector Trim End",
                        animated_property(
                            ValueKind::VectorScalar,
                            &[value.end],
                            Some((0.0, 100.0)),
                            end.as_ref(),
                        )?,
                    ),
                    (
                        "ADBE Vector Trim Offset",
                        animated_property(
                            ValueKind::VectorAngle,
                            &[value.offset],
                            None,
                            offset.as_ref(),
                        )?,
                    ),
                    (
                        "ADBE Vector Trim Type",
                        property(
                            ValueKind::VectorEnum,
                            &[match value.mode {
                                ShapeTrimMode::Simultaneously => 1.0,
                                ShapeTrimMode::Individually => 2.0,
                            }],
                            None,
                        )?,
                    ),
                ],
            )?,
        ),
    })
}

fn vector_group_transform(
    transform: &VectorGroupTransform,
    animations: Option<&VectorGroupAnimations>,
    clock: super::keyframes::PropertyClock,
) -> Result<Chunk, AepWriteError> {
    let animated_property = |kind, values: &[f64], bounds, animation: Option<&NumericTrack>| {
        views::property_with_clock(kind, values, bounds, animation, clock)
    };
    Ok(views::group(
        1,
        "Transform",
        vec![
            (
                "ADBE Vector Anchor",
                animated_property(
                    ValueKind::VectorSpatial,
                    &transform.anchor,
                    None,
                    animations.and_then(|value| value.anchor.as_ref()),
                )?,
            ),
            (
                "ADBE Vector Position",
                animated_property(
                    ValueKind::VectorSpatial,
                    &transform.position,
                    None,
                    animations.and_then(|value| value.position.as_ref()),
                )?,
            ),
            (
                "ADBE Vector Scale",
                animated_property(
                    ValueKind::VectorPair,
                    &transform.scale,
                    Some((-32000.0, 32000.0)),
                    animations.and_then(|value| value.scale.as_ref()),
                )?,
            ),
            (
                "ADBE Vector Skew",
                animated_property(
                    ValueKind::VectorSkew,
                    &[transform.skew],
                    Some((-85.0, 85.0)),
                    animations.and_then(|value| value.skew.as_ref()),
                )?,
            ),
            (
                "ADBE Vector Skew Axis",
                animated_property(
                    ValueKind::VectorAngle,
                    &[transform.skew_axis],
                    None,
                    animations.and_then(|value| value.skew_axis.as_ref()),
                )?,
            ),
            (
                "ADBE Vector Rotation",
                animated_property(
                    ValueKind::VectorAngle,
                    &[transform.rotation],
                    None,
                    animations.and_then(|value| value.rotation.as_ref()),
                )?,
            ),
            (
                "ADBE Vector Group Opacity",
                animated_property(
                    ValueKind::VectorScalar,
                    &[transform.opacity],
                    Some((0.0, 100.0)),
                    animations.and_then(|value| value.opacity.as_ref()),
                )?,
            ),
        ],
    )?)
}

fn boolean_entries(
    op: BooleanOp,
    operands: &[VectorGeometry],
    clock: super::keyframes::PropertyClock,
) -> Result<Vec<(&'static str, Chunk)>, AepWriteError> {
    let mut contents = Vec::with_capacity(operands.len() + 1);
    let mut ordinal = 1usize;
    let mut append_operand = |operand: &VectorGeometry| -> Result<(), AepWriteError> {
        contents.push(boolean_operand_entry(operand, ordinal, clock)?);
        ordinal = ordinal
            .checked_add(1)
            .ok_or(AepWriteError::Invalid("native Boolean ordinal overflow"))?;
        Ok(())
    };
    if op == BooleanOp::Subtract {
        for operand in operands.iter().rev() {
            append_operand(operand)?;
        }
    } else {
        for operand in operands {
            append_operand(operand)?;
        }
    }
    contents.push(merge_entry(boolean_ordinal(op), clock)?);
    Ok(contents)
}

fn boolean_operand_entry(
    geometry: &VectorGeometry,
    index: usize,
    clock: super::keyframes::PropertyClock,
) -> Result<(&'static str, Chunk), AepWriteError> {
    let mut contents = geometry_entries(geometry, index, None, clock)?;
    match contents.len() {
        0 => {
            return Err(AepWriteError::Invalid(
                "native Boolean operand has no geometry",
            ));
        }
        1 => return Ok(contents.remove(0)),
        _ => {}
    }
    // A compound Path is one Boolean operand, not an operation on every
    // contour separately. Native Merge (1), unlike Add (2), preserves winding
    // and holes before the outer Boolean consumes this identity-transformed Group.
    contents.push(merge_entry(1.0, clock)?);
    Ok((
        "ADBE Vector Group",
        views::group(
            1,
            &format!("Compound Path {index}"),
            vec![
                (
                    "ADBE Vectors Group",
                    views::indexed_group("Contents", contents)?,
                ),
                (
                    "ADBE Vector Transform Group",
                    super::rects::vector_transform_with_clock(clock)?,
                ),
            ],
        )?,
    ))
}

fn merge_entry(
    mode: f64,
    clock: super::keyframes::PropertyClock,
) -> Result<(&'static str, Chunk), AepWriteError> {
    Ok((
        "ADBE Vector Filter - Merge",
        views::group(
            1,
            "Merge Paths 1",
            vec![(
                "ADBE Vector Merge Type",
                views::property_with_clock(ValueKind::VectorEnum, &[mode], None, None, clock)?,
            )],
        )?,
    ))
}

fn boolean_ordinal(op: BooleanOp) -> f64 {
    match op {
        BooleanOp::Union => 2.0,
        BooleanOp::Subtract => 3.0,
        BooleanOp::Intersect => 4.0,
        BooleanOp::Exclude => 5.0,
    }
}

fn geometry_entries(
    geometry: &VectorGeometry,
    index: usize,
    animations: Option<&GeometryAnimations>,
    clock: super::keyframes::PropertyClock,
) -> Result<Vec<(&'static str, Chunk)>, AepWriteError> {
    if let VectorGeometry::Path(path) = geometry {
        if animations.is_some_and(|value| value.has_parametric_tracks()) {
            return Err(AepWriteError::Invalid(
                "parametric geometry keys cannot be written on a Path",
            ));
        }
        if let Some(track) = animations.and_then(|value| value.path.as_ref()) {
            return super::split_path_track(track)?
                .into_iter()
                .enumerate()
                .map(|(offset, track)| {
                    let contour_index = index
                        .checked_add(offset)
                        .ok_or(AepWriteError::Invalid("native Path ordinal overflow"))?;
                    Ok((
                        "ADBE Vector Shape - Group",
                        views::group(
                            1,
                            &format!("Path {contour_index}"),
                            vec![(
                                "ADBE Vector Shape",
                                super::path_geometry::animated_property(&track)?,
                            )],
                        )?,
                    ))
                })
                .collect();
        }
        return super::path_geometry::contours(path)?
            .iter()
            .enumerate()
            .map(|(offset, contour)| {
                let contour_index = index
                    .checked_add(offset)
                    .ok_or(AepWriteError::Invalid("native Path ordinal overflow"))?;
                path_entry(contour, contour_index, clock)
            })
            .collect();
    }
    Ok(vec![geometry_entry(geometry, index, animations, clock)?])
}

fn path_entry(
    path: &ShapePath,
    index: usize,
    clock: super::keyframes::PropertyClock,
) -> Result<(&'static str, Chunk), AepWriteError> {
    Ok((
        "ADBE Vector Shape - Group",
        views::group(
            1,
            &format!("Path {index}"),
            vec![(
                "ADBE Vector Shape",
                super::path_geometry::property_with_clock(path, clock)?,
            )],
        )?,
    ))
}

fn geometry_entry(
    geometry: &VectorGeometry,
    index: usize,
    animations: Option<&GeometryAnimations>,
    clock: super::keyframes::PropertyClock,
) -> Result<(&'static str, Chunk), AepWriteError> {
    let property = |kind, values: &[f64], bounds| {
        views::property_with_clock(kind, values, bounds, None, clock)
    };
    let animated_property = |kind, values: &[f64], bounds, animation: Option<&NumericTrack>| {
        views::property_with_clock(kind, values, bounds, animation, clock)
    };
    match geometry {
        VectorGeometry::Path(path) => {
            if let Some(track) = animations.and_then(|animations| animations.path.as_ref()) {
                Ok((
                    "ADBE Vector Shape - Group",
                    views::group(
                        1,
                        &format!("Path {index}"),
                        vec![(
                            "ADBE Vector Shape",
                            super::path_geometry::animated_property(track)?,
                        )],
                    )?,
                ))
            } else {
                path_entry(path, index, clock)
            }
        }
        VectorGeometry::Ellipse(value) => Ok((
            "ADBE Vector Shape - Ellipse",
            views::group(
                1,
                &format!("Ellipse Path {index}"),
                vec![
                    (
                        "ADBE Vector Ellipse Size",
                        animated_property(
                            ValueKind::VectorPair,
                            &value.size,
                            Some((-32000.0, 32000.0)),
                            animations.and_then(|value| value.ellipse_size.as_ref()),
                        )?,
                    ),
                    (
                        "ADBE Vector Ellipse Position",
                        animated_property(
                            ValueKind::VectorSpatial,
                            &value.position,
                            None,
                            animations.and_then(|value| value.ellipse_position.as_ref()),
                        )?,
                    ),
                    (
                        "ADBE Vector Shape Direction",
                        property(
                            ValueKind::VectorEnum,
                            &[if value.reversed { 3.0 } else { 1.0 }],
                            None,
                        )?,
                    ),
                ],
            )?,
        )),
        VectorGeometry::PolyStar(value) => Ok((
            "ADBE Vector Shape - Star",
            views::group(
                1,
                &format!("Polystar Path {index}"),
                vec![
                    (
                        "ADBE Vector Star Type",
                        property(
                            ValueKind::VectorEnum,
                            &[match value.star_type {
                                ShapePolyStarType::Star => 1.0,
                                ShapePolyStarType::Polygon => 2.0,
                            }],
                            None,
                        )?,
                    ),
                    (
                        "ADBE Vector Star Position",
                        animated_property(
                            ValueKind::VectorSpatial,
                            &value.position,
                            None,
                            animations.and_then(|value| value.star_position.as_ref()),
                        )?,
                    ),
                    (
                        "ADBE Vector Star Points",
                        animated_property(
                            ValueKind::VectorScalar,
                            &[value.points],
                            Some((0.0, 100.0)),
                            animations.and_then(|value| value.star_points.as_ref()),
                        )?,
                    ),
                    (
                        "ADBE Vector Star Rotation",
                        animated_property(
                            ValueKind::VectorAngle,
                            &[value.rotation],
                            None,
                            animations.and_then(|value| value.star_rotation.as_ref()),
                        )?,
                    ),
                    (
                        "ADBE Vector Star Outer Radius",
                        animated_property(
                            ValueKind::VectorScalar,
                            &[value.outer_radius],
                            Some((0.0, 100.0)),
                            animations.and_then(|value| value.star_outer_radius.as_ref()),
                        )?,
                    ),
                    (
                        "ADBE Vector Star Inner Radius",
                        animated_property(
                            ValueKind::VectorScalar,
                            &[value.inner_radius],
                            Some((0.0, 100.0)),
                            animations.and_then(|value| value.star_inner_radius.as_ref()),
                        )?,
                    ),
                    (
                        "ADBE Vector Star Outer Roundess",
                        animated_property(
                            ValueKind::VectorScalar,
                            &[value.outer_roundness],
                            Some((0.0, 100.0)),
                            animations.and_then(|value| value.star_outer_roundness.as_ref()),
                        )?,
                    ),
                    (
                        "ADBE Vector Star Inner Roundess",
                        animated_property(
                            ValueKind::VectorScalar,
                            &[value.inner_roundness],
                            Some((0.0, 100.0)),
                            animations.and_then(|value| value.star_inner_roundness.as_ref()),
                        )?,
                    ),
                    (
                        "ADBE Vector Shape Direction",
                        property(
                            ValueKind::VectorEnum,
                            &[if value.reversed { 3.0 } else { 1.0 }],
                            None,
                        )?,
                    ),
                ],
            )?,
        )),
        VectorGeometry::Rect {
            size,
            position,
            roundness,
        } => {
            let center = [position[0] + size[0] / 2.0, position[1] + size[1] / 2.0];
            let native_animations = animations.map(|value| super::RectAnimations {
                size: value.rect_size.clone(),
                position: value.rect_position.clone(),
                roundness: value.rect_roundness.clone(),
                ..Default::default()
            });
            Ok((
                "ADBE Vector Shape - Rect",
                super::rects::rectangle_geometry_with_clock(
                    size,
                    &center,
                    *roundness,
                    native_animations.as_ref(),
                    &format!("Rectangle Path {index}"),
                    clock,
                )?,
            ))
        }
        VectorGeometry::Boolean { op, operands } => {
            let contents =
                views::indexed_group("Contents", boolean_entries(*op, operands, clock)?)?;
            let group = views::group(
                1,
                &format!("Boolean Group {index}"),
                vec![
                    ("ADBE Vectors Group", contents),
                    (
                        "ADBE Vector Transform Group",
                        super::rects::vector_transform_with_clock(clock)?,
                    ),
                ],
            )?;
            Ok(("ADBE Vector Group", group))
        }
    }
}
