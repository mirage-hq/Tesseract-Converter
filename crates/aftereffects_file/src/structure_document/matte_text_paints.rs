//! Preserve both text paints when the existing matte renderer samples a provider.
//! Its stroke-only path is activated by a separate editable stroke-only Text.
use std::collections::{HashMap, HashSet};

use super::{
    MAX_GROUP_DEPTH,
    animation_budget::{GeneratedKeyframeIdSize, PropertyTrackEstimate},
    reserve_ids, stored_layers,
};
use fx_schema::{
    BlendMode, FxItemId, LayerData, LayerId, PropType, PropertyTarget, TextLayer,
    animator::{
        AnimationGraphEntry, KeyframeId, PropertyAnimator, PropertyKeyframe, PropertyKeyframeTrack,
    },
};

fn owned(target: &PropertyTarget, layer: LayerId, items: &HashMap<FxItemId, FxItemId>) -> bool {
    target.layer_id() == Some(layer)
        || target
            .fx_item_id()
            .is_some_and(|id| items.contains_key(&id))
}

fn copy_text(
    text: &TextLayer,
    entries: &[AnimationGraphEntry],
    next: &mut u64,
    budget: &mut super::animation_budget::AnimationBudget,
) -> Result<(TextLayer, Vec<AnimationGraphEntry>, usize), String> {
    if text.blend_mode != BlendMode::Normal
        || text.transform.opacity.value() != 100.
        || !text.effects.is_empty()
        || !text.masks.is_empty()
        || text.track_matte.is_some()
        || text.path_options.is_some()
        || text.source_text.fill_color[3] != 1.
        || text
            .source_text
            .stroke_color
            .is_none_or(|color| color[3] != 1.)
        || text.animators.iter().any(|animator| {
            !animator.wiggly_selectors.is_empty()
                || animator.opacity.is_some_and(|opacity| opacity != 100.)
        })
    {
        return Err("combined text paints require Normal, opaque paints and neutral opacity with no text-local masks/effects/path or wiggly selectors".into());
    }
    let mut stroke = text.clone();
    stroke.id =
        LayerId::new(reserve_ids(next, 1).ok_or("text paint identity allocation exhausted")?);
    stroke.source_text.apply_fill = false;
    let mut items = HashMap::new();
    if let Some(anchor) = &mut stroke.anchor_options {
        let id =
            FxItemId::new(reserve_ids(next, 1).ok_or("text anchor identity allocation exhausted")?);
        items.insert(anchor.id, id);
        anchor.id = id;
    }
    if let Some(variations) = &mut stroke.source_text.font_variations {
        let id = FxItemId::new(
            reserve_ids(next, 1).ok_or("text variation identity allocation exhausted")?,
        );
        if items.insert(variations.id(), id).is_some() {
            return Err("duplicate text variation identity".into());
        }
        variations.remap_id(id);
    }
    for animator in &mut stroke.animators {
        let id = FxItemId::new(
            reserve_ids(next, 1).ok_or("text animator identity allocation exhausted")?,
        );
        if items.insert(animator.id, id).is_some() {
            return Err("duplicate text animator identity".into());
        }
        animator.id = id;
        for selector in &mut animator.selectors {
            let id = FxItemId::new(
                reserve_ids(next, 1).ok_or("text selector identity allocation exhausted")?,
            );
            if items.insert(selector.id, id).is_some() {
                return Err("duplicate text selector identity".into());
            }
            selector.id = id;
        }
    }
    let mut copies = Vec::new();
    let mut reservation = 0_usize;
    for (entry_index, entry) in entries.iter().enumerate() {
        if !owned(&entry.target, text.id, &items) {
            if entry
                .dependencies
                .iter()
                .any(|target| owned(target, text.id, &items))
                || entry
                    .random_seed_target
                    .as_ref()
                    .is_some_and(|target| owned(target, text.id, &items))
                || entry
                    .layer_refs
                    .values()
                    .any(|reference| reference.layer_id == text.id)
            {
                return Err("text paint scope has external graph references".into());
            }
            continue;
        }
        if !entry.dependencies.is_empty()
            || entry.random_seed_target.is_some()
            || !entry.layer_refs.is_empty()
        {
            return Err("text paint animation requires independent native keyframes".into());
        }
        let target = match &entry.target {
            PropertyTarget::LayerProperty(property) => {
                if matches!(
                    property.property_type(),
                    PropType::Opacity | PropType::FillEnabled | PropType::StrokeEnabled
                ) {
                    return Err(
                        "animated text opacity or paint switches are outside split-paint profile"
                            .into(),
                    );
                }
                PropertyTarget::layer(stroke.id, property.property_type())
            }
            PropertyTarget::FxItemProperty(target) => {
                if target.property_name() == "opacity" {
                    return Err("animated character opacity is outside split-paint profile".into());
                }
                PropertyTarget::fx_item(items[&target.item_id()], target.property_name())
            }
            PropertyTarget::EffectProperty(_) => unreachable!("text has no local effects"),
        };
        if !matches!(
            entry.animator.data(),
            fx_schema::animator::AnimatorData::Keyframes {
                enabled: true,
                disabled_value: None,
                ..
            }
        ) {
            return Err("disabled native text keys are outside split-paint profile".into());
        }
        let track = entry
            .animator
            .keyframe_track()
            .ok_or("text paint animation requires native keyframe tracks")?;
        let mut estimate = PropertyTrackEstimate::default();
        for (key_index, key) in track.keyframes().iter().enumerate() {
            let id = GeneratedKeyframeIdSize::new(format_args!(
                "aep-matte-paint-{}-{entry_index}-{key_index}",
                stroke.id
            ))
            .map_err(|e| e.to_string())?;
            estimate.push_copied(key, &id).map_err(|e| e.to_string())?;
        }
        let bytes = estimate
            .entry_reservation_bytes(&target)
            .map_err(|e| e.to_string())?;
        budget.reserve(bytes).map_err(|e| e.to_string())?;
        reservation = reservation
            .checked_add(bytes)
            .ok_or("text paint animation reservation overflow")?;
        let keys = track
            .keyframes()
            .iter()
            .enumerate()
            .map(|(key_index, key)| {
                PropertyKeyframe::new(
                    KeyframeId::new(format!(
                        "aep-matte-paint-{}-{entry_index}-{key_index}",
                        stroke.id
                    )),
                    key.layer_time(),
                    key.value().clone(),
                    key.easing(),
                )
                .with_spatial_tangents(key.spatial_in_tangent(), key.spatial_out_tangent())
            })
            .collect();
        let mut copy = entry.clone();
        copy.target = target;
        copy.animator = PropertyAnimator::keyframes(
            PropertyKeyframeTrack::new(keys).map_err(|e| e.to_string())?,
        );
        copies.push(copy);
    }
    Ok((stroke, copies, reservation))
}

