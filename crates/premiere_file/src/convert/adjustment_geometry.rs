//! Editable Geometry2 over an adjustment's composed lower picture.
//! Intrinsic adjustment Motion remains coverage, never the picture transform.
use super::{
    background::{identity_transform, plain_group},
    nested::LayerScope,
    premiere_to_tesseract::{
        crop_rect, guide_layer, guide_mask, into_stage, set_tracks, staged_video_transform,
        tick_range, transform_stage_tracks,
    },
    tesseract_to_premiere::{layer_animations, Guide, MotionHost},
};
use crate::{
    approximate,
    error::{unsupported, Result},
    omit,
    schema::{
        PrBlendMode, PrEffect, PrEffectParams, PrSequence, PrStaticCrop, PrStaticTransform,
        PrVideoOccurrence, TRANSFORM_ANCHOR_POINT, TRANSFORM_POSITION, TRANSFORM_ROTATION,
        TRANSFORM_SCALE_HEIGHT,
    },
    Omission, OmissionScope,
};
use fx_schema::{
    AnimationGraph, BlendMode, FxItemId, GroupLayer, Layer, LayerData, LayerId, Time,
    TimeRangeProperty,
};

pub(super) const STAGE_NAME: &str = "Premiere adjustment Geometry2 ";
pub(super) const APPROXIMATION: &str = "adjustment Geometry2 approximated as editable composed Group G x M with a nonpainting sequence-canvas Add guide; native raster sampling and transparent edge alpha differ; keys and media clocks retain integer-millisecond precision";
pub(super) const ROOT_ADJUSTMENT_APPROXIMATION: &str = "root Adjustment above a Geometry2 stage uses ordinary document-plane/backdrop semantics; its effects also act on exposed exterior/root black; if the whole Geometry2 stage and its lower picture are omitted on export, the remaining Adjustment does not restore that input; native backdrop and alpha fidelity remain unmeasured";

/// Geometry2 first in apply order; the suffix remains on its ordinary host.
struct Admitted<'a> {
    geometry: &'a PrEffect,
    suffix: &'a [PrEffect],
}

fn admitted<'a>(
    clip: &'a PrVideoOccurrence,
    scope: &LayerScope<'_, '_>,
    canvas: [u32; 2],
) -> std::result::Result<Admitted<'a>, &'static str> {
    let Some((effect, suffix)) = clip.effects.split_first() else {
        return Err("requires adjustment Geometry2");
    };
    if !matches!(effect.params, PrEffectParams::AdjustmentGeometry2(_))
        || suffix.iter().any(|effect| {
            effect.mask.is_some()
                || !match effect.params {
                    PrEffectParams::GaussianBlur(_)
                    | PrEffectParams::FilmImpactBlur(_)
                    | PrEffectParams::Sharpen(_)
                    | PrEffectParams::Noise { .. }
                    | PrEffectParams::ModernNoise { .. }
                    | PrEffectParams::BrightnessContrast(_)
                    | PrEffectParams::LumetriExposure(_)
                    | PrEffectParams::LumetriTemperature(_)
                    | PrEffectParams::LumetriTint(_)
                    | PrEffectParams::LumetriSaturation(_)
                    | PrEffectParams::LumetriVignette(_) => true,
                    PrEffectParams::CornerPin(_)
                    | PrEffectParams::DirectionalBlur(_)
                    | PrEffectParams::FilmImpactDirectionalBlur(_)
                    | PrEffectParams::Levels(_)
                    | PrEffectParams::Offset(_)
                    | PrEffectParams::Invert(_)
                    | PrEffectParams::FindEdges(_)
                    | PrEffectParams::Tint(_)
                    | PrEffectParams::BlackWhite
                    | PrEffectParams::Ramp(_)
                    | PrEffectParams::Mosaic(_)
                    | PrEffectParams::Replicate(_)
                    | PrEffectParams::Posterize(_)
                    | PrEffectParams::AlphaGlow { .. }
                    | PrEffectParams::LegacyLuma { .. }
                    | PrEffectParams::LensDistortion(_)
                    | PrEffectParams::PosterizeTime { .. }
                    | PrEffectParams::Transform(_)
                    | PrEffectParams::AdjustmentGeometry2(_) => false,
                }
        })
    {
        return Err("requires Geometry2 alone or first before mapped Lumetri, Brightness & Contrast, Gaussian Blur, Sharpen or Noise; prefix/mixed and other sibling-effect stacks are unmeasured");
    }
    if !suffix.is_empty() && (scope.parent.is_some() || canvas != scope.document_canvas) {
        return Err("post-Geometry2 effects require a root sequence matching the document canvas");
    }
    if !clip.enabled
        || !effect.enabled
        || effect.mask.is_some()
        || clip.transform != PrStaticTransform::default()
        || clip.opacity != 100.0
        || clip.blend_mode != PrBlendMode::Normal
        || !clip.animations.is_empty()
        || clip.playback_rate != 1.0
        || clip.time_remap.is_some()
        || !clip.crop.is_default()
        || clip.linear_wipe.is_some()
        || clip.opacity_mask.is_some()
        || clip.track_matte.is_some()
        || clip
            .source_effects
            .as_ref()
            .is_some_and(|source| !source.effects.is_empty())
    {
        return Err("requires neutral intrinsic Motion, Opacity and blend, unit clock and no other masks or source effects");
    }
    if let Some(reason) = affine_reason(effect) {
        return Err(reason);
    }
    Ok(Admitted {
        geometry: effect,
        suffix,
    })
}

