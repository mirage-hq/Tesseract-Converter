//! Sampled geometry for the independently measured default Film Impact Pop.
//! Early native motion blur/fade is not inferred from the plugin control names.
use super::{keyframe_id, time_from_ticks, ItemLayer, LayerScope};
use crate::{
    error::{ensure, unsupported, Result},
    format::{FrameRate, MediaId, PrMedia, PrVideoItem},
    schema::{
        PrAnimatedProperty, PrBlendMode, PrMediaKind, PrVideoTrack, PrVideoTransition,
        PrVideoTransitionKind,
    },
};
use fx_schema::{
    animator::{PropertyKeyframe, PropertyKeyframeEasing, PropertyKeyframeTrack},
    EffectData, EffectPayload, Layer, LayerData, LayerEffect, PropType, Property, PropertyValue,
    TimeOffset,
};
use std::collections::BTreeMap;

// Diagnostic white-title fits at these normalized native sample phases. The
// invisible start has no valid fit; collapse it geometrically and settle at 1.
const CURVE: [(i64, f64); 16] = [
    (0, 0.0),
    (1, 0.314),
    (2, 0.576),
    (3, 0.776),
    (4, 0.928),
    (5, 1.034),
    (6, 1.102),
    (7, 1.128),
    (8, 1.122),
    (9, 1.098),
    (10, 1.068),
    (12, 1.014),
    (14, 0.990),
    (18, 0.998),
    (21, 1.0),
    (30, 1.0),
];

