//! Conservative finite all-time bounds for native hierarchy precompositions.
//!
//! This module deliberately reasons from finite independent animator tracks. It
//! never samples frames: temporal and spatial cubic control hulls, interval
//! arithmetic, and a rotation circumradius enclose every representable value.

use std::collections::BTreeMap;

use fx_schema::{
    Dimensions, GroupLayer, Layer, LayerData, LayerId, Position, PropType, PropertyValue,
    ShapeContent, ShapeLineJoin, ShapePath, ShapePolyStar, ShapeStrokeStyle, Transform,
    animator::{AnimationGraphEntry, AnimatorData, PropertyKeyframeEasing, PropertyKeyframeTrack},
    effect::{EffectData, EffectPayload, LayerEffect},
};

use super::{Bounds, media};

/// Computes one finite enclosure for every child over all values reachable from
/// independent native-exportable tracks. The root group's own transform is not
/// applied; the caller owns the precomposition occurrence transform.
pub(super) fn child_union(
    group: &GroupLayer,
    dynamics: &[AnimationGraphEntry],
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: Dimensions,
) -> Result<Option<Bounds>, &'static str> {
    Analyzer {
        dynamics,
        resolved_media,
        canvas,
    }
    .layers_union(&group.layers)
}

pub(super) fn layer_bounds(
    layer: &Layer,
    dynamics: &[AnimationGraphEntry],
    resolved_media: &BTreeMap<String, media::ResolvedMediaSource>,
    canvas: Dimensions,
) -> Result<Option<Bounds>, &'static str> {
    Analyzer {
        dynamics,
        resolved_media,
        canvas,
    }
    .layer_bounds(layer)
}

/// Invert an animated planar occurrence using all-time keyframe/control hulls.
/// This deliberately loses correlation between keys: every rotation is covered
/// by a circumradius, so a crossing or vanishing scale cannot certify a crop.
pub(super) fn inverse_planar_demand(
    demand: &mut super::Demand,
    id: LayerId,
    transform: &Transform,
    dynamics: &[AnimationGraphEntry],
    canvas: Dimensions,
) -> Result<(), &'static str> {
    let Position::TwoD([x, y]) = transform.position else {
        return Err("3D occurrence cannot invert a planar consumer viewport");
    };
    if transform.orientation != [0.0; 3]
        || transform.rotation_x != 0.0
        || transform.rotation_y != 0.0
        || transform.skew != 0.0
        || transform.skew_axis != 0.0
    {
        return Err("Out-of-plane or skewed occurrence has no certified inverse");
    }
    if !dynamics
        .iter()
        .any(|entry| entry.target.layer_id() == Some(id))
    {
        let matrix = super::skew::matrix_components(
            transform.scale,
            transform.rotation,
            transform.skew,
            transform.skew_axis,
        )
        .map_err(|_| "Group affine transform matrix is not finite")?;
        let [ax, ay] = transform.anchor_point;
        let plane = super::demand::Homography::affine(
            matrix,
            [
                x - matrix[0] * ax - matrix[1] * ay,
                y - matrix[2] * ax - matrix[3] * ay,
            ],
        );
        demand.inverse_segments(|_| plane);
        return demand.finite_union().map(|_| ());
    }
    let empty_media = BTreeMap::new();
    let analyzer = Analyzer {
        dynamics,
        resolved_media: &empty_media,
        canvas,
    };
    analyzer.validate_layer_animators(id)?;
    // Reject properties which can change the native geometry independently of
    // the planar position/anchor/rotation/scale envelope computed below.
    if dynamics.iter().any(|entry| {
        entry.target.as_property().is_some_and(|target| {
            target.layer_id() == id
                && matches!(
                    target.property_type(),
                    PropType::Skew
                        | PropType::SkewAxis
                        | PropType::RotationX
                        | PropType::RotationY
                        | PropType::PositionZ
                        | PropType::OrientationX
                        | PropType::OrientationY
                        | PropType::OrientationZ
                )
        })
    }) {
        return Err("Animated skew or out-of-plane occurrence has no certified inverse");
    }
    let position = [
        analyzer.scalar_range(id, PropType::PositionX, x)?,
        analyzer.scalar_range(id, PropType::PositionY, y)?,
    ];
    let anchor = [
        analyzer.scalar_range(id, PropType::AnchorPointX, transform.anchor_point[0])?,
        analyzer.scalar_range(id, PropType::AnchorPointY, transform.anchor_point[1])?,
    ];
    let scale = [
        analyzer.scalar_range(id, PropType::ScaleX, transform.scale[0])?,
        analyzer.scalar_range(id, PropType::ScaleY, transform.scale[1])?,
    ];
    // A zero crossing is not invertible even if the two endpoint keys are nonzero.
    let min_scale = scale.map(|range| {
        if range.min <= 0.0 && range.max >= 0.0 {
            0.0
        } else {
            range.min.abs().min(range.max.abs()) / 100.0
        }
    });
    if min_scale
        .iter()
        .any(|scale| !scale.is_finite() || *scale <= f64::EPSILON)
    {
        return Err("Animated occurrence scale crosses zero or is singular");
    }
    analyzer.scalar_range_with_motion(id, PropType::Rotation, transform.rotation)?;
    demand.inverse_regions(|region| {
        let displacement = [0, 1].map(|axis| {
            (region.min[axis] - position[axis].max)
                .abs()
                .max((region.max[axis] - position[axis].min).abs())
        });
        let [dx, dy] = displacement;
        let radius = dx.hypot(dy);
        let region = Bounds {
            min: std::array::from_fn(|axis| anchor[axis].min - radius / min_scale[axis]),
            max: std::array::from_fn(|axis| anchor[axis].max + radius / min_scale[axis]),
        };
        if region
            .min
            .iter()
            .chain(region.max.iter())
            .any(|v| !v.is_finite())
        {
            return Err("Animated inverse consumer viewport overflowed");
        }
        Ok(region)
    });
    demand.finite_union().map(|_| ())
}

struct Analyzer<'a> {
    dynamics: &'a [AnimationGraphEntry],
    resolved_media: &'a BTreeMap<String, media::ResolvedMediaSource>,
    canvas: Dimensions,
}

