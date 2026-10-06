//! Graphic objects and transform keys in both directions.
//!
//! A graphic's objects are its Text and Shape components. One object imports
//! as a root text or shape layer, unless the group below is needed; two or
//! more import as an FX group whose transform is the Vector Motion (the
//! identity without one) and whose layers are the objects in paint order
//! ([`in_paint_order`]). Export maps a root text or shape layer, and a root
//! group of text and shape layers, back to one graphic; a shape keeps one
//! solid or gradient fill and at most one centred stroke. A multi-contour
//! path becomes certified pieces with inverted hole masks ([`shape_object`]).
//! A static Shape attachment uses an identity sibling guide in graphic pixels.
//!
//! A graphic's keys use its generator clock, so a key's layer time is its time
//! minus the placement's `InPoint`. Import maps the text object's Position,
//! Scale, Rotation and Opacity keys to the text layer's FX transform tracks
//! with the Motion easing map; the reader has already omitted a graphic whose
//! keys have Bezier timing. Source Text keys, each one complete document that
//! Premiere holds until the next, become Hold tracks of the text and of each
//! field that differs between keys ([`source_text_tracks`]); export rebuilds
//! one document per key time from the tracks of a text layer whose text is
//! keyed ([`source_text_keys`]). Keyed Vector Motion moves the whole graphic,
//! and the clip's own Opacity fades the whole graphic, so either makes a graphic
//! group: an FX group whose transform and tracks are the Vector Motion and the
//! clip Opacity and whose layers are the graphic's objects, on the group's
//! clock. A static Vector Motion stays composed into the text, unless that
//! would take the text outside its native ranges; then it makes the group too.
//! The clip Opacity's one mask masks the whole graphic after its
//! Vector Motion and makes the group as well, unless the graphic is one
//! unkeyed block of complete lines without a shadow, whose line group owns
//! the mask instead; the mask's guide is beside its owner, which export takes
//! back ([`export_graphic_group`]).
//! A text background box makes a group as well: FX has no text background,
//! and a group's own background is a box around its content, padded by the
//! background size on every side, filled with its color and rounded by its
//! radius, as AME renders of caption cues measured Premiere's
//! ([`PrTextBackground`]).
//!
//! Export maps a root text layer, or a group whose only layer is a text layer,
//! back to one graphic, on the clock that the writer starts one hour into the
//! generator. A keyed group becomes keyed Vector Motion and a static group is
//! composed into the text, or becomes a static Vector Motion when composing
//! would take the text outside its native ranges; the group's opacity and its
//! keys become the clip Opacity. A property whose keys Premiere cannot hold,
//! or whose keys the native readers would not accept, is
//! reported and keeps its static value, as on video layers, and so is a
//! property with cubic Bezier easing whose parameter has unverified Bezier
//! speeds ([`BEZIER_KEYS_UNVERIFIED`]); a group property, or a text mask or
//! track matte, that a graphic cannot carry omits the graphic. The background
//! of a group of one text becomes the text's background when it has the
//! calibrated form ([`group_background`]); any other is reported and the text
//! still exports.

mod capsule;
mod contours;
pub(crate) use capsule::template_objects;

use self::contours::ContourRole;
use super::{
    background::{identity_transform, plain_group},
    fonts, keyframes,
    nested::{LayerExport, LayerScope},
    premiere_to_tesseract::{
        into_stage, keyframe_id, motion_transform, object_transform, opacity_path_mask,
        position_tracks, rgba, scalar_keys, set_tracks, shape_guide, text_layer, tick_range,
        validate_time_range,
    },
    tesseract_to_premiere::{
        consumed_layer_ids, export_position_keys, export_scalar_keys, graphic_mask, graphic_of,
        graphic_opacity_mask, has_background, layer_animations, mask_guide_ids, omit_object_blend,
        rgb, scale_tracks_match, text_document, text_object, unexported_layer_fields, ClipLayer,
        WrittenAnimation,
    },
    text::{automatic_line_spacing, STROKE_WIDTH_RATIO},
    text_shadow,
};
use crate::{
    error::{ensure, unsupported, Result},
    export_loss::{omit_field, ExportField, ExportLossDomain, OmissionSink},
    format::PrGraphic,
    schema::{
        text::{
            omitted_part, stroke_join, unverified_mask_composite, GraphicParamSpec, PrAppearance,
            PrFill, PrGradient, PrGradientKind, PrGradientOpacityStop, PrGradientStop,
            PrGraphicGroup, PrGraphicObject, PrMaskSource, PrPathVertex, PrRgb, PrShape,
            PrShapePath, PrShapeStroke, PrSourceTextKey, PrText, PrTextBackground, PrTextFrame,
            PrTextLines, PrTextTransform, PrVectorMotion, PrVerticalAlign, SourceTextField,
            StrokeJoin, GRADIENT_Y_UNCONVERTED, OPAQUE_OPACITY_STOPS, TEXT_PARAMS,
            VECTOR_MOTION_PARAMS,
        },
        PrAnimatedProperty, PrBlendMode, PrPropertyAnimation, PrScalarKeyframe, PrStaticTransform,
    },
    {approximate, omit, Omission, OmissionKind, OmissionScope},
};
use fx_schema::{
    animator::{
        AnimatorData, PropertyAnimator, PropertyKeyframe, PropertyKeyframeEasing,
        PropertyKeyframeTrack,
    },
    AnimationGraph, BlendMode, Duration, EffectData, EffectPayload, EffectRecord, FxItemId,
    GroupLayer, Layer, LayerData, LayerEffect, LayerId, MotionBlurSettings, NonNegativeProperty,
    PathMask, PercentageProperty, Position, PositiveProperty, PropType, Property, PropertyTarget,
    PropertyValue, ShapeContent, ShapeFillRule, ShapeFillStyle, ShapeGradientStop,
    ShapeGradientType, ShapeHandleMirror, ShapeLayer, ShapeLineCap, ShapeLineJoin, ShapePaint,
    ShapePath, ShapePathCommand, ShapeStrokeStyle, TextAnimator, TextDocument, TextLayer, Time,
    TimeOffset, TimeRangeProperty, TrackMatte, TrackMatteType, Transform,
};
use std::collections::{BTreeMap, BTreeSet};

/// Why export keeps the static value of a graphic property with cubic Bezier
/// easing whose parameter lacks `bezier_speeds_verified`. Premiere would
/// store the curve's handles as speeds, and their unit was measured only for
/// Text Scale and Opacity and Vector Motion Scale and Rotation.
const BEZIER_KEYS_UNVERIFIED: &str =
    "Bezier graphic keys are unsupported until their speed unit is verified";

/// Whether the parameter of `specs` that keys `property` holds cubic Bezier
/// easing ([`BEZIER_KEYS_UNVERIFIED`]).
pub(super) fn bezier_keys_verified(
    specs: &[GraphicParamSpec],
    property: PrAnimatedProperty,
) -> bool {
    specs
        .iter()
        .any(|spec| spec.role.animation() == Some(property) && spec.bezier_speeds_verified)
}

/// Whether any key of `track`, which keys `property`, has cubic Bezier easing
/// that the parameter of `specs` for `property` cannot hold.
fn unverified_bezier(
    specs: &[GraphicParamSpec],
    property: PrAnimatedProperty,
    track: &PropertyKeyframeTrack,
) -> bool {
    !bezier_keys_verified(specs, property)
        && track
            .keyframes()
            .iter()
            .any(|key| matches!(key.easing(), PropertyKeyframeEasing::CubicBezier { .. }))
}

/// The property as omission messages name it.
fn property_name(property: PrAnimatedProperty) -> &'static str {
    match property {
        PrAnimatedProperty::Opacity => "Opacity",
        PrAnimatedProperty::Position => "Position",
        PrAnimatedProperty::AnchorPoint => "Anchor Point",
        PrAnimatedProperty::Rotation => "Rotation",
        PrAnimatedProperty::UniformScale => "Scale",
        PrAnimatedProperty::ScaleWidth => "Scale Width",
    }
}

/// Map one graphic to its FX object layers with the text objects' keys,
/// inside a graphic group when the graphic has several objects, keyed or
/// kept Vector Motion, or a nondefault clip Opacity, blend mode or clip
/// Opacity mask, which the group applies to the whole graphic. The line group
/// of one unkeyed block of complete lines without a shadow owns a clip Opacity
/// mask that nothing else groups ([`clip_opacity_mask`]). Returns the root
/// layer and the guide of the clip Opacity mask, the root's sibling, or
/// no occurrence when every object was lost with its mask composite.
pub(super) fn import_graphic(
    graphic: &PrGraphic,
    dimensions: [u32; 2],
    layer_id: LayerId,
    index: usize,
    scope: &mut LayerScope<'_, '_>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
) -> Result<Option<(Layer, Option<Layer>)>> {
    let record = graphic.id().unwrap_or("graphic");
    if let Some(mask) = &graphic.opacity_mask {
        if let Err(reason) =
            super::mask_animation::import_tracks(mask, FxItemId::new(1), graphic.in_ticks)
        {
            omit(
                omissions,
                OmissionScope::Occurrence,
                record,
                format!("graphic was not imported: numeric Opacity mask keys: {reason}"),
            );
            return Ok(None);
        }
    }
    let motion = graphic.vector_motion.as_ref();
    // Only the caption reader sets a background, on a graphic of one text.
    let background = match graphic.objects.as_slice() {
        [PrGraphicObject::Text(text)] => text.document.background,
        _ => None,
    };
    // A graphic group made for the clip Opacity mask alone would only own the
    // mask. The line group of an unkeyed block without a shadow owns it
    // instead: FX applies the mask over a guide beside it after the block's
    // transform, as Premiere applies the mask after the whole graphic.
    let block_owns_mask = matches!(
        graphic.objects.as_slice(),
        [PrGraphicObject::TextLines(block)] if block.animations.is_empty()
            && block.documents.iter().all(|document| document.shadow.is_none())
    );
    let grouped = graphic.objects.len() > 1
        || graphic
            .effect_loss
            .as_ref()
            .is_some_and(|loss| !loss.mapped_ramps.is_empty())
        || graphic.objects.iter().any(|object| {
            object.mask_source().is_some()
                || matches!(object, PrGraphicObject::Group(_))
                || matches!(object, PrGraphicObject::Shape(shape) if shape.mask.is_some())
        })
        || motion.is_some()
        || graphic.opacity != 100.0
        || graphic.blend_mode.fx_mode() != BlendMode::Normal
        || !graphic.animations.is_empty()
        || background.is_some()
        || (graphic.opacity_mask.is_some() && !block_owns_mask);
    // The group takes the placement's range and visibility; its objects start
    // with it, so all keep the graphic's clock.
    let group_id = grouped.then(|| next_layer_id(scope));
    let mut objects = ObjectImport {
        graphic,
        dimensions,
        index,
        record,
        motion,
        first_id: Some(layer_id),
        grouped: false,
        mask_sources: 0,
        scope,
        dynamics,
        omissions,
    };
    let parent = group_id.or(objects.scope.parent);
    let mut layers = objects.level(&graphic.objects, parent)?;
    let mask_sources = objects.mask_sources;
    let (scope, dynamics, omissions) = (objects.scope, objects.dynamics, objects.omissions);
    if layers.is_empty() {
        omit(
            omissions,
            OmissionScope::Occurrence,
            record,
            "graphic was not converted: none of its objects converts",
        );
        return Ok(None);
    }
    if mask_sources > 0 {
        approximate(omissions, record, MASK_SOURCE_LINEAR_LIGHT_APPROXIMATION);
    }
    if let Some(warning) = graphic.blend_mode.approximation() {
        approximate(omissions, record, warning);
    }
    let Some(group_id) = group_id else {
        let [layer] = layers.as_mut_slice() else {
            return Err(unsupported("an ungrouped graphic holds one object"));
        };
        let (mask, guide) =
            clip_opacity_mask(graphic, dimensions, index, scope, dynamics, omissions)?.unzip();
        if let Some(mask) = mask {
            let LayerData::Group(block) = layer else {
                return Err(unsupported(
                    "an ungrouped graphic's clip Opacity mask needs its line group",
                ));
            };
            block.masks = vec![mask];
        }
        return Ok(Some((Layer::from_data(layer)?, guide)));
    };
    let mut tracks = Vec::new();
    for animations in [
        motion.map(|motion| &motion.animations),
        Some(&graphic.animations),
    ]
    .into_iter()
    .flatten()
    {
        tracks.extend(object_tracks(
            animations,
            true,
            graphic.in_ticks,
            group_id,
            dimensions,
            record,
            omissions,
        ));
    }
    let (effects, effect_tracks) =
        super::effects::import_graphic_ramps(graphic, group_id, scope.effect_ids, omissions);
    set_tracks(dynamics, tracks)?;
    for (target, track) in effect_tracks {
        dynamics
            .set_property(target, PropertyAnimator::keyframes(track), Vec::new())
            .map_err(super::premiere_to_tesseract::map_animation_graph_error)?;
    }
    let opacity = PercentageProperty::new(graphic.opacity)
        .ok_or_else(|| unsupported("Premiere opacity must be between 0 and 100"))?;
    // A static Vector Motion that folds within one object's ranges is already composed into it.
    let transform = match motion {
        Some(motion) => Transform {
            anchor_point: motion.anchor,
            position: Position::xy(motion.position[0], motion.position[1]),
            scale: [motion.scale; 2],
            rotation: motion.rotation,
            opacity,
            ..identity_transform()
        },
        None => Transform {
            opacity,
            ..identity_transform()
        },
    };
    let padding = NonNegativeProperty::new(background.map_or(0.0, |box_| f64::from(box_.size)))
        .ok_or_else(|| unsupported("text background size must be nonnegative"))?;
    let radius = NonNegativeProperty::new(background.map_or(0.0, |box_| f64::from(box_.radius)))
        .ok_or_else(|| unsupported("text background radius must be nonnegative"))?;
    let window = tick_range(graphic.start_ticks, graphic.end_ticks)?;
    let (mask, guide) =
        clip_opacity_mask(graphic, dimensions, index, scope, dynamics, omissions)?.unzip();
    let group = Layer::from_data(&LayerData::Group(GroupLayer {
        id: group_id,
        name: format!("Premiere graphic {}", index + 1),
        description: String::new(),
        is_hidden: !graphic.enabled,
        parent: scope.parent,
        blend_mode: graphic.blend_mode.fx_mode(),
        track_matte: None,
        masks: mask.into_iter().collect(),
        playback: fx_schema::LayerPlayback::linear(
            window,
            window,
            TimeRangeProperty::new(Time::ZERO, window.duration),
            0,
        )
        .map_err(unsupported)?,
        effects,
        motion_blur: false,
        padding_top: padding,
        padding_right: padding,
        padding_bottom: padding,
        padding_left: padding,
        fills: background
            .map(|box_| ShapeFillStyle::solid(rgba(box_.color)))
            .into_iter()
            .collect(),
        corner_radius_top_left: radius,
        corner_radius_top_right: radius,
        corner_radius_bottom_right: radius,
        corner_radius_bottom_left: radius,
        transform,
        layers: in_paint_order(layers)
            .iter()
            .map(Layer::from_data)
            .collect::<std::result::Result<_, _>>()?,
    }))?;
    Ok(Some((group, guide)))
}

/// The FX mask of a graphic's clip Opacity mask and its guide, at the
/// identity in sequence pixels over the placement: the sibling of the mask's
/// owner, the graphic group or a block's line group. Premiere applies the mask
/// after the Vector Motion, in the sequence frame (fixture
/// `feature_graphic_masks_d_26_5`, probe d1), and FX applies a sibling guide's
/// mask after its owner's transform, so the transform moves the objects, not
/// the mask. `None` without a mask.
fn clip_opacity_mask(
    graphic: &PrGraphic,
    dimensions: [u32; 2],
    index: usize,
    scope: &mut LayerScope<'_, '_>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
) -> Result<Option<(PathMask, Layer)>> {
    let Some(mask) = &graphic.opacity_mask else {
        return Ok(None);
    };
    for warning in mask.approximations() {
        approximate(omissions, graphic.id().unwrap_or("graphic"), warning);
    }
    let guide_id = next_layer_id(scope);
    let mask_id = FxItemId::new(*scope.next_index as u64 + 1);
    *scope.next_index += 1;
    let path_mask = opacity_path_mask(mask_id, guide_id, mask)?;
    for (target, track) in super::mask_animation::import_tracks(mask, mask_id, graphic.in_ticks)
        .map_err(unsupported)?
    {
        dynamics
            .set_property(
                target,
                fx_schema::PropertyAnimator::keyframes(track),
                Vec::new(),
            )
            .map_err(super::premiere_to_tesseract::map_animation_graph_error)?;
    }
    let guide = Layer::from_data(&LayerData::Shape(shape_guide(
        guide_id,
        format!("Premiere Opacity mask {}", index + 1),
        scope.parent,
        tick_range(graphic.start_ticks, graphic.end_ticks)?,
        identity_transform(),
        scaled_path(&fx_path(&mask.path), dimensions.map(f64::from)),
    )))?;
    Ok(Some((path_mask, guide)))
}

pub(super) const MASK_SOURCE_LINEAR_LIGHT_APPROXIMATION: &str = "a partly covering Mask with Shape or Text mixes into the masked objects in linear light in Premiere; converted as an FX track matte, which mixes in encoded sRGB, so partly covered pixels differ in brightness depending on their colors";

/// The import of one graphic's objects: what every object layer shares, and
/// the document's id allocation. The first object layer keeps the
/// graphic's layer id.
struct ObjectImport<'g, 's, 'a, 'm> {
    graphic: &'g PrGraphic,
    dimensions: [u32; 2],
    index: usize,
    record: &'g str,
    motion: Option<&'g PrVectorMotion>,
    first_id: Option<LayerId>,
    /// Whether the level being imported is inside the graphic group.
    grouped: bool,
    /// The Mask with Shape and Text objects imported as track mattes.
    mask_sources: usize,
    scope: &'s mut LayerScope<'a, 'm>,
    dynamics: &'s mut AnimationGraph,
    omissions: &'s mut Vec<Omission>,
}

impl ObjectImport<'_, '_, '_, '_> {
    fn next_id(&mut self) -> LayerId {
        self.first_id
            .take()
            .unwrap_or_else(|| next_layer_id(self.scope))
    }

