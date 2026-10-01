//! Graphic objects and transform keys in both directions.
//!
//! A graphic's objects are its Text and Shape components. One object imports
//! as a root text or shape layer, unless the group below is needed; two or
//! more import as an FX group whose transform is the Vector Motion (the
//! identity without one) and whose layers are the objects in paint order
//! ([`in_paint_order`]). Export maps a root text or shape layer, and a root
//! group of text and shape layers, back to one graphic; a shape keeps one
//! single-contour path, one solid or gradient fill and at most one centred
//! stroke ([`shape_object`]).
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

use super::{
    background::identity_transform,
    fonts, keyframes,
    nested::{LayerExport, LayerScope},
    premiere_to_tesseract::{
        keyframe_id, object_transform, position_tracks, rgba, scalar_keys, set_tracks, text_layer,
        tick_range, validate_time_range,
    },
    tesseract_to_premiere::{
        export_position_keys, export_scalar_keys, graphic_of, has_background, omit_object_blend,
        rgb, scale_tracks_match, text_document, text_object, unexported_layer_fields, ClipLayer,
    },
    text::{automatic_line_spacing, STROKE_WIDTH_RATIO},
    text_shadow,
};
use crate::{
    error::{ensure, unsupported, Result},
    export_loss::{omit_field, ExportField, OmissionSink},
    format::PrGraphic,
    schema::{
        text::{
            stroke_join, GraphicParamSpec, PrAppearance, PrFill, PrGradient, PrGradientKind,
            PrGradientOpacityStop, PrGradientStop, PrGraphicObject, PrPathVertex, PrShape,
            PrShapePath, PrShapeStroke, PrSourceTextKey, PrText, PrTextBackground, PrTextFrame,
            PrTextLines, PrTextTransform, PrVectorMotion, PrVerticalAlign, SourceTextField,
            StrokeJoin, GRADIENT_Y_UNCONVERTED, OPAQUE_OPACITY_STOPS, TEXT_PARAMS,
            VECTOR_MOTION_PARAMS,
        },
        PrAnimatedProperty, PrBlendMode, PrPropertyAnimation, PrScalarKeyframe,
    },
    {approximate, omit, Omission, OmissionScope},
};
use fx_schema::{
    animator::{PropertyKeyframe, PropertyKeyframeEasing, PropertyKeyframeTrack},
    AnimationGraph, BlendMode, EffectRecord, GroupLayer, Layer, LayerData, LayerId,
    NonNegativeProperty, PercentageProperty, Position, PositiveProperty, PropType, Property,
    PropertyValue, ShapeContent, ShapeFillRule, ShapeFillStyle, ShapeGradientStop,
    ShapeGradientType, ShapeHandleMirror, ShapeLayer, ShapeLineCap, ShapeLineJoin, ShapePaint,
    ShapePath, ShapePathCommand, ShapeStrokeStyle, TextDocument, TextLayer, Time, TimeOffset,
    TimeRangeProperty, Transform,
};
use std::collections::{BTreeMap, BTreeSet};