/// The same bounded native control class on import and current-control export.
fn affine_reason(effect: &PrEffect) -> Option<&'static str> {
    let PrEffectParams::AdjustmentGeometry2(t) = &effect.params else {
        return Some("requires adjustment Geometry2");
    };
    let keyed = [
        TRANSFORM_ANCHOR_POINT.id,
        TRANSFORM_POSITION.id,
        TRANSFORM_SCALE_HEIGHT.id,
        TRANSFORM_ROTATION.id,
    ];
    (!t.uniform_scale
        || t.scale_height <= 0.0
        || t.skew != 0.0
        || t.skew_axis != 0.0
        || t.opacity != 100.0
        || t.shutter_angle != 0.0
        || t.bicubic_sampling
        || effect.animations.iter().any(|animation| {
            !keyed.contains(&animation.param.id)
                || (animation.param.id == TRANSFORM_SCALE_HEIGHT.id
                    && animation.keys.scalar().is_none_or(|keys| keys.iter().any(|key| key.value <= 0.0)))
        }))
        .then_some("requires positive uniform Scale, Position, Anchor and Rotation only, no Skew, opacity mix or motion blur, and bilinear Sampling")
}

fn report_control_approximations(
    effect: &PrEffect,
    record: &str,
    reports: &mut dyn crate::export_loss::OmissionSink,
) {
    for animation in &effect.animations {
        if animation
            .keys
            .point()
            .is_some_and(|keys| crate::schema::spatial::curved_segment(keys).is_some())
        {
            approximate(reports, record, format!("Geometry2 {} curved spatial path retains editable tangents but FX traverses parametrically rather than native constant-speed distance", animation.param.label));
        }
        if animation.param.id == TRANSFORM_ANCHOR_POINT.id {
            approximate(reports, record, "Geometry2 animated Anchor retained as editable point tracks; independent native rendered Anchor animation remains unmeasured");
        }
    }
}

