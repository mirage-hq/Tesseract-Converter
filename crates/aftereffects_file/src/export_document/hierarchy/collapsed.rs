//! The independently Adobe-probed 2D vector boundary, not a raster crop.
//! See the native collapse/mask cases in the support ledger. No 3D scene,
//! external dependency, media sampler, or nonidentity clock crosses it.
//! [`text_only`] is the separate, not yet Adobe-probed plain Text boundary.

use std::collections::BTreeSet;

use fx_schema::effect::{EffectData, EffectPayload, EffectRecord, LayerEffect};
use fx_schema::{GroupLayer, Layer, LayerData, LayerId};

use super::Bounds;

pub(super) fn eligible(
    group: &GroupLayer,
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> bool {
    // A vector source keeps the canonical identity clock. The export caller
    // allows its checked short visibility plan, but rejects nonidentity playback.
    if !super::super::playback_is_identity(&group.playback)
        || !collapsible_owner(group, dynamics)
        || !effects_supported(&group.effects)
    {
        return false;
    }
    let mut ids = BTreeSet::from([group.id]);
    collect_ids(&group.layers, &mut ids);
    group
        .layers
        .iter()
        .all(|layer| vector(layer, dynamics, &ids))
}

/// Plain 2D Text in plain Groups. FX Text has no glyph bounds, so a required
/// precomposition cannot prove a finite canvas; collapse keeps the Text in
/// parent space instead. Disabled or diagnosed omitted source effects create
/// no native effects; every retained effect, mask, matte, blend mode, motion
/// blur or projection still rejects collapse, as does mixed content. Nested Group clocks are allowed:
/// each nested Group is classified on its own. The owner clock is validated
/// upstream: classification admits only a full-span identity clock, and the
/// export caller moves a supported occurrence clock onto the occurrence record.
pub(super) fn text_only(
    group: &GroupLayer,
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> bool {
    if !collapsible_owner(group, dynamics)
        || !native_effects_absent(&group.effects)
        || group.layers.is_empty()
    {
        return false;
    }
    let mut ids = BTreeSet::from([group.id]);
    collect_ids(&group.layers, &mut ids);
    group
        .layers
        .iter()
        .all(|layer| plain_text(layer, dynamics, &ids))
}

/// Owner switches that native collapse transformations keep exact. The owner
/// clock rule differs by source, so it is not part of this shared check.
fn collapsible_owner(
    group: &GroupLayer,
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> bool {
    !(group.motion_blur
        || super::has_skew(group)
        || group.transform.opacity.value() != 100.0
        || dynamics.iter().any(|entry| {
            entry.target.as_property().is_some_and(|target| {
                target.layer_id() == group.id
                    && target.property_type() == fx_schema::PropType::Opacity
            })
        })
        || group.track_matte.is_some()
        || group.blend_mode != Default::default()
        || super::super::transform3d::requires_native_3d(dynamics, &group.transform, group.id))
}

/// Validate the complete input geometry before calling this function. This is
/// the support of the *masked output*, never permission to crop Glow's input.
/// Effects on the mask owner run after the mask and therefore invalidate it.
pub(super) fn mask_output(
    group: &GroupLayer,
    dynamics: &crate::export_document::AnimationIndex<'_>,
) -> Option<Bounds> {
    if !group.effects.is_empty() || !eligible(group, dynamics) {
        return None;
    }
    super::finite_mask_gate(&group.masks, dynamics).ok()
}

fn collect_ids(layers: &[Layer], ids: &mut BTreeSet<LayerId>) {
    for layer in layers {
        ids.insert(layer.id());
        match layer.data() {
            LayerData::Group(group) => collect_ids(&group.layers, ids),
            LayerData::BooleanOperation(boolean) => collect_ids(&boolean.layers, ids),
            _ => {}
        }
    }
}

fn vector(
    layer: &Layer,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    ids: &BTreeSet<LayerId>,
) -> bool {
    let (transform, motion_blur, effects, masks, matte, children) = match layer.data() {
        LayerData::Rect(value) => (
            &value.transform,
            value.motion_blur,
            &value.effects,
            &value.masks,
            &value.track_matte,
            &[][..],
        ),
        LayerData::Shape(value) => (
            &value.transform,
            value.motion_blur,
            &value.effects,
            &value.masks,
            &value.track_matte,
            &[][..],
        ),
        LayerData::Group(value)
            if super::super::playback_is_identity(&value.playback) && value.fills.is_empty() =>
        {
            (
                &value.transform,
                value.motion_blur,
                &value.effects,
                &value.masks,
                &value.track_matte,
                value.layers.as_slice(),
            )
        }
        LayerData::BooleanOperation(value) => (
            &value.transform,
            value.motion_blur,
            &value.effects,
            &value.masks,
            &value.track_matte,
            value.layers.as_slice(),
        ),
        _ => return false,
    };
    !motion_blur
        && !super::super::transform3d::requires_native_3d(dynamics, transform, layer.id())
        && layer.parent_id().is_none_or(|parent| ids.contains(&parent))
        && matte
            .as_ref()
            .is_none_or(|matte| ids.contains(&matte.layer))
        && effects_supported(effects)
        && (masks.is_empty() || super::finite_mask_gate(masks, dynamics).is_ok())
        && children.iter().all(|child| vector(child, dynamics, ids))
}

fn plain_text(
    layer: &Layer,
    dynamics: &crate::export_document::AnimationIndex<'_>,
    ids: &BTreeSet<LayerId>,
) -> bool {
    let (transform, blend_mode, motion_blur, effects, masks, matte, children) = match layer.data() {
        LayerData::Text(value) => (
            &value.transform,
            value.blend_mode,
            value.motion_blur,
            &value.effects,
            &value.masks,
            &value.track_matte,
            &[][..],
        ),
        LayerData::Group(value) if value.fills.is_empty() && !value.layers.is_empty() => (
            &value.transform,
            value.blend_mode,
            value.motion_blur,
            &value.effects,
            &value.masks,
            &value.track_matte,
            value.layers.as_slice(),
        ),
        _ => return false,
    };
    blend_mode == Default::default()
        && !motion_blur
        && native_effects_absent(effects)
        && masks.is_empty()
        && matte.is_none()
        && !super::super::transform3d::requires_native_3d(dynamics, transform, layer.id())
        && layer.parent_id().is_none_or(|parent| ids.contains(&parent))
        && children
            .iter()
            .all(|child| plain_text(child, dynamics, ids))
}

fn native_effects_absent(effects: &[EffectRecord]) -> bool {
    effects.iter().all(|effect| {
        matches!(effect.data(), EffectData::Identified { enabled: false, .. })
            || super::super::effects::unmapped_warning(effect).is_some()
    })
}

fn effects_supported(effects: &[EffectRecord]) -> bool {
    effects.iter().all(|effect| {
        // These effects are explicitly omitted by the existing converter with
        // diagnostics; they do not become newly implemented through collapse.
        if super::super::effects::unmapped_warning(effect).is_some() {
            return true;
        }
        let payload = match effect.data() {
            EffectData::Identified { enabled: false, .. } => return true,
            EffectData::Identified { effect, .. } | EffectData::Legacy(effect) => effect,
        };
        matches!(
            payload,
            EffectPayload::Known(
                LayerEffect::Glow { .. }
                    | LayerEffect::Exposure { .. }
                    | LayerEffect::HueSaturation { .. }
                    | LayerEffect::GaussianBlur { .. }
            )
        )
    })
}