/// Why export keeps the static value of a graphic property with cubic Bezier
/// easing whose parameter lacks `bezier_speeds_verified`. Premiere would
/// store the curve's handles as speeds, and their unit was measured only for
/// Text Scale and Opacity and Vector Motion Scale and Rotation (JRB-1990).
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
/// kept Vector Motion, or a nondefault clip Opacity or blend mode, which the
/// group applies to the whole graphic.
pub(super) fn import_graphic(
    graphic: &PrGraphic,
    dimensions: [u32; 2],
    layer_id: LayerId,
    index: usize,
    scope: &mut LayerScope<'_, '_, '_>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
) -> Result<Layer> {
    let record = graphic.id().unwrap_or("graphic");
    let motion = graphic.vector_motion.as_ref();
    // Only the caption reader sets a background, on a graphic of one text.
    let background = match graphic.objects.as_slice() {
        [PrGraphicObject::Text(text)] => text.document.background,
        _ => None,
    };
    let grouped = graphic.objects.len() > 1
        || motion.is_some()
        || graphic.opacity != 100.0
        || graphic.blend_mode.fx_mode() != BlendMode::Normal
        || !graphic.animations.is_empty()
        || background.is_some();
    // The group takes the placement's range and visibility; its objects start
    // with it, so all keep the graphic's clock.
    let group_id = grouped.then(|| next_layer_id(scope));
    // An ungrouped object is a layer of the list, a nest's included.
    let list_parent = scope.parent;
    let join = |parent: &mut Option<LayerId>, hidden: &mut bool, range: &mut TimeRangeProperty| {
        match group_id {
            Some(group_id) => {
                *parent = Some(group_id);
                *hidden = false;
                *range = TimeRangeProperty::new(Time::ZERO, range.duration);
            }
            None => *parent = list_parent,
        }
    };
    let mut layers = Vec::with_capacity(graphic.objects.len());
    for (position, object) in graphic.objects.iter().enumerate() {
        // The first object keeps the graphic's layer id.
        let object_id = if position == 0 {
            layer_id
        } else {
            next_layer_id(scope)
        };
        let data = match object {
            PrGraphicObject::TextLines(text) => LayerData::Group(import_text_lines(
                graphic,
                text,
                TextBlockPlacement {
                    id: object_id,
                    parent: group_id.or(list_parent),
                    inside_graphic: group_id.is_some(),
                },
                dimensions,
                scope,
                dynamics,
                omissions,
            )?),
            PrGraphicObject::Text(text) => {
                let mut layer = text_layer(graphic, text, object_id, index)?;
                layer.effects.extend(text_shadow::import_text_shadow(
                    text,
                    motion,
                    record,
                    scope.effect_ids,
                    omissions,
                )?);
                validate_time_range("active_range", layer.active_range)?;
                let mut tracks = object_tracks(
                    &text.animations,
                    graphic.in_ticks,
                    object_id,
                    dimensions,
                    record,
                    omissions,
                );
                tracks.extend(source_text_tracks(
                    text,
                    graphic.in_ticks,
                    object_id,
                    &mut layer.source_text,
                )?);
                set_tracks(dynamics, tracks)?;
                join(
                    &mut layer.parent,
                    &mut layer.is_hidden,
                    &mut layer.active_range,
                );
                LayerData::Text(layer)
            }
            PrGraphicObject::Shape(shape) => {
                let mut layer = shape_layer(graphic, shape, object_id, index)?;
                let shadow = text_shadow::import_shape_shadow(
                    shape,
                    motion,
                    record,
                    scope.effect_ids,
                    omissions,
                )?;
                for warning in shape.gradient_approximations(graphic, shadow.is_some()) {
                    approximate(omissions, record, warning);
                }
                layer.effects.extend(shadow);
                validate_time_range("active_range", layer.active_range)?;
                join(
                    &mut layer.parent,
                    &mut layer.is_hidden,
                    &mut layer.active_range,
                );
                LayerData::Shape(layer)
            }
        };
        layers.push(data);
    }
    if let Some(warning) = graphic.blend_mode.approximation() {
        approximate(omissions, record, warning);
    }
    let Some(group_id) = group_id else {
        let [layer] = layers.as_slice() else {
            return Err(unsupported("an ungrouped graphic holds one object"));
        };
        return Ok(Layer::from_data(layer)?);
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
            graphic.in_ticks,
            group_id,
            dimensions,
            record,
            omissions,
        ));
    }
    set_tracks(dynamics, tracks)?;
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
    Ok(Layer::from_data(&LayerData::Group(GroupLayer {
        id: group_id,
        name: format!("Premiere graphic {}", index + 1),
        description: String::new(),
        is_hidden: !graphic.enabled,
        parent: scope.parent,
        blend_mode: graphic.blend_mode.fx_mode(),
        track_matte: None,
        masks: Vec::new(),
        playback: fx_schema::LayerPlayback::linear(
            window,
            window,
            TimeRangeProperty::new(Time::ZERO, window.duration),
            0,
        )
        .map_err(unsupported)?,
        effects: Vec::new(),
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
    }))?)
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
    scope: &mut LayerScope<'_, '_, '_>,
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
fn next_layer_id(scope: &mut LayerScope<'_, '_, '_>) -> LayerId {
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
                scalar_tracks(&[PropType::ScaleX, PropType::ScaleY])
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
        .expect("every Source Text field has a carrier property")
}