/// Wrap only complete, coextensive lower occurrences. A crossing occurrence is
/// left intact rather than split/baked or accidentally hidden outside the window.
pub(super) fn wrap(
    project: &PrSequence,
    adjustment_id: LayerId,
    clip: &PrVideoOccurrence,
    scope: &mut LayerScope<'_, '_>,
    layers: &mut Vec<Layer>,
    dynamics: &mut AnimationGraph,
    omissions: &mut Vec<Omission>,
) -> Result<()> {
    if !clip
        .effects
        .iter()
        .any(|effect| matches!(effect.params, PrEffectParams::AdjustmentGeometry2(_)))
    {
        return Ok(());
    }
    let prepare = || -> Result<_> {
        let admitted = admitted(clip, scope, project.dimensions()).map_err(unsupported)?;
        let effect = admitted.geometry;
        let position = layers
            .iter()
            .position(|layer| layer.id() == adjustment_id)
            .ok_or_else(|| unsupported("ordinary adjustment was omitted; no safe suffix owner or lower-composite boundary remains"))?;
        let separate_suffix = !admitted.suffix.is_empty();
        // Even an empty/disabled/rejected mapped suffix keeps its ordinary owner.
        // Geometry2 alone retains its existing layer identity and tree shape.
        let group_id = if separate_suffix {
            LayerId::new(*scope.next_index as u64 + 1)
        } else {
            adjustment_id
        };
        let allocated = if separate_suffix { 3 } else { 2 };
        let guide_id = LayerId::new(*scope.next_index as u64 + allocated - 1);
        let mask_id = FxItemId::new(*scope.next_index as u64 + allocated);
        let window = tick_range(clip.start_ticks, clip.end_ticks)?;
        let end = window.start.as_millis() + window.duration.as_millis();
        let lower: Vec<_> = layers[position + 1..]
            .iter()
            .filter(|layer| {
                let range = layer.active_range();
                range.start.as_millis() < end
                    && range.start.as_millis() + range.duration.as_millis()
                        > window.start.as_millis()
            })
            .collect();
        if lower.is_empty() || lower.iter().any(|layer| layer.active_range() != window) {
            return Err(unsupported("requires complete lower occurrences sharing the adjustment interval; crossing/partial lower windows retain their original clocks and stack"));
        }
        // The admitted lower list must not depend on any upper/disjoint sibling.
        let lower_ids: std::collections::BTreeSet<_> =
            lower.iter().map(|layer| layer.id()).collect();
        let selected: Vec<_> = lower.iter().map(|layer| (*layer).clone()).collect();
        let outside: Vec<_> = layers
            .iter()
            .filter(|layer| !lower_ids.contains(&layer.id()) && layer.id() != adjustment_id)
            .cloned()
            .collect();
        if let Some(reason) =
            super::adjustment_wipe::boundary_reason(&selected, &outside, scope.nesting_depth)
        {
            return Err(unsupported(reason));
        }
        let PrEffectParams::AdjustmentGeometry2(transform) = &effect.params else {
            return Err(unsupported("missing Geometry2 affine controls"));
        };
        let tracks = transform_stage_tracks(
            effect,
            transform,
            project.dimensions(),
            clip.in_ticks,
            group_id,
        )?;
        let mut children = lower
            .iter()
            .map(|layer| into_stage(layer, group_id, window.duration, false))
            .collect::<Result<Vec<_>>>()?;
        let local = TimeRangeProperty::new(Time::ZERO, window.duration);
        children.push(Layer::from_data(&LayerData::Rect(guide_layer(
            guide_id,
            "Adjustment sequence canvas".into(),
            Some(group_id),
            local,
            identity_transform(),
            crop_rect(&PrStaticCrop::default(), project.dimensions()),
        )))?);
        let mut group = plain_group(
            group_id,
            format!("{STAGE_NAME}composite"),
            window,
            staged_video_transform(transform, project.dimensions())?,
            children,
        )?;
        group.parent = scope.parent;
        group.masks.push(guide_mask(mask_id, guide_id, 0.0));
        let group = Layer::from_data(&LayerData::Group(group))?;
        let mut staged_dynamics = dynamics.clone();
        set_tracks(&mut staged_dynamics, tracks)?;
        Ok((
            effect,
            position,
            separate_suffix,
            allocated,
            lower_ids,
            group,
            staged_dynamics,
        ))
    };
    let (effect, position, separate_suffix, allocated, lower_ids, group, staged_dynamics) =
        match prepare() {
            Ok(prepared) => prepared,
            Err(error) => {
                omit(omissions, OmissionScope::Feature, clip.record(), format!("adjustment Geometry2 not imported: {error}; existing layers and tracks were left unchanged"));
                return Ok(());
            }
        };
    // Publish ownership and tracks only after all fallible preparation succeeds.
    *scope.next_index += allocated as usize;
    layers.retain(|layer| !lower_ids.contains(&layer.id()));
    if separate_suffix {
        layers.insert(position + 1, group);
    } else {
        layers[position] = group;
    }
    *dynamics = staged_dynamics;
    if separate_suffix {
        approximate(omissions, clip.record(), ROOT_ADJUSTMENT_APPROXIMATION);
    }
    approximate(omissions, clip.record(), APPROXIMATION);
    report_control_approximations(effect, clip.record(), omissions);
    Ok(())
}

