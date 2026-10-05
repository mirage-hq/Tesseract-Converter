//! Authored Color Matte clips ↔ editable solid-fill rectangles.
//!
//! A matte is opaque content, unlike the implicit black canvas that
//! `background` adds beneath every imported sequence: it keeps its own layer,
//! colour, placement, duration and stacking, so time it covers alone is not a
//! gap. Only a plain, static, canvas-sized solid fill is a Color Matte; export
//! draws every other rectangle, at the root or in a nest, as a graphic of one
//! Shape ([`rect_as_shape`]), which the Shape rules export or omit with their
//! reason.

use super::{
    background::identity_transform,
    graphic::{export_object, unexported_gradient, ObjectLayer},
    nested::LayerExport,
    premiere_to_tesseract::{crop_rect, guide_layer, guide_mask},
    tesseract_to_premiere::{canonical_matte_source, unplaced_track_matte},
};
use crate::{
    approximate,
    error::{ensure, unsupported, Result},
    export_loss::OmissionSink,
    format::{MediaId, PrMedia, PrSequence, PrVideoItem, PrVideoOccurrence},
    omit,
    schema::{
        color_matte::{COLOR_MATTE_INTRINSIC_TICKS, COLOR_MATTE_NAME},
        PrBlendMode, PrColorMatte, PrMediaKind, PrStaticCrop,
    },
    OmissionScope,
};
use fx_schema::{
    BlendMode, FxItemId, Layer, LayerId, Position, RectLayer, RectShape, ShapeContent,
    ShapeFillStyle, ShapeGradientType, ShapeLayer, ShapePaint, ShapePath, ShapePathCommand,
    ShapeStrokeStyle, TimeRangeProperty, Transform,
};
use std::collections::BTreeSet;

/// A full-frame solid fill with no stroke, roundness, gradient or paint blend:
/// the implicit canvas shape in another colour.
fn solid_shape(width: u32, height: u32, fill_color: [f64; 4]) -> RectShape {
    RectShape {
        fill_color,
        ..super::background::black_shape(width, height)
    }
}

/// The static transforms that leave a full-frame fill in place: the origin
/// pivot written on import, and the canvas-centred anchor and position that
/// `ProjectAction::InsertFullscreenLayer` writes. Export does not retain which
/// one was used; a re-import has the origin pivot.
fn neutral_transforms(width: u32, height: u32) -> [Transform; 2] {
    let center = [f64::from(width) / 2.0, f64::from(height) / 2.0];
    let centered = Transform {
        anchor_point: center,
        position: Position::xy(center[0], center[1]),
        ..identity_transform()
    };
    [identity_transform(), centered]
}

/// Build the neutral filled owner for one matte occurrence's timeline range.
/// The caller sets its static Opacity and binds any admitted sharp Crop.
/// A head Cross
/// Dissolve (Legacy) on the matte later keys that Opacity up from zero
/// (`import_transitions`).
pub(super) fn rect_layer(
    project: &PrSequence,
    matte: PrColorMatte,
    active_range: TimeRangeProperty,
    layer_id: LayerId,
    name: String,
    is_hidden: bool,
) -> RectLayer {
    RectLayer {
        id: layer_id,
        name,
        description: String::new(),
        is_hidden,
        parent: None,
        blend_mode: BlendMode::Normal,
        track_matte: None,
        masks: Vec::new(),
        active_range,
        effects: Vec::new(),
        motion_blur: false,
        transform: identity_transform(),
        rect: solid_shape(project.width, project.height, matte.fill_color()),
    }
}

/// Bind a validated sharp Crop before the matte's Opacity. The neutral sibling
/// guide stays opaque and nonpainting even when its filled owner is disabled.
pub(super) fn bind_sharp_crop(
    owner: &mut RectLayer,
    crop: &PrStaticCrop,
    canvas: [u32; 2],
    guide_id: LayerId,
    mask_id: FxItemId,
) -> Result<RectLayer> {
    ensure!(
        crop.edge_feather == 0.0,
        "Color Matte Crop must have zero feather"
    );
    let guide = guide_layer(
        guide_id,
        format!("{} Crop guide", owner.name),
        owner.parent,
        owner.active_range,
        identity_transform(),
        crop_rect(crop, canvas),
    );
    owner.masks.push(guide_mask(mask_id, guide_id, 0.0));
    Ok(guide)
}