impl Analyzer<'_> {
    fn layers_union(&self, layers: &[Layer]) -> Result<Option<Bounds>, &'static str> {
        // FX stores siblings top-to-bottom. Walk from the bottom so an
        // Adjustment can reason from exactly the already-enclosed suffix that
        // it consumes, without substituting its contentless guide rectangle.
        let mut result: Option<Bounds> = None;
        for layer in layers.iter().rev() {
            let next = match layer.data() {
                LayerData::Adjustment(adjustment) => self.adjustment_bounds(adjustment, result)?,
                _ => self.layer_bounds(layer)?,
            };
            if let Some(next) = next {
                match &mut result {
                    Some(result) => result.include(next),
                    None => result = Some(next),
                }
            }
        }
        Ok(result)
    }

    fn adjustment_bounds(
        &self,
        adjustment: &fx_schema::AdjustmentLayer,
        lower_stack: Option<Bounds>,
    ) -> Result<Option<Bounds>, &'static str> {
        self.validate_layer_animators(adjustment.id)?;
        let Some(lower_stack) = lower_stack else {
            return Ok(None);
        };
        if adjustment.is_hidden {
            return Ok(None);
        }
        let mut enabled_effects =
            adjustment
                .effects
                .iter()
                .filter_map(|effect| match effect.data() {
                    EffectData::Identified {
                        enabled: true,
                        effect,
                        ..
                    }
                    | EffectData::Legacy(effect) => Some(effect),
                    EffectData::Identified { enabled: false, .. } => None,
                });
        let Some(first) = enabled_effects.next() else {
            return Ok(None);
        };
        if !matches!(first, EffectPayload::Known(LayerEffect::Mosaic { .. }))
            || enabled_effects
                .any(|effect| !matches!(effect, EffectPayload::Known(LayerEffect::Mosaic { .. })))
        {
            return Err(
                "Animated Adjustment effect can expand or move lower-stack support; only Mosaic has a proven finite sampling-domain enclosure",
            );
        }

        // FX Mosaic samples neighboring pixels, so it can paint OUTSIDE the
        // lower stack's geometry. Its declared layer_size is the composition
        // canvas (AdjustmentLayer::apply_to_stack), and the renderer's Mosaic
        // content cutoff uses that rect while measurable geometry stays inside
        // it, or the escaped geometry AABB otherwise. Enclose both domains over
        // all times; never substitute the Adjustment's own geometric Transform.
        // Unknown/unmeasurable descendants still fail in layer_bounds below.
        let mut support = lower_stack;
        support.include(Bounds {
            min: [0.0, 0.0],
            max: [f64::from(self.canvas.width), f64::from(self.canvas.height)],
        });
        Ok(Some(support))
    }

    fn layer_bounds(&self, layer: &Layer) -> Result<Option<Bounds>, &'static str> {
        self.validate_layer_animators(layer.id())?;
        match layer.data() {
            LayerData::Rect(rect) => {
                if rect.is_hidden {
                    return Ok(None);
                }
                let size = self.vector_range(rect.id, PropType::RectSize, rect.rect.size)?;
                let mut bounds = bounds_from_origin_and_size(rect.rect.position, size)?;
                if rect.rect.stroke_enabled && rect.rect.stroke_color.is_some() {
                    let width = self.scalar_range(
                        rect.id,
                        PropType::StrokeWidth,
                        rect.rect.stroke_width.value(),
                    )?;
                    let miter = self.scalar_range(
                        rect.id,
                        PropType::StrokeMiterLimit,
                        rect.rect.stroke_miter_limit,
                    )?;
                    bounds = expand(bounds, stroke_reach(width, miter)?)?;
                }
                self.transform_bounds(bounds, rect.id, &rect.transform)
                    .map(Some)
            }
            LayerData::Shape(shape) => {
                if shape.is_hidden {
                    return Ok(None);
                }
                let mut bounds = self.shape_content_bounds(shape.id, &shape.shape)?;
                bounds = expand(bounds, self.shape_modifier_reach(shape.id, &shape.shape)?)?;
                self.transform_bounds(bounds, shape.id, &shape.transform)
                    .map(Some)
            }
            LayerData::Group(group) => {
                if group.is_hidden {
                    return Ok(None);
                }
                super::check_static_nested_group(group)?;
                let Some(bounds) = self.layers_union(&group.layers)? else {
                    return Ok(None);
                };
                // Inspect the entire input first, including geometry outside
                // the mask that can contribute through child Glow. Only its
                // masked output has the independently certified finite hull.
                let bounds = super::collapsed::mask_output(group, self.dynamics).unwrap_or(bounds);
                self.transform_bounds(bounds, group.id, &group.transform)
                    .map(Some)
            }
            LayerData::BooleanOperation(boolean) => {
                if boolean.is_hidden {
                    return Ok(None);
                }
                let Some(mut bounds) = self.layers_union(&boolean.layers)? else {
                    return Ok(None);
                };
                bounds = expand(bounds, self.strokes_reach(boolean.id, &boolean.strokes)?)?;
                self.transform_bounds(bounds, boolean.id, &boolean.transform)
                    .map(Some)
            }
            LayerData::Image(image) => {
                if image.is_hidden {
                    return Ok(None);
                }
                self.media_bounds(layer, image.id, &image.transform)
            }
            LayerData::Video(video) => {
                if video.is_hidden {
                    return Ok(None);
                }
                self.media_bounds(layer, video.id, &video.transform)
            }
            LayerData::Audio(_) => Ok(None),
            LayerData::Text(_) => Err("Text/font glyph bounds are not known from the FX text box"),
            _ => Err("Layer kind has no proven finite animated precomposition render bounds"),
        }
    }

    fn media_bounds(
        &self,
        layer: &Layer,
        id: LayerId,
        transform: &Transform,
    ) -> Result<Option<Bounds>, &'static str> {
        let request = media::request(layer).ok_or("Visual media has no archive request")?;
        let source = self
            .resolved_media
            .get(request.asset_id.as_str())
            .ok_or("Visual media archive source was not resolved for animated bounds")?;
        let spec = media::lower(layer, source, self.canvas)?;
        let size = spec.source.dimensions.map(f64::from);
        let geometry = spec.source_geometry;
        let bounds = normalized_bounds(
            geometry.origin,
            [
                geometry.origin[0] + size[0] * geometry.scale[0],
                geometry.origin[1] + size[1] * geometry.scale[1],
            ],
        )?;
        self.transform_bounds(bounds, id, transform).map(Some)
    }

    fn shape_content_bounds(
        &self,
        id: LayerId,
        shape: &ShapeContent,
    ) -> Result<Bounds, &'static str> {
        match (&shape.poly_star, &shape.ellipse) {
            (None, None) => self.path_bounds(id, &shape.path),
            (Some(star), None) => self.poly_star_bounds(id, star),
            (None, Some(ellipse)) => {
                let center = self.vector_range(id, PropType::EllipsePosition, ellipse.position)?;
                let size = self.vector_range(id, PropType::EllipseSize, ellipse.size)?;
                centered_bounds(center, [size[0].max_abs()? / 2.0, size[1].max_abs()? / 2.0])
            }
            (Some(star), Some(ellipse)) => {
                let mut bounds = self.poly_star_bounds(id, star)?;
                let center = self.vector_range(id, PropType::EllipsePosition, ellipse.position)?;
                let size = self.vector_range(id, PropType::EllipseSize, ellipse.size)?;
                bounds.include(centered_bounds(
                    center,
                    [size[0].max_abs()? / 2.0, size[1].max_abs()? / 2.0],
                )?);
                Ok(bounds)
            }
        }
    }

    fn path_bounds(&self, id: LayerId, base: &ShapePath) -> Result<Bounds, &'static str> {
        let bounds = |value: &PropertyValue| match value {
            PropertyValue::Path(path) => super::path_bounds(&path.commands),
            _ => Err("Path animator has a non-Path value"),
        };
        let track = match self.source(id, PropType::ShapePath)? {
            None => return super::path_bounds(&base.commands),
            Some(Source::Constant(value)) => return bounds(value),
            Some(Source::Track(track)) => track,
        };
        let keys = track.keyframes();
        let mut result = bounds(keys.first().ok_or("Empty Path keyframe track")?.value())?;
        for pair in keys.windows(2) {
            let from = bounds(pair[0].value())?;
            let to = bounds(pair[1].value())?;
            result.include(to);
            if pair[1].easing() == PropertyKeyframeEasing::Hold {
                continue;
            }
            let progress = easing_progress_range(pair[1].easing())?;
            if progress.min >= 0.0 && progress.max <= 1.0 {
                continue;
            }
            // Path controls interpolate coordinates, so their convex hull
            // bounds the whole curve. Interval arithmetic also encloses easing
            // overshoot, without sampling frames or assuming control alignment.
            let axis = |axis: usize| {
                let left = Interval::new(from.min[axis], from.max[axis])?;
                let right = Interval::new(to.min[axis], to.max[axis])?;
                left.add(right.subtract(left)?.multiply(progress)?)
            };
            result.include(bounds_from_intervals([axis(0)?, axis(1)?])?);
        }
        Ok(result)
    }

    fn poly_star_bounds(&self, id: LayerId, star: &ShapePolyStar) -> Result<Bounds, &'static str> {
        let center = self.vector_range(id, PropType::PolyStarPosition, star.position)?;
        let outer = self
            .scalar_range(id, PropType::PolyStarOuterRadius, star.outer_radius)?
            .max_abs()?;
        let inner = self
            .scalar_range(id, PropType::PolyStarInnerRadius, star.inner_radius)?
            .max_abs()?;
        let outer_roundness = self
            .scalar_range(id, PropType::PolyStarOuterRoundness, star.outer_roundness)?
            .max_abs()?;
        let inner_roundness = self
            .scalar_range(id, PropType::PolyStarInnerRoundness, star.inner_roundness)?
            .max_abs()?;
        // The generator clamps points to at least three. Both its polygon
        // angleDelta/4 and star angleDelta/2 tangent factors are <= PI/6.
        let tangent = std::f64::consts::PI / 6.0;
        let reach = |radius: f64, roundness: f64| {
            radius * (1.0 + (roundness / 100.0 * tangent).powi(2)).sqrt()
        };
        let radius = reach(outer, outer_roundness).max(reach(inner, inner_roundness));
        if !radius.is_finite() {
            return Err("PolyStar animated control hull is non-finite");
        }
        // These do not enlarge the circumradius but must still be finite and
        // independently representable.
        self.scalar_range(id, PropType::PolyStarPoints, star.points)?;
        self.scalar_range(id, PropType::PolyStarRotation, star.rotation)?;
        centered_bounds(center, [radius; 2])
    }

    fn shape_modifier_reach(&self, id: LayerId, shape: &ShapeContent) -> Result<f64, &'static str> {
        if let Some(round) = &shape.round_corners {
            self.scalar_range(id, PropType::RoundCornersRadius, round.radius.value())?;
        }
        if let Some(trim) = &shape.trim {
            self.scalar_range(id, PropType::TrimStart, trim.start)?;
            self.scalar_range(id, PropType::TrimEnd, trim.end)?;
            self.scalar_range(id, PropType::TrimOffset, trim.offset)?;
        }
        let offset = match shape.offset_paths {
            Some(offset) => {
                let amount = self
                    .scalar_range(id, PropType::OffsetPathsAmount, offset.amount)?
                    .max_abs()?;
                let multiplier = if offset.line_join == ShapeLineJoin::Miter {
                    finite_max_one(offset.miter_limit)?
                } else {
                    1.0
                };
                checked_product(amount, multiplier, "Offset Paths reach is non-finite")?
            }
            None => 0.0,
        };
        checked_sum(
            offset,
            self.strokes_reach(id, &shape.strokes)?,
            "Shape style expansion is non-finite",
        )
    }

    fn strokes_reach(
        &self,
        id: LayerId,
        strokes: &[ShapeStrokeStyle],
    ) -> Result<f64, &'static str> {
        let mut result: f64 = 0.0;
        for stroke in strokes.iter().filter(|stroke| stroke.enabled) {
            let width = self.scalar_range(id, PropType::StrokeWidth, stroke.width.value())?;
            let miter = self.scalar_range(id, PropType::StrokeMiterLimit, stroke.miter_limit)?;
            result = result.max(stroke_reach(width, miter)?);
        }
        Ok(result)
    }

    fn transform_bounds(
        &self,
        bounds: Bounds,
        id: LayerId,
        transform: &Transform,
    ) -> Result<Bounds, &'static str> {
        if transform.orientation != [0.0; 3] {
            return Err("Orientation descendant bounds are not proven for precomposition");
        }
        let anchor = [
            self.scalar_range(id, PropType::AnchorPointX, transform.anchor_point[0])?,
            self.scalar_range(id, PropType::AnchorPointY, transform.anchor_point[1])?,
        ];
        let (base_position, is_3d) = match transform.position {
            Position::TwoD([x, y]) => {
                if self.source(id, PropType::PositionZ)?.is_some() {
                    return Err("A 2D descendant cannot have Position Z bounds");
                }
                ([x, y, 0.0], false)
            }
            Position::ThreeD(position) => (position, true),
        };
        let position = [
            self.scalar_range(id, PropType::PositionX, base_position[0])?,
            self.scalar_range(id, PropType::PositionY, base_position[1])?,
            self.scalar_range(id, PropType::PositionZ, base_position[2])?,
        ];
        let scale = [
            self.scalar_range(id, PropType::ScaleX, transform.scale[0])?
                .scaled(0.01)?,
            self.scalar_range(id, PropType::ScaleY, transform.scale[1])?
                .scaled(0.01)?,
        ];
        let rotation = self
            .scalar_range_with_motion(id, PropType::Rotation, transform.rotation)?
            .0;
        let rotation_x = self
            .scalar_range_with_motion(id, PropType::RotationX, transform.rotation_x)?
            .0;
        let rotation_y = self
            .scalar_range_with_motion(id, PropType::RotationY, transform.rotation_y)?
            .0;
        let skew = self
            .scalar_range_with_motion(id, PropType::Skew, transform.skew)?
            .0
            .clamp(-89.9, 89.9)?;
        let skew_axis = self
            .scalar_range_with_motion(id, PropType::SkewAxis, transform.skew_axis)?
            .0;
        let local = [
            Interval::new(bounds.min[0], bounds.max[0])?
                .subtract(anchor[0])?
                .multiply(scale[0])?,
            Interval::new(bounds.min[1], bounds.max[1])?
                .subtract(anchor[1])?
                .multiply(scale[1])?,
        ];

        // Match FX's corner projection exactly: Rz · Ry · Rx · R(-axis) ·
        // Shear(-tan(skew)) · R(axis), applied after scale and anchor.
        let (axis_sin, axis_cos) = degree_sin_cos(skew_axis)?;
        let axis_x = axis_cos
            .multiply(local[0])?
            .subtract(axis_sin.multiply(local[1])?)?;
        let axis_y = axis_sin
            .multiply(local[0])?
            .add(axis_cos.multiply(local[1])?)?;
        let shear = skew
            .scaled(std::f64::consts::PI / 180.0)?
            .tan_monotonic()?
            .scaled(-1.0)?;
        let sheared_x = axis_x.add(shear.multiply(axis_y)?)?;
        let unaxis_x = axis_cos
            .multiply(sheared_x)?
            .add(axis_sin.multiply(axis_y)?)?;
        let unaxis_y = axis_sin
            .scaled(-1.0)?
            .multiply(sheared_x)?
            .add(axis_cos.multiply(axis_y)?)?;

        let (sin_x, cos_x) = degree_sin_cos(rotation_x)?;
        let x1 = unaxis_x;
        let y1 = unaxis_y.multiply(cos_x)?;
        let z1 = unaxis_y.multiply(sin_x)?;
        let (sin_y, cos_y) = degree_sin_cos(rotation_y)?;
        let x2 = x1.multiply(cos_y)?.add(z1.multiply(sin_y)?)?;
        let y2 = y1;
        let z2 = x1.scaled(-1.0)?.multiply(sin_y)?.add(z1.multiply(cos_y)?)?;
        let (sin_z, cos_z) = degree_sin_cos(rotation)?;
        let world = [
            position[0].add(x2.multiply(cos_z)?.subtract(y2.multiply(sin_z)?)?)?,
            position[1].add(x2.multiply(sin_z)?.add(y2.multiply(cos_z)?)?)?,
            position[2].add(z2)?,
        ];

        if !is_3d && rotation_x.is_zero() && rotation_y.is_zero() {
            return bounds_from_intervals([world[0], world[1]]);
        }
        project_through_root_camera(world, self.canvas)
    }

    fn validate_layer_animators(&self, id: LayerId) -> Result<(), &'static str> {
        for entry in self
            .dynamics
            .iter()
            .filter(|entry| entry.target.layer_id() == Some(id))
        {
            if !entry.dependencies.is_empty()
                || entry.random_seed_target.is_some()
                || !entry.layer_refs.is_empty()
            {
                return Err("Dependent animator has no finite independent all-time enclosure");
            }
            match entry.animator.data() {
                AnimatorData::JsScript { .. } => {
                    return Err("JavaScript animator has no finite all-time enclosure");
                }
                AnimatorData::Constant { value } if !value.is_finite() => {
                    return Err("Constant animator value is non-finite");
                }
                AnimatorData::Keyframes {
                    track,
                    enabled: true,
                    ..
                } if track.keyframes().iter().any(|key| !key.value().is_finite()) => {
                    return Err("Keyframe animator contains a non-finite value");
                }
                AnimatorData::Keyframes {
                    enabled: false,
                    disabled_value: Some(value),
                    ..
                } if !value.is_finite() => {
                    return Err("Disabled keyframe animator value is non-finite");
                }
                AnimatorData::Keyframes {
                    enabled: false,
                    disabled_value: None,
                    ..
                } => {
                    return Err("Disabled keyframe animator has no static fallback value");
                }
                _ => {}
            }
            let property = entry
                .target
                .as_property()
                .ok_or("Layer animator target is not a native property")?
                .property_type();
            match property {
                PropType::OrientationX | PropType::OrientationY | PropType::OrientationZ => {
                    return Err("Animated Orientation descendant bounds are not proven");
                }

                PropType::StrokeEnabled => {
                    return Err(
                        "Animated stroke presence has no established native bounds mapping",
                    );
                }
                PropType::MediaSourceAssetId => {
                    return Err("Animated media source dimensions are not fixed for bounds");
                }
                PropType::DropShadowEnabled
                | PropType::DropShadowOffset
                | PropType::DropShadowBlurRadius
                | PropType::DropShadowSpreadRadius => {
                    return Err("Animated expanding layer style has no proven render enclosure");
                }
                PropType::PaddingTop
                | PropType::PaddingRight
                | PropType::PaddingBottom
                | PropType::PaddingLeft
                | PropType::CornerRadiusTopLeft
                | PropType::CornerRadiusTopRight
                | PropType::CornerRadiusBottomRight
                | PropType::CornerRadiusBottomLeft => {
                    return Err("Animated Group background geometry is not bounded here");
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn scalar_range(
        &self,
        id: LayerId,
        property: PropType,
        base: f64,
    ) -> Result<Interval, &'static str> {
        self.scalar_range_with_motion(id, property, base)
            .map(|(range, _)| range)
    }

    fn scalar_range_with_motion(
        &self,
        id: LayerId,
        property: PropType,
        base: f64,
    ) -> Result<(Interval, bool), &'static str> {
        match self.source(id, property)? {
            None => Ok((Interval::point(base)?, false)),
            Some(Source::Constant(PropertyValue::Float(value))) => {
                Ok((Interval::point(*value)?, false))
            }
            Some(Source::Constant(_)) => Err("Animator value is not scalar"),
            Some(Source::Track(track)) => {
                Ok((track_component_range(track, Component::Scalar)?, true))
            }
        }
    }

    fn vector_range(
        &self,
        id: LayerId,
        property: PropType,
        base: [f64; 2],
    ) -> Result<[Interval; 2], &'static str> {
        match self.source(id, property)? {
            None => Ok([Interval::point(base[0])?, Interval::point(base[1])?]),
            Some(Source::Constant(PropertyValue::Vector2(value))) => {
                Ok([Interval::point(value[0])?, Interval::point(value[1])?])
            }
            Some(Source::Constant(_)) => Err("Animator value is not a two-dimensional vector"),
            Some(Source::Track(track)) => Ok([
                track_component_range(track, Component::Vector(0))?,
                track_component_range(track, Component::Vector(1))?,
            ]),
        }
    }

    fn source(&self, id: LayerId, property: PropType) -> Result<Option<Source<'_>>, &'static str> {
        let Some(entry) = self.dynamics.iter().find(|entry| {
            entry
                .target
                .as_property()
                .is_some_and(|target| target.layer_id() == id && target.property_type() == property)
        }) else {
            return Ok(None);
        };
        match entry.animator.data() {
            AnimatorData::Constant { value } => Ok(Some(Source::Constant(value))),
            AnimatorData::Keyframes {
                track,
                enabled: true,
                ..
            } => Ok(Some(Source::Track(track))),
            AnimatorData::Keyframes {
                enabled: false,
                disabled_value: Some(value),
                ..
            } => Ok(Some(Source::Constant(value))),
            AnimatorData::Keyframes {
                enabled: false,
                disabled_value: None,
                ..
            } => Err("Disabled keyframe animator has no static fallback value"),
            AnimatorData::JsScript { .. } => {
                Err("JavaScript animator has no finite all-time enclosure")
            }
        }
    }
}