    /// The FX layers (front first) of one group level of objects whose
    /// parent is `parent`: a graphic group, a SubGroup, a masked group, or
    /// the layer list of an ungrouped graphic's one object. A Mask with
    /// Shape or Text keeps its object layer, which becomes the track-matte
    /// source, alpha or inverted alpha, of an FX group of every object below
    /// it in the level: FX draws a group with a track matte in isolation, so
    /// one matte covers the composite of the objects below, as Premiere
    /// masks it, and the source draws only through
    /// the matte. A SubGroup becomes an FX group at the identity.
    fn level(
        &mut self,
        objects: &[PrGraphicObject],
        parent: Option<LayerId>,
    ) -> Result<Vec<LayerData>> {
        let grouped = std::mem::replace(&mut self.grouped, parent != self.scope.parent);
        let mut layers = Vec::with_capacity(objects.len());
        for (position, object) in objects.iter().enumerate() {
            let id = self.next_id();
            match object {
                PrGraphicObject::Group(group) => {
                    let name = if group.name.is_empty() {
                        format!("Premiere group {}", self.index + 1)
                    } else {
                        group.name.clone()
                    };
                    let members = self.level(&group.objects, Some(id))?;
                    if !members.is_empty() {
                        layers.push(LayerData::Group(
                            self.plain_group(id, name, parent, members)?,
                        ));
                    }
                }
                object => {
                    if let PrGraphicObject::Shape(shape) = object {
                        if shape
                            .mask
                            .as_ref()
                            .is_some_and(|mask| !mask.path_keys.is_empty())
                        {
                            let reason = format!(
                                "{:?}: animated Shape-attached masks are unsupported",
                                shape.name
                            );
                            let masks_below = object.mask_source().is_some();
                            omit(
                                self.omissions,
                                OmissionScope::Feature,
                                self.record,
                                if masks_below {
                                    omitted_part(&reason, objects.len() - position - 1)
                                } else {
                                    reason
                                },
                            );
                            if masks_below {
                                break;
                            }
                            continue;
                        }
                    }
                    let (layer, guide) = match self.object_layer(object, id, parent)? {
                        ObjectLayerImport::Kept(kept) => *kept,
                        ObjectLayerImport::Unrendered(reason) => {
                            let masks_below = object.mask_source().is_some();
                            omit(
                                self.omissions,
                                OmissionScope::Feature,
                                self.record,
                                if masks_below {
                                    omitted_part(&reason, objects.len() - position - 1)
                                } else {
                                    reason
                                },
                            );
                            if masks_below {
                                break;
                            }
                            continue;
                        }
                    };
                    let source = object.mask_source();
                    layers.push(layer);
                    layers.extend(guide.map(LayerData::Shape));
                    if let Some(source) = source {
                        self.mask_sources += 1;
                        let group_id = next_layer_id(self.scope);
                        let lower = self.level(&objects[position + 1..], Some(group_id))?;
                        let mut group = self.plain_group(
                            group_id,
                            format!("Premiere masked objects {}", self.index + 1),
                            parent,
                            lower,
                        )?;
                        group.track_matte = Some(TrackMatte {
                            mode: if source.inverted {
                                TrackMatteType::AlphaInverted
                            } else {
                                TrackMatteType::Alpha
                            },
                            layer: id,
                        });
                        layers.push(LayerData::Group(group));
                        break;
                    }
                }
            }
        }
        self.grouped = grouped;
        Ok(layers)
    }

    /// An FX group at the identity under `parent`, over the graphic's clock.
    fn plain_group(
        &self,
        id: LayerId,
        name: String,
        parent: Option<LayerId>,
        layers: Vec<LayerData>,
    ) -> Result<GroupLayer> {
        let range = tick_range(self.graphic.start_ticks, self.graphic.end_ticks)?;
        let range = TimeRangeProperty::new(Time::ZERO, range.duration);
        Ok(GroupLayer {
            parent,
            ..plain_group(
                id,
                name,
                range,
                identity_transform(),
                layers
                    .iter()
                    .map(Layer::from_data)
                    .collect::<std::result::Result<_, _>>()?,
            )?
        })
    }

    /// The layer of one Text or Shape object with id `id` under `parent`,
    /// and its optional identity sibling mask guide. A Mask
    /// with Shape or Text whose shadow or keys do not import is
    /// [`ObjectLayerImport::Unrendered`] instead.
    fn object_layer(
        &mut self,
        object: &PrGraphicObject,
        id: LayerId,
        parent: Option<LayerId>,
    ) -> Result<ObjectLayerImport> {
        let (graphic, record, motion) = (self.graphic, self.record, self.motion);
        let grouped = self.grouped;
        let join = |layer_parent: &mut Option<LayerId>,
                    hidden: &mut bool,
                    range: &mut TimeRangeProperty| {
            *layer_parent = parent;
            if grouped {
                *hidden = false;
                *range = TimeRangeProperty::new(Time::ZERO, range.duration);
            }
        };
        match object {
            PrGraphicObject::Text(text) => {
                let mut layer = text_layer(graphic, text, id, self.index)?;
                // What of the text does not import, reported once whether
                // it can keep a mask is known.
                let mut parts = Vec::new();
                layer.effects.extend(text_shadow::import_text_shadow(
                    text,
                    motion,
                    record,
                    self.scope.effect_ids,
                    &mut parts,
                )?);
                validate_time_range("active_range", layer.active_range)?;
                let mut tracks = object_tracks(
                    &text.animations,
                    text.horizontal_scale.is_none(),
                    graphic.in_ticks,
                    id,
                    self.dimensions,
                    record,
                    &mut parts,
                );
                tracks.extend(source_text_tracks(
                    text,
                    graphic.in_ticks,
                    id,
                    &mut layer.source_text,
                )?);
                if let Some(lost) = lost_mask_parts(text.mask_source, parts, self.omissions) {
                    return Ok(ObjectLayerImport::Unrendered(format!(
                        "{:?}: a Mask with Text draws what did not convert ({lost}), so it would mask differently",
                        text.name
                    )));
                }
                set_tracks(self.dynamics, tracks)?;
                import_stroke_width(
                    text,
                    graphic.in_ticks,
                    &mut layer,
                    self.scope,
                    self.dynamics,
                )?;
                join(
                    &mut layer.parent,
                    &mut layer.is_hidden,
                    &mut layer.active_range,
                );
                Ok(ObjectLayerImport::Kept(Box::new((
                    LayerData::Text(layer),
                    None,
                ))))
            }
            PrGraphicObject::Shape(shape) => {
                let mut layer = shape_layer(graphic, shape, id, self.index)?;
                let mut parts = Vec::new();
                let shadow = text_shadow::import_shape_shadow(
                    shape,
                    motion,
                    record,
                    self.scope.effect_ids,
                    &mut parts,
                )?;
                if let Some(lost) =
                    lost_mask_parts(shape.appearance.mask_source, parts, self.omissions)
                {
                    return Ok(ObjectLayerImport::Unrendered(format!(
                        "{:?}: a Mask with Shape draws what did not convert ({lost}), so it would mask differently",
                        shape.name
                    )));
                }
                for warning in shape.gradient_approximations(graphic, shadow.is_some()) {
                    approximate(self.omissions, record, warning);
                }
                layer.effects.extend(shadow);
                validate_time_range("active_range", layer.active_range)?;
                join(
                    &mut layer.parent,
                    &mut layer.is_hidden,
                    &mut layer.active_range,
                );
                let guide = if let Some(mask) = &shape.mask {
                    let guide_id = next_layer_id(self.scope);
                    let mask_id = FxItemId::new(*self.scope.next_index as u64 + 1);
                    *self.scope.next_index += 1;
                    let tracks =
                        match super::mask_animation::import_tracks(mask, mask_id, graphic.in_ticks)
                        {
                            Ok(tracks) => tracks,
                            Err(reason) => {
                                return Ok(ObjectLayerImport::Unrendered(format!(
                            "{:?}: numeric Shape-attached mask keys cannot convert: {reason}",
                            shape.name
                        )))
                            }
                        };
                    for (target, track) in tracks {
                        self.dynamics
                            .set_property(
                                target,
                                fx_schema::PropertyAnimator::keyframes(track),
                                Vec::new(),
                            )
                            .map_err(super::premiere_to_tesseract::map_animation_graph_error)?;
                    }
                    layer
                        .masks
                        .push(opacity_path_mask(mask_id, guide_id, mask)?);
                    for warning in mask.approximations() {
                        approximate(self.omissions, record, warning);
                    }
                    Some(shape_guide(
                        guide_id,
                        format!("{} mask", layer.name),
                        parent,
                        layer.active_range,
                        identity_transform(),
                        scaled_path(&fx_path(&mask.path), self.dimensions.map(f64::from)),
                    ))
                } else {
                    None
                };
                Ok(ObjectLayerImport::Kept(Box::new((
                    LayerData::Shape(layer),
                    guide,
                ))))
            }
            PrGraphicObject::TextLines(text) => Ok(ObjectLayerImport::Kept(Box::new((
                LayerData::Group(import_text_lines(
                    graphic,
                    text,
                    TextBlockPlacement {
                        id,
                        parent,
                        inside_graphic: grouped,
                    },
                    self.dimensions,
                    self.scope,
                    self.dynamics,
                    self.omissions,
                )?),
                None,
            )))),
            PrGraphicObject::Group(_) => Err(unsupported("a SubGroup is not one object layer")),
        }
    }
}

/// What importing one Text or Shape object makes ([`ObjectImport::object_layer`]).
enum ObjectLayerImport {
    /// Its layer and an optional identity sibling guide.
    Kept(Box<(LayerData, Option<ShapeLayer>)>),
    /// Nothing, and why: it is a Mask with Shape or Text that loses a part
    /// it draws, its shadow or keys. FX would draw the matte without it, so
    /// the mask would keep or hide other pixels than Premiere's; it and the
    /// objects below it are left out ([`ObjectImport::level`]).
    Unrendered(String),
}

/// What did not import of an object with the Mask with Shape or Text
/// `source`, `parts`, as one reason when it is a mask that loses a part
/// ([`OmissionKind::Omitted`]), whose composite then goes
/// ([`ObjectLayerImport::Unrendered`]); otherwise `parts` join `omissions`,
/// as any object's do, and `None`.
fn lost_mask_parts(
    source: Option<PrMaskSource>,
    parts: Vec<Omission>,
    omissions: &mut Vec<Omission>,
) -> Option<String> {
    if source.is_some() && parts.iter().any(|part| part.kind == OmissionKind::Omitted) {
        let lost: Vec<_> = parts.iter().map(|part| part.reason.as_str()).collect();
        return Some(lost.join("; "));
    }
    for part in parts {
        omissions.emit(part);
    }
    None
}

/// Map a graphic that its own static clip Motion moves (a Source Graphic
/// placement) to a group that holds the root layer [`import_graphic`] makes
/// of it, which keeps the Vector Motion, clip Opacity and blend mode as for
/// any graphic. The group takes the placement's range, visibility and clip
/// Motion, so the Motion moves the whole converted graphic once, as Premiere
/// moves the clip's picture, and the root keeps the graphic's clock on the
/// group clock. A graphic's picture is its sequence frame, so the Motion maps
/// as a media clip's of a canvas-sized picture does.
pub(super) fn import_moved_graphic(
    graphic: &PrGraphic,
    dimensions: [u32; 2],
    layer_id: LayerId,
    index: usize,
    scope: &mut LayerScope<'_, '_>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
) -> Result<Option<Layer>> {
    graphic.validate_clip_motion_mask()?;
    let content = PrGraphic {
        clip_motion: PrStaticTransform::default(),
        enabled: true,
        ..graphic.clone()
    };
    let Some((root, guide)) = import_graphic(
        &content, dimensions, layer_id, index, scope, dynamics, omissions,
    )?
    else {
        return Ok(None);
    };
    ensure!(
        guide.is_none(),
        "a moved graphic cannot have a clip Opacity mask guide"
    );
    let group_id = next_layer_id(scope);
    let active_range = tick_range(graphic.start_ticks, graphic.end_ticks)?;
    // The root moves onto the group clock, which starts with the placement.
    let root = into_stage(
        &root,
        group_id,
        active_range.duration,
        scope.on_document_clock && active_range.start == Time::ZERO,
    )?;
    Ok(Some(Layer::from_data(&LayerData::Group(GroupLayer {
        is_hidden: !graphic.enabled,
        parent: scope.parent,
        ..plain_group(
            group_id,
            format!("Premiere graphic Motion {}", index + 1),
            active_range,
            motion_transform(&graphic.clip_motion, dimensions, dimensions),
            vec![root],
        )?
    }))?))
}

/// One common owner applies the source object's transform, opacity and shadow
/// after its independently editable lines have been laid out in local pixels.
struct TextBlockPlacement {
    id: LayerId,
    parent: Option<LayerId>,
    inside_graphic: bool,
}

fn import_text_lines(
    graphic: &PrGraphic,
    text: &PrTextLines,
    placement: TextBlockPlacement,
    dimensions: [u32; 2],
    scope: &mut LayerScope<'_, '_>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
) -> Result<GroupLayer> {
    let first = text
        .documents
        .first()
        .ok_or_else(|| unsupported("a text block has no lines"))?;
    let PrTextFrame::Point { vertical } = first.frame else {
        return Err(unsupported("mixed-style box text is unsupported"));
    };
    let mut baselines = Vec::with_capacity(text.documents.len());
    let mut baseline = 0.0;
    for (index, document) in text.documents.iter().enumerate() {
        // Native whole-line text advances by the incoming line's size. The
        // independent opening-title render distinguishes this from the
        // outgoing/max font's spacing.
        if index != 0 {
            baseline += super::text::line_spacing(document);
        }
        baselines.push(baseline);
    }
    let offset = super::text::vertical_offset(vertical, baseline);
    let mut layers = Vec::with_capacity(text.documents.len());
    for (line, (document, baseline)) in text.documents.iter().zip(baselines).enumerate() {
        let mut document = document.clone();
        document.frame = PrTextFrame::Point {
            vertical: PrVerticalAlign::Top,
        };
        document.shadow = None;
        let object = PrText {
            horizontal_scale: None,
            mask_source: None,
            name: format!("{} — line {}", text.name, line + 1),
            document,
            transform: PrTextTransform {
                position: [0.0, baseline - offset],
                anchor: [0.0; 2],
                scale: 100.0,
                rotation: 0.0,
                opacity: 100.0,
            },
            animations: Vec::new(),
            source_text_keys: Vec::new(),
        };
        let mut layer = text_layer(graphic, &object, next_layer_id(scope), line)?;
        layer.parent = Some(placement.id);
        layer.is_hidden = false;
        layer.active_range = TimeRangeProperty::new(Time::ZERO, layer.active_range.duration);
        layers.push(Layer::from_data(&LayerData::Text(layer))?);
    }
    let record = graphic.id().unwrap_or("graphic");
    set_tracks(
        dynamics,
        object_tracks(
            &text.animations,
            true,
            graphic.in_ticks,
            placement.id,
            dimensions,
            record,
            omissions,
        ),
    )?;
    let mut shadow_document = first.clone();
    // A common shadow is supported only when every contributing line has a
    // fill. Reuse the ordinary text guard with the whole block's animation.
    if text
        .documents
        .iter()
        .any(|document| document.fill.is_none())
    {
        shadow_document.fill = None;
    }
    let shadow_owner = PrText {
        horizontal_scale: None,
        mask_source: None,
        name: text.name.clone(),
        document: shadow_document,
        transform: text.transform,
        animations: text.animations.clone(),
        source_text_keys: Vec::new(),
    };
    let effects = text_shadow::import_text_shadow(
        &shadow_owner,
        graphic.vector_motion.as_ref(),
        record,
        scope.effect_ids,
        omissions,
    )?;
    let graphic_window = tick_range(graphic.start_ticks, graphic.end_ticks)?;
    let window = if placement.inside_graphic {
        TimeRangeProperty::new(Time::ZERO, graphic_window.duration)
    } else {
        graphic_window
    };
    let zero = NonNegativeProperty::new(0.0).expect("zero is nonnegative");
    Ok(GroupLayer {
        id: placement.id,
        name: text.name.clone(),
        description: String::new(),
        is_hidden: !placement.inside_graphic && !graphic.enabled,
        parent: placement.parent,
        blend_mode: BlendMode::Normal,
        track_matte: None,
        masks: Vec::new(),
        playback: fx_schema::LayerPlayback::linear(
            window,
            window,
            TimeRangeProperty::new(Time::ZERO, window.duration),
            0,
        )
        .map_err(unsupported)?,
        effects: effects.into_iter().collect(),
        motion_blur: false,
        padding_top: zero,
        padding_right: zero,
        padding_bottom: zero,
        padding_left: zero,
        fills: Vec::new(),
        corner_radius_top_left: zero,
        corner_radius_top_right: zero,
        corner_radius_bottom_right: zero,
        corner_radius_bottom_left: zero,
        transform: object_transform(&text.transform, "text block")?,
        layers,
    })
}

/// The next unassigned layer id of the document.
fn next_layer_id(scope: &mut LayerScope<'_, '_>) -> LayerId {
    let id = LayerId::new(*scope.next_index as u64 + 1);
    *scope.next_index += 1;
    id
}

/// A graphic's objects as FX layers (front first) from the order that its
/// component chain lists them, and back: the mapping is its own inverse.
/// Premiere draws the first listed object in front (the fixture's G1 and G2
/// renders, one object order each).
fn in_paint_order<T>(objects: Vec<T>) -> Vec<T> {
    objects
}

