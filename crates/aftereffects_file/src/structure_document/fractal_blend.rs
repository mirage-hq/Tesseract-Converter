//! Fractal generators combined with editable source-stage blends.

use fx_schema::animator::AnimationGraphEntry;

use fx_schema::{
    BlendMode, EffectData, EffectPayload, EffectRecord, GroupLayer, LayerData, LayerEffect,
    LayerId, PercentageProperty, TimeRangeProperty,
};

use super::animation_budget::{AnimationBudget, committed_entry_reservation_bytes};
use super::{MAX_GROUP_DEPTH, group, reserve_ids, shapes::OutputBudget, stored_layers, transform};
use crate::structure::SolidSource;

/// The identified generator stays separate until all stage wrappers validate.
pub(super) struct Stage {
    pub native_ordinal: usize,
    pub generator: EffectRecord,
    /// Validated parent-clock tracks, reserved and published only with the wrappers.
    pub animations: Vec<AnimationGraphEntry>,
    pub blend_mode: BlendMode,
    pub opacity: PercentageProperty,
}

pub(super) struct Visibility {
    pub range: TimeRangeProperty,
    pub hidden: bool,
}

pub(super) struct Context<'a> {
    pub ordinals: &'a [usize],
    pub size: [u16; 2],
    pub visibility: Visibility,
    /// Existing Groups outside this occurrence, including transform ancestors.
    pub parent_depth: usize,
}

pub(super) struct State<'a> {
    pub next: &'a mut u64,
    pub budget: &'a mut OutputBudget,
    pub animation_budget: &'a mut AnimationBudget,
    pub animations: &'a mut Vec<AnimationGraphEntry>,
}