enum Source<'a> {
    Constant(&'a PropertyValue),
    Track(&'a PropertyKeyframeTrack),
}

#[derive(Clone, Copy)]
enum Component {
    Scalar,
    Vector(usize),
}

fn component(value: &PropertyValue, component: Component) -> Result<f64, &'static str> {
    match (value, component) {
        (PropertyValue::Float(value), Component::Scalar) => Ok(*value),
        (PropertyValue::Vector2(value), Component::Vector(axis)) => Ok(value[axis]),
        _ => Err("Keyframe value kind does not match its bounded property"),
    }
}

fn track_component_range(
    track: &PropertyKeyframeTrack,
    component_kind: Component,
) -> Result<Interval, &'static str> {
    let keys = track.keyframes();
    let first = keys.first().ok_or("Animator keyframe track is empty")?;
    let mut result = Interval::point(component(first.value(), component_kind)?)?;
    for key in &keys[1..] {
        result.include(component(key.value(), component_kind)?)?;
    }
    for pair in keys.windows(2) {
        let from = &pair[0];
        let to = &pair[1];
        if to.easing() == PropertyKeyframeEasing::Hold {
            continue;
        }
        let from_value = component(from.value(), component_kind)?;
        let to_value = component(to.value(), component_kind)?;
        let progress = easing_progress_range(to.easing())?;
        let has_spatial = matches!(component_kind, Component::Scalar)
            && (from.spatial_out_tangent().is_some() || to.spatial_in_tangent().is_some());
        if has_spatial {
            let delta = to_value - from_value;
            let first_control = from_value + from.spatial_out_tangent().unwrap_or(delta / 3.0);
            let second_control = to_value + to.spatial_in_tangent().unwrap_or(-delta / 3.0);
            let segment = cubic_range(
                from_value,
                first_control,
                second_control,
                to_value,
                progress,
            )?;
            result.include_interval(segment);
        } else {
            result.include_interval(
                Interval::point(from_value)?
                    .add(Interval::point(to_value - from_value)?.multiply(progress)?)?,
            );
        }
    }
    Ok(result)
}