/// Hold FX tracks of a text object's Source Text keys, on the layer clock
/// that starts at the generator time `in_ticks`: the text always, and each
/// field that differs between keys. FX leading is the whole line spacing
/// while Premiere adds its leading to 120 % of the size, so size keys beside
/// any leading key the leading too. FX text has no stroke color or
/// width track, so a stroke that keys switch on becomes the layer's static
/// stroke in `source_text`. A track the FX document cannot hold (over its
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

    fn effects(self) -> &'a [EffectRecord] {
        match self {
            Self::Text(layer) => &layer.effects,
            Self::Shape(layer) => &layer.effects,
        }
    }
}

/// The objects of a group that exports as one graphic: its layers, when all
/// are text and shape layers. Groups of other layers are nested sequences.
pub(super) fn graphic_objects(group: &GroupLayer) -> Option<Vec<ObjectLayer<'_>>> {
    group
        .layers
        .iter()
        .map(ObjectLayer::of)
        .collect::<Option<Vec<_>>>()
        .filter(|objects| !objects.is_empty())
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
    if let Some(reason) = object.unsupported(consumed) {
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
    let mut graphic = export_objects(&[object], None, parent, context, omissions, record)?;
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
/// that FX shows (`consumed_layer_ids`).
pub(super) fn export_graphic_group(
    group: &GroupLayer,
    objects: &[ObjectLayer<'_>],
    consumed: &BTreeSet<LayerId>,
    context: &mut LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
) -> Option<PrGraphic> {
    let record = format!("layer {} ({:?})", group.id, group.name);
    if let Some(reason) = unsupported_graphic_group(group, objects, consumed) {
        omit(
            omissions,
            OmissionScope::Occurrence,
            &record,
            format!("graphic group was not exported: {reason}"),
        );
        return None;
    }
    if !group.description.is_empty() {
        omit_field(
            omissions,
            group.id,
            ExportField::Description,
            &record,
            "description was not exported",
        );
    }
    let record = match objects {
        [object] => object.record(),
        _ => record,
    };
    export_objects(
        objects,
        Some(group),
        Some(group.id),
        context,
        omissions,
        &record,
    )
}

/// Why a group of `objects` cannot export as one graphic, if it cannot:
/// Vector Motion is a uniform 2D transform, whose opacity is the clip
/// Opacity's, a graphic has one clock and no group effects or masks, only a
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
        (!group.masks.is_empty(), "a graphic has no group masks"),
        (
            group.track_matte.is_some(),
            "a graphic has no group track matte",
        ),
        (background, "a graphic has no group background"),
        (
            !super::timing::is_plain_group_playback(&group.playback),
            "graphic time remapping is unsupported",
        ),
        (group.motion_blur, "a graphic has no motion blur"),
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
            !shape.masks.is_empty(),
            "its shape layer must have no masks",
        ),
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

/// The native object of one FX object layer whose parent should be `parent`,
/// or `None` when its font is not exportable, which is reported.
fn object_part(
    object: ObjectLayer<'_>,
    parent: Option<LayerId>,
    context: &LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Option<Result<PrGraphicObject>> {
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
            Some(text_object(layer, parent, font, omissions, record).map(PrGraphicObject::Text))
        }
        ObjectLayer::Shape(layer) => {
            for (changed, field) in unexported_layer_fields(ClipLayer::Shape(layer), parent, false)
            {
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
            Some(shape_object(layer).map(PrGraphicObject::Shape))
        }
    }
}

/// Export text and shape layers as one graphic whose objects follow their
/// paint order: one layer of a list, or the layers of `group` with the
/// group's Vector Motion; `parent` is the parent they have, the list's or
/// `group`. Only a graphic of one text keeps the layer's keys: keys on a
/// shape, or on an object of several, omit the graphic. One object reports
/// under its own layer and several under their group; a gradient shape
/// reports its approximations ([`PrShape::gradient_approximations`]) under
/// its own layer.
fn export_objects(
    objects: &[ObjectLayer<'_>],
    group: Option<&GroupLayer>,
    parent: Option<LayerId>,
    context: &mut LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Option<PrGraphic> {
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
    let mut natives = Vec::with_capacity(objects.len());
    for &object in objects {
        let object_record = object.record();
        let Some(native) = object_part(object, parent, context, omissions, &object_record) else {
            return if several {
                failed(omissions, format!("{object_record} cannot export"))
            } else {
                None
            };
        };
        let native = native.and_then(|native| {
            native.validate()?;
            ensure!(
                !several || !context.property_tracks.contains_key(&object.id()),
                "keyed objects in a graphic with several objects are unsupported"
            );
            Ok(native)
        });
        match native {
            Ok(native) => natives.push(native),
            Err(error) if several => return failed(omissions, format!("{object_record}: {error}")),
            Err(error) => return failed(omissions, error.to_string()),
        }
    }
    let range = group.map_or(objects[0].active_range(), |group| {
        group.playback.input_range()
    });
    let enabled = objects.iter().all(|object| !object.is_hidden())
        && group.is_none_or(|group| !group.is_hidden);
    let exported = graphic_of(natives, &range, enabled, context).and_then(|mut graphic| {
        // Only one object can still have keys here.
        if let Some(mut tracks) = context.property_tracks.remove(&objects[0].id()) {
            let ([PrGraphicObject::Text(text)], ObjectLayer::Text(layer)) =
                (graphic.objects.as_mut_slice(), objects[0])
            else {
                return Err(unsupported("keyed graphic shapes are unsupported"));
            };
            let source_text: BTreeMap<_, _> = SOURCE_TEXT_PROPERTIES
                .iter()
                .filter_map(|&(field, property, _)| {
                    tracks.remove(&property).map(|track| (field, track))
                })
                .collect();
            if !source_text.is_empty() {
                text.source_text_keys = source_text_keys(
                    &source_text,
                    &layer.source_text,
                    &text.document.font,
                    graphic.in_ticks,
                )?;
                if let Some(anchor) = tracks.remove(&PropType::AnchorPointY) {
                    restore_point_alignment(text, anchor, graphic.in_ticks)?;
                }
                text.document = text.source_text_keys[0].document.clone();
            }
            text.animations = object_keys(
                tracks,
                &mut text.transform.position,
                &TEXT_PARAMS,
                graphic.in_ticks,
                frame,
                omissions,
                record,
            );
        }
        if let Some(group) = group {
            apply_group(&mut graphic, group, context, omissions);
        }
        graphic.validate(context.frame_rate)?;
        Ok(graphic)
    });
    match exported {
        Ok(mut graphic) => {
            export_effects(objects, &mut graphic, context.dynamics, omissions);
            for (object, native) in objects.iter().zip(&graphic.objects) {
                if let PrGraphicObject::Shape(shape) = native {
                    let shadowed = shape.appearance.shadow.is_some();
                    for warning in shape.gradient_approximations(&graphic, shadowed) {
                        approximate(omissions, object.record(), warning);
                    }
                }
            }
            // The caller places the graphic, which writes these keys.
            if let ([object], [PrGraphicObject::Text(text)]) = (objects, graphic.objects.as_slice())
            {
                context
                    .written
                    .record_animations(object.id(), &text.animations);
            }
            if let Some(group) = group {
                if let Some(warning) = PrBlendMode::export_approximation(group.blend_mode) {
                    let group_record = format!("layer {} ({:?})", group.id, group.name);
                    approximate(omissions, group_record, warning);
                }
                if let Some(motion) = &graphic.vector_motion {
                    context
                        .written
                        .record_animations(group.id, &motion.animations);
                }
                context
                    .written
                    .record_animations(group.id, &graphic.animations);
            }
            graphic.objects = in_paint_order(graphic.objects);
            Some(graphic)
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
    let absorbed = motion.animations.is_empty()
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
/// order: a text or shape shadow, with every other effect reported.
fn export_effects(
    layers: &[ObjectLayer<'_>],
    graphic: &mut PrGraphic,
    dynamics: &AnimationGraph,
    omissions: &mut dyn OmissionSink,
) {
    let motion = graphic.vector_motion.as_ref();
    for (layer, object) in layers.iter().zip(&mut graphic.objects) {
        let record = layer.record();
        let effects = (layer.effects(), layer.id());
        match object {
            // FX export creates ordinary single-style objects from the edited
            // children; it never reconstructs an imported native style block.
            PrGraphicObject::TextLines(_) => {}
            PrGraphicObject::Text(text) => {
                // Source Text keys share the document's shadow (`PrText::validate`).
                let shadow = text_shadow::export_text_effects(
                    effects, dynamics, text, motion, omissions, &record,
                );
                text.document.shadow = shadow;
                for key in &mut text.source_text_keys {
                    key.document.shadow = shadow;
                }
            }
            PrGraphicObject::Shape(shape) => {
                shape.appearance.shadow = text_shadow::export_shape_effects(
                    effects, dynamics, shape, motion, omissions, &record,
                );
            }
        }
    }
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

/// Native Source Text keys of a text layer's Source Text tracks: one
/// complete document at every key time of every track, each field read from
/// its track by Hold, on the generator clock that starts at `in_ticks`. The
/// first key's document is the text shown before it. Premiere holds Source
/// Text between keys, so a key with another easing after the first, a value
/// of another kind, or a document the encoding cannot hold is an error,
/// which omits the graphic: static text would show the wrong content.
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
/// Path holds one contour, so a hole needs `Mask with Shape` (JRB-2083).
const ONE_CONTOUR: &str =
    "a Premiere shape path holds one contour; holes need Mask with Shape (JRB-2083)";

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
/// native smooth vertices and the rounded bar
/// (`oracle/EX2b/curved-path/facts.md`); a smooth cusp and a smooth vertex
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

/// The Premiere Shape of an FX shape layer, or why it has none: one path of
/// one contour with sharp anchors, at most one fill ([`premiere_fill`]) and
/// one stroke in the forms that Premiere draws alike, no skew or 3D rotation,
/// and no primitive or path modifier. Its shadow is exported with its
/// effects.
fn shape_object(layer: &ShapeLayer) -> Result<PrShape> {
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
    let path = premiere_path(&content.path)?;
    let fill = match content.fills.as_slice() {
        [] => None,
        [fill] => Some(premiere_fill(fill)?),
        _ => return Err(unsupported("a Premiere shape has one fill")),
    };
    let stroke = match content.strokes.as_slice() {
        [] => None,
        [stroke] => Some(shape_stroke(stroke, &path)?),
        _ => return Err(unsupported("a Premiere shape has one stroke")),
    };
    // Premiere scales x by Horizontal Scale while Uniform Scale is off.
    let [horizontal, vertical] = transform.scale;
    Ok(PrShape {
        name: layer.name.clone(),
        path,
        appearance: PrAppearance {
            fill,
            stroke,
            shadow: None,
        },
        transform: PrTextTransform {
            position: [transform.position.x(), transform.position.y()],
            anchor: transform.anchor_point,
            scale: vertical,
            rotation: transform.rotation,
            opacity: transform.opacity.value(),
        },
        horizontal_scale: (horizontal != vertical).then_some(horizontal),
    })
}

/// The Premiere fill of an FX shape fill that blends normally at full
/// opacity with the nonzero rule: a solid color, or a linear or radial
/// gradient on the layer's x axis, which Premiere stores with midpoints of
/// 50 % and [`fx_fill`] imports alike: a color stop at each FX stop, and an
/// opacity stop at each too unless every stop is opaque ([`OPAQUE_OPACITY_STOPS`]).
/// Coordinates, positions and opacities narrow to f32, colors to 8 bits;
/// `PrShape::validate` checks the axis and the stops.
fn premiere_fill(fill: &ShapeFillStyle) -> Result<PrFill> {
    ensure!(
        fill.fill_rule == ShapeFillRule::NonZeroWinding
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
                "reflected and conic gradient shape fills are unsupported (JRB-2015)",
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
    match premiere_fill(fill) {
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
        return Err(unsupported(
            "gradient shape strokes are unsupported (JRB-2015)",
        ));
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