/// The colour of a rectangle that a Color Matte carries unchanged: a static,
/// plain, canvas-sized solid fill with a neutral transform, whose blend mode
/// the matte's Opacity writes ([`PrBlendMode::from_fx_mode`]). Any other
/// rectangle has none and exports as a Shape.
///
/// `animated` says whether a Motion/Opacity keyframe track is on this layer;
/// it can change coverage or transparency over time, which a static matte
/// would misrepresent.
fn solid_fill_matte(
    rect: &RectLayer,
    parent: Option<LayerId>,
    width: u32,
    height: u32,
    animated: bool,
) -> Option<PrColorMatte> {
    let plain = !animated
        && rect.parent == parent
        && rect.masks.is_empty()
        && rect.effects.is_empty()
        && !rect.motion_blur
        && neutral_transforms(width, height).contains(&rect.transform)
        && solid_shape(width, height, rect.rect.fill_color) == rect.rect;
    if !plain {
        return None;
    }
    PrColorMatte::from_fill_color(rect.rect.fill_color).ok()
}

/// The Color Matte that `rect`, a layer of `parent`'s list, exports as, if it
/// exports as one ([`export_rect_layer`]): a plain solid fill without keys
/// ([`solid_fill_matte`]) that no layer in `consumed` uses as its track matte
/// or mask.
pub(super) fn exported_matte(
    rect: &RectLayer,
    parent: Option<LayerId>,
    consumed: &BTreeSet<LayerId>,
    context: &LayerExport<'_, '_>,
    [width, height]: [u32; 2],
) -> Option<PrColorMatte> {
    let animated = context.property_tracks.contains_key(&rect.id);
    let dissolve = super::cross_dissolve::matte_animation(rect, context).is_some();
    let mut normalized = rect.clone();
    if dissolve {
        normalized.transform.opacity = identity_transform().opacity;
        normalized.rect.fill_color[3] = 1.0;
    }
    solid_fill_matte(&normalized, parent, width, height, animated && !dissolve)
        .filter(|_| !consumed.contains(&rect.id))
}

/// The handle length of a cubic quarter circle as a fraction of its radius,
/// 4(√2 − 1)/3, with which FX rounds a rectangle's corners (`KAPPA` in
/// `scene::Path::rounded_rect_radii`).
const KAPPA: f32 = 0.552_284_8;

/// The corner radius, in layer pixels, below which FX draws a rectangle with
/// square corners (`scene::Path::rounded_rect_radii`).
const MIN_CORNER_RADIUS: f32 = 1e-6;

/// The outline that FX draws for `shape` (`scene::Path::rounded_rect`), in
/// layer pixels and in its f32 arithmetic: from `position` to
/// `position + size`, each corner rounded by `roundness`, which one factor
/// scales down until adjacent radii fit their edge (CSS border-radius), as
/// four cubic quarter circles clockwise from the top edge and the straight
/// edges between them that have a positive length in f32. FX also draws an
/// edge that the radii leave with no length, or by rounding a few f32 steps
/// backward; the outline is the same without it, and its two quarter
/// circles then meet smoothly, where a stroke has no corner to join. Every
/// corner is square below [`MIN_CORNER_RADIUS`], and for a size or
/// roundness that is not finite in f32.
pub(super) fn rounded_rect_outline(position: [f64; 2], size: [f64; 2], radius: f64) -> ShapePath {
    let mut shape = solid_shape(0, 0, [0.0; 4]);
    shape.position = position;
    shape.size = size;
    shape.roundness = radius;
    rect_outline(&shape)
}