pub(super) fn tracks(
    transition: &PrVideoTransition,
    track: &PrVideoTrack,
    frame_rate: FrameRate,
    scope: &LayerScope<'_, '_>,
    track_index: usize,
    layers: &[Layer],
    media: &BTreeMap<MediaId, PrMedia>,
) -> Result<Vec<(Property, PropertyKeyframeTrack)>> {
    let id = match (
        transition.outgoing_clip.as_deref(),
        transition.incoming_clip.as_deref(),
    ) {
        (None, Some(id)) => id,
        _ => return Err(unsupported("Pop requires an incoming-only head")),
    };
    let clip = track
        .items
        .iter()
        .filter_map(PrVideoItem::media)
        .find(|clip| clip.id.as_deref() == Some(id))
        .ok_or_else(|| unsupported("Pop has no retained picture occurrence"))?;
    let source = media
        .get(&clip.media)
        .and_then(|media| media.video.as_ref())
        .ok_or_else(|| unsupported("Pop has no physical picture source"))?;
    ensure!(
        matches!(
            source.kind,
            PrMediaKind::Video { .. } | PrMediaKind::Still { .. }
        ) && scope.parent.is_none()
            && scope.on_document_clock
            && clip.blend_mode == PrBlendMode::Normal
            && clip.playback_rate == 1.0
            && clip.time_remap.is_none()
            && clip.crop.is_default()
            && clip.linear_wipe.is_none()
            && clip.opacity_mask.is_none()
            && clip.track_matte.is_none()
            && clip.active_transforms == 0
            && clip
                .animations
                .iter()
                .all(|animation| animation.property() == PrAnimatedProperty::Opacity),
        "Pop requires an unstaged flat picture with static 2D Motion on the document clock, Normal blend and unit playback"
    );
    ensure!(
        transition.cut_ticks == transition.start_ticks
            && transition.start_ticks == clip.start_ticks
            && transition.end_ticks <= clip.end_ticks,
        "Pop must meet the incoming clip start and end inside its active range"
    );
    let duration = transition
        .end_ticks
        .checked_sub(transition.start_ticks)
        .ok_or_else(|| unsupported("Pop duration overflow"))?;
    let frame = frame_rate.ticks_per_frame();
    ensure!(
        duration == 30 * frame || duration == 32 * frame,
        "Pop is measured only for 30 or 32 sequence frames"
    );
    ensure!(
        track
            .transitions
            .iter()
            .filter(|other| other.kind == PrVideoTransitionKind::FilmImpactPop
                && (other.outgoing_clip.as_deref() == Some(id)
                    || other.incoming_clip.as_deref() == Some(id)))
            .count()
            == 1,
        "multiple Pops target the same clip transform"
    );
    ensure!(
        track
            .transitions
            .iter()
            .filter(
                |other| other.kind == PrVideoTransitionKind::FilmImpactDissolve
                    && (other.outgoing_clip.as_deref() == Some(id)
                        || other.incoming_clip.as_deref() == Some(id))
            )
            .all(|other| other.outgoing_clip.as_deref() == Some(id)
                && other.incoming_clip.is_none()
                && other.start_ticks >= transition.end_ticks),
        "Pop may coexist only with a nonoverlapping tail dissolve on this owner"
    );
    let owner = scope
        .item_layers
        .get(&(track_index, clip.start_ticks))
        .copied()
        .ok_or_else(|| unsupported("Pop picture was omitted"))?;
    let layer_id = owner.id();
    let (transform, effects) = layers
        .iter()
        .find(|layer| layer.id() == layer_id)
        .and_then(|layer| match layer.data() {
            LayerData::Image(layer) => Some((&layer.transform, layer.effects.as_slice())),
            LayerData::Video(layer) => Some((&layer.transform, layer.effects.as_slice())),
            LayerData::Group(layer) if matches!(owner, ItemLayer::Stroke(_)) => {
                Some((&layer.transform, layer.effects.as_slice()))
            }
            _ => None,
        })
        .ok_or_else(|| unsupported("Pop needs an Image or Video transform owner"))?;
    ensure!(
        !effects.iter().any(|effect| matches!(
            effect.data(),
            EffectData::Identified {
                enabled: true,
                effect: EffectPayload::Known(
                    LayerEffect::DirectionalBlur { .. } | LayerEffect::GradientRamp { .. }
                ),
                ..
            }
        )),
        "Pop cannot animate a picture with enabled Directional Blur or Ramp; these effects require static Motion"
    );
    let position = [transform.position.x(), transform.position.y()];
    let [width, height] = source.display_dimensions();
    let center = transform_point(
        position,
        transform.anchor_point,
        transform.scale,
        transform.rotation,
        [f64::from(width) * 0.5, f64::from(height) * 0.5],
    );
    ensure!(
        center.iter().all(|value| value.is_finite()),
        "Pop center is nonfinite"
    );
    let origin = time_from_ticks(clip.start_ticks)?.as_millis();
    let mut tracks = Vec::with_capacity(4);
    for (property, axis, name) in [
        (PropType::ScaleX, 0, "film-impact-pop-scale-x"),
        (PropType::ScaleY, 1, "film-impact-pop-scale-y"),
        (PropType::PositionX, 0, "film-impact-pop-position-x"),
        (PropType::PositionY, 1, "film-impact-pop-position-y"),
    ] {
        let keys = CURVE
            .iter()
            .enumerate()
            .map(|(index, (phase, relative))| {
                let tick = i128::from(transition.start_ticks)
                    + (i128::from(duration) * i128::from(*phase) + 15) / 30;
                let tick =
                    i64::try_from(tick).map_err(|_| unsupported("Pop knot tick overflow"))?;
                let millis = time_from_ticks(tick)?
                    .as_millis()
                    .checked_sub(origin)
                    .and_then(|value| i64::try_from(value).ok())
                    .ok_or_else(|| unsupported("Pop local knot time overflow"))?;
                let value = if matches!(property, PropType::ScaleX | PropType::ScaleY) {
                    transform.scale[axis] * relative
                } else {
                    center[axis] + relative * (position[axis] - center[axis])
                };
                ensure!(value.is_finite(), "Pop knot is nonfinite");
                Ok(PropertyKeyframe::new(
                    keyframe_id(layer_id, name, index),
                    TimeOffset::from_millis(millis),
                    PropertyValue::Float(value),
                    PropertyKeyframeEasing::Linear,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        tracks.push((
            Property::new(layer_id, property),
            PropertyKeyframeTrack::new(keys)
                .map_err(|error| unsupported(format!("Pop geometry keys: {error}")))?,
        ));
    }
    Ok(tracks)
}

/// A transition half on one retained graphic's local clock. Several disjoint
/// halves share one geometry track, so a tail cannot replace its owner's head.
struct GraphicHalf<'a> {
    owner: fx_schema::LayerId,
    transform: &'a fx_schema::Transform,
    center: [f64; 2],
    samples: Vec<(i64, f64)>,
}

pub(super) fn import_graphics(
    track: &PrVideoTrack,
    frame_rate: FrameRate,
    scope: &LayerScope<'_, '_>,
    track_index: usize,
    layers: &[Layer],
    dynamics: &mut fx_schema::AnimationGraph,
    omissions: &mut Vec<crate::Omission>,
) {
    let mut owners: BTreeMap<fx_schema::LayerId, Vec<GraphicHalf<'_>>> = BTreeMap::new();
    let mut accepted = Vec::new();
    for transition in track.transitions.iter().filter(|transition| {
        transition.kind == PrVideoTransitionKind::FilmImpactPop
            && transition.outgoing_clip.is_some()
            && transition.incoming_clip.is_some()
    }) {
        match graphic_halves(transition, track, frame_rate, scope, track_index, layers) {
            Ok(halves) => {
                accepted.push(transition);
                for half in halves {
                    owners.entry(half.owner).or_default().push(half);
                }
            }
            Err(reason) => crate::omit(
                omissions,
                crate::OmissionScope::Feature,
                &transition.id,
                reason.to_string(),
            ),
        }
    }
    if accepted.is_empty() {
        return;
    }
    let result = publish_owner_tracks(&owners, dynamics);
    for transition in accepted {
        match &result {
            Ok(()) => crate::approximate(
                omissions,
                &transition.id,
                "Film Impact default two-sided graphic Pop retained as editable whole-block scale/position around the authored text-block origin; native ink-center pivot, fitted curve and early blur/fade remain approximate",
            ),
            Err(reason) => crate::omit(
                omissions,
                crate::OmissionScope::Feature,
                &transition.id,
                reason.to_string(),
            ),
        }
    }
}

/// Both halves of one two-sided graphic Pop, outgoing first.
fn graphic_halves<'a>(
    transition: &'a PrVideoTransition,
    track: &'a PrVideoTrack,
    frame_rate: FrameRate,
    scope: &LayerScope<'_, '_>,
    track_index: usize,
    layers: &'a [Layer],
) -> Result<[GraphicHalf<'a>; 2]> {
    ensure!(
        transition.outgoing_clip != transition.incoming_clip,
        "two-sided graphic Pop requires distinct outgoing and incoming owners"
    );
    let half = |incoming| {
        graphic_half(
            transition,
            incoming,
            track,
            frame_rate,
            scope,
            track_index,
            layers,
        )
    };
    Ok([half(false)?, half(true)?])
}

fn graphic_half<'a>(
    transition: &'a PrVideoTransition,
    incoming: bool,
    track: &'a PrVideoTrack,
    frame_rate: FrameRate,
    scope: &LayerScope<'_, '_>,
    track_index: usize,
    layers: &'a [Layer],
) -> Result<GraphicHalf<'a>> {
    let id = if incoming {
        &transition.incoming_clip
    } else {
        &transition.outgoing_clip
    }
    .as_deref()
    .ok_or_else(|| unsupported("two-sided graphic Pop has a missing owner"))?;
    let graphic = track
        .items
        .iter()
        .filter_map(PrVideoItem::graphic)
        .find(|graphic| graphic.id() == Some(id))
        .ok_or_else(|| {
            unsupported("two-sided Pop requires retained point-text graphics on both sides")
        })?;
    ensure!(
        scope.parent.is_none()
            && scope.on_document_clock
            && graphic.blend_mode == PrBlendMode::Normal
            && graphic
                .animations
                .iter()
                .all(|animation| animation.property() == PrAnimatedProperty::Opacity)
            && graphic
                .vector_motion
                .as_ref()
                .is_none_or(|motion| motion.animations.is_empty()),
        "graphic Pop requires static 2D geometry, Normal blend and the document clock"
    );
    // The group pops, while its clip Opacity mask's guide, its sibling,
    // stays in the sequence frame.
    ensure!(
        graphic.opacity_mask.is_none(),
        "graphic Pop of a graphic with a clip Opacity mask is unsupported: the mask's guide would not move with the graphic"
    );
    let placement = static_point_text(&graphic.objects).ok_or_else(|| {
        unsupported(
            "graphic Pop requires one static point-text object or complete-line block; shapes, independent object centers and keyed text topology are unsupported",
        )
    })?;
    let (start, end) = half_window(transition, incoming);
    if incoming {
        ensure!(
            start == graphic.start_ticks && end <= graphic.end_ticks,
            "incoming graphic Pop half must start at its owner cut and end inside its range"
        );
    } else {
        ensure!(
            end == graphic.end_ticks && start >= graphic.start_ticks,
            "outgoing graphic Pop half must end at its owner cut and start inside its range"
        );
    }
    let duration = end
        .checked_sub(start)
        .ok_or_else(|| unsupported("graphic Pop half duration overflow"))?;
    ensure!(
        duration > 0 && duration % frame_rate.ticks_per_frame() == 0,
        "graphic Pop half requires a positive whole number of sequence frames"
    );
    for other in track.transitions.iter().filter(|other| {
        !std::ptr::eq(*other, transition)
            && (other.outgoing_clip.as_deref() == Some(id)
                || other.incoming_clip.as_deref() == Some(id))
    }) {
        ensure!(
            other.kind == PrVideoTransitionKind::FilmImpactPop
                && other.outgoing_clip.is_some()
                && other.incoming_clip.is_some(),
            "graphic Pop has conflicting transition ownership on this graphic"
        );
        let (other_start, other_end) =
            half_window(other, other.incoming_clip.as_deref() == Some(id));
        ensure!(
            start >= other_end || other_start >= end,
            "graphic Pop head/tail windows overlap on the same owner"
        );
    }
    let owner = scope
        .item_layers
        .get(&(track_index, graphic.start_ticks))
        .ok_or_else(|| unsupported("graphic Pop owner was omitted"))?
        .id();
    let root = layers
        .iter()
        .find(|layer| layer.id() == owner)
        .ok_or_else(|| unsupported("graphic Pop has no retained common owner"))?;
    ensure!(
        !has_retained_shadow(root),
        "graphic Pop cannot animate a retained text shadow; shadow admission requires static unscaled geometry"
    );
    let transform = match root.data() {
        LayerData::Text(text) => &text.transform,
        LayerData::Group(group) => &group.transform,
        _ => {
            return Err(unsupported(
                "graphic Pop has no retained common Text/Group transform owner",
            ))
        }
    };
    // Source text's authored origin, including its native anchor and any kept
    // Vector Motion. FX's extra point-alignment anchor is deliberately excluded.
    let mut center = transform_point(
        placement.position,
        placement.anchor,
        [placement.scale; 2],
        placement.rotation,
        [0.0; 2],
    );
    if let Some(motion) = &graphic.vector_motion {
        center = transform_point(
            motion.position,
            motion.anchor,
            [motion.scale; 2],
            motion.rotation,
            center,
        );
    }
    ensure!(
        center.iter().all(|value| value.is_finite()),
        "graphic Pop pivot is nonfinite"
    );
    let origin = time_from_ticks(graphic.start_ticks)?.as_millis();
    let mut samples = CURVE
        .iter()
        .map(|(phase, relative)| {
            let offset = (i128::from(duration) * i128::from(*phase) + 15) / 30;
            let tick = if incoming {
                i128::from(start) + offset
            } else {
                i128::from(end) - offset
            };
            let tick =
                i64::try_from(tick).map_err(|_| unsupported("graphic Pop knot tick overflow"))?;
            let local = time_from_ticks(tick)?
                .as_millis()
                .checked_sub(origin)
                .and_then(|value| i64::try_from(value).ok())
                .ok_or_else(|| unsupported("graphic Pop local knot time overflow"))?;
            Ok((local, *relative))
        })
        .collect::<Result<Vec<_>>>()?;
    samples.sort_by_key(|(time, _)| *time);
    ensure!(
        samples.windows(2).all(|pair| pair[0].0 < pair[1].0),
        "graphic Pop half knots collapse after millisecond rounding"
    );
    Ok(GraphicHalf {
        owner,
        transform,
        center,
        samples,
    })
}