pub(super) fn apply(
    owner: &mut GroupLayer,
    stages: &[Stage],
    context: Context<'_>,
    state: State<'_>,
) -> Result<bool, String> {
    if stages.is_empty() {
        return Ok(false);
    }
    if context.size.contains(&0)
        || owner.layers.len() != 1
        || owner.playback != super::identity_playback(owner.playback.input_range())
    {
        return Err("requires one editable content Group in an identity occurrence clock".into());
    }
    if context.ordinals.len() > owner.effects.len()
        || context.ordinals.windows(2).any(|p| p[0] > p[1])
        || stages
            .windows(2)
            .any(|p| p[0].native_ordinal >= p[1].native_ordinal)
        || stages.iter().any(|stage| {
            context.ordinals.contains(&stage.native_ordinal)
                || !matches!(stage.blend_mode, BlendMode::Multiply | BlendMode::Screen)
                || !matches!(
                    stage.generator.data(),
                    EffectData::Identified {
                        enabled: true,
                        effect: EffectPayload::Known(LayerEffect::TurbulentNoise {
                            blend: Some(1.),
                            ..
                        }),
                        ..
                    }
                )
        })
    {
        return Err("Fractal stage/effect ordinal metadata is inconsistent".into());
    }
    let additional_depth = stages
        .len()
        .checked_mul(2)
        .ok_or("Fractal stage depth overflow")?;
    let max_depth = content_depth(owner, context.parent_depth)?;
    if max_depth
        .checked_add(additional_depth)
        .is_none_or(|depth| depth >= MAX_GROUP_DEPTH)
    {
        return Err("Fractal stage helper depth exceeds allowance".into());
    }
    let mut candidate = owner.clone();
    let mut cursor = *state.next;
    let mut effect_cursor = 0;
    for stage in stages {
        let first = reserve_ids(&mut cursor, 4)
            .ok_or("Fractal stage helper identity allocation exhausted")?;
        let mut output = group(
            LayerId::new(first),
            "Fractal blend output".into(),
            Some(owner.id),
            owner.playback.input_range(),
        );
        let mut input = group(
            LayerId::new(first + 1),
            "Source before Fractal blend".into(),
            Some(output.id),
            owner.playback.input_range(),
        );
        let prefix_count = context.ordinals[effect_cursor..]
            .iter()
            .take_while(|ordinal| **ordinal < stage.native_ordinal)
            .count();
        input.effects = candidate.effects.drain(..prefix_count).collect();
        effect_cursor += prefix_count;
        let mut original = candidate.layers.remove(0).data().clone();
        let LayerData::Group(original_group) = &mut original else {
            return Err("requires editable source content Group".into());
        };
        original_group.parent = Some(input.id);
        input.layers = stored_layers(vec![original]).map_err(|error| error.to_string())?;
        let mut noise = group(
            LayerId::new(first + 2),
            "Independent Fractal generator".into(),
            Some(output.id),
            owner.playback.input_range(),
        );
        noise.blend_mode = stage.blend_mode;
        noise.transform.opacity = stage.opacity;
        // Effect-only disabling leaves opaque neutral carrier paint. It is
        // RGB-neutral over opaque input, but can still change transparent
        // input alpha. Bypass the enclosing generator Group for a full bypass.
        let neutral = match stage.blend_mode {
            BlendMode::Multiply => [1.; 3],
            BlendMode::Screen => [0.; 3],
            _ => unreachable!("stage blend modes validated above"),
        };
        let mut rect = transform::solid_rect(
            &SolidSource {
                width: context.size[0],
                height: context.size[1],
                pixel_aspect: (1, 1),
                color: neutral,
            },
            &noise,
            LayerId::new(first + 3),
            noise.transform,
        );
        rect.name = "Fractal noise source plane".into();
        rect.transform.opacity = PercentageProperty::new(100.).expect("100 is valid opacity");
        rect.active_range = context.visibility.range;
        rect.is_hidden = context.visibility.hidden;
        rect.effects = vec![stage.generator.clone()];
        noise.layers =
            stored_layers(vec![LayerData::Rect(rect)]).map_err(|error| error.to_string())?;
        // FX layer stacks store their topmost layer first.
        output.layers = stored_layers(vec![LayerData::Group(noise), LayerData::Group(input)])
            .map_err(|error| error.to_string())?;
        candidate.layers =
            stored_layers(vec![LayerData::Group(output)]).map_err(|error| error.to_string())?;
    }
    fx_schema::Layer::from_data(&LayerData::Group(candidate.clone()))
        .map_err(|error| error.to_string())?;
    let reservations = stages
        .iter()
        .flat_map(|stage| &stage.animations)
        .map(committed_entry_reservation_bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let animation_checkpoint = state.animation_budget.checkpoint();
    state
        .animation_budget
        .reserve_all(reservations)
        .map_err(|error| error.to_string())?;
    let checkpoint = state.budget.checkpoint();
    if !state.budget.reserve(&candidate) {
        state.animation_budget.rollback(animation_checkpoint);
        state.budget.restore(checkpoint);
        return Err(
            "Fractal helper output serialization failed or its byte count overflowed".into(),
        );
    }
    *owner = candidate;
    *state.next = cursor;
    state.animations.extend(
        stages
            .iter()
            .flat_map(|stage| stage.animations.iter().cloned()),
    );
    Ok(true)
}

fn content_depth(owner: &GroupLayer, parent_depth: usize) -> Result<usize, String> {
    let depth = parent_depth
        .checked_add(1)
        .ok_or("Fractal source depth overflow")?;
    let mut maximum = depth;
    let mut pending = vec![(owner, depth)];
    while let Some((group, depth)) = pending.pop() {
        if depth >= MAX_GROUP_DEPTH {
            return Err("Fractal source depth exceeds allowance".into());
        }
        maximum = maximum.max(depth);
        for layer in &group.layers {
            if let LayerData::Group(child) = layer.data() {
                pending.push((child, depth + 1));
            }
        }
    }
    Ok(maximum)
}

#[cfg(test)]
mod tests;