fn rect_outline(shape: &RectShape) -> ShapePath {
    let [x, y] = shape.position.map(|value| value as f32);
    let [w, h] = shape.size.map(|value| value as f32);
    let roundness = shape.roundness as f32;
    let r = if [w, h, roundness].iter().all(|value| value.is_finite()) {
        let r = roundness.max(0.0);
        r * [w, h]
            .into_iter()
            .fold(1.0_f32, |scale, edge| scale.min((edge / (r + r)).max(0.0)))
    } else {
        0.0
    };
    let point = |x: f32, y: f32| (f64::from(x), f64::from(y));
    let move_to = |(x, y)| ShapePathCommand::MoveTo {
        x,
        y,
        mirror: None,
        corner_radius: None,
    };
    let line = |(x, y)| ShapePathCommand::LineTo {
        x,
        y,
        mirror: None,
        corner_radius: None,
    };
    let curve = |(c1x, c1y), (c2x, c2y), (x, y)| ShapePathCommand::CubicTo {
        c1x,
        c1y,
        c2x,
        c2y,
        x,
        y,
        mirror: None,
        corner_radius: None,
    };
    let commands = if r < MIN_CORNER_RADIUS {
        vec![
            move_to(point(x, y)),
            line(point(x + w, y)),
            line(point(x + w, y + h)),
            line(point(x, y + h)),
            ShapePathCommand::Close,
        ]
    } else {
        let k = r * KAPPA;
        let [horizontal, vertical] =
            [(x, w), (y, h)].map(|(start, size)| start + size - r > start + r);
        [
            Some(move_to(point(x + r, y))),
            horizontal.then(|| line(point(x + w - r, y))),
            Some(curve(
                point(x + w - r + k, y),
                point(x + w, y + r - k),
                point(x + w, y + r),
            )),
            vertical.then(|| line(point(x + w, y + h - r))),
            Some(curve(
                point(x + w, y + h - r + k),
                point(x + w - r + k, y + h),
                point(x + w - r, y + h),
            )),
            horizontal.then(|| line(point(x + r, y + h))),
            Some(curve(
                point(x + r - k, y + h),
                point(x, y + h - r + k),
                point(x, y + h - r),
            )),
            vertical.then(|| line(point(x, y + r))),
            Some(curve(
                point(x, y + r - k),
                point(x + r - k, y),
                point(x + r, y),
            )),
            Some(ShapePathCommand::Close),
        ]
        .into_iter()
        .flatten()
        .collect()
    };
    ShapePath { commands }
}

/// The FX shape layer that draws `rect` as FX draws a rectangle, for the
/// Shape rules to export or omit: the rectangle's layer fields, its outline
/// ([`rect_outline`]), its nonzero fill while enabled, painted with
/// `fill_paint` or else the solid `fill_color`, with the fill blend mode,
/// and the butt-capped stroke with the rectangle's join, miter limit and
/// dashes while it is enabled, coloured and wider than 0. FX draws an
/// undashed stroke whatever its dash offset.
pub(super) fn rect_as_shape(rect: &RectLayer) -> ShapeLayer {
    let shape = &rect.rect;
    let fill = shape.fill_enabled.then(|| ShapeFillStyle {
        paint: shape.fill_paint.clone().unwrap_or(ShapePaint::Solid {
            color: shape.fill_color,
        }),
        blend_mode: shape.fill_blend_mode.unwrap_or_default(),
        ..ShapeFillStyle::solid(shape.fill_color)
    });
    let stroke = shape
        .stroke_color
        .filter(|_| shape.stroke_enabled && shape.stroke_width.value() > 0.0)
        .map(|color| ShapeStrokeStyle {
            join: shape.stroke_join,
            miter_limit: shape.stroke_miter_limit,
            dashes: shape.stroke_dashes.clone(),
            dash_offset: if shape.stroke_dashes.is_empty() {
                0.0
            } else {
                shape.stroke_dash_offset
            },
            ..ShapeStrokeStyle::solid(color, shape.stroke_width)
        });
    ShapeLayer {
        id: rect.id,
        name: rect.name.clone(),
        description: rect.description.clone(),
        is_hidden: rect.is_hidden,
        parent: rect.parent,
        blend_mode: rect.blend_mode,
        track_matte: rect.track_matte.clone(),
        masks: rect.masks.clone(),
        active_range: rect.active_range,
        effects: rect.effects.clone(),
        motion_blur: rect.motion_blur,
        transform: rect.transform,
        shape: ShapeContent {
            path: rect_outline(shape),
            fills: fill.into_iter().collect(),
            strokes: stroke.into_iter().collect(),
            round_corners: None,
            offset_paths: None,
            trim: None,
            poly_star: None,
            ellipse: None,
        },
    }
}