pub(super) fn split(
    root: &mut LayerData,
    entries: &[AnimationGraphEntry],
    next: &mut u64,
    depth: usize,
    budget: &mut super::animation_budget::AnimationBudget,
) -> Result<(Vec<AnimationGraphEntry>, usize), String> {
    struct PaintWalk<'a> {
        entries: &'a [AnimationGraphEntry],
        next: &'a mut u64,
        budget: &'a mut super::animation_budget::AnimationBudget,
        copied: Vec<AnimationGraphEntry>,
        reservation: usize,
        seen: HashSet<LayerId>,
    }
    impl PaintWalk<'_> {
        fn walk(&mut self, layer: &mut LayerData, depth: usize) -> Result<(), String> {
            if depth >= MAX_GROUP_DEPTH || !self.seen.insert(layer.id()) {
                return Err("text paint source exceeds depth or has duplicate identities".into());
            }
            let LayerData::Group(group) = layer else {
                return Ok(());
            };
            let mut children = Vec::with_capacity(group.layers.len());
            for stored in &group.layers {
                let mut child = stored.data().clone();
                if let LayerData::Text(text) = &mut child
                    && text.source_text.apply_fill
                    && text.source_text.apply_stroke
                {
                    if depth + 1 >= MAX_GROUP_DEPTH || !self.seen.insert(text.id) {
                        return Err(
                            "text paint child exceeds depth or has duplicate identity".into()
                        );
                    }
                    let (stroke, tracks, bytes) =
                        copy_text(text, self.entries, self.next, self.budget)?;
                    text.source_text.apply_stroke = false;
                    self.reservation = self
                        .reservation
                        .checked_add(bytes)
                        .ok_or("text paint animation reservation overflow")?;
                    self.copied.extend(tracks);
                    if text.source_text.stroke_over_fill {
                        children.push(LayerData::Text(stroke));
                        children.push(child);
                    } else {
                        children.push(child);
                        children.push(LayerData::Text(stroke));
                    }
                } else {
                    self.walk(&mut child, depth + 1)?;
                    children.push(child);
                }
            }
            group.layers = stored_layers(children).map_err(|e| e.to_string())?;
            Ok(())
        }
    }
    let mut state = PaintWalk {
        entries,
        next,
        budget,
        copied: Vec::new(),
        reservation: 0,
        seen: HashSet::new(),
    };
    state.walk(root, depth)?;
    fx_schema::AnimationGraph::from_entries(state.copied.clone()).map_err(|e| e.to_string())?;
    Ok((state.copied, state.reservation))
}