/// The timeline window of the incoming or outgoing half of a two-sided Pop.
fn half_window(transition: &PrVideoTransition, incoming: bool) -> (i64, i64) {
    if incoming {
        (transition.cut_ticks, transition.end_ticks)
    } else {
        (transition.start_ticks, transition.cut_ticks)
    }
}

/// The transform of a graphic's only object when it is static point text
/// without a background: one text object or one complete-line block.
fn static_point_text(
    objects: &[crate::schema::text::PrGraphicObject],
) -> Option<&crate::schema::text::PrTextTransform> {
    use crate::schema::text::{PrGraphicObject, PrTextDocument, PrTextFrame};
    let point = |document: &PrTextDocument| {
        matches!(document.frame, PrTextFrame::Point { .. }) && document.background.is_none()
    };
    match objects {
        [PrGraphicObject::Text(text)]
            if text.mask_source.is_none()
                && text.animations.is_empty()
                && text.source_text_keys.is_empty()
                && point(&text.document) =>
        {
            Some(&text.transform)
        }
        [PrGraphicObject::TextLines(text)]
            if text.animations.is_empty()
                && !text.documents.is_empty()
                && text.documents.iter().all(point) =>
        {
            Some(&text.transform)
        }
        _ => None,
    }
}

// Shadow offsets/kernels are measured in frame pixels under static geometry.
// Scaling a retained effect after admission would silently bypass that guard.
fn has_retained_shadow(layer: &Layer) -> bool {
    let (effects, children) = match layer.data() {
        LayerData::Text(text) => (text.effects.as_slice(), &[][..]),
        LayerData::Group(group) => (group.effects.as_slice(), group.layers.as_slice()),
        _ => return false,
    };
    effects.iter().any(|effect| {
        matches!(
            effect.data(),
            EffectData::Identified {
                enabled: true,
                effect: EffectPayload::Known(LayerEffect::DropShadow(shadow)),
                ..
            } if shadow.enabled
        )
    }) || children.iter().any(has_retained_shadow)
}