/// Paint the fill of `shape`, the view of a rectangle with `rect`
/// ([`rect_as_shape`]), with `rect`'s solid `fill_color`, FX's fallback for
/// its gradient, when the Shape rules do not export that gradient
/// ([`unexported_gradient`]), and return the warning that names the
/// approximation: the gradient kind, the Shape rules' reason and the largest
/// channel difference between a stop and the fill colour, which bounds the
/// error because a gradient interpolates its stops.
fn approximate_unexported_gradient(shape: &mut ShapeLayer, rect: &RectShape) -> Option<String> {
    let Some(ShapePaint::Gradient {
        gradient_type,
        stops,
        ..
    }) = &rect.fill_paint
    else {
        return None;
    };
    let reason = unexported_gradient(shape)?;
    for fill in &mut shape.shape.fills {
        fill.paint = ShapePaint::Solid {
            color: rect.fill_color,
        };
    }
    let kind = match gradient_type {
        ShapeGradientType::Linear => "linear",
        ShapeGradientType::Radial => "radial",
        ShapeGradientType::Reflected => "reflected",
        ShapeGradientType::Conic => "conic",
    };
    let difference = stops
        .iter()
        .flat_map(|stop| stop.color.iter().zip(rect.fill_color))
        .fold(0.0_f64, |largest, (stop, fill)| {
            largest.max((stop - fill).abs())
        });
    Some(format!(
        "rectangle gradient fill {kind} approximated by its solid fill colour: {reason}; colours differ from the gradient's stops by up to {}/255",
        (difference * 255.0).round()
    ))
}