/// FX transform tracks of one graphic object's keys, on the layer clock that
/// starts at the generator time `in_ticks`. A property whose keys cannot
/// import is reported and keeps its static value, as on video layers.
fn object_tracks(
    animations: &[PrPropertyAnimation],
    uniform_scale: bool,
    in_ticks: i64,
    layer_id: LayerId,
    dimensions: [u32; 2],
    record: &str,
    omissions: &mut Vec<Omission>,
) -> Vec<(Property, PropertyKeyframeTrack)> {
    let mut tracks = Vec::new();
    for animation in animations {
        let scalar_tracks = |property_types: &[PropType]| {
            property_types
                .iter()
                .map(|&property_type| {
                    scalar_keys(animation, in_ticks, layer_id, property_type)
                        .map(|track| (Property::new(layer_id, property_type), track))
                })
                .collect()
        };
        let imported: Result<Vec<_>> = match animation.property() {
            PrAnimatedProperty::Position => {
                position_tracks(animation, in_ticks, layer_id, dimensions).map(|(x, y)| {
                    vec![
                        (Property::new(layer_id, PropType::PositionX), x),
                        (Property::new(layer_id, PropType::PositionY), y),
                    ]
                })
            }
            PrAnimatedProperty::Opacity => scalar_tracks(&[PropType::Opacity]),
            PrAnimatedProperty::Rotation => scalar_tracks(&[PropType::Rotation]),
            PrAnimatedProperty::UniformScale => {
                if uniform_scale {
                    scalar_tracks(&[PropType::ScaleX, PropType::ScaleY])
                } else {
                    scalar_tracks(&[PropType::ScaleY])
                }
            }
            // No graphic parameter keys these clip Motion properties.
            PrAnimatedProperty::AnchorPoint | PrAnimatedProperty::ScaleWidth => Err(unsupported(
                "a graphic object has no keyable clip Motion Anchor Point or Scale Width",
            )),
        };
        match imported {
            Ok(imported) => tracks.extend(imported),
            Err(error) => omit(
                omissions,
                OmissionScope::Feature,
                record,
                format!(
                    "{} animation was not imported: {error}",
                    property_name(animation.property())
                ),
            ),
        }
    }
    tracks
}

/// The FX text property that carries each Source Text field, with the
/// keyframe id name of its imported keys.
const SOURCE_TEXT_PROPERTIES: [(SourceTextField, PropType, &str); 8] = [
    (SourceTextField::Text, PropType::TextContent, "text-content"),
    (SourceTextField::Size, PropType::FontSize, "font-size"),
    (
        SourceTextField::FillEnabled,
        PropType::FillEnabled,
        "fill-enabled",
    ),
    (
        SourceTextField::FillColor,
        PropType::FillColor,
        "fill-color",
    ),
    (SourceTextField::Tracking, PropType::Tracking, "tracking"),
    (SourceTextField::Leading, PropType::Leading, "leading"),
    (
        SourceTextField::StrokeEnabled,
        PropType::StrokeEnabled,
        "stroke-enabled",
    ),
    (SourceTextField::AllCaps, PropType::AllCaps, "all-caps"),
];

/// The Source Text field that a text layer's `property` carries, if one does.
pub(super) fn source_text_field(property: PropType) -> Option<SourceTextField> {
    SOURCE_TEXT_PROPERTIES
        .iter()
        .find_map(|&(field, carrier, _)| (carrier == property).then_some(field))
}

fn source_text_property(field: SourceTextField) -> (PropType, &'static str) {
    SOURCE_TEXT_PROPERTIES
        .iter()
        .find_map(|&(carried, property, name)| (carried == field).then_some((property, name)))
        .expect("Source Text fields other than width have a layer carrier property")
}

/// Hold FX tracks of a text object's Source Text keys, on the layer clock
/// that starts at the generator time `in_ticks`: the text always, and each
/// field that differs between keys. FX leading is the whole line spacing
/// while Premiere adds its leading to 120 % of the size, so size keys beside
/// any leading key the leading too. A stroke that keys switch on becomes
/// the layer's static stroke in `source_text`; varying width is carried by
/// an all-character text animator ([`import_stroke_width`]). A track the FX document cannot hold (over its
/// serialized size limit) stops the conversion, as text it cannot hold does.
fn source_text_tracks(
    text: &PrText,
    in_ticks: i64,
    layer_id: LayerId,
    source_text: &mut TextDocument,
) -> Result<Vec<(Property, PropertyKeyframeTrack)>> {
    let keys = &text.source_text_keys;
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let mut fields = text.keyed_fields()?;
    fields.remove(&SourceTextField::StrokeWidth);
    fields.insert(SourceTextField::Text);
    if fields.contains(&SourceTextField::Size) {
        fields.insert(SourceTextField::Leading);
    }
    // The layer's static stroke and fill color come from the first key that
    // has one when the first key does not; a key without a fill keeps the
    // color of the first that has one.
    if text.document.stroke.is_none() {
        if let Some(stroke) = keys.iter().find_map(|key| key.document.stroke) {
            source_text.stroke_color = Some(rgba(stroke.color));
            source_text.stroke_width =
                NonNegativeProperty::new(STROKE_WIDTH_RATIO * f64::from(stroke.width))
                    .ok_or_else(|| unsupported("text stroke width must be nonnegative"))?;
        }
    }
    let fill = keys.iter().find_map(|key| key.document.fill);
    if text.document.fill.is_none() {
        if let Some(fill) = fill {
            source_text.fill_color = rgba(fill);
        }
    }
    let mut tracks = Vec::with_capacity(fields.len());
    for field in fields {
        let (property, name) = source_text_property(field);
        let mut keyframes = Vec::with_capacity(keys.len());
        for (index, key) in keys.iter().enumerate() {
            let doc = &key.document;
            let value = match field {
                SourceTextField::Text => PropertyValue::String(doc.text.clone()),
                SourceTextField::Size => PropertyValue::Float(f64::from(doc.size)),
                SourceTextField::FillEnabled => PropertyValue::Bool(doc.fill.is_some()),
                SourceTextField::FillColor => {
                    PropertyValue::Color(doc.fill.or(fill).map_or([1.0; 4], rgba))
                }
                SourceTextField::Tracking => PropertyValue::Float(f64::from(doc.tracking)),
                SourceTextField::Leading => {
                    PropertyValue::Float(automatic_line_spacing(doc.size) + f64::from(doc.leading))
                }
                SourceTextField::StrokeEnabled => PropertyValue::Bool(doc.stroke.is_some()),
                SourceTextField::StrokeWidth => {
                    unreachable!("width uses a text animator, removed above")
                }
                SourceTextField::AllCaps => PropertyValue::Bool(doc.all_caps),
            };
            keyframes.push(PropertyKeyframe::new(
                keyframe_id(layer_id, name, index),
                TimeOffset::from_millis(keyframes::layer_millis(key.source_ticks, in_ticks)?),
                value,
                PropertyKeyframeEasing::Hold,
            ));
        }
        let track = PropertyKeyframeTrack::new(keyframes).map_err(|error| {
            unsupported(format!(
                "Source Text {field} keys cannot be imported: {error}"
            ))
        })?;
        tracks.push((Property::new(layer_id, property), track));
    }
    let anchor_offsets: Vec<_> = keys
        .iter()
        .map(|key| super::text::point_anchor_offset(&key.document))
        .collect();
    if anchor_offsets.windows(2).any(|pair| pair[0] != pair[1]) {
        let keyframes = keys
            .iter()
            .zip(anchor_offsets)
            .enumerate()
            .map(|(index, (key, offset))| {
                Ok(PropertyKeyframe::new(
                    keyframe_id(layer_id, "point-alignment", index),
                    TimeOffset::from_millis(keyframes::layer_millis(key.source_ticks, in_ticks)?),
                    PropertyValue::Float(text.transform.anchor[1] + offset),
                    PropertyKeyframeEasing::Hold,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let track = PropertyKeyframeTrack::new(keyframes).map_err(|error| {
            unsupported(format!(
                "point-text alignment keys cannot be imported: {error}"
            ))
        })?;
        tracks.push((Property::new(layer_id, PropType::AnchorPointY), track));
    }
    Ok(tracks)
}

/// The existing text-animator wire property that adds a width delta per glyph.
pub(super) const TEXT_ANIMATOR_STROKE_WIDTH: &str = "strokeWidth";

/// FX text has no layer StrokeWidth property; a selector-free animator adds
/// the same delta to every glyph. Keep the static base and key only the delta.
fn import_stroke_width(
    text: &PrText,
    in_ticks: i64,
    layer: &mut TextLayer,
    scope: &mut LayerScope<'_, '_>,
    dynamics: &mut AnimationGraph,
) -> Result<()> {
    if !text.keyed_fields()?.contains(&SourceTextField::StrokeWidth) {
        return Ok(());
    }
    let id = FxItemId::new(next_layer_id(scope).value());
    let base = layer.source_text.stroke_width.value();
    let keys = text
        .source_text_keys
        .iter()
        .enumerate()
        .map(|(index, key)| {
            let width = key
                .document
                .stroke
                .map_or(base, |stroke| STROKE_WIDTH_RATIO * f64::from(stroke.width));
            Ok(PropertyKeyframe::new(
                keyframe_id(layer.id, "stroke-width", index),
                TimeOffset::from_millis(keyframes::layer_millis(key.source_ticks, in_ticks)?),
                PropertyValue::Float(width - base),
                PropertyKeyframeEasing::Hold,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let track = PropertyKeyframeTrack::new(keys).map_err(|error| unsupported(error.to_string()))?;
    dynamics
        .set_property(
            PropertyTarget::fx_item(id, TEXT_ANIMATOR_STROKE_WIDTH),
            PropertyAnimator::keyframes(track),
            Vec::new(),
        )
        .map_err(|error| unsupported(error.to_string()))?;
    layer.animators.push(TextAnimator {
        id,
        name: "Premiere Source Text stroke width".to_owned(),
        stroke_width: Some(0.0),
        ..TextAnimator::default()
    });
    Ok(())
}

/// Only one all-character width-only animator has a Source Text mapping.
/// Selectors, other properties and multiple animators would alter glyphs
/// independently; never collapse them into a uniform text document.
pub(super) fn stroke_width_animator(layer: &TextLayer) -> Result<Option<&TextAnimator>> {
    if layer.animators.is_empty() {
        return Ok(None);
    }
    let [animator] = layer.animators.as_slice() else {
        return Err(unsupported(
            "only one all-character stroke-width text animator can be exported",
        ));
    };
    let supported = TextAnimator {
        id: animator.id,
        name: animator.name.clone(),
        stroke_width: animator.stroke_width,
        ..TextAnimator::default()
    };
    ensure!(
        animator.stroke_width.is_some() && *animator == supported,
        "only an all-character width-only text animator can be exported"
    );
    ensure!(
        layer.source_text.stroke_color.is_some(),
        "a stroke-width text animator needs an explicit stroke color"
    );
    Ok(Some(animator))
}

/// Static widths use the authored base plus delta; enabled width keys replace
/// that delta, so only their sampled totals are validated during reconstruction.
pub(super) fn stroke_width_source(
    layer: &TextLayer,
    dynamics: &AnimationGraph,
) -> Result<TextDocument> {
    let mut source = layer.source_text.clone();
    if let Some(animator) = stroke_width_animator(layer)? {
        if stroke_width_track(animator, dynamics)?.is_some() {
            return Ok(source);
        }
        if let Some(delta) = animator.stroke_width {
            source.stroke_width = total_stroke_width(source.stroke_width.value(), delta)?;
        }
    }
    Ok(source)
}

fn total_stroke_width(base: f64, delta: f64) -> Result<NonNegativeProperty> {
    NonNegativeProperty::new(base + delta)
        .ok_or_else(|| unsupported("Source Text total stroke width must be finite and nonnegative"))
}

/// The owned width track must be independent, enabled, finite scalar keys;
/// Source Text export separately requires Hold segments and valid total widths.
fn stroke_width_track<'a>(
    animator: &TextAnimator,
    dynamics: &'a AnimationGraph,
) -> Result<Option<&'a PropertyKeyframeTrack>> {
    let target = PropertyTarget::fx_item(animator.id, TEXT_ANIMATOR_STROKE_WIDTH);
    ensure!(
        dynamics
            .entries()
            .iter()
            .all(|entry| entry.target.fx_item_id() != Some(animator.id) || entry.target == target),
        "only stroke width may be animated on an all-character width-only text animator"
    );
    let Some(entry) = dynamics
        .entries()
        .iter()
        .find(|entry| entry.target == target)
    else {
        return Ok(None);
    };
    ensure!(
        entry.dependencies.is_empty(),
        "Source Text stroke-width keys must be independent"
    );
    let AnimatorData::Keyframes {
        track,
        enabled: true,
        ..
    } = entry.animator.data()
    else {
        return Err(unsupported(
            "Source Text stroke-width animation must be enabled keyframes",
        ));
    };
    Ok(Some(track))
}

/// A text or shape layer that exports as one object of a graphic.
#[derive(Clone, Copy)]
pub(super) enum ObjectLayer<'a> {
    Text(&'a TextLayer),
    Shape(&'a ShapeLayer),
}

impl<'a> ObjectLayer<'a> {
    /// The object of a text or shape layer.
    pub(super) fn of(layer: &'a Layer) -> Option<Self> {
        match layer.data() {
            LayerData::Text(text) => Some(Self::Text(text)),
            LayerData::Shape(shape) => Some(Self::Shape(shape)),
            _ => None,
        }
    }

    /// Why this layer cannot become an object of a graphic, if it cannot.
    fn unsupported(self, consumed: &BTreeSet<LayerId>) -> Option<&'static str> {
        match self {
            Self::Text(text) => unsupported_graphic_text(text),
            Self::Shape(shape) => unsupported_graphic_shape(shape, consumed),
        }
    }

    fn id(self) -> LayerId {
        match self {
            Self::Text(layer) => layer.id,
            Self::Shape(layer) => layer.id,
        }
    }

    fn record(self) -> String {
        match self {
            Self::Text(layer) => format!("layer {} ({:?})", layer.id, layer.name),
            Self::Shape(layer) => format!("layer {} ({:?})", layer.id, layer.name),
        }
    }

    /// The layer kind, as omission reasons name it.
    fn kind(self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Shape(_) => "shape",
        }
    }

    fn active_range(self) -> TimeRangeProperty {
        match self {
            Self::Text(layer) => layer.active_range,
            Self::Shape(layer) => layer.active_range,
        }
    }

    fn is_hidden(self) -> bool {
        match self {
            Self::Text(layer) => layer.is_hidden,
            Self::Shape(layer) => layer.is_hidden,
        }
    }

    fn blend_mode(self) -> BlendMode {
        match self {
            Self::Text(layer) => layer.blend_mode,
            Self::Shape(layer) => layer.blend_mode,
        }
    }

    fn parent(self) -> Option<LayerId> {
        match self {
            Self::Text(layer) => layer.parent,
            Self::Shape(layer) => layer.parent,
        }
    }

    fn effects(self) -> &'a [EffectRecord] {
        match self {
            Self::Text(layer) => &layer.effects,
            Self::Shape(layer) => &layer.effects,
        }
    }
}

enum GraphicPart<'a> {
    /// A text or shape layer, used as Mask with Shape or Text when a
    /// masked sibling group takes it as its track matte.
    Object {
        layer: ObjectLayer<'a>,
        siblings: &'a [Layer],
        mask_source: Option<PrMaskSource>,
    },
    /// A SubGroup, named: a group at the identity without a track matte, or
    /// a Mask with Shape or Text with its masked group's objects when other
    /// objects follow them, which a Premiere mask would cut too.
    Group(String, Vec<GraphicPart<'a>>),
}

/// The parts of `layers`, one FX list of a graphic group whose parent spans
/// `duration`, or `None` when a layer belongs to no graphic, which makes the
/// group a nested sequence, as does a group inside it that `dynamics` keys.
/// A masked group is a group at the identity whose
/// alpha or inverted alpha track matte is a text or shape layer beside it,
/// which no other masked group uses: that Mask with Shape or Text, then the
/// group's own objects ([`import_graphic`] writes that form). Object masks
/// remain unsupported and their guides are not consumed here.
fn graphic_parts<'a>(
    layers: &'a [Layer],
    duration: Duration,
    dynamics: &AnimationGraph,
) -> Option<Vec<GraphicPart<'a>>> {
    let mut sources: BTreeMap<LayerId, usize> = BTreeMap::new();
    for layer in layers {
        if let LayerData::Group(GroupLayer {
            track_matte: Some(matte),
            ..
        }) = layer.data()
        {
            *sources.entry(matte.layer).or_default() += 1;
        }
    }
    if sources.values().any(|&uses| uses > 1) {
        return None;
    }
    let guides = mask_guide_ids(layers);
    let part_of = |layer: &Layer| {
        !(sources.contains_key(&layer.id())
            || (guides.contains(&layer.id()) && matches!(layer.data(), LayerData::Shape(_))))
    };
    let mut parts = Vec::new();
    for (position, layer) in layers.iter().enumerate() {
        if !part_of(layer) {
            continue;
        }
        let group = match layer.data() {
            LayerData::Text(_) | LayerData::Shape(_) => {
                parts.push(GraphicPart::Object {
                    layer: ObjectLayer::of(layer)?,
                    siblings: layers,
                    mask_source: None,
                });
                continue;
            }
            LayerData::Group(group) if graphic_subgroup(group, duration, dynamics) => group,
            _ => return None,
        };
        let inner = graphic_parts(
            &group.layers,
            group.playback.input_range().duration,
            dynamics,
        )?;
        let Some(matte) = &group.track_matte else {
            parts.push(GraphicPart::Group(group.name.clone(), inner));
            continue;
        };
        let inverted = match matte.mode {
            TrackMatteType::Alpha => false,
            TrackMatteType::AlphaInverted => true,
            TrackMatteType::Luma | TrackMatteType::LumaInverted => return None,
        };
        let source = layers
            .iter()
            .find(|layer| layer.id() == matte.layer)
            .and_then(ObjectLayer::of)?;
        let masked = std::iter::once(GraphicPart::Object {
            layer: source,
            siblings: layers,
            mask_source: Some(PrMaskSource { inverted }),
        })
        .chain(inner);
        if layers[position + 1..].iter().any(part_of) {
            parts.push(GraphicPart::Group(group.name.clone(), masked.collect()));
        } else {
            parts.extend(masked);
        }
    }
    Some(parts)
}

/// Whether `group`, inside a graphic group whose layers span `duration`, is
/// a SubGroup or a masked group: at the identity, without keys in
/// `dynamics`, over the whole span, shown, blending normally, and without
/// effects, masks, background, time remapping or motion blur.
fn graphic_subgroup(group: &GroupLayer, duration: Duration, dynamics: &AnimationGraph) -> bool {
    group.transform == identity_transform()
        && layer_animations(dynamics, group.id).next().is_none()
        && group.playback.input_range().start == Time::ZERO
        && group.playback.input_range().duration >= duration
        && !group.is_hidden
        && group.blend_mode == BlendMode::Normal
        && group.effects.is_empty()
        && group.masks.is_empty()
        && !has_background(group)
        && super::timing::is_plain_group_playback(&group.playback)
        && !group.motion_blur
}

/// The objects of `parts`, in chain order.
fn part_objects<'a>(parts: &[GraphicPart<'a>]) -> Vec<ObjectLayer<'a>> {
    let mut objects = Vec::new();
    for part in parts {
        match part {
            GraphicPart::Object { layer, .. } => objects.push(*layer),
            GraphicPart::Group(_, inner) => objects.extend(part_objects(inner)),
        }
    }
    objects
}