fn easing_progress_range(easing: PropertyKeyframeEasing) -> Result<Interval, &'static str> {
    match easing {
        PropertyKeyframeEasing::Hold | PropertyKeyframeEasing::Linear => Interval::new(0.0, 1.0),
        PropertyKeyframeEasing::CubicBezier { y1, y2, .. } => {
            let mut result = Interval::new(0.0, 1.0)?;
            result.include(y1)?;
            result.include(y2)?;
            Ok(result)
        }
    }
}

fn cubic_range(
    p0: f64,
    p1: f64,
    p2: f64,
    p3: f64,
    parameter: Interval,
) -> Result<Interval, &'static str> {
    let evaluate = |t: f64| {
        let inverse = 1.0 - t;
        inverse.powi(3) * p0
            + 3.0 * inverse.powi(2) * t * p1
            + 3.0 * inverse * t.powi(2) * p2
            + t.powi(3) * p3
    };
    let mut result = Interval::new(evaluate(parameter.min), evaluate(parameter.max))?;
    for control in [p0, p1, p2, p3] {
        result.include(control)?;
    }
    let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
    let b = 3.0 * p0 - 6.0 * p1 + 3.0 * p2;
    let c = -3.0 * p0 + 3.0 * p1;
    if a.abs() <= f64::EPSILON {
        if b.abs() > f64::EPSILON {
            let root = -c / (2.0 * b);
            if root >= parameter.min && root <= parameter.max {
                result.include(evaluate(root))?;
            }
        }
    } else {
        let discriminant = 4.0 * b * b - 12.0 * a * c;
        if discriminant.is_finite() && discriminant >= 0.0 {
            let root = discriminant.sqrt();
            for candidate in [(-2.0 * b - root) / (6.0 * a), (-2.0 * b + root) / (6.0 * a)] {
                if candidate >= parameter.min && candidate <= parameter.max {
                    result.include(evaluate(candidate))?;
                }
            }
        }
    }
    Ok(result)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Interval {
    min: f64,
    max: f64,
}