/// Ordinary nest publication with an editable flagged adjustment above its
/// exported lower children. The guide is consumed, never emitted as paint.
pub(super) fn place_exported(
    group: &GroupLayer,
    guide_id: LayerId,
    effect: PrEffect,
    tracks: &mut Vec<crate::schema::PrVideoTrack>,
    context: &mut super::nested::LayerExport<'_, '_>,
    omissions: &mut dyn crate::export_loss::OmissionSink,
) -> Result<()> {
    let local = TimeRangeProperty::new(Time::ZERO, group.playback.input_range().duration);
    let adjustment = fx_schema::AdjustmentLayer {
        id: guide_id,
        name: "Adjustment Geometry2".into(),
        description: String::new(),
        is_hidden: false,
        parent: Some(group.id),
        blend_mode: BlendMode::Normal,
        track_matte: None,
        masks: Vec::new(),
        active_range: local,
        effects: Vec::new(),
        transform: identity_transform(),
    };
    context.boundary = Some(context.packer.begin_boundary(context.container, guide_id)?);
    let Some((mut clip, media)) = super::adjustment::export_adjustment_layer(
        &adjustment,
        &[],
        context,
        omissions,
        &group.name,
    )?
    else {
        return Err(unsupported("admitted Geometry2 adjustment did not export"));
    };
    clip.effects.push(effect);
    context.media.entry(clip.media.clone()).or_insert(media);
    super::tesseract_to_premiere::place_item(
        tracks,
        crate::schema::PrVideoItem::Media(clip),
        0,
        context,
    )?;
    Ok(())
}

pub(super) fn is_stage(group: &GroupLayer) -> bool {
    group.name.starts_with(STAGE_NAME)
}

/// Authored Group opacity belongs to ordinary nest Motion/Opacity, not the
/// unmeasured native adjustment effect's mix parameter.
pub(super) fn has_authored_opacity(group: &GroupLayer, dynamics: &AnimationGraph) -> bool {
    group.transform.opacity.value() != 100.0
        || layer_animations(dynamics, group.id)
            .any(|(property, _)| property == fx_schema::PropType::Opacity)
}

/// Validate the unchanged coverage guide for either native Geometry2 or its
/// ordinary moved-nest opacity fallback. The guide never becomes paint.
pub(super) fn stage_guide(
    group: &GroupLayer,
    dynamics: &AnimationGraph,
    canvas: [u32; 2],
) -> std::result::Result<LayerId, String> {
    let invalid = || {
        "adjustment Geometry2 stage requires neutral group fields, a unit local clock and an unchanged nonpainting whole-canvas Add guide".to_owned()
    };
    if group.blend_mode != BlendMode::Normal
        || group.track_matte.is_some()
        || !group.effects.is_empty()
        || super::tesseract_to_premiere::has_background(group)
        || group.transform.skew != 0.0
        || group
            .layers
            .iter()
            .any(|layer| layer.parent_id() != Some(group.id))
        || !super::timing::is_plain_group_playback(&group.playback)
    {
        return Err(invalid());
    }
    let [mask] = group.masks.as_slice() else {
        return Err(invalid());
    };
    let guide_id = mask.layer.ok_or_else(invalid)?;
    if *mask != guide_mask(mask.id, guide_id, 0.0)
        || layer_animations(dynamics, guide_id).next().is_some()
        || dynamics
            .entries()
            .iter()
            .any(|entry| entry.target.fx_item_id() == Some(mask.id))
    {
        return Err(invalid());
    }
    let guide = group
        .layers
        .iter()
        .find(|layer| layer.id() == guide_id)
        .ok_or_else(invalid)?;
    let LayerData::Rect(guide) = guide.data() else {
        return Err(invalid());
    };
    let local = TimeRangeProperty::new(Time::ZERO, group.playback.input_range().duration);
    if Guide::Rect(guide)
        .unsupported("adjustment Geometry2", Some(group.id), local)
        .is_some()
        || guide.transform != identity_transform()
        || guide.rect != crop_rect(&PrStaticCrop::default(), canvas)
    {
        return Err(invalid());
    }
    Ok(guide_id)
}

/// Read the *current* editable controls/guide. No source XML or imported native
/// placement is retained. The ordinary native nest holds the lower children and
/// an actual adjustment Geometry2; its outer placement stays at identity.
pub(super) fn export_stage(
    group: &GroupLayer,
    dynamics: &AnimationGraph,
    canvas: [u32; 2],
    source_in: i64,
    reports: &mut Vec<Omission>,
) -> std::result::Result<(LayerId, PrEffect), String> {
    let guide_id = stage_guide(group, dynamics, canvas)?;
    if has_authored_opacity(group, dynamics) {
        return Err(
            "Group opacity requires an ordinary moved nest, not adjustment Geometry2 opacity mix"
                .into(),
        );
    }
    let effect = super::effects::export_adjustment_geometry2(
        MotionHost {
            id: group.id,
            transform: &group.transform,
        },
        dynamics,
        canvas,
        source_in,
        &group.name,
        reports,
    )?;
    if let Some(reason) = affine_reason(&effect) {
        return Err(reason.into());
    }
    approximate(reports, &group.name, APPROXIMATION);
    report_control_approximations(&effect, &group.name, reports);
    Ok((guide_id, effect))
}