/// The Mask with Shape and Text layers of `parts`, which their masked
/// groups consume.
fn part_mask_sources(parts: &[GraphicPart<'_>]) -> BTreeSet<LayerId> {
    let mut sources = BTreeSet::new();
    for part in parts {
        match part {
            GraphicPart::Object {
                layer,
                mask_source: Some(_),
                ..
            } => {
                sources.insert(layer.id());
            }
            GraphicPart::Object { .. } => {}
            GraphicPart::Group(_, inner) => sources.extend(part_mask_sources(inner)),
        }
    }
    sources
}

/// The objects of a group that exports as one graphic, in Premiere's chain
/// order ([`graphic_parts`]), whose keys are in `dynamics`; every caller that
/// routes a group asks this. Groups of other layers are nested sequences. A
/// group of text and shape layers alone is a graphic, as before SubGroups
/// converted, and its export reports what it cannot keep. A group with groups
/// of its own is one only when no object of several is keyed, which no
/// graphic of several objects carries, each text converts whatever its font
/// ([`text_converts`]), no layer of its own uses one of its objects as a
/// track matte, mask or text path but as a Mask with Shape or Text, and no
/// other group rule omits it ([`unsupported_graphic_group`]: its transform,
/// a child that does not span it, and the others); otherwise it stays the
/// nested sequence that it was, which exports the parts it can. In its
/// graphic, a Shape that does not convert, or a Text whose font is not
/// packaged, goes alone ([`export_objects`]), while a layer outside the
/// group that uses one of its layers omits the graphic
/// ([`export_graphic_group`]).
pub(super) fn graphic_objects<'a>(
    group: &'a GroupLayer,
    dynamics: &AnimationGraph,
) -> Option<Vec<ObjectLayer<'a>>> {
    if !group.masks.is_empty()
        && group
            .layers
            .iter()
            .any(|layer| matches!(layer.data(), LayerData::Group(_)))
    {
        return None;
    }
    let parts = graphic_parts(
        &group.layers,
        group.playback.input_range().duration,
        dynamics,
    )?;
    let mask_sources = part_mask_sources(&parts);
    // This also describes a moved Source Graphic placement. Without provenance,
    // retain its established nest route rather than absorb its Motion wrapper.
    if group.transform != identity_transform()
        && group.layers.len() == 1
        && matches!(parts.as_slice(), [GraphicPart::Group(..)])
        && mask_sources.is_empty()
    {
        return None;
    }
    let objects = part_objects(&parts);
    if objects.is_empty() {
        return None;
    }
    if holds_only_objects(group) {
        return Some(objects);
    }
    let keyed = objects.len() > 1
        && objects.iter().any(|object| {
            layer_animations(dynamics, object.id()).next().is_some()
                || matches!(object, ObjectLayer::Text(text) if text.animators.iter().any(|animator| {
                    dynamics.entries().iter().any(|entry| entry.target.fx_item_id() == Some(animator.id))
                }))
        });
    // Whether a group around this one hides it is unknown here: a text's
    // path counts as used unless the text or this group is hidden, and the
    // group then stays the nest that it was.
    let used: BTreeSet<LayerId> = consumed_layer_ids(&group.layers, group.is_hidden)
        .difference(&mask_sources)
        .copied()
        .collect();
    let texts_convert = objects.iter().all(|object| match object {
        ObjectLayer::Text(text) => text.path_options.is_none() && text_converts(text, dynamics),
        ObjectLayer::Shape(_) => true,
    });
    let nest =
        keyed || !texts_convert || unsupported_graphic_group(group, &objects, &used).is_some();
    (!nest).then_some(objects)
}

/// Whether text layer `layer` converts to a graphic Text ([`text_object`])
/// whatever font export finds for it: which fonts are packaged only export
/// knows, and there an unpackaged one leaves out that Text alone
/// ([`export_objects`]). The check reports nothing.
fn text_converts(layer: &TextLayer, dynamics: &AnimationGraph) -> bool {
    let doc = &layer.source_text;
    // A style names a packaged face, whose PostScript name export looks up;
    // the text's checks read the name only as a name.
    let font = fonts::postscript_name(&doc.font_family, &doc.font_style, &BTreeMap::new())
        .unwrap_or_else(|_| "PackagedFace".to_owned());
    text_object(layer, layer.parent, font, dynamics, &mut Vec::new(), "")
        .is_ok_and(|text| PrGraphicObject::Text(text).validate().is_ok())
}

/// Whether `group` holds text and shape layers alone, a graphic as before
/// SubGroups converted, whose export keeps the rules that it had then.
fn holds_only_objects(group: &GroupLayer) -> bool {
    group
        .layers
        .iter()
        .all(|layer| ObjectLayer::of(layer).is_some())
}