impl Interval {
    fn new(a: f64, b: f64) -> Result<Self, &'static str> {
        if !a.is_finite() || !b.is_finite() {
            return Err("Animated bounds interval is non-finite");
        }
        Ok(Self {
            min: a.min(b),
            max: a.max(b),
        })
    }

    fn point(value: f64) -> Result<Self, &'static str> {
        Self::new(value, value)
    }

    fn include(&mut self, value: f64) -> Result<(), &'static str> {
        if !value.is_finite() {
            return Err("Animated bounds interval is non-finite");
        }
        self.min = self.min.min(value);
        self.max = self.max.max(value);
        Ok(())
    }

    fn include_interval(&mut self, other: Self) {
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
    }

    fn add(self, other: Self) -> Result<Self, &'static str> {
        Self::new(self.min + other.min, self.max + other.max)
    }

    fn subtract(self, other: Self) -> Result<Self, &'static str> {
        Self::new(self.min - other.max, self.max - other.min)
    }

    fn multiply(self, other: Self) -> Result<Self, &'static str> {
        let values = [
            self.min * other.min,
            self.min * other.max,
            self.max * other.min,
            self.max * other.max,
        ];
        if values.iter().any(|value| !value.is_finite()) {
            return Err("Animated bounds multiplication overflowed");
        }
        Ok(Self {
            min: values.into_iter().fold(f64::INFINITY, f64::min),
            max: values.into_iter().fold(f64::NEG_INFINITY, f64::max),
        })
    }

    fn scaled(self, value: f64) -> Result<Self, &'static str> {
        self.multiply(Self::point(value)?)
    }

    fn clamp(self, min: f64, max: f64) -> Result<Self, &'static str> {
        if !min.is_finite() || !max.is_finite() || min > max {
            return Err("Animated bounds clamp is invalid");
        }
        Self::new(self.min.clamp(min, max), self.max.clamp(min, max))
    }

    fn tan_monotonic(self) -> Result<Self, &'static str> {
        Self::new(self.min.tan(), self.max.tan())
    }

    fn reciprocal_positive(self) -> Result<Self, &'static str> {
        if self.min <= f64::EPSILON {
            return Err("3D descendant reaches the root camera near plane");
        }
        Self::new(1.0 / self.max, 1.0 / self.min)
    }

    fn is_zero(self) -> bool {
        self.min == 0.0 && self.max == 0.0
    }

    fn max_abs(self) -> Result<f64, &'static str> {
        let value = self.min.abs().max(self.max.abs());
        value
            .is_finite()
            .then_some(value)
            .ok_or("Animated bounds absolute extent is non-finite")
    }
}

fn degree_sin_cos(degrees: Interval) -> Result<(Interval, Interval), &'static str> {
    let radians = degrees.scaled(std::f64::consts::PI / 180.0)?;
    Ok((
        trigonometric_range(radians, std::f64::consts::FRAC_PI_2, f64::sin)?,
        trigonometric_range(radians, 0.0, f64::cos)?,
    ))
}