/// Export one rectangle layer of a list whose layers have `parent` (a nest's
/// group, or none at the root): a canvas-sized static solid of that list that
/// no other layer consumes as a Color Matte occurrence on the export grid,
/// whose generator media joins the export's media, or any other rectangle as
/// a graphic of one Shape ([`rect_as_shape`], [`export_object`]), its
/// gradient fill approximated when the Shape rules do not export it
/// ([`approximate_unexported_gradient`], one Feature warning when the Shape
/// exports); `None` when it is omitted with its reason. `consumed` holds the
/// layers that other layers use as a track matte or mask, or as the path of a
/// text that FX shows (`consumed_layer_ids`). The matte placement starts at
/// the generator in-point of the sequence rate
/// ([`FrameRate::generator_in_ticks`](crate::format::FrameRate::generator_in_ticks)).
///
/// A Motion/Opacity keyframe track on this layer makes it a keyed Shape,
/// which the Shape rules omit. The context's media facts list every packaged
/// asset, so a document asset ID spelled like a matte's `color-matte:rrggbb`
/// ID rejects whatever the layer order.
pub(super) fn export_rect_layer(
    rect: &RectLayer,
    parent: Option<LayerId>,
    consumed: &BTreeSet<LayerId>,
    layers: &[Layer],
    context: &mut LayerExport<'_, '_>,
    omissions: &mut dyn OmissionSink,
    record: &str,
) -> Result<Option<PrVideoItem>> {
    let (width, height, frame_rate) = (context.width, context.height, context.frame_rate);
    let Some(matte) = exported_matte(rect, parent, consumed, context, [width, height]) else {
        let mut shape = rect_as_shape(rect);
        let approximation = approximate_unexported_gradient(&mut shape, &rect.rect);
        let graphic = export_object(
            ObjectLayer::Shape(&shape),
            parent,
            consumed,
            context,
            omissions,
            record,
        );
        if let (Some(_), Some(warning)) = (&graphic, approximation) {
            approximate(omissions, record, warning);
        }
        return Ok(graphic.map(PrVideoItem::Graphic));
    };
    let mask = match rect
        .track_matte
        .as_ref()
        .map(|matte| {
            if rect.is_hidden || rect.blend_mode != BlendMode::Normal {
                return Err("a keyed Color Matte requires an enabled Normal-blend fill".to_owned());
            }
            canonical_matte_source(
                matte,
                layers,
                parent,
                rect.active_range,
                context.dynamics,
                [width, height],
                false,
            )
        })
        .transpose()
    {
        Ok(mask) => mask,
        Err(reason) => {
            omit(
                omissions,
                OmissionScope::Occurrence,
                record,
                format!("Color Matte track matte was not exported: {reason}"),
            );
            return Ok(None);
        }
    };
    let media = MediaId(format!("color-matte:{}", matte.hex()));
    if context.media_facts.contains_key(media.as_str()) {
        return Err(unsupported(format!(
            "asset {media} names both a packaged asset and a Color Matte solid fill"
        )));
    }
    let active_end = rect
        .active_range
        .start
        .checked_add_duration(rect.active_range.duration)
        .ok_or_else(|| unsupported("activeRange end exceeds Premiere's tick range"))?;
    let start_ticks = context.frame_ticks(rect.active_range.start, "activeRange.start")?;
    let end_ticks = context.picture_end_ticks(active_end, mask.as_ref())?;
    ensure!(
        end_ticks > start_ticks,
        "activeRange {}..{} ms collapses to zero duration on the {frame_rate} sequence grid",
        rect.active_range.start.as_millis(),
        active_end.as_millis()
    );
    let in_ticks = frame_rate.generator_in_ticks();
    let out_ticks = in_ticks
        .checked_add(end_ticks - start_ticks)
        .filter(|out| *out <= COLOR_MATTE_INTRINSIC_TICKS)
        .ok_or_else(|| unsupported("solid fill outlasts the Color Matte generator"))?;
    let animation = super::cross_dissolve::matte_animation(rect, context);
    let occurrence = PrVideoOccurrence {
        animations: animation
            .into_iter()
            .map(crate::schema::PrPropertyAnimation::Opacity)
            .collect(),
        track_matte: unplaced_track_matte(mask.as_ref()),
        blend_mode: PrBlendMode::from_fx_mode(rect.blend_mode),
        enabled: !rect.is_hidden,
        ..PrVideoOccurrence::unedited(media, start_ticks..end_ticks, in_ticks..out_ticks)
    };
    let media = PrMedia {
        name: COLOR_MATTE_NAME.to_owned(),
        relative_path: None,
        relative_paths: Vec::new(),
        absolute_paths: Vec::new(),
        video: Some(crate::schema::PrVideoStream {
            pixel_aspect: Default::default(),
            interpretation: Default::default(),
            orientation: crate::schema::VideoOrientation::Identity,
            intrinsic_ticks: COLOR_MATTE_INTRINSIC_TICKS,
            frame_rate: frame_rate.into(),
            width,
            height,
            kind: PrMediaKind::ColorMatte(matte),
        }),
        audio: None,
    };
    context
        .media
        .entry(occurrence.media.clone())
        .or_insert(media);
    if let Some(warning) = PrBlendMode::export_approximation(rect.blend_mode) {
        approximate(omissions, record, warning);
    }
    if !occurrence.animations.is_empty() {
        context
            .written
            .record(rect.id, fx_schema::PropType::Opacity);
        context.property_tracks.remove(&rect.id);
    }
    Ok(Some(PrVideoItem::Media(occurrence)))
}

#[cfg(test)]
#[path = "tests/color_matte.rs"]
mod tests;