fn transform_point(
    position: [f64; 2],
    anchor: [f64; 2],
    scale: [f64; 2],
    rotation: f64,
    point: [f64; 2],
) -> [f64; 2] {
    let offset = [
        (point[0] - anchor[0]) * scale[0] / 100.0,
        (point[1] - anchor[1]) * scale[1] / 100.0,
    ];
    let (sin, cos) = rotation.to_radians().sin_cos();
    [
        position[0] + cos * offset[0] - sin * offset[1],
        position[1] + sin * offset[0] + cos * offset[1],
    ]
}

/// Construct and validate every owner track before publication. A temporary
/// graph makes shared transition halves atomic even if graph validation fails.
fn publish_owner_tracks(
    owners: &BTreeMap<fx_schema::LayerId, Vec<GraphicHalf<'_>>>,
    dynamics: &mut fx_schema::AnimationGraph,
) -> Result<()> {
    let tracks = owners
        .values()
        .map(|halves| graphic_geometry_tracks(halves))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        tracks
            .iter()
            .flatten()
            .all(|(property, _)| dynamics.property_animator(*property).is_none()),
        "graphic Pop conflicts with an existing geometry animator on its owner"
    );
    let mut candidate = dynamics.clone();
    for (property, keys) in tracks.into_iter().flatten() {
        candidate
            .set_property(
                property,
                fx_schema::PropertyAnimator::keyframes(keys),
                Vec::new(),
            )
            .map_err(super::map_animation_graph_error)?;
    }
    *dynamics = candidate;
    Ok(())
}