fn trigonometric_range(
    angle: Interval,
    first_critical: f64,
    endpoint: fn(f64) -> f64,
) -> Result<Interval, &'static str> {
    let span = angle.max - angle.min;
    // Argument reduction cannot reliably locate critical points once angles are
    // this large. Returning the complete codomain remains finite and proved.
    if !span.is_finite() || span >= std::f64::consts::TAU || angle.max_abs()? > 1_000_000_000_000.0
    {
        return Interval::new(-1.0, 1.0);
    }
    let mut result = Interval::new(endpoint(angle.min), endpoint(angle.max))?;
    let first = ((angle.min - first_critical) / std::f64::consts::PI).ceil();
    let last = ((angle.max - first_critical) / std::f64::consts::PI).floor();
    let mut critical = first;
    // A sub-TAU interval contains at most two extrema. The explicit cap keeps
    // the proof budget fixed even in the face of floating-point boundary noise.
    for _ in 0..3 {
        if critical > last {
            break;
        }
        let extremum = if critical.rem_euclid(2.0) == 0.0 {
            1.0
        } else {
            -1.0
        };
        result.include(extremum)?;
        critical += 1.0;
    }
    Ok(result)
}

fn project_through_root_camera(
    world: [Interval; 3],
    canvas: Dimensions,
) -> Result<Bounds, &'static str> {
    const CAMERA_DISTANCE_PER_WIDTH: f64 = 1.388;

    let distance = f64::from(canvas.width) * CAMERA_DISTANCE_PER_WIDTH;
    if !distance.is_finite() || distance <= 0.0 {
        return Err("Root camera distance is not finite and positive");
    }
    let denominator = Interval::point(distance)?.add(world[2])?;
    let factor = denominator.reciprocal_positive()?.scaled(distance)?;
    let center = [
        f64::from(canvas.width) / 2.0,
        f64::from(canvas.height) / 2.0,
    ];
    bounds_from_intervals([
        Interval::point(center[0])?.add(
            world[0]
                .subtract(Interval::point(center[0])?)?
                .multiply(factor)?,
        )?,
        Interval::point(center[1])?.add(
            world[1]
                .subtract(Interval::point(center[1])?)?
                .multiply(factor)?,
        )?,
    ])
}

fn bounds_from_origin_and_size(
    origin: [f64; 2],
    size: [Interval; 2],
) -> Result<Bounds, &'static str> {
    let axis = |index: usize| {
        Interval::new(
            origin[index].min(origin[index] + size[index].min),
            origin[index].max(origin[index] + size[index].max),
        )
    };
    bounds_from_intervals([axis(0)?, axis(1)?])
}

fn centered_bounds(center: [Interval; 2], radius: [f64; 2]) -> Result<Bounds, &'static str> {
    bounds_from_intervals([
        center[0].add(Interval::new(-radius[0], radius[0])?)?,
        center[1].add(Interval::new(-radius[1], radius[1])?)?,
    ])
}

fn bounds_from_intervals(axis: [Interval; 2]) -> Result<Bounds, &'static str> {
    normalized_bounds([axis[0].min, axis[1].min], [axis[0].max, axis[1].max])
}

fn normalized_bounds(a: [f64; 2], b: [f64; 2]) -> Result<Bounds, &'static str> {
    if a.into_iter().chain(b).any(|value| !value.is_finite()) {
        return Err("Animated render enclosure is non-finite");
    }
    Ok(Bounds {
        min: [a[0].min(b[0]), a[1].min(b[1])],
        max: [a[0].max(b[0]), a[1].max(b[1])],
    })
}

fn expand(bounds: Bounds, amount: f64) -> Result<Bounds, &'static str> {
    if !amount.is_finite() || amount < 0.0 {
        return Err("Animated shape expansion is not finite and nonnegative");
    }
    normalized_bounds(
        [bounds.min[0] - amount, bounds.min[1] - amount],
        [bounds.max[0] + amount, bounds.max[1] + amount],
    )
}

fn stroke_reach(width: Interval, miter: Interval) -> Result<f64, &'static str> {
    checked_product(
        width.max_abs()? / 2.0,
        miter.max_abs()?.max(1.0),
        "Animated stroke/miter reach is non-finite",
    )
}

fn finite_max_one(value: f64) -> Result<f64, &'static str> {
    value
        .is_finite()
        .then_some(value.abs().max(1.0))
        .ok_or("Shape miter limit is non-finite")
}