/// Export one FX text or shape layer of a list whose layers have `parent`
/// (a nest's group, or none at the root) as a graphic of that one object, a
/// text keeping the layer's transform keys, or omit the layer with the
/// reason. `consumed` holds the layers that other layers use as a track
/// matte or mask, or as the path of a text that FX shows
/// (`consumed_layer_ids`). The graphic's Opacity carries the layer's blend
/// mode, as import reads a graphic's onto a group of its objects.
pub(super) fn export_object(
    object: ObjectLayer<'_>,
    parent: Option<LayerId>,
    consumed: &BTreeSet<LayerId>,
    context: &mut LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Option<PrGraphic> {
    if let Some(reason) = object.unsupported(consumed).or(match object {
        ObjectLayer::Shape(shape) if !shape.masks.is_empty() => {
            Some("its shape layer must have no masks")
        }
        _ => None,
    }) {
        omit(
            omissions,
            OmissionScope::Occurrence,
            record,
            format!("graphic was not exported: {reason}"),
        );
        return None;
    }
    let blend_mode = object.blend_mode();
    let (text, shape);
    let object = match object {
        ObjectLayer::Text(layer) if blend_mode != BlendMode::Normal => {
            text = TextLayer {
                blend_mode: BlendMode::Normal,
                ..layer.clone()
            };
            ObjectLayer::Text(&text)
        }
        ObjectLayer::Shape(layer) if blend_mode != BlendMode::Normal => {
            shape = ShapeLayer {
                blend_mode: BlendMode::Normal,
                ..layer.clone()
            };
            ObjectLayer::Shape(&shape)
        }
        object => object,
    };
    let mut graphic = export_objects(
        &[object],
        None,
        parent,
        &BTreeSet::new(),
        context,
        omissions,
        record,
    )?
    .graphic;
    if let [PrGraphicObject::Group(pieces)] = graphic.objects.as_slice() {
        if let Some((_, reason)) = unverified_mask_composite(&pieces.objects, 1) {
            omit(omissions, OmissionScope::Occurrence, record, reason);
            return None;
        }
    }
    graphic.blend_mode = PrBlendMode::from_fx_mode(blend_mode);
    if let Some(warning) = PrBlendMode::export_approximation(blend_mode) {
        approximate(omissions, record, warning);
    }
    Some(graphic)
}

/// Export a group of text and shape layers as one graphic whose Vector
/// Motion is the group transform, or omit it with the reason; a nest's
/// graphic exports into its sequence. No native nested graphic is measured
/// yet: that form is structurally tested only. `consumed` holds the layers
/// that other layers use as a track matte or mask, or as the path of a text
/// that FX shows (`consumed_layer_ids`). The group's one mask, over a guide
/// among `siblings`, the list that holds the group, is the clip Opacity
/// mask ([`graphic_opacity_mask`]).
pub(super) fn export_graphic_group(
    group: &GroupLayer,
    objects: &[ObjectLayer<'_>],
    siblings: &[Layer],
    consumed: &BTreeSet<LayerId>,
    context: &mut LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
) -> Option<PrGraphic> {
    let record = format!("layer {} ({:?})", group.id, group.name);
    let parts = graphic_parts(
        &group.layers,
        group.playback.input_range().duration,
        context.dynamics,
    )?;
    let own = part_mask_sources(&parts);
    let consumed: BTreeSet<LayerId> = consumed.difference(&own).copied().collect();
    if let Some(reason) = unsupported_graphic_group(group, objects, &consumed) {
        omit(
            omissions,
            OmissionScope::Occurrence,
            &record,
            format!("graphic group was not exported: {reason}"),
        );
        return None;
    }
    let frame = [context.width, context.height];
    let opacity_mask = match graphic_opacity_mask(
        group,
        siblings,
        context.dynamics,
        frame,
        context.frame_rate.generator_in_ticks(),
    ) {
        Ok(mask) => mask,
        Err(reason) => {
            omit(
                omissions,
                OmissionScope::Occurrence,
                &record,
                format!("graphic group was not exported: its mask cannot be exported: {reason}"),
            );
            return None;
        }
    };
    if !group.description.is_empty() {
        omit_field(
            omissions,
            group.id,
            ExportField::Description,
            &record,
            "description was not exported",
        );
    }
    let object_record = match objects {
        [object] => object.record(),
        _ => record.clone(),
    };
    let ExportedObjects {
        mut graphic,
        unexported,
    } = export_objects(
        objects,
        Some(group),
        Some(group.id),
        &own,
        context,
        omissions,
        &object_record,
    )?;
    let mut natives = NativeObjects {
        objects: std::mem::take(&mut graphic.objects).into_iter(),
        unexported,
    };
    let (objects, mut written) = arrange(
        &parts,
        &mut natives,
        0,
        context,
        graphic.in_ticks,
        omissions,
        &record,
    );
    graphic.objects = objects;
    if graphic.objects.is_empty() {
        omit(
            omissions,
            OmissionScope::Occurrence,
            &record,
            "graphic group was not exported: none of its objects can be exported",
        );
        return None;
    }
    if !own.is_empty() {
        approximate(omissions, &record, MASK_SOURCE_LINEAR_LIGHT_APPROXIMATION);
    }
    if let Some(mask) = &opacity_mask {
        for warning in mask.approximations() {
            approximate(omissions, &record, warning);
        }
        context.written.record_mask(group.masks[0].id, mask);
    }
    graphic.opacity_mask = opacity_mask;
    context.written.append(&mut written);
    if let Some(motion) = &graphic.vector_motion {
        context
            .written
            .record_animations(group.id, &motion.animations);
    }
    context
        .written
        .record_animations(group.id, &graphic.animations);
    Some(graphic)
}

/// The native objects of a graphic group's parts ([`export_objects`]), in
/// chain order, and why an object layer of several has none or is left out.
struct NativeObjects {
    objects: std::vec::IntoIter<PrGraphicObject>,
    unexported: BTreeMap<LayerId, Unexported>,
}

impl NativeObjects {
    /// The native object of the next part, object layer `layer`, or why it
    /// has none or is left out.
    fn take(&mut self, layer: LayerId) -> std::result::Result<PrGraphicObject, String> {
        match self.unexported.get(&layer) {
            Some(Unexported::Unconverted(reason)) => Err(reason.clone()),
            Some(Unexported::Unrendered(reason)) => {
                self.objects.next();
                Err(reason.clone())
            }
            None => self
                .objects
                .next()
                .ok_or_else(|| "its objects were not exported".to_owned()),
        }
    }

    /// Passes over the native object of the next part, object layer `layer`,
    /// which is not exported.
    fn skip(&mut self, layer: LayerId) {
        if !matches!(
            self.unexported.get(&layer),
            Some(Unexported::Unconverted(_))
        ) {
            self.objects.next();
        }
    }
}

/// Why a SubGroup without objects is not exported: no native save holds one.
const EMPTY_SUBGROUP_UNVERIFIED: &str = "an empty SubGroup is unverified against Premiere";

/// The native objects of `parts` at SubGroup `depth`, from `natives`, one
/// per object in chain order: in their SubGroups, with each Mask with Shape
/// or Text role. A part that cannot convert is
/// reported and left out, and a Mask with Shape or Text takes the objects
/// below it in its group with it, as does a composite outside the rendered
/// forms ([`unverified_mask_composite`]), so that nothing it masks shows. A
/// SubGroup left with no object draws nothing and is left out, its objects
/// reported; one without objects of its own is reported
/// ([`EMPTY_SUBGROUP_UNVERIFIED`]).
fn arrange(
    parts: &[GraphicPart<'_>],
    natives: &mut NativeObjects,
    depth: usize,
    context: &LayerExport<'_, '_>,
    source_in: i64,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> (Vec<PrGraphicObject>, WrittenAnimation) {
    let mut objects = Vec::with_capacity(parts.len());
    let mut object_keys = Vec::with_capacity(parts.len());
    let mut failed = None;
    for (position, part) in parts.iter().enumerate() {
        let mut written = WrittenAnimation::default();
        let arranged = match part {
            GraphicPart::Group(name, inner) => {
                let (members, mut keys) = arrange(
                    inner,
                    natives,
                    depth + 1,
                    context,
                    source_in,
                    omissions,
                    record,
                );
                written.append(&mut keys);
                if !members.is_empty() {
                    Ok(PrGraphicObject::Group(PrGraphicGroup {
                        name: name.clone(),
                        objects: members,
                    }))
                } else if part_objects(inner).is_empty() {
                    Err(EMPTY_SUBGROUP_UNVERIFIED.to_owned())
                } else {
                    continue;
                }
            }
            GraphicPart::Object {
                layer,
                siblings,
                mask_source,
            } => natives.take(layer.id()).and_then(|native| {
                masked_object(
                    native,
                    *layer,
                    siblings,
                    *mask_source,
                    depth,
                    context,
                    source_in,
                )
            }),
        };
        match arranged {
            Ok(object) => {
                if let PrGraphicObject::Shape(shape) = &object {
                    if let Some(mask) = &shape.mask {
                        for warning in mask.approximations() {
                            approximate(omissions, record, warning);
                        }
                        if let GraphicPart::Object {
                            layer: ObjectLayer::Shape(layer),
                            ..
                        } = part
                        {
                            written.record_mask(layer.masks[0].id, mask);
                        }
                    }
                }
                if let (GraphicPart::Object { layer, .. }, PrGraphicObject::Text(text)) =
                    (part, &object)
                {
                    written.record_animations(layer.id(), &text.animations);
                }
                objects.push(object);
                object_keys.push(written);
            }
            Err(reason) => {
                let object_record = match part {
                    GraphicPart::Object { layer, .. } => layer.record(),
                    GraphicPart::Group(name, _) => format!("group {name:?}"),
                };
                let masks = matches!(
                    part,
                    GraphicPart::Object {
                        mask_source: Some(_),
                        ..
                    }
                );
                omit(
                    omissions,
                    OmissionScope::Feature,
                    record,
                    format!(
                        "{object_record} was not exported: {reason}{}",
                        if masks {
                            "; the objects that it masks are not exported either"
                        } else {
                            ""
                        }
                    ),
                );
                if masks {
                    failed = Some(position);
                    break;
                }
            }
        }
    }
    // The parts below a failed mask are not exported.
    if let Some(position) = failed {
        for part in &parts[position + 1..] {
            for object in part_objects(std::slice::from_ref(part)) {
                natives.skip(object.id());
            }
        }
    }
    if let Some((index, reason)) = unverified_mask_composite(&objects, depth) {
        let below = match objects.len() - index - 1 {
            1 => "the 1 object".to_owned(),
            below => format!("the {below} objects"),
        };
        omit(
            omissions,
            OmissionScope::Feature,
            record,
            format!("a Mask with Shape or Text and {below} below it were not exported: {reason}"),
        );
        objects.truncate(index);
        object_keys.truncate(index);
    }
    let mut written = WrittenAnimation::default();
    for mut keys in object_keys {
        written.append(&mut keys);
    }
    (objects, written)
}

fn masked_object(
    mut native: PrGraphicObject,
    layer: ObjectLayer<'_>,
    siblings: &[Layer],
    mask_source: Option<PrMaskSource>,
    depth: usize,
    context: &LayerExport<'_, '_>,
    source_in: i64,
) -> std::result::Result<PrGraphicObject, String> {
    match (&mut native, layer) {
        (PrGraphicObject::Shape(shape), ObjectLayer::Shape(shape_layer)) => {
            shape.mask = graphic_mask(
                &shape_layer.masks,
                siblings,
                ("shape", shape_layer.parent, shape_layer.active_range),
                context.dynamics,
                [context.width, context.height],
            )?;
            if let Some(mask) = &mut shape.mask {
                super::mask_animation::export_tracks(
                    mask,
                    shape_layer.masks[0].id,
                    context.dynamics,
                    source_in,
                )?;
            }
            shape.appearance.mask_source = mask_source;
        }
        (PrGraphicObject::Text(text), ObjectLayer::Text(_)) => text.mask_source = mask_source,
        (PrGraphicObject::Group(pieces), ObjectLayer::Shape(shape_layer)) => {
            if !shape_layer.masks.is_empty() || mask_source.is_some() {
                return Err("a shape of several contours cannot be a mask or have one".to_owned());
            }
            if let Some((_, reason)) = unverified_mask_composite(&pieces.objects, depth + 1) {
                return Err(format!("its pieces cannot export: {reason}"));
            }
        }
        _ => return Err("its object does not follow its layer".to_owned()),
    }
    Ok(native)
}

/// Why a group of `objects` cannot export as one graphic, if it cannot:
/// Vector Motion is a uniform 2D transform, whose opacity is the clip
/// Opacity's, a graphic has one clock and no group effects, its one mask is
/// its clip Opacity mask, which [`export_graphic_group`] checks, only a
/// text has a background ([`group_background`]), several objects have one
/// visibility, no other layer may use the group or its layers as a track
/// matte or mask, or as the path of a text that FX shows (a graphic of one
/// text exports as root text does), and each object must pass
/// [`unsupported_graphic_text`] or [`unsupported_graphic_shape`].
fn unsupported_graphic_group(
    group: &GroupLayer,
    objects: &[ObjectLayer<'_>],
    consumed: &BTreeSet<LayerId>,
) -> Option<&'static str> {
    let transform = &group.transform;
    let background = has_background(group) && !matches!(objects, [ObjectLayer::Text(_)]);
    [
        (!group.effects.is_empty(), "a graphic has no group effects"),
        (
            group.track_matte.is_some(),
            "a graphic has no group track matte",
        ),
        (background, "a graphic has no group background"),
        (
            !super::timing::is_plain_group_playback(&group.playback),
            "graphic time remapping is unsupported",
        ),
        (
            transform.scale[0] != transform.scale[1],
            "Vector Motion scale must be uniform",
        ),
        (
            transform.skew != 0.0
                || transform.skew_axis != 0.0
                || transform.rotation_x != 0.0
                || transform.rotation_y != 0.0
                || transform.orientation != [0.0; 3]
                || transform.position.z().is_some(),
            "Vector Motion has no skew or 3D rotation",
        ),
        (
            objects.len() > 1 && objects.iter().any(|object| object.is_hidden()),
            "the objects of a graphic are hidden together",
        ),
        (
            !matches!(objects, [ObjectLayer::Text(_)])
                && std::iter::once(group.id)
                    .chain(objects.iter().map(|object| object.id()))
                    .any(|id| consumed.contains(&id)),
            "the graphic or one of its layers is another layer's track matte, mask or text path",
        ),
    ]
    .into_iter()
    .find_map(|(unsupported, reason)| unsupported.then_some(reason))
    .or_else(|| {
        objects.iter().find_map(|&object| {
            let range = object.active_range();
            if range.start != Time::ZERO || range.duration < group.playback.input_range().duration {
                return Some(match object {
                    ObjectLayer::Text(_) => "its text layer must span the group",
                    ObjectLayer::Shape(_) => "its shape layer must span the group",
                });
            }
            object.unsupported(consumed)
        })
    })
}

/// The text background of a group of one text, `text`: the group's
/// background in the calibrated form, one padding on every side, one
/// opaque solid fill and one radius on every corner, drawn as
/// [`PrTextBackground::unverified_reason`] allows. `None` without a
/// background; an error names why the group's background is not one.
///
/// `text` holds the text layer's own transform and keys, before the group's
/// Vector Motion is composed into it. The box was rendered around an
/// unscaled, unrotated, opaque text without keys; the FX renderer pads its group box
/// in group space whatever the child's transform and opacity, while Premiere
/// draws the box as part of the transformed text object, so any other child
/// is unverified. The child's position and anchor are not checked: at scale
/// 100 and rotation 0 they translate the text, and both engines translate
/// the box with it. The group's own Vector Motion scales or rotates box and
/// text together in both engines and stays allowed.
fn group_background(group: &GroupLayer, text: &PrText) -> Result<Option<PrTextBackground>> {
    if !has_background(group) {
        return Ok(None);
    }
    if let Some(reason) = text_shadow::scaled_or_rotated_text(text) {
        return Err(unsupported(reason));
    }
    ensure!(text.animations.is_empty(), "its text has keys");
    ensure!(text.transform.opacity == 100.0, "its text is not opaque");
    let padding = group.padding_top.value();
    ensure!(
        [
            group.padding_right,
            group.padding_bottom,
            group.padding_left
        ]
        .iter()
        .all(|value| value.value() == padding),
        "a text background has one size on every side"
    );
    let radius = group.corner_radius_top_left.value();
    ensure!(
        [
            group.corner_radius_top_right,
            group.corner_radius_bottom_right,
            group.corner_radius_bottom_left,
        ]
        .iter()
        .all(|value| value.value() == radius),
        "a text background has one radius on every corner"
    );
    let [fill] = group.fills.as_slice() else {
        return Err(unsupported("a text background has one fill"));
    };
    let ShapePaint::Solid { color } = fill.paint else {
        return Err(unsupported("a text background has a solid fill"));
    };
    ensure!(
        *fill == ShapeFillStyle::solid(color),
        "a text background fill blends normally at full opacity"
    );
    let background = PrTextBackground {
        color: rgb(color, "text background fill")?,
        opacity: 100.0,
        size: padding as f32,
        radius: radius as f32,
    };
    match background.unverified_reason(&text.document) {
        Some(reason) => Err(unsupported(reason)),
        None => Ok(Some(background)),
    }
}

/// Why `text` cannot become the text of a graphic, if it cannot, for a root
/// text layer and for the text of a graphic group alike. Graphic export
/// writes no text masks or track matte, and the text would show where they
/// hide it.
fn unsupported_graphic_text(text: &TextLayer) -> Option<&'static str> {
    [
        (!text.masks.is_empty(), "its text layer must have no masks"),
        (
            text.track_matte.is_some(),
            "its text layer must have no track matte",
        ),
    ]
    .into_iter()
    .find_map(|(unsupported, reason)| unsupported.then_some(reason))
}

/// Why `shape` cannot become a Shape of a graphic, if it cannot, as
/// [`unsupported_graphic_text`] for text. A shape in `consumed`, which
/// another layer uses as a track matte or mask, or a text that FX shows as
/// its path, is not content of its own.
fn unsupported_graphic_shape(
    shape: &ShapeLayer,
    consumed: &BTreeSet<LayerId>,
) -> Option<&'static str> {
    [
        (
            shape.track_matte.is_some(),
            "its shape layer must have no track matte",
        ),
        (
            consumed.contains(&shape.id),
            "its shape layer is another layer's track matte, mask or text path",
        ),
    ]
    .into_iter()
    .find_map(|(unsupported, reason)| unsupported.then_some(reason))
}

/// Why the shape `shape` is no track matte source, if it is not, as
/// [`super::still::unsupported_matte_still`] for a still. A matte shape
/// exports as the graphic of one static Shape ([`export_object`]), whose
/// rendered alpha is the coverage of its fill and stroke, as FX reads it: a
/// plain path with one fill and one stroke in Premiere's forms, no
/// primitive or modifier, Normal blending, and no masks, effects or keys. A
/// graphic's effects and a blended or keyed graphic export with their own
/// limits, and a Track Matte Key over such a source is unmeasured, so a
/// matte's coverage must export whole or the key gates its clip by a
/// different picture. This mirrors the decision that [`export_object`] takes
/// for the same layer, so a clip never keys a source that does not export.
pub(super) fn unsupported_matte_shape(
    shape: &ShapeLayer,
    dynamics: &AnimationGraph,
) -> Option<String> {
    let unsupported = [
        (
            !shape.masks.is_empty(),
            "the track matte source shape has masks",
        ),
        (
            !shape.effects.is_empty(),
            "the track matte source shape has effects; a Track Matte Key over an effected graphic is unmeasured",
        ),
        (
            shape.blend_mode != BlendMode::Normal,
            "the track matte source shape blends; a Track Matte Key over a blended graphic is unmeasured",
        ),
        (
            layer_animations(dynamics, shape.id).next().is_some(),
            "the track matte source shape has keys; keyed graphic shapes are unsupported",
        ),
    ]
    .into_iter()
    .find_map(|(unsupported, reason)| unsupported.then_some(reason.to_owned()));
    if unsupported.is_some() {
        return unsupported;
    }
    let exports = || -> Result<()> {
        shape_object(shape, None)?.object.validate()?;
        Ok(())
    };
    exports()
        .err()
        .map(|error| format!("the track matte source shape does not export as a graphic: {error}"))
}

/// The native object of one FX object layer whose parent should be `parent`,
/// or `None` when its font is not exportable, which is reported.
fn object_part(
    object: ObjectLayer<'_>,
    parent: Option<LayerId>,
    motion_scale: Option<f64>,
    context: &LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Option<Result<(PrGraphicObject, Vec<String>)>> {
    match object {
        ObjectLayer::Text(layer) => {
            let doc = &layer.source_text;
            let font =
                match fonts::postscript_name(&doc.font_family, &doc.font_style, context.fonts) {
                    Ok(font) => font,
                    Err(reason) => {
                        omit(omissions, OmissionScope::Occurrence, record, reason);
                        return None;
                    }
                };
            Some(
                text_object(layer, parent, font, context.dynamics, omissions, record)
                    .map(|text| (PrGraphicObject::Text(text), Vec::new())),
            )
        }
        ObjectLayer::Shape(layer) => {
            for (changed, field) in unexported_layer_fields(ClipLayer::Shape(layer), parent, true) {
                if changed {
                    omit_field(
                        omissions,
                        layer.id,
                        field,
                        record,
                        format!("{field} was not exported"),
                    );
                }
            }
            omit_object_blend(layer.id, layer.blend_mode, omissions, record);
            let part = shape_object(layer, motion_scale).map(|shape| {
                let ShapeObject { object, near } = shape;
                let mut approximations = Vec::new();
                if let PrGraphicObject::Group(pieces) = &object {
                    let outlined = pieces.objects.iter().any(|piece| {
                        matches!(piece, PrGraphicObject::Shape(shape)
                            if shape.appearance.fill.is_none() && shape.appearance.stroke.is_some())
                    });
                    if outlined && layer.transform.opacity.value() != 100.0 {
                        approximations.push(COMPOUND_OPACITY_APPROXIMATION.to_owned());
                    }
                    let holed = pieces.objects.iter().any(|piece| {
                        matches!(piece, PrGraphicObject::Shape(shape)
                            if shape.appearance.mask_source.is_some())
                    });
                    if holed {
                        approximations.push(COMPOUND_HOLE_EDGE_APPROXIMATION.to_owned());
                    }
                }
                match near {
                    Some(NearContours::Pair([first, second])) => {
                        approximations.push(compound_near_approximation(first, second));
                    }
                    Some(NearContours::Unbounded) => {
                        approximations.push(compound_unbounded_approximation());
                    }
                    None => {}
                }
                (object, approximations)
            });
            Some(part)
        }
    }
}

/// Reported for a shape of several contours with a hole: the hole's edge
/// pixels take the coverage of an inverted mask, one minus the hole's, where
/// FX's one path gives them the band's own. FX's antialiasing is not the
/// covered area (a pixel that an edge covers by 0.5 drew at 0.75), so the two
/// differ at any gap. Measured, not a bound: FX's own render of the pieces
/// drew 0.44 to 0.56 of a pixel's alpha less per hole-edge pixel on average,
/// 0.75 at the worst pixel, in the native margin controls
/// and the 1 to 3 px gap sweep. Retained native hole controls later
/// measured +0.519/+0.278/+0.496 px native-minus-FX coverage offsets;
/// neither measurement bounds other paths or proves current export parity.
const COMPOUND_HOLE_EDGE_APPROXIMATION: &str = "a shape of several contours exports each hole as an inverted Mask with Shape, whose edge pixels take one minus the hole's antialiased coverage, not the path's own: in the Premiere margin controls, FX's render of such pieces drew hole edges about half a pixel's alpha fainter on average (0.44 to 0.56, 0.75 at the worst pixel); retained native hole controls measured +0.519/+0.278/+0.496 px native-minus-FX offsets; these measurements are not bounds for other paths or proof of current export parity";

/// Reported for a shape of several contours of which two may pass within
/// [`COMPOUND_EDGE_PIXELS`] of each other on screen: their pieces antialias
/// apart, so a band between them narrower than a pixel fades. Measured, not
/// a bound: FX's render of the pieces kept 36 to 56 % of such bands'
/// coverage at gaps of 0.25 and 0.625 px; Premiere's antialiasing is unmeasured.
fn compound_near_approximation(first: usize, second: usize) -> String {
    format!(
        "contours {first} and {second} of the shape path may pass within {COMPOUND_EDGE_PIXELS} px of each other on screen, at the smallest scale of the shape, its graphic and the nests that hold it, and their Premiere pieces antialias apart: a band between them narrower than a pixel can lose most of its coverage (in the Premiere margin controls FX's render of such pieces kept 36 to 56 % at 0.25 to 0.625 px, measured there); Premiere's antialiasing is unmeasured"
    )
}

/// Reported for a shape of several contours whose smallest scale on screen
/// has no positive lower bound: a Scale key of its graphic or of a nest that
/// holds it eases past its keys, or a scale reaches 0 or changes sign, so any
/// two of its contours may pass within [`COMPOUND_EDGE_PIXELS`] of each
/// other ([`compound_near_approximation`]).
fn compound_unbounded_approximation() -> String {
    format!(
        "the contours of this shape path may pass within {COMPOUND_EDGE_PIXELS} px of each other on screen: a Scale key of its graphic or of a nest that holds it eases past its keys, or a scale reaches 0, so its smallest scale has no positive lower bound, and the Premiere pieces antialias apart: a band between them narrower than a pixel can lose most of its coverage (in the Premiere margin controls FX's render of such pieces kept 36 to 56 % at 0.25 to 0.625 px, measured there); Premiere's antialiasing is unmeasured"
    )
}

/// Reported for a translucent shape of several contours whose inner
/// contours are stroked: each Premiere piece takes the shape's Opacity, so
/// where such a stroke covers the fill beside it the two blend one over the
/// other, where FX blends the whole shape once; the band is half the stroke
/// wide. Inferred from the construction, unmeasured.
const COMPOUND_OPACITY_APPROXIMATION: &str = "a translucent shape of several contours exports as pieces that each take its Opacity; where an inner contour's stroke covers the fill beside it, Premiere blends the two one over the other where FX blends the shape once (inferred, unmeasured)";

/// Why an object layer of a graphic of several exports without its native
/// object ([`arrange`] reports it).
enum Unexported {
    /// It has none: it does not convert.
    Unconverted(String),
    /// Its native object lacks an effect that FX draws; it is a Mask with
    /// Shape or Text, whose matte FX draws with its effects, so it would mask
    /// differently.
    Unrendered(String),
}

/// A graphic that [`export_objects`] made, and the object layers of several
/// that it leaves to [`arrange`] to report.
struct ExportedObjects {
    graphic: PrGraphic,
    unexported: BTreeMap<LayerId, Unexported>,
}

/// Export text and shape layers as one graphic whose objects follow their
/// paint order: one layer of a list, or the layers of `group` with the
/// group's Vector Motion; `parent` is the parent they have, the list's or
/// `group`. Each text keeps its supported authored keys on the graphic's
/// generator clock; an unsupported object is omitted without its independent
/// siblings. Optional Source Text keys retain the valid base document and
/// independent Motion keys on failure. One object reports
/// under its own layer and several under their group; a gradient shape
/// reports its approximations ([`PrShape::gradient_approximations`]) under
/// its own layer.
fn export_objects(
    objects: &[ObjectLayer<'_>],
    group: Option<&GroupLayer>,
    parent: Option<LayerId>,
    mask_sources: &BTreeSet<LayerId>,
    context: &mut LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Option<ExportedObjects> {
    let frame = [context.width, context.height];
    let several = objects.len() > 1;
    let failed = |omissions: &mut dyn OmissionSink, error: String| {
        let reason = match objects {
            [object] => format!("{} layer was not exported: {error}", object.kind()),
            _ => format!("graphic group was not exported: {error}"),
        };
        omit(omissions, OmissionScope::Occurrence, record, reason);
        None
    };
    let motion_scale = group
        .map_or(Some(1.0), |group| smallest_scale(group, context))
        .zip(context.nest_scale)
        .map(|(own, outer)| own * outer);
    let mut approximations = Vec::new();
    let mut natives = Vec::with_capacity(objects.len());
    let mut kept = Vec::new();
    let mut unexported = BTreeMap::new();
    let mut matte_reports = BTreeMap::new();
    for &object in objects {
        let object_record = object.record();
        let source = mask_sources.contains(&object.id());
        let mut held = MatteReports::default();
        let sink: &mut dyn OmissionSink = if source { &mut held } else { &mut *omissions };
        let object_parent = if group.is_some() {
            object.parent()
        } else {
            parent
        };
        let native = object_part(
            object,
            object_parent,
            motion_scale,
            context,
            sink,
            &object_record,
        );
        let missing_font = native.is_none();
        let keyed = several && context.property_tracks.contains_key(&object.id());
        if missing_font && group.is_none() {
            held.forward(omissions);
            return None;
        }
        let native = native
            .unwrap_or_else(|| Err(unsupported("its font is not packaged")))
            .and_then(|(native, approximations)| {
                native.validate()?;
                if let PrGraphicObject::Text(text) = &native {
                    text.document.validate_font()?;
                }
                ensure!(
                    !keyed || matches!(native, PrGraphicObject::Text(_)),
                    "keyed graphic shapes are unsupported"
                );
                Ok((native, approximations))
            });
        match native {
            Ok((native, reports)) => {
                approximations.extend(
                    reports
                        .into_iter()
                        .map(|reason| (object_record.clone(), reason)),
                );
                natives.push(native);
                kept.push(object);
                if source {
                    matte_reports.insert(object.id(), held);
                }
            }
            Err(error) => {
                held.forward(omissions);
                if group.is_some() {
                    unexported.insert(object.id(), Unexported::Unconverted(error.to_string()));
                } else {
                    return failed(
                        omissions,
                        if several {
                            format!("{object_record}: {error}")
                        } else {
                            error.to_string()
                        },
                    );
                }
            }
        }
    }
    if natives.is_empty() {
        return failed(omissions, "none of its objects can be exported".to_owned());
    }
    let range = group.map_or(objects[0].active_range(), |group| {
        group.playback.input_range()
    });
    let enabled = objects.iter().all(|object| !object.is_hidden())
        && group.is_none_or(|group| !group.is_hidden);
    let exported = graphic_of(natives, &range, enabled, context).and_then(|mut graphic| {
        for (&object, native) in kept.iter().zip(&mut graphic.objects) {
            let mut tracks = context
                .property_tracks
                .remove(&object.id())
                .unwrap_or_default();
            let (PrGraphicObject::Text(text), ObjectLayer::Text(layer)) = (native, object) else {
                ensure!(tracks.is_empty(), "keyed graphic shapes are unsupported");
                continue;
            };
            if let Err(error) =
                prepare_source_text(text, layer, &mut tracks, context.dynamics, graphic.in_ticks)
            {
                omit(
                    omissions,
                    OmissionScope::Feature,
                    object.record(),
                    format!(
                        "Source Text animation was not exported; keeping base Source Text: {error}"
                    ),
                );
                if mask_sources.contains(&object.id()) {
                    // Static fallback changes animated coverage; do not expose
                    // consumers of this mask with the altered source.
                    unexported.insert(object.id(), Unexported::Unrendered(error.to_string()));
                }
            }
            // A static horizontal axis plus keyed vertical scale maps to
            // native Uniform=false. Uniform keys still drive both axes.
            let vertical = if !tracks.contains_key(&PropType::ScaleX) {
                tracks.remove(&PropType::ScaleY)
            } else {
                None
            };
            if vertical.is_some() {
                text.horizontal_scale = Some(layer.transform.scale[0]);
            }
            text.animations = object_keys(
                tracks,
                &mut text.transform.position,
                &TEXT_PARAMS,
                graphic.in_ticks,
                frame,
                omissions,
                &object.record(),
            );
            // Do not discard a static independent axis if paired keys failed
            // validation and object_keys retained the static transform.
            if text
                .animations
                .iter()
                .any(|animation| matches!(animation, PrPropertyAnimation::UniformScale(_)))
            {
                text.horizontal_scale = None;
            }
            if let Some(track) = vertical {
                match bounded_keys(
                    &TEXT_PARAMS,
                    PrAnimatedProperty::UniformScale,
                    track,
                    graphic.in_ticks,
                    omissions,
                    &object.record(),
                )
                .and_then(|keys| readable(PrPropertyAnimation::UniformScale(keys)))
                {
                    Ok(animation) => text.animations.push(animation),
                    Err(error) => omit(
                        omissions,
                        OmissionScope::Feature,
                        object.record(),
                        format!("Vertical Scale animation was not exported: {error}"),
                    ),
                }
            }
        }
        if let Some(group) = group {
            apply_group(&mut graphic, group, context, omissions);
        }
        graphic.validate(context.frame_rate)?;
        Ok(graphic)
    });
    match exported {
        Ok(mut graphic) => {
            for (record, reason) in approximations {
                approximate(omissions, &record, reason);
            }
            unexported.extend(
                export_effects(
                    &kept,
                    &mut graphic,
                    context.dynamics,
                    matte_reports,
                    context.motion_blur,
                    omissions,
                )
                .into_iter()
                .map(|(id, reason)| (id, Unexported::Unrendered(reason))),
            );
            for (object, native) in kept.iter().zip(&graphic.objects) {
                let shape = match native {
                    PrGraphicObject::Shape(shape) => Some(shape),
                    PrGraphicObject::Group(pieces) => {
                        pieces.objects.iter().find_map(|piece| match piece {
                            PrGraphicObject::Shape(shape) if shape.appearance.fill.is_some() => {
                                Some(shape)
                            }
                            _ => None,
                        })
                    }
                    _ => None,
                };
                if let Some(shape) = shape {
                    let shadowed = shape.appearance.shadow.is_some();
                    for warning in shape.gradient_approximations(&graphic, shadowed) {
                        approximate(omissions, object.record(), warning);
                    }
                }
            }
            // The caller places the graphic, which writes these keys.
            for (object, native) in kept.iter().zip(&graphic.objects) {
                if group.is_none() {
                    if let PrGraphicObject::Text(text) = native {
                        context
                            .written
                            .record_animations(object.id(), &text.animations);
                    }
                }
            }
            if let Some(group) = group {
                if let Some(warning) = PrBlendMode::export_approximation(group.blend_mode) {
                    let group_record = format!("layer {} ({:?})", group.id, group.name);
                    approximate(omissions, group_record, warning);
                }
            }
            graphic.objects = in_paint_order(graphic.objects);
            Some(ExportedObjects {
                graphic,
                unexported,
            })
        }
        Err(error) => failed(omissions, error.to_string()),
    }
}

/// Give `graphic` its group's transform as Vector Motion and the group's
/// opacity and keys as the clip Opacity. A static group composes into a
/// graphic's one object while that stays inside its native ranges, and a
/// graphic with several objects keeps it unless it is the identity.
fn apply_group(
    graphic: &mut PrGraphic,
    group: &GroupLayer,
    context: &mut LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
) {
    let frame = [context.width, context.height];
    let group_record = format!("layer {} ({:?})", group.id, group.name);
    let mut tracks = context
        .property_tracks
        .remove(&group.id)
        .unwrap_or_default();
    let opacity_track = tracks.remove(&PropType::Opacity);
    // The background is classified on the text's own transform, before the
    // group's motion folds into it.
    if let [PrGraphicObject::Text(text)] = graphic.objects.as_mut_slice() {
        match group_background(group, text) {
            Ok(background) => text.document.background = background,
            Err(error) => omit(
                omissions,
                OmissionScope::Feature,
                &group_record,
                format!("group background was not exported: {error}"),
            ),
        }
    }
    let motion = vector_motion(
        group,
        tracks,
        graphic.in_ticks,
        frame,
        omissions,
        &group_record,
    );
    let shape_masked = graphic_objects(group, context.dynamics).is_some_and(|objects| {
        objects
            .iter()
            .any(|object| matches!(object, ObjectLayer::Shape(shape) if !shape.masks.is_empty()))
    });
    let absorbed = !shape_masked
        && motion.animations.is_empty()
        && match graphic.objects.as_mut_slice() {
            [object] => object.compose_static_vector_motion_in_range(&motion, frame),
            _ => {
                motion.scale == 100.0 && motion.rotation == 0.0 && motion.position == motion.anchor
            }
        };
    if !absorbed {
        graphic.vector_motion = Some(motion);
    }
    graphic.opacity = group.transform.opacity.value();
    graphic.blend_mode = PrBlendMode::from_fx_mode(group.blend_mode);
    if let Some(track) = opacity_track {
        match clip_opacity_keys(track, graphic.in_ticks, omissions, &group_record) {
            Ok(animation) => graphic.animations = vec![animation],
            Err(error) => omit(
                omissions,
                OmissionScope::Feature,
                &group_record,
                format!("Opacity animation was not exported: {error}"),
            ),
        }
    }
}

/// Export the effect stack of each object layer, in the graphic's object
/// order: a text or shape shadow, with every other effect reported. FX draws
/// a Mask with Shape or Text into its matte with its effects and its layer
/// fields, so a source of `matte_reports`, which hold the reports of its
/// layer fields, whose effects do not all export or that loses a field that
/// FX draws into its matte under the composition's `motion_blur`
/// ([`drawn_into_matte`]) would mask differently: its reports become why it
/// is left out, returned by its layer, and a mask that keeps what FX draws
/// reports them as any object does.
fn export_effects(
    layers: &[ObjectLayer<'_>],
    graphic: &mut PrGraphic,
    dynamics: &AnimationGraph,
    mut matte_reports: BTreeMap<LayerId, MatteReports>,
    motion_blur: MotionBlurSettings,
    omissions: &mut dyn OmissionSink,
) -> BTreeMap<LayerId, String> {
    let motion = graphic.vector_motion.as_ref();
    let mut unrendered = BTreeMap::new();
    for (layer, object) in layers.iter().zip(&mut graphic.objects) {
        let record = layer.record();
        let effects = (layer.effects(), layer.id());
        let mut reports = matte_reports.remove(&layer.id());
        // The layer-field reports come first, then the effects'.
        let fields = reports.as_ref().map_or(0, |reports| reports.0.len());
        let sink: &mut dyn OmissionSink = match &mut reports {
            Some(reports) => reports,
            None => &mut *omissions,
        };
        let shadowed = match object {
            PrGraphicObject::Text(text) => {
                // Source Text keys share the document's shadow (`PrText::validate`).
                let shadow = text_shadow::export_text_effects(
                    effects, dynamics, text, motion, sink, &record,
                );
                text.document.shadow = shadow;
                for key in &mut text.source_text_keys {
                    key.document.shadow = shadow;
                }
                shadow.is_some()
            }
            PrGraphicObject::Shape(shape) => {
                shape.appearance.shadow = text_shadow::export_shape_effects(
                    effects, dynamics, shape, motion, sink, &record,
                );
                shape.appearance.shadow.is_some()
            }
            PrGraphicObject::TextLines(_) => false,
            PrGraphicObject::Group(group) => {
                // shape_object gives these direct pieces the same owner transform.
                // Map once so unsupported effects are reported once per owner;
                // synthetic hole masks must retain their unshadowed coverage.
                let shadow = group
                    .objects
                    .iter()
                    .find_map(|object| match object {
                        PrGraphicObject::Shape(shape) if shape.appearance.mask_source.is_none() => {
                            Some(shape)
                        }
                        _ => None,
                    })
                    .and_then(|shape| {
                        text_shadow::export_shape_effects(
                            effects, dynamics, shape, motion, sink, &record,
                        )
                    });
                for object in &mut group.objects {
                    if let PrGraphicObject::Shape(shape) = object {
                        if shape.appearance.mask_source.is_none() {
                            shape.appearance.shadow = shadow;
                        }
                    }
                }
                if shadow.is_some() {
                    approximate(sink, &record,
                        "compound shape shadow is approximated by editable shadows on visible pieces; hole-edge clipping and overlapping piece shadows can differ from the whole-shape shadow; synthetic hole masks remain unshadowed");
                }
                shadow.is_some()
            }
        };
        let Some(reports) = reports else {
            continue;
        };
        let (field_reports, effect_reports) = reports.0.split_at(fields);
        let mut lost = Vec::new();
        let lost_fields: Vec<String> = field_reports
            .iter()
            .filter_map(|(_, field)| *field)
            .filter(|&(_, field)| drawn_into_matte(field, motion_blur))
            .map(|(_, field)| field.to_string())
            .collect();
        if let Some((last, first)) = lost_fields.split_last() {
            let (fields, verb) = match first {
                [] => (last.clone(), "it does"),
                first => (format!("{} and {last}", first.join(", ")), "they do"),
            };
            lost.push(format!(
                "FX draws its {fields} into its matte, and {verb} not export"
            ));
        }
        let drawn = layer
            .effects()
            .iter()
            .filter(|effect| draws(effect))
            .count();
        if drawn > usize::from(shadowed) {
            let reasons: Vec<_> = effect_reports
                .iter()
                .map(|(report, _)| report.reason.as_str())
                .collect();
            lost.push(format!(
                "FX draws its effects into its matte, and they do not all export: {}",
                reasons.join("; ")
            ));
        }
        if lost.is_empty() {
            reports.forward(omissions);
        } else {
            unrendered.insert(layer.id(), lost.join("; "));
            reports.forward(omissions);
        }
    }
    unrendered
}

/// The reports of a Mask with Shape or Text, held until [`export_effects`]
/// knows whether FX draws what they lose into its matte, each with the layer
/// field that it reports, if it reports one.
#[derive(Default)]
struct MatteReports(Vec<(Omission, Option<(LayerId, ExportField)>)>);

impl MatteReports {
    /// Report each held report to `omissions` as it came, a field loss as
    /// the typed loss that it is.
    fn forward(self, omissions: &mut dyn OmissionSink) {
        for (report, field) in self.0 {
            match field {
                Some((layer, field)) => omissions.emit_field(report, layer, field),
                None => omissions.emit(report),
            }
        }
    }
}

impl OmissionSink for MatteReports {
    fn emit(&mut self, omission: Omission) {
        self.0.push((omission, None));
    }

    fn emit_field(&mut self, omission: Omission, layer: LayerId, field: ExportField) {
        self.0.push((omission, Some((layer, field))));
    }
}

/// Whether FX draws the layer `field` that a Mask with Shape or Text loses
/// into its matte, where the composition blurs by `motion_blur`: a field of
/// its picture or of its place and time does, but not its blend mode, since
/// FX draws a matte from its source alone, over nothing, where every blend
/// is Normal (the renderer's mask pass), and not a motion blur that the
/// composition does not draw. Whether the source moves is not traced, so the
/// motion blur of a still one leaves its composite out too.
fn drawn_into_matte(field: ExportField, motion_blur: MotionBlurSettings) -> bool {
    match field {
        ExportField::BlendMode => false,
        ExportField::MotionBlur => motion_blur.enabled && motion_blur.shutter_angle.value() > 0.0,
        field => matches!(
            field.domain(),
            ExportLossDomain::Picture | ExportLossDomain::SharedContext
        ),
    }
}

/// Whether FX draws `effect`: its record is enabled, and a drop shadow is
/// itself. Nothing else of an effect's own state is read here.
fn draws(effect: &EffectRecord) -> bool {
    let (enabled, payload) = match effect.data() {
        EffectData::Identified {
            enabled, effect, ..
        } => (*enabled, effect),
        EffectData::Legacy(effect) => (true, effect),
    };
    enabled
        && !matches!(payload, EffectPayload::Known(LayerEffect::DropShadow(shadow)) if !shadow.enabled)
}

/// The Vector Motion of a graphic group: its transform and the keys among
/// `tracks`, which hold every group track except opacity.
fn vector_motion(
    group: &GroupLayer,
    tracks: BTreeMap<PropType, &PropertyKeyframeTrack>,
    in_ticks: i64,
    frame: [u32; 2],
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> PrVectorMotion {
    let transform = &group.transform;
    let mut motion = PrVectorMotion {
        position: [transform.position.x(), transform.position.y()],
        anchor: transform.anchor_point,
        scale: transform.scale[0],
        rotation: transform.rotation,
        animations: Vec::new(),
    };
    motion.animations = object_keys(
        tracks,
        &mut motion.position,
        &VECTOR_MOTION_PARAMS,
        in_ticks,
        frame,
        omissions,
        record,
    );
    motion
}

/// Clip Opacity keys of a graphic group's opacity track, by the Motion export
/// rules, with values inside Premiere's Opacity range. Its Bezier speeds were
/// measured in value per second, so Bezier easing exports.
fn clip_opacity_keys(
    track: &PropertyKeyframeTrack,
    in_ticks: i64,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Result<PrPropertyAnimation> {
    let keys = export_scalar_keys(track, in_ticks, "Opacity", omissions, record)?;
    ensure!(
        keys.iter().all(|key| (0.0..=100.0).contains(&key.value)),
        "Opacity keys must stay within Premiere's range 0..100"
    );
    readable(PrPropertyAnimation::Opacity(keys))
}

/// `animation` when the native key readers accept it, so that an exported
/// graphic reads back; otherwise the reason, which omits only its property.
fn readable(animation: PrPropertyAnimation) -> Result<PrPropertyAnimation> {
    animation.validate_keys()?;
    Ok(animation)
}

/// Native keys of one exported graphic object from its FX tracks, on the
/// generator clock that starts at `in_ticks`. Each property follows the
/// Motion export rules, with Bezier easing only where its parameter in
/// `specs` has verified Bezier speeds, and its values must fit that
/// parameter; otherwise it is reported and keeps its static value. The first
/// Position key replaces the static `position`, in sequence pixels, as on
/// video layers.
fn object_keys(
    mut tracks: BTreeMap<PropType, &PropertyKeyframeTrack>,
    position: &mut [f64; 2],
    specs: &[GraphicParamSpec],
    in_ticks: i64,
    frame: [u32; 2],
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Vec<PrPropertyAnimation> {
    let mut animations = Vec::new();
    let report = |omissions: &mut dyn OmissionSink, property, reason: String| {
        omit(
            omissions,
            OmissionScope::Feature,
            record,
            format!(
                "{} animation was not exported: {reason}",
                property_name(property)
            ),
        );
    };
    type Keyed = fn(Vec<PrScalarKeyframe>) -> PrPropertyAnimation;
    for (property_type, property, keyed) in [
        (
            PropType::Opacity,
            PrAnimatedProperty::Opacity,
            PrPropertyAnimation::Opacity as Keyed,
        ),
        (
            PropType::Rotation,
            PrAnimatedProperty::Rotation,
            PrPropertyAnimation::Rotation,
        ),
    ] {
        let Some(track) = tracks.remove(&property_type) else {
            continue;
        };
        if unverified_bezier(specs, property, track) {
            report(omissions, property, BEZIER_KEYS_UNVERIFIED.to_owned());
            continue;
        }
        match bounded_keys(specs, property, track, in_ticks, omissions, record)
            .and_then(|keys| readable(keyed(keys)))
        {
            Ok(animation) => animations.push(animation),
            Err(error) => report(omissions, property, error.to_string()),
        }
    }
    match (
        tracks.remove(&PropType::PositionX),
        tracks.remove(&PropType::PositionY),
    ) {
        (Some(x), Some(y))
            if unverified_bezier(specs, PrAnimatedProperty::Position, x)
                || unverified_bezier(specs, PrAnimatedProperty::Position, y) =>
        {
            report(
                omissions,
                PrAnimatedProperty::Position,
                BEZIER_KEYS_UNVERIFIED.to_owned(),
            )
        }
        (Some(x), Some(y)) => {
            let size = frame.map(f64::from);
            match export_position_keys(x, y, in_ticks, frame)
                .and_then(|keys| readable(PrPropertyAnimation::Position(keys)))
            {
                Ok(animation) => {
                    if let Some(first) = animation.point_keys().and_then(<[_]>::first) {
                        *position = [first.value[0] * size[0], first.value[1] * size[1]];
                    }
                    animations.push(animation);
                }
                Err(error) => report(omissions, PrAnimatedProperty::Position, error.to_string()),
            }
        }
        (None, None) => {}
        _ => omit(
            omissions,
            OmissionScope::Feature,
            record,
            "unpaired Position keyframes were not exported",
        ),
    }
    match (
        tracks.remove(&PropType::ScaleX),
        tracks.remove(&PropType::ScaleY),
    ) {
        (Some(x), Some(y))
            if scale_tracks_match(x, y)
                && unverified_bezier(specs, PrAnimatedProperty::UniformScale, x) =>
        {
            report(
                omissions,
                PrAnimatedProperty::UniformScale,
                BEZIER_KEYS_UNVERIFIED.to_owned(),
            )
        }
        (Some(x), Some(y)) if scale_tracks_match(x, y) => {
            let property = PrAnimatedProperty::UniformScale;
            match bounded_keys(specs, property, x, in_ticks, omissions, record)
                .and_then(|keys| readable(PrPropertyAnimation::UniformScale(keys)))
            {
                Ok(animation) => animations.push(animation),
                Err(error) => report(omissions, property, error.to_string()),
            }
        }
        (None, None) => {}
        _ => omit(
            omissions,
            OmissionScope::Feature,
            record,
            "nonuniform or unpaired Scale keyframes were not exported",
        ),
    }
    animations
}

/// Prepare a complete optional Source Text/alignment unit before replacing
/// the independently valid native base. Rejected tracks never reach Motion
/// export, and no partially reconstructed document is committed.
fn prepare_source_text(
    text: &mut PrText,
    layer: &TextLayer,
    tracks: &mut BTreeMap<PropType, &PropertyKeyframeTrack>,
    dynamics: &AnimationGraph,
    in_ticks: i64,
) -> Result<()> {
    let mut source_text: BTreeMap<_, _> = SOURCE_TEXT_PROPERTIES
        .iter()
        .filter_map(|&(field, property, _)| tracks.remove(&property).map(|track| (field, track)))
        .collect();
    let width_track = stroke_width_animator(layer)?
        .map(|animator| stroke_width_track(animator, dynamics))
        .transpose()?
        .flatten();
    if let Some(track) = width_track {
        source_text.insert(SourceTextField::StrokeWidth, track);
    }
    if source_text.is_empty() {
        return Ok(());
    }
    let anchor = tracks.remove(&PropType::AnchorPointY);
    let base = stroke_width_source(layer, dynamics)?;
    // This is only the prospective native text object, not an FX layer
    // clone/retry: the already validated base remains untouched on failure.
    let mut prepared = PrText {
        source_text_keys: source_text_keys(&source_text, &base, &text.document.font, in_ticks)?,
        ..text.clone()
    };
    if let Some(anchor) = anchor {
        restore_point_alignment(&mut prepared, anchor, in_ticks)?;
    }
    prepared.document = prepared.source_text_keys[0].document.clone();
    prepared.validate()?;
    *text = prepared;
    Ok(())
}

/// Native Source Text keys of a text layer's Source Text tracks: one
/// complete document at every key time of every track, each field read from
/// its track by Hold, on the generator clock that starts at `in_ticks`. The
/// first key's document is the text shown before it. Premiere holds Source
/// Text between keys, so a key with another easing after the first, a value
/// of another kind, or a document the encoding cannot hold is an error,
/// which the exporter diagnoses while retaining the independent base text.
fn source_text_keys(
    tracks: &BTreeMap<SourceTextField, &PropertyKeyframeTrack>,
    source_text: &TextDocument,
    font: &str,
    in_ticks: i64,
) -> Result<Vec<PrSourceTextKey>> {
    let mut times = BTreeSet::new();
    for (field, track) in tracks {
        ensure!(
            track.keyframes()[1..]
                .iter()
                .all(|key| key.easing() == PropertyKeyframeEasing::Hold),
            "Source Text {field} keys must hold: Premiere holds Source Text between keys"
        );
        times.extend(
            track
                .keyframes()
                .iter()
                .map(|key| key.layer_time().as_millis()),
        );
    }
    // The value of `track` at `millis` by Hold: its last key at or before
    // that time, or its first key before all of them.
    let hold_value = |track: &PropertyKeyframeTrack, millis: i64| {
        let keys = track.keyframes();
        keys.iter()
            .rev()
            .find(|key| key.layer_time().as_millis() <= millis)
            .unwrap_or(&keys[0])
            .value()
            .clone()
    };
    let mut keys = Vec::with_capacity(times.len());
    for millis in times {
        let mut doc = source_text.clone();
        for (&field, track) in tracks {
            let value = hold_value(track, millis);
            let positive = |value: f64| {
                PositiveProperty::new(value).ok_or_else(|| {
                    unsupported(format!("Source Text {field} keys must be positive"))
                })
            };
            match (field, value) {
                (SourceTextField::Text, PropertyValue::String(text)) => doc.text = text,
                (SourceTextField::Size, PropertyValue::Float(size)) => {
                    doc.font_size = positive(size)?;
                }
                (SourceTextField::FillEnabled, PropertyValue::Bool(enabled)) => {
                    doc.apply_fill = enabled;
                }
                (SourceTextField::FillColor, PropertyValue::Color(color)) => {
                    doc.fill_color = color;
                }
                (SourceTextField::Tracking, PropertyValue::Float(tracking)) => {
                    doc.tracking = tracking;
                }
                (SourceTextField::Leading, PropertyValue::Float(leading)) => {
                    doc.leading = Some(positive(leading)?);
                }
                (SourceTextField::StrokeEnabled, PropertyValue::Bool(enabled)) => {
                    doc.apply_stroke = enabled;
                }
                (SourceTextField::StrokeWidth, PropertyValue::Float(delta)) => {
                    doc.stroke_width = total_stroke_width(source_text.stroke_width.value(), delta)?;
                }
                (SourceTextField::AllCaps, PropertyValue::Bool(all_caps)) => {
                    doc.all_caps = all_caps;
                }
                // The FX document validates each track's value kind against
                // its property, so this names a document the graph rejected.
                (field, value) => {
                    return Err(unsupported(format!(
                        "Source Text {field} keys cannot hold {:?} values",
                        value.kind()
                    )));
                }
            }
        }
        let (document, _) = text_document(&doc, font.to_owned())?;
        keys.push(PrSourceTextKey {
            source_ticks: keyframes::source_ticks(in_ticks, millis)?,
            document,
        });
    }
    Ok(keys)
}

/// Recover the native alignment from coherent held Source Text / local anchor
/// keys. An arbitrary keyed pivot cannot be represented by Premiere Text and
/// must not be silently discarded while exporting animated text.
fn restore_point_alignment(
    text: &mut PrText,
    anchor: &PropertyKeyframeTrack,
    in_ticks: i64,
) -> Result<()> {
    ensure!(
        matches!(text.document.frame, PrTextFrame::Point { .. })
            && anchor
                .keyframes()
                .iter()
                .skip(1)
                .all(|key| key.easing() == PropertyKeyframeEasing::Hold),
        "keyed text anchor does not match held point-text alignment"
    );
    let held_anchor = |millis: i64| -> Result<f64> {
        let keys = anchor.keyframes();
        let key = keys
            .iter()
            .rev()
            .find(|key| key.layer_time().as_millis() <= millis)
            .unwrap_or(&keys[0]);
        match key.value() {
            PropertyValue::Float(value) => Ok(*value),
            _ => Err(unsupported("point-text anchor keys must be scalar")),
        }
    };
    let key_times = text
        .source_text_keys
        .iter()
        .map(|key| keyframes::layer_millis(key.source_ticks, in_ticks))
        .collect::<Result<Vec<_>>>()?;
    let times: BTreeSet<i64> = key_times
        .iter()
        .copied()
        .chain(
            anchor
                .keyframes()
                .iter()
                .map(|key| key.layer_time().as_millis()),
        )
        .collect();
    for vertical in [PrVerticalAlign::Center, PrVerticalAlign::Bottom] {
        let mut documents: Vec<_> = text
            .source_text_keys
            .iter()
            .map(|key| key.document.clone())
            .collect();
        for document in &mut documents {
            document.frame = PrTextFrame::Point { vertical };
        }
        let base = held_anchor(key_times[0])? - super::text::point_anchor_offset(&documents[0]);
        let mut coherent = true;
        for &millis in &times {
            // The Source Text key held at `millis`, or the first before them all.
            let selected = key_times
                .iter()
                .rposition(|time| *time <= millis)
                .unwrap_or(0);
            let expected = base + super::text::point_anchor_offset(&documents[selected]);
            let actual = held_anchor(millis)?;
            // Allow only arithmetic roundoff in subtracting/readding the base;
            // this is not a geometric or native-render tolerance.
            coherent &= (actual - expected).abs()
                <= 8.0 * f64::EPSILON * actual.abs().max(expected.abs()).max(1.0);
        }
        if coherent {
            text.transform.anchor[1] = base;
            for (key, document) in text.source_text_keys.iter_mut().zip(documents) {
                key.document = document;
            }
            return Ok(());
        }
    }
    Err(unsupported(
        "keyed text anchor does not match held point-text alignment",
    ))
}

/// Scalar keys exported by the Motion rules whose values fit the parameter
/// of `specs` that keys `property`.
fn bounded_keys(
    specs: &[GraphicParamSpec],
    property: PrAnimatedProperty,
    track: &PropertyKeyframeTrack,
    in_ticks: i64,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Result<Vec<PrScalarKeyframe>> {
    let name = property_name(property);
    let spec = specs
        .iter()
        .find(|spec| spec.role.animation() == Some(property))
        .ok_or_else(|| unsupported(format!("the graphic has no {name} parameter")))?;
    let keys = export_scalar_keys(track, in_ticks, name, omissions, record)?;
    ensure!(
        keys.iter().all(|key| spec.holds(key.value)),
        "{name} keys must stay within Premiere's range {}..{}",
        spec.lower.unwrap_or("-inf"),
        spec.upper.unwrap_or("inf")
    );
    Ok(keys)
}

/// Why an FX path with several contours has no Premiere Shape: a Premiere
/// Path holds one contour, so a hole needs `Mask with Shape`.
const ONE_CONTOUR: &str = "a Premiere shape path holds one contour; holes need Mask with Shape";

/// Map one graphic Shape to an editable FX shape layer with the graphic's
/// range and visibility: its path in layer pixels, its fill ([`fx_fill`]),
/// which FX draws on an open path as if closed, as Premiere does, and its
/// centred stroke, which FX draws over the fill as Premiere does, with butt
/// caps and the join that [`stroke_join`] proves.
fn shape_layer(
    graphic: &PrGraphic,
    shape: &PrShape,
    layer_id: LayerId,
    index: usize,
) -> Result<ShapeLayer> {
    let appearance = &shape.appearance;
    let stroke = appearance
        .stroke
        .map(|stroke| {
            let width = NonNegativeProperty::new(f64::from(stroke.width))
                .ok_or_else(|| unsupported("shape stroke width must be nonnegative"))?;
            let mut style = ShapeStrokeStyle::solid(rgba(stroke.color), width);
            match stroke_join(&shape.path, None).map_err(unsupported)? {
                StrokeJoin::Miter(limit) => style.miter_limit = limit,
                StrokeJoin::Bevel => style.join = ShapeLineJoin::Bevel,
                StrokeJoin::Round => style.join = ShapeLineJoin::Round,
            }
            Ok::<_, crate::error::BuildError>(style)
        })
        .transpose()?;
    let mut transform = object_transform(&shape.transform, "shape")?;
    if let Some(horizontal) = shape.horizontal_scale {
        transform.scale[0] = horizontal;
    }
    Ok(ShapeLayer {
        id: layer_id,
        name: if shape.name.is_empty() {
            format!("Premiere shape {}", index + 1)
        } else {
            shape.name.clone()
        },
        description: String::new(),
        is_hidden: !graphic.enabled,
        parent: None,
        blend_mode: BlendMode::Normal,
        track_matte: None,
        masks: Vec::new(),
        active_range: tick_range(graphic.start_ticks, graphic.end_ticks)?,
        effects: Vec::new(),
        motion_blur: false,
        transform,
        shape: ShapeContent {
            path: fx_path(&shape.path),
            fills: appearance.fill.as_ref().map(fx_fill).into_iter().collect(),
            strokes: stroke.into_iter().collect(),
            round_corners: None,
            offset_paths: None,
            trim: None,
            poly_star: None,
            ellipse: None,
        },
    })
}

/// The FX fill of a Premiere fill, blending normally at full opacity with the
/// nonzero rule. A gradient keeps its kind, its stops ([`fx_gradient_stops`])
/// and its layer-pixel start and end on the x axis: FX centres a radial
/// gradient on start with radius |end − start|, and both interpolate the
/// color stops component-wise on encoded RGB, as Premiere does (G3, G4).
fn fx_fill(fill: &PrFill) -> ShapeFillStyle {
    let gradient = match fill {
        PrFill::Solid(color) => return ShapeFillStyle::solid(rgba(*color)),
        PrFill::Gradient(gradient) => gradient,
    };
    let point = |x: f32| [f64::from(x), 0.0];
    ShapeFillStyle {
        paint: ShapePaint::Gradient {
            gradient_type: match gradient.kind {
                PrGradientKind::Linear => ShapeGradientType::Linear,
                PrGradientKind::Radial => ShapeGradientType::Radial,
            },
            start: point(gradient.start_x),
            end: point(gradient.end_x),
            stops: fx_gradient_stops(gradient),
        },
        fill_rule: ShapeFillRule::NonZeroWinding,
        blend_mode: BlendMode::Normal,
        opacity: 1.0,
    }
}

/// The FX stops of a Premiere gradient, whose color and opacity stops
/// Premiere interpolates apart, each linearly (midpoints 50 %) and held
/// beyond its ends (G2, G5): under a constant opacity, one FX stop per color
/// stop with that alpha; otherwise one FX stop at each position of a color
/// or an opacity stop, with the color and opacity there, and a second one
/// where either list steps. FX interpolates the stops' straight RGBA alike,
/// so they draw the same colors and opacities; only the composite differs
/// ([`GRADIENT_OPACITY_APPROXIMATION`]).
///
/// [`GRADIENT_OPACITY_APPROXIMATION`]: crate::schema::text::GRADIENT_OPACITY_APPROXIMATION
fn fx_gradient_stops(gradient: &PrGradient) -> Vec<ShapeGradientStop> {
    let colors: Vec<_> = gradient
        .stops
        .iter()
        .map(|stop| (stop.position, rgba(stop.color)))
        .collect();
    let opacities: Vec<_> = gradient
        .opacity_stops
        .iter()
        .map(|stop| (stop.position, [f64::from(stop.opacity)]))
        .collect();
    if let Some(&(_, [opacity])) = opacities.first() {
        if opacities.iter().all(|&(_, [value])| value == opacity) {
            return colors
                .iter()
                .map(|&(position, [red, green, blue, _])| ShapeGradientStop {
                    offset: f64::from(position),
                    color: [red, green, blue, opacity],
                })
                .collect();
        }
    }
    let mut positions: Vec<f32> = colors.iter().map(|&(position, _)| position).collect();
    positions.extend(opacities.iter().map(|&(position, _)| position));
    positions.sort_by(f32::total_cmp);
    positions.dedup();
    let mut stops: Vec<ShapeGradientStop> = Vec::with_capacity(positions.len());
    for position in positions {
        for after in [false, true] {
            let (Some([red, green, blue, _]), Some([opacity])) = (
                stop_value(&colors, position, after),
                stop_value(&opacities, position, after),
            ) else {
                continue;
            };
            let stop = ShapeGradientStop {
                offset: f64::from(position),
                color: [red, green, blue, opacity],
            };
            if stops.last() != Some(&stop) {
                stops.push(stop);
            }
        }
    }
    stops
}

/// The value of `stops`, sorted by position, just before `position` or, with
/// `after`, at and just after it: linear between two stops, a stop's own
/// value at its position (so both sides of a stop that does not step are
/// equal), and held beyond the ends; `None` without stops.
fn stop_value<const N: usize>(
    stops: &[(f32, [f64; N])],
    position: f32,
    after: bool,
) -> Option<[f64; N]> {
    let next = stops.partition_point(|&(at, _)| at < position || (after && at == position));
    match (
        next.checked_sub(1).map(|index| stops[index]),
        stops.get(next),
    ) {
        (Some((from, low)), Some(&(to, high))) => {
            let t = (f64::from(position) - f64::from(from)) / (f64::from(to) - f64::from(from));
            Some(std::array::from_fn(|channel| {
                low[channel] * (1.0 - t) + high[channel] * t
            }))
        }
        (Some((_, value)), None) | (None, Some(&(_, value))) => Some(value),
        (None, None) => None,
    }
}

/// FX drawing commands of a Premiere path, in layer pixels: a `moveTo`, then
/// one segment per pair of vertices, from the out tangent through the next
/// in tangent, a `lineTo` when both tangents rest on their vertices. A
/// closed path ends with `close`, which draws a straight closing segment
/// itself; a curved one ends on the first vertex, where FX welds it. A
/// smooth vertex couples its handles as FX `straight` does, an editing mode:
/// FX draws the tangents alone.
pub(super) fn fx_path(path: &PrShapePath) -> ShapePath {
    let widen = |point: [f32; 2]| (f64::from(point[0]), f64::from(point[1]));
    let mirror = |vertex: &PrPathVertex| vertex.smooth.then_some(ShapeHandleMirror::Straight);
    let segment = |from: &PrPathVertex, to: &PrPathVertex, mirror| {
        let (x, y) = widen(to.point);
        if from.out_tangent == from.point && to.in_tangent == to.point {
            ShapePathCommand::LineTo {
                x,
                y,
                mirror,
                corner_radius: None,
            }
        } else {
            let ((c1x, c1y), (c2x, c2y)) = (widen(from.out_tangent), widen(to.in_tangent));
            ShapePathCommand::CubicTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
                mirror,
                corner_radius: None,
            }
        }
    };
    let (Some(first), Some(last)) = (path.vertices.first(), path.vertices.last()) else {
        return ShapePath {
            commands: Vec::new(),
        };
    };
    let (x, y) = widen(first.point);
    let mut commands = vec![ShapePathCommand::MoveTo {
        x,
        y,
        mirror: mirror(first),
        corner_radius: None,
    }];
    commands.extend(
        path.vertices
            .windows(2)
            .map(|pair| segment(&pair[0], &pair[1], mirror(&pair[1]))),
    );
    if path.closed {
        // The first vertex's mode stays on its `moveTo`.
        let closing = segment(last, first, None);
        if matches!(closing, ShapePathCommand::CubicTo { .. }) {
            commands.push(closing);
        }
        commands.push(ShapePathCommand::Close);
    }
    ShapePath { commands }
}

/// `path` with every coordinate scaled by `[x, y]`: between a mask outline in
/// unit fractions of a frame and the same outline in that frame's pixels. The
/// products are exact in f64 for f32 coordinates and integer frame sizes, so a
/// scale and its inverse return the native coordinates.
pub(super) fn scaled_path(path: &ShapePath, [x, y]: [f64; 2]) -> ShapePath {
    let commands = path
        .commands
        .iter()
        .map(|command| match *command {
            ShapePathCommand::MoveTo {
                x: px,
                y: py,
                mirror,
                corner_radius,
            } => ShapePathCommand::MoveTo {
                x: px * x,
                y: py * y,
                mirror,
                corner_radius,
            },
            ShapePathCommand::LineTo {
                x: px,
                y: py,
                mirror,
                corner_radius,
            } => ShapePathCommand::LineTo {
                x: px * x,
                y: py * y,
                mirror,
                corner_radius,
            },
            ShapePathCommand::CubicTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x: px,
                y: py,
                mirror,
                corner_radius,
            } => ShapePathCommand::CubicTo {
                c1x: c1x * x,
                c1y: c1y * y,
                c2x: c2x * x,
                c2y: c2y * y,
                x: px * x,
                y: py * y,
                mirror,
                corner_radius,
            },
            ShapePathCommand::Close => ShapePathCommand::Close,
        })
        .collect();
    ShapePath { commands }
}

/// The Premiere path of FX drawing commands, as [`fx_path`] writes them: one
/// contour from its `moveTo`, closed by `close`. A closing segment that ends
/// on the first vertex is FX's weld, whose in tangent belongs to the first
/// vertex. A vertex is smooth where a tangent leaves its point or its anchor
/// is `straight` or `symmetrical`, and otherwise a corner: Premiere 26.5.1
/// and AME build 85 draw a smooth vertex's tangents and ignore a corner's,
/// whose tangents Premiere's own saves hold on its point, as measured on
/// native smooth vertices and the rounded bar; a smooth cusp and a smooth vertex
/// with one handle beside an angled edge are inferred. A Premiere Path vertex
/// has no corner radius, and FX rounds an anchor with one, so a rounded
/// anchor has no Premiere path. Coordinates narrow to f32.
pub(super) fn premiere_path(path: &ShapePath) -> Result<PrShapePath> {
    ensure!(
        path.commands
            .iter()
            .all(|command| command.corner_radius().is_none_or(|radius| radius == 0.0)),
        "rounded shape path corners are unsupported"
    );
    let narrow = |x: f64, y: f64| [x as f32, y as f32];
    let smooth = |mirror: Option<ShapeHandleMirror>| {
        matches!(
            mirror,
            Some(ShapeHandleMirror::Straight | ShapeHandleMirror::Symmetrical)
        )
    };
    let Some((ShapePathCommand::MoveTo { x, y, mirror, .. }, rest)) = path.commands.split_first()
    else {
        return Err(unsupported("a shape path must start with a move"));
    };
    let (drawn, closed) = match rest.split_last() {
        Some((ShapePathCommand::Close, drawn)) => (drawn, true),
        _ => (rest, false),
    };
    let start = narrow(*x, *y);
    let mut vertices = vec![PrPathVertex {
        smooth: smooth(*mirror),
        point: start,
        in_tangent: start,
        out_tangent: start,
    }];
    for command in drawn {
        let (end, handles, mirror) = match *command {
            ShapePathCommand::LineTo { x, y, mirror, .. } => (narrow(x, y), None, mirror),
            ShapePathCommand::CubicTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
                mirror,
                ..
            } => (
                narrow(x, y),
                Some((narrow(c1x, c1y), narrow(c2x, c2y))),
                mirror,
            ),
            ShapePathCommand::MoveTo { .. } | ShapePathCommand::Close => {
                return Err(unsupported(ONE_CONTOUR))
            }
        };
        let mut in_tangent = end;
        if let (Some((out_tangent, into)), Some(previous)) = (handles, vertices.last_mut()) {
            previous.out_tangent = out_tangent;
            in_tangent = into;
        }
        vertices.push(PrPathVertex {
            smooth: smooth(mirror),
            point: end,
            in_tangent,
            out_tangent: end,
        });
    }
    if closed && vertices.len() > 1 && vertices.last().map(|vertex| vertex.point) == Some(start) {
        if let Some(weld) = vertices.pop() {
            vertices[0].in_tangent = weld.in_tangent;
        }
    }
    for vertex in &mut vertices {
        vertex.smooth |= vertex.in_tangent != vertex.point || vertex.out_tangent != vertex.point;
    }
    Ok(PrShapePath { vertices, closed })
}

/// The Premiere object of an FX shape layer, or why it has none: a Shape
/// of one path of one contour with sharp anchors, at most one fill
/// ([`premiere_fill`]) and one stroke in the forms that Premiere draws
/// alike, no skew or 3D rotation, normal blending, and no primitive or path
/// modifier. Its shadow is exported with its effects.
///
/// A path of several contours, which no Premiere Path holds, is a SubGroup
/// of pieces, deepest first, whose contours [`contours::contours`] certifies
/// simple, apart and nested as the fill rule reads them: a filled region's
/// contour is a Shape with the fill and stroke, a hole an inverted Mask with
/// Shape of its closed outline, opaque at full opacity since its alpha is
/// its coverage, under a Shape of its stroke, and a contour between two
/// regions alike a Shape of its stroke alone. Every piece keeps the layer's
/// transform, so fills, gradients and strokes keep their layer coordinates.
/// The contours lie twice the stroke's reach apart (half its width, times
/// the miter limit at a miter join), in layer pixels, which every transform
/// keeps; [`ShapeObject::near`] names two that may come within
/// [`COMPOUND_EDGE_PIXELS`] of each other at the layer's scale times
/// `motion_scale`, the smallest that its Vector Motion and nests give it, or
/// says that any two may when that has no positive lower bound. That
/// construction has retained native hole-control evidence, not general parity.
fn shape_object(layer: &ShapeLayer, motion_scale: Option<f64>) -> Result<ShapeObject> {
    let content = &layer.shape;
    ensure!(
        content.round_corners.is_none()
            && content.offset_paths.is_none()
            && content.trim.is_none()
            && content.poly_star.is_none()
            && content.ellipse.is_none(),
        "shape primitives and path modifiers are unsupported"
    );
    let transform = &layer.transform;
    ensure!(
        transform.skew == 0.0
            && transform.skew_axis == 0.0
            && transform.rotation_x == 0.0
            && transform.rotation_y == 0.0
            && transform.orientation == [0.0; 3]
            && transform.position.z().is_none(),
        "a graphic shape has no skew or 3D rotation"
    );
    ensure!(
        content.path.is_finite(),
        "shape path coordinates must be finite"
    );
    let (fill, stroke) = match (content.fills.as_slice(), content.strokes.as_slice()) {
        ([_, _, ..], _) => return Err(unsupported("a Premiere shape has one fill")),
        (_, [_, _, ..]) => return Err(unsupported("a Premiere shape has one stroke")),
        (fills, strokes) => (fills.first(), strokes.first()),
    };
    // Premiere scales x by Horizontal Scale while Uniform Scale is off.
    let [horizontal, vertical] = transform.scale;
    let shape = |path: PrShapePath, fill: Option<PrFill>, stroke: Option<PrShapeStroke>| PrShape {
        name: layer.name.clone(),
        path,
        appearance: PrAppearance {
            fill,
            stroke,
            shadow: None,
            mask_source: None,
        },
        transform: PrTextTransform {
            position: [transform.position.x(), transform.position.y()],
            anchor: transform.anchor_point,
            scale: vertical,
            rotation: transform.rotation,
            opacity: transform.opacity.value(),
        },
        horizontal_scale: (horizontal != vertical).then_some(horizontal),
        mask: None,
    };
    let contour_count = content
        .path
        .commands
        .iter()
        .filter(|command| matches!(command, ShapePathCommand::MoveTo { .. }))
        .count();
    if contour_count <= 1 {
        let path = premiere_path(&content.path)?;
        let fill = fill.map(|fill| premiere_fill(fill, false)).transpose()?;
        let stroke = stroke
            .map(|stroke| shape_stroke(stroke, &path))
            .transpose()?;
        return Ok(ShapeObject {
            object: PrGraphicObject::Shape(shape(path, fill, stroke)),
            near: None,
        });
    }
    // A path of several contours: one piece per contour, in a SubGroup that
    // bounds the masks of its holes.
    let reach = stroke.map_or(0.0, |stroke| {
        let half = stroke.width.value() / 2.0;
        match stroke.join {
            ShapeLineJoin::Miter => half * stroke.miter_limit.max(1.0),
            ShapeLineJoin::Bevel | ShapeLineJoin::Round => half,
        }
    });
    // The smallest scale of the pieces on screen, if it has a positive lower
    // bound; without one, any two contours may come near each other.
    let scale = motion_scale
        .map(|motion| horizontal.abs().min(vertical.abs()) / 100.0 * motion)
        .filter(|scale| *scale > 0.0);
    let fill_rule = fill.map(|fill| fill.fill_rule);
    let fill = fill.map(|fill| premiere_fill(fill, true)).transpose()?;
    let contours::Contours { contours, near } = contours::contours(
        &content.path,
        fill_rule,
        2.0 * reach,
        scale.map_or(0.0, |scale| COMPOUND_EDGE_PIXELS / scale),
    )
    .map_err(unsupported)?;
    let near = match scale {
        Some(_) => near.map(NearContours::Pair),
        None => Some(NearContours::Unbounded),
    };
    let mut pieces = Vec::with_capacity(contours.len() + 1);
    for contour in contours {
        let path = premiere_path(&contour.path)?;
        let stroke = stroke
            .map(|stroke| shape_stroke(stroke, &path))
            .transpose()?;
        match contour.role {
            ContourRole::Filled => pieces.push(shape(path, fill.clone(), stroke)),
            ContourRole::Hole => {
                if stroke.is_some() {
                    pieces.push(shape(path.clone(), None, stroke));
                }
                // A mask's coverage is its rendered alpha, so the hole is an
                // opaque closed outline at full opacity.
                let mut hole = shape(
                    PrShapePath {
                        closed: true,
                        ..path
                    },
                    Some(PrFill::Solid(PrRgb([255; 3]))),
                    None,
                );
                hole.appearance.mask_source = Some(PrMaskSource { inverted: true });
                hole.transform.opacity = 100.0;
                pieces.push(hole);
            }
            ContourRole::Boundary => {
                if stroke.is_some() {
                    pieces.push(shape(path, None, stroke));
                }
            }
        }
    }
    Ok(ShapeObject {
        object: PrGraphicObject::Group(PrGraphicGroup {
            name: layer.name.clone(),
            objects: pieces.into_iter().map(PrGraphicObject::Shape).collect(),
        }),
        near,
    })
}

/// The Premiere object of a shape layer ([`shape_object`]).
struct ShapeObject {
    object: PrGraphicObject,
    /// For pieces, which contours may come within [`COMPOUND_EDGE_PIXELS`] of
    /// each other on screen.
    near: Option<NearContours>,
}

/// Contours of a shape's pieces that may come within [`COMPOUND_EDGE_PIXELS`]
/// of each other on screen.
enum NearContours {
    /// These two, one-based ([`contours::Contours::near`]).
    Pair([usize; 2]),
    /// Any two: the shape's smallest scale has no positive lower bound.
    Unbounded,
}

/// The clearance in device pixels, beyond the strokes' reach, from which two
/// contours of a shape that exports as pieces antialias as the one path
/// does, apart from a hole's own edge; nearer contours are reported
/// ([`compound_near_approximation`]). FX's renders lost nothing more from one
/// pixel in the native 1 to 3 px gap sweep; two leaves room
/// for a wider antialiasing in Premiere, which is unmeasured. It is counted
/// at the layer's scale times the smallest scale of its graphic's Vector
/// Motion and of each nest that holds it ([`smallest_scale`]); without a
/// positive lower bound it cannot be counted, and the contours are reported
/// ([`compound_unbounded_approximation`]).
const COMPOUND_EDGE_PIXELS: f64 = 2.0;

/// The smallest factor by which `group`, a graphic group whose transform is
/// its Vector Motion or the group of a nest, scales what it holds on screen:
/// of its axes' static values and Scale keys, which bound the scale that
/// Premiere draws too, since export keeps the static value of keys that it
/// reports instead of writing them. A key whose Bezier
/// easing may pass beyond the keys around it (a control point's y outside 0
/// to 1) bounds nothing, `None`; keys of both signs pass through 0.
pub(super) fn smallest_scale(group: &GroupLayer, context: &LayerExport<'_, '_>) -> Option<f64> {
    let mut smallest = group
        .transform
        .scale
        .into_iter()
        .map(f64::abs)
        .fold(f64::INFINITY, f64::min);
    let tracks = context
        .property_tracks
        .get(&group.id)
        .into_iter()
        .flat_map(|tracks| {
            [PropType::ScaleX, PropType::ScaleY]
                .into_iter()
                .filter_map(|axis| tracks.get(&axis))
        });
    for track in tracks {
        let keys = track.keyframes();
        let overshoots = keys.iter().any(|key| {
            matches!(key.easing(), PropertyKeyframeEasing::CubicBezier { y1, y2, .. }
                if !(0.0..=1.0).contains(&y1) || !(0.0..=1.0).contains(&y2))
        });
        if overshoots {
            return None;
        }
        let values: Vec<f64> = keys
            .iter()
            .filter_map(|key| match key.value() {
                PropertyValue::Float(value) => Some(*value),
                _ => None,
            })
            .collect();
        let crosses_zero =
            values.iter().any(|value| *value < 0.0) && values.iter().any(|value| *value > 0.0);
        let track_smallest = if crosses_zero {
            0.0
        } else {
            values
                .into_iter()
                .map(f64::abs)
                .fold(f64::INFINITY, f64::min)
        };
        smallest = smallest.min(track_smallest);
    }
    Some(smallest / 100.0)
}

/// The Premiere fill of an FX shape fill that blends normally at full
/// opacity with the nonzero rule: a solid color, or a linear or radial
/// gradient on the layer's x axis, which Premiere stores with midpoints of
/// 50 % and [`fx_fill`] imports alike: a color stop at each FX stop, and an
/// opacity stop at each too unless every stop is opaque ([`OPAQUE_OPACITY_STOPS`]).
/// Coordinates, positions and opacities narrow to f32, colors to 8 bits;
/// `PrShape::validate` checks the axis and the stops.
fn premiere_fill(fill: &ShapeFillStyle, compound: bool) -> Result<PrFill> {
    ensure!(
        (compound || fill.fill_rule == ShapeFillRule::NonZeroWinding)
            && fill.blend_mode == BlendMode::Normal
            && fill.opacity == 1.0,
        "shape fills blend normally at full opacity with the nonzero rule"
    );
    let (gradient_type, start, end, stops) = match &fill.paint {
        ShapePaint::Solid { color } => return Ok(PrFill::Solid(rgb(*color, "shape fill")?)),
        ShapePaint::Gradient {
            gradient_type,
            start,
            end,
            stops,
        } => (gradient_type, start, end, stops),
    };
    let kind = match gradient_type {
        ShapeGradientType::Linear => PrGradientKind::Linear,
        ShapeGradientType::Radial => PrGradientKind::Radial,
        ShapeGradientType::Reflected | ShapeGradientType::Conic => {
            return Err(unsupported(
                "reflected and conic gradient shape fills are unsupported",
            ))
        }
    };
    ensure!(start[1] == 0.0 && end[1] == 0.0, "{GRADIENT_Y_UNCONVERTED}");
    let opacity_stops = if stops.iter().all(|stop| stop.color[3] == 1.0) {
        OPAQUE_OPACITY_STOPS.to_vec()
    } else {
        stops
            .iter()
            .map(|stop| PrGradientOpacityStop {
                position: stop.offset as f32,
                opacity: stop.color[3] as f32,
            })
            .collect()
    };
    let stops = stops
        .iter()
        .map(|stop| {
            let [red, green, blue, _] = stop.color;
            Ok(PrGradientStop {
                position: stop.offset as f32,
                color: rgb([red, green, blue, 1.0], "gradient stop")?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(PrFill::Gradient(PrGradient {
        kind,
        start_x: start[0] as f32,
        end_x: end[0] as f32,
        stops,
        opacity_stops,
    }))
}

/// Why the Shape rules do not export the gradient fill of `layer` as
/// written, if they do not: the reason of [`premiere_fill`] or of the
/// gradient's own checks. `None` for a solid fill, and for a gradient that
/// they export, which reports its approximations there, a transform or a
/// shadow among them ([`PrShape::gradient_approximations`]).
/// [`premiere_fill`]'s reason can concern the fill's blending, which a solid
/// fill shares, so a caller that replaces the fill reports that only for a
/// layer that then exports.
pub(super) fn unexported_gradient(layer: &ShapeLayer) -> Option<String> {
    let [fill] = layer.shape.fills.as_slice() else {
        return None;
    };
    if !matches!(fill.paint, ShapePaint::Gradient { .. }) {
        return None;
    }
    match premiere_fill(fill, false) {
        Ok(PrFill::Gradient(gradient)) => gradient.validate().err().map(|error| error.to_string()),
        Ok(PrFill::Solid(_)) => None,
        Err(error) => Some(error.to_string()),
    }
}

/// The centred stroke of an FX shape stroke on `path`: an enabled, solid,
/// undashed, normal-blend stroke with butt caps, which Premiere draws
/// (calibration-2), and a join that [`stroke_join`] proves.
fn shape_stroke(stroke: &ShapeStrokeStyle, path: &PrShapePath) -> Result<PrShapeStroke> {
    ensure!(stroke.enabled, "a disabled shape stroke is unsupported");
    let ShapePaint::Solid { color } = stroke.paint else {
        return Err(unsupported("gradient shape strokes are unsupported"));
    };
    ensure!(
        stroke.dashes.is_empty() && stroke.dash_offset == 0.0,
        "dashed shape strokes are unsupported"
    );
    ensure!(
        stroke.blend_mode == BlendMode::Normal && stroke.opacity == 1.0,
        "shape strokes blend normally at full opacity"
    );
    ensure!(
        stroke.cap == ShapeLineCap::Butt,
        "round and square stroke caps are unverified"
    );
    let join = match stroke.join {
        ShapeLineJoin::Miter => StrokeJoin::Miter(stroke.miter_limit),
        ShapeLineJoin::Bevel => StrokeJoin::Bevel,
        ShapeLineJoin::Round => StrokeJoin::Round,
    };
    stroke_join(path, Some(join)).map_err(unsupported)?;
    Ok(PrShapeStroke {
        color: rgb(color, "shape stroke")?,
        width: stroke.width.value() as f32,
    })
}

#[cfg(test)]
#[path = "tests/graphic.rs"]
mod tests;