fn graphic_geometry_tracks(
    halves: &[GraphicHalf<'_>],
) -> Result<Vec<(Property, PropertyKeyframeTrack)>> {
    let first = &halves[0];
    let mut samples = halves
        .iter()
        .flat_map(|half| half.samples.iter().copied())
        .collect::<Vec<_>>();
    samples.sort_by_key(|(time, _)| *time);
    // Touching disjoint windows share their scale-one endpoint.
    samples.dedup();
    if samples[0].0 != 0 {
        samples.insert(0, (0, 1.0));
    }
    let position = [first.transform.position.x(), first.transform.position.y()];
    [
        (PropType::ScaleX, 0, "film-impact-pop-scale-x"),
        (PropType::ScaleY, 1, "film-impact-pop-scale-y"),
        (PropType::PositionX, 0, "film-impact-pop-position-x"),
        (PropType::PositionY, 1, "film-impact-pop-position-y"),
    ]
    .into_iter()
    .map(|(property, axis, name)| {
        let keys = samples
            .iter()
            .enumerate()
            .map(|(index, (time, relative))| {
                let value = if matches!(property, PropType::ScaleX | PropType::ScaleY) {
                    first.transform.scale[axis] * relative
                } else {
                    first.center[axis] + relative * (position[axis] - first.center[axis])
                };
                ensure!(value.is_finite(), "graphic Pop geometry knot is nonfinite");
                Ok(PropertyKeyframe::new(
                    keyframe_id(first.owner, name, index),
                    TimeOffset::from_millis(*time),
                    PropertyValue::Float(value),
                    PropertyKeyframeEasing::Linear,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok((
            Property::new(first.owner, property),
            PropertyKeyframeTrack::new(keys)
                .map_err(|error| unsupported(format!("graphic Pop geometry keys: {error}")))?,
        ))
    })
    .collect()
}