fn checked_product(a: f64, b: f64, message: &'static str) -> Result<f64, &'static str> {
    let value = a * b;
    value.is_finite().then_some(value).ok_or(message)
}

fn checked_sum(a: f64, b: f64, message: &'static str) -> Result<f64, &'static str> {
    let value = a + b;
    value.is_finite().then_some(value).ok_or(message)
}

#[cfg(test)]
#[path = "continuous_3d_bounds_tests.rs"]
mod continuous_3d_bounds_tests;

#[cfg(test)]
mod tests {
    use fx_schema::{
        LayerId, PropertyTarget, PropertyValue, ShapePath, TimeOffset,
        animator::{
            AnimationGraphEntry, AnimatorData, KeyframeId, PropertyAnimator, PropertyKeyframe,
            PropertyKeyframeTrack,
        },
    };
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::{structure::read_project, structure_document::to_structural_fx_document};

    fn key(
        name: &str,
        millis: i64,
        value: f64,
        easing: PropertyKeyframeEasing,
    ) -> PropertyKeyframe {
        PropertyKeyframe::new(
            KeyframeId::new(name),
            TimeOffset::from_millis(millis),
            PropertyValue::Float(value),
            easing,
        )
    }

    fn entry(property: PropType, animator: PropertyAnimator) -> AnimationGraphEntry {
        AnimationGraphEntry {
            target: PropertyTarget::layer(LayerId::new(7), property),
            animator,
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        }
    }

    fn find_group_with_adjustment<'a>(group: &'a GroupLayer, name: &str) -> Option<&'a GroupLayer> {
        if group
            .layers
            .iter()
            .any(|layer| matches!(layer.data(), LayerData::Adjustment(value) if value.name == name))
        {
            return Some(group);
        }
        group.layers.iter().find_map(|layer| match layer.data() {
            LayerData::Group(child) => find_group_with_adjustment(child, name),
            _ => None,
        })
    }

    fn find_layer(layers: &[Layer], id: LayerId) -> Option<&Layer> {
        layers.iter().find_map(|layer| {
            if layer.id() == id {
                return Some(layer);
            }
            layer
                .child_layers()
                .and_then(|children| find_layer(children, id))
        })
    }

    #[test]
    fn animated_bounds_native_nested_mosaic_encloses_sampling_domain() {
        const SOURCE: &[u8] =
            include_bytes!("../../../tests/fixtures/adjustment/native_controls.aep");
        assert_eq!(SOURCE.len(), 1_304_431);
        assert_eq!(
            format!("{:x}", Sha256::digest(SOURCE)),
            "ed45319ed014ba979a9e0b4868aa635775f288fe851c39d9242aab097020f26f"
        );
        let project = read_project(SOURCE).expect("independently authored Adjustment fixture");
        let converted =
            to_structural_fx_document(&project, Some(146)).expect("fresh nested Adjustment import");
        let root = match converted.document.composition().layers()[0].data() {
            LayerData::Group(root) => root,
            _ => panic!("imported composition root must be a Group"),
        };
        let imported = find_group_with_adjustment(root, "nested-local-adjustment")
            .expect("nested Adjustment remains scoped to its imported support Group");
        assert_eq!(
            imported.layers.iter().map(Layer::name).collect::<Vec<_>>(),
            [
                "upper-sentinel-37x23",
                "nested-local-adjustment",
                "lower-amber-83x117",
                "lower-cobalt-127x71"
            ]
        );

        let sources = BTreeMap::new();
        let analyzer = Analyzer {
            dynamics: converted.document.composition().dynamics().entries(),
            resolved_media: &sources,
            canvas: converted.document.dimensions(),
        };
        let unsupported = analyzer.layers_union(&imported.layers);
        match unsupported {
            Err(message) => assert_eq!(
                message,
                "Animated Adjustment effect can expand or move lower-stack support; only Mosaic has a proven finite sampling-domain enclosure"
            ),
            Ok(_) => panic!("Gaussian Adjustment bounds must remain explicitly unproved"),
        }

        // The native source proves the nested Adjustment stack; this edited
        // Mosaic variant is supplementary bounds evidence, not native Mosaic
        // render proof. Neighbor sampling can paint beyond the geometry AABB.
        let mut mosaic_group = imported.clone();
        mosaic_group.layers = imported
            .layers
            .iter()
            .map(|layer| match layer.data() {
                LayerData::Adjustment(value) if value.name == "nested-local-adjustment" => {
                    let mut value = value.clone();
                    value.effects = vec![
                        serde_json::from_value(serde_json::json!({
                            "id": 14631,
                            "enabled": true,
                            "effect": {
                                "type": "mosaic",
                                "horizontalBlocks": 12,
                                "verticalBlocks": 8,
                                "sharpColors": true
                            }
                        }))
                        .expect("valid Mosaic effect"),
                    ];
                    Layer::from_data(&LayerData::Adjustment(value))
                        .expect("valid adjusted fixture layer")
                }
                _ => layer.clone(),
            })
            .collect();
        let mut without_adjustment = mosaic_group.clone();
        without_adjustment
            .layers
            .retain(|layer| !matches!(layer.data(), LayerData::Adjustment(_)));

        let mut expected = analyzer
            .layers_union(&without_adjustment.layers)
            .expect("ordinary imported siblings have finite bounds")
            .expect("ordinary imported siblings paint");
        expected.include(Bounds {
            min: [0.0, 0.0],
            max: [
                f64::from(analyzer.canvas.width),
                f64::from(analyzer.canvas.height),
            ],
        });
        let actual = analyzer
            .layers_union(&mosaic_group.layers)
            .expect("Mosaic Adjustment has finite sampling-domain bounds")
            .expect("Mosaic Adjustment scope paints");
        assert_eq!(actual.min, expected.min);
        assert_eq!(actual.max, expected.max);
        assert!(actual.min[0] <= 0.0 && actual.min[1] <= 0.0);
        assert!(actual.max[0] >= f64::from(analyzer.canvas.width));
        assert!(actual.max[1] >= f64::from(analyzer.canvas.height));
    }

    #[test]
    fn animated_bounds_constant_shape_path_uses_effective_geometry() {
        let layer: Layer = serde_json::from_value(serde_json::json!({
            "id": 7,
            "name": "constant-path",
            "type": "Shape",
            "parent": null,
            "activeRange": { "start": 0, "duration": 2000 },
            "transform": {
                "anchorPoint": [0, 0],
                "position": [0, 0],
                "scale": [100, 100],
                "rotation": 0,
                "opacity": 100
            },
            "shape": {
                "path": { "commands": [
                    { "type": "moveTo", "x": 0, "y": 0 },
                    { "type": "lineTo", "x": 10, "y": 0 },
                    { "type": "lineTo", "x": 10, "y": 10 },
                    { "type": "close" }
                ] },
                "fills": [],
                "strokes": []
            }
        }))
        .expect("valid Shape layer");
        let effective: ShapePath = serde_json::from_value(serde_json::json!({
            "commands": [
                { "type": "moveTo", "x": 100, "y": 50 },
                { "type": "lineTo", "x": 140, "y": 50 },
                { "type": "lineTo", "x": 140, "y": 90 },
                { "type": "close" }
            ]
        }))
        .expect("valid effective Shape Path");
        let entries = vec![entry(
            PropType::ShapePath,
            PropertyAnimator::constant(PropertyValue::Path(effective)).unwrap(),
        )];
        let sources = BTreeMap::new();
        let analyzer = Analyzer {
            dynamics: &entries,
            resolved_media: &sources,
            canvas: Dimensions::new(320, 180),
        };
        let bounds = analyzer
            .layer_bounds(&layer)
            .expect("constant Shape Path has finite bounds")
            .expect("constant Shape Path paints");
        assert_eq!(bounds.min, [100.0, 50.0]);
        assert_eq!(bounds.max, [140.0, 90.0]);
    }

    #[test]
    fn animated_bounds_native_constant_shape_paths_remain_finite() {
        const SOURCE: &[u8] =
            include_bytes!("../../../tests/fixtures/export_repairs/native-controls.aep");
        assert_eq!(
            format!("{:x}", Sha256::digest(SOURCE)),
            "f62f72bf6ec281a63be63ec22b35e7552ee1ef48e4b8db97bb19773f372450a3"
        );
        let project = read_project(SOURCE).expect("independently authored Scale fixture");
        let converted =
            to_structural_fx_document(&project, Some(17)).expect("fresh native Scale import");
        let composition = converted.document.composition();
        let constant_path_owners = composition
            .dynamics()
            .entries()
            .iter()
            .filter_map(|entry| {
                let target = entry.target.as_property()?;
                (target.property_type() == PropType::ShapePath
                    && matches!(entry.animator.data(), AnimatorData::Constant { .. }))
                .then_some(target.layer_id())
            })
            .collect::<Vec<_>>();
        assert_eq!(constant_path_owners.len(), 3);
        let sources = BTreeMap::new();
        let analyzer = Analyzer {
            dynamics: composition.dynamics().entries(),
            resolved_media: &sources,
            canvas: converted.document.dimensions(),
        };
        let painted = constant_path_owners
            .into_iter()
            .filter(|&id| {
                let layer = find_layer(composition.layers(), id).expect("Shape Path owner");
                analyzer
                    .layer_bounds(layer)
                    .expect("constant Shape Path owner has finite all-time bounds")
                    .is_some()
            })
            .count();
        assert_eq!(painted, 2, "the two visible native Scale owners paint");
    }

    #[test]
    fn path_keys_bounds_enclose_easing_overshoot_and_disabled_outline() {
        let path = |x: f64| {
            serde_json::from_value::<fx_schema::ShapePath>(serde_json::json!({"commands":[
                {"type":"moveTo","x":x,"y":0},
                {"type":"lineTo","x":x+1.0,"y":10}
            ]}))
            .unwrap()
        };
        let track = PropertyKeyframeTrack::new(vec![
            PropertyKeyframe::new(
                KeyframeId::new("a"),
                TimeOffset::from_millis(0),
                PropertyValue::Path(path(0.0)),
                PropertyKeyframeEasing::Linear,
            ),
            PropertyKeyframe::new(
                KeyframeId::new("b"),
                TimeOffset::from_millis(1000),
                PropertyValue::Path(path(10.0)),
                PropertyKeyframeEasing::CubicBezier {
                    x1: 0.2,
                    y1: 2.0,
                    x2: 0.8,
                    y2: 2.0,
                },
            ),
        ])
        .unwrap();
        let entries = [entry(
            PropType::ShapePath,
            PropertyAnimator::keyframes(track),
        )];
        let media = BTreeMap::new();
        let analyzer = Analyzer {
            dynamics: &entries,
            resolved_media: &media,
            canvas: Dimensions::new(100, 100),
        };
        let bounds = analyzer
            .path_bounds(LayerId::new(7), &path(-100.0))
            .unwrap();
        assert!(bounds.min[0] <= 0.0 && bounds.max[0] >= 18.0);
        // A constant/disabled value must not inherit the inactive key history.
        let entries = [entry(
            PropType::ShapePath,
            PropertyAnimator::constant(PropertyValue::Path(path(35.0))).unwrap(),
        )];
        let analyzer = Analyzer {
            dynamics: &entries,
            resolved_media: &media,
            canvas: Dimensions::new(100, 100),
        };
        let bounds = analyzer
            .path_bounds(LayerId::new(7), &path(-100.0))
            .unwrap();
        assert_eq!(bounds.min[0], 35.0);
        assert_eq!(bounds.max[0], 36.0);
    }

    #[test]
    fn temporal_cubic_control_hull_includes_overshoot_not_just_endpoints() {
        let track = PropertyKeyframeTrack::new(vec![
            key("a", 0, 0.0, PropertyKeyframeEasing::Linear),
            key(
                "b",
                1000,
                10.0,
                PropertyKeyframeEasing::CubicBezier {
                    x1: 0.2,
                    y1: 2.0,
                    x2: 0.8,
                    y2: 2.0,
                },
            ),
        ])
        .unwrap();
        let range = track_component_range(&track, Component::Scalar).unwrap();
        assert_eq!(range.min, 0.0);
        assert!(range.max >= 20.0);
    }

    #[test]
    fn spatial_position_control_hull_is_enclosed() {
        let track = PropertyKeyframeTrack::new(vec![
            key("a", 0, 0.0, PropertyKeyframeEasing::Linear)
                .with_spatial_tangents(None, Some(100.0)),
            key("b", 1000, 10.0, PropertyKeyframeEasing::Linear)
                .with_spatial_tangents(Some(100.0), None),
        ])
        .unwrap();
        let range = track_component_range(&track, Component::Scalar).unwrap();
        assert!(range.max >= 110.0);
    }

    #[test]
    fn signed_scale_anchor_and_rotation_use_a_circumradius() {
        let local = Bounds {
            min: [0.0, 0.0],
            max: [200.0, 50.0],
        };
        let transform: Transform = serde_json::from_value(serde_json::json!({
            "anchorPoint": [25.0, 10.0],
            "position": [300.0, 200.0],
            "scale": [-200.0, 50.0],
            "rotation": 0.0,
            "opacity": 100.0
        }))
        .unwrap();
        let rotation = PropertyKeyframeTrack::new(vec![
            key("a", 0, 0.0, PropertyKeyframeEasing::Linear),
            key("b", 1000, 180.0, PropertyKeyframeEasing::Linear),
        ])
        .unwrap();
        let entries = vec![entry(
            PropType::Rotation,
            PropertyAnimator::keyframes(rotation),
        )];
        let media = BTreeMap::new();
        let analyzer = Analyzer {
            dynamics: &entries,
            resolved_media: &media,
            canvas: Dimensions::new(1920, 1080),
        };
        let bounds = analyzer
            .transform_bounds(local, LayerId::new(7), &transform)
            .unwrap();
        assert!(bounds.min[0] < -50.0);
        assert!(bounds.max[0] > 650.0);
    }

    #[test]
    fn stroke_miter_and_offset_reaches_are_additive() {
        let stroke = stroke_reach(
            Interval::new(2.0, 20.0).unwrap(),
            Interval::new(1.0, 8.0).unwrap(),
        )
        .unwrap();
        let offset = checked_product(12.0, 4.0, "overflow").unwrap();
        assert_eq!(stroke, 80.0);
        assert_eq!(checked_sum(stroke, offset, "overflow").unwrap(), 128.0);
    }

    #[test]
    fn review_graph_disabled_keyframes_bound_runtime_disabled_value() {
        let track = PropertyKeyframeTrack::new(vec![
            key("a", 0, 10.0, PropertyKeyframeEasing::Linear),
            key("b", 1000, 20.0, PropertyKeyframeEasing::Linear),
        ])
        .unwrap();
        let disabled = PropertyAnimator::from_data(&AnimatorData::Keyframes {
            track,
            enabled: false,
            disabled_value: Some(PropertyValue::Float(1000.0)),
        })
        .unwrap();
        let entries = vec![entry(PropType::ScaleX, disabled)];
        let media = BTreeMap::new();
        let analyzer = Analyzer {
            dynamics: &entries,
            resolved_media: &media,
            canvas: Dimensions::new(1, 1),
        };
        let transform: Transform = serde_json::from_value(serde_json::json!({
            "anchorPoint": [0.0, 0.0],
            "position": [0.0, 0.0],
            "scale": [100.0, 100.0],
            "rotation": 0.0,
            "opacity": 100.0
        }))
        .unwrap();

        let bounds = analyzer
            .transform_bounds(
                Bounds {
                    min: [0.0, 0.0],
                    max: [100.0, 100.0],
                },
                LayerId::new(7),
                &transform,
            )
            .unwrap();

        assert_eq!(bounds.min, [0.0, 0.0]);
        assert_eq!(bounds.max, [1000.0, 100.0]);
    }

    #[test]
    fn constant_animator_overrides_static_base() {
        let entries = vec![entry(
            PropType::ScaleX,
            PropertyAnimator::constant(PropertyValue::Float(125.0)).unwrap(),
        )];
        let media = BTreeMap::new();
        let analyzer = Analyzer {
            dynamics: &entries,
            resolved_media: &media,
            canvas: Dimensions::new(1, 1),
        };
        assert_eq!(
            analyzer
                .scalar_range(LayerId::new(7), PropType::ScaleX, -75.0)
                .unwrap(),
            Interval::point(125.0).unwrap()
        );
    }

    #[test]
    fn nonfinite_overflow_is_rejected_and_oversize_is_left_for_the_caller() {
        assert!(Interval::new(f64::INFINITY, 1.0).is_err());
        assert!(
            Interval::point(f64::MAX)
                .unwrap()
                .multiply(Interval::point(2.0).unwrap())
                .is_err()
        );
        let bounds = normalized_bounds([0.0, 0.0], [70_000.0, 80_000.0]).unwrap();
        assert!(bounds.max[0] - bounds.min[0] > f64::from(u16::MAX));
    }

    #[test]
    fn text_remains_an_explicit_unknown_geometry_fact() {
        let result: Result<Bounds, &'static str> =
            Err("Text/font glyph bounds are not known from the FX text box");
        match result {
            Err(message) => assert_eq!(
                message,
                "Text/font glyph bounds are not known from the FX text box"
            ),
            Ok(_) => panic!("text bounds must not be guessed from its authored box"),
        }
    }
}
