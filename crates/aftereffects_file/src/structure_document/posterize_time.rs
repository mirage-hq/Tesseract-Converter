//! A full-span Adjustment holds only its closed visual sibling stack.
//! Interval gates stay outside the static hold owners, so switching rates
//! neither holds the gate nor restarts the source quantization grid.

use std::{collections::HashSet, ops::Range};

use fx_schema::{
    EffectData, EffectId, EffectPayload, EffectRecord, LayerData as FxLayer, LayerEffect, LayerId,
    PropertyTarget, Time, TimeRangeProperty, TimeRemapExtrapolation, TimeRemapKeyframe,
    TimeRemapProperty,
    animator::{AnimationGraphEntry, KeyframeId, PropertyKeyframeEasing},
};

use super::{
    Converter, LayerContext, LayerPurpose, Limitation, MAX_GROUP_DEPTH, group, remapped_playback,
};
use crate::{document::DocumentError, effects::native, properties, structure::Layer};

const MATCH_NAME: &str = "ADBE Posterize Time";
const RATE: &str = "ADBE Posterize Time-0001";
const MAX_INTERVALS: usize = 16;

#[derive(Debug, PartialEq)]
struct Interval {
    start: u64,
    end: u64,
    rate: f64,
}

fn schedule(layer: &Layer, duration: f64) -> Result<Option<Vec<Interval>>, String> {
    let (effects, warnings) = native::read_effects(&layer.content, [1.0, 1.0]);
    let active: Vec<_> = effects.iter().filter(|effect| effect.enabled).collect();
    if !layer.record.flags().effects_active
        || !active.iter().any(|effect| effect.match_name == MATCH_NAME)
    {
        return Ok(None);
    }
    if active.len() != 1 || !warnings.is_empty() {
        return Err("requires one valid enabled Posterize Time effect".into());
    }
    if layer.record.start_time() != Some(0.0)
        || layer.record.in_point() != Some(0.0)
        || layer.record.out_point() != Some(duration)
        || layer.record.stretch() != Some(1.0)
        || layer.record.parent_id() != 0
        || layer.record.blend_mode() != 2
        || layer.record.flags().three_d_layer
        || layer.record.flags().preserve_transparency
    {
        return Err("requires an unparented full-span unit-clock Normal adjustment".into());
    }
    let effect = active[0];
    validate_raw_controls(layer, effect.index)?;
    if effect.parameters.len() != 1 || effect.parameters[0].match_name != RATE {
        return Err("unknown Posterize Time controls".into());
    }
    let numeric = effect.parameters[0]
        .numeric
        .as_ref()
        .map_err(|e| e.to_string())?;
    intervals(numeric, duration).map(Some)
}

fn validate_raw_controls(layer: &Layer, index: usize) -> Result<(), String> {
    let roots = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    let parade = super::control_links::unique_run(&roots, "ADBE Effect Parade")
        .map_err(|e| e.to_string())?;
    let groups = properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?;
    let instances = properties::runs(groups).map_err(|e| e.to_string())?;
    let (name, run) = instances
        .get(index.checked_sub(1).ok_or("invalid effect index")?)
        .ok_or("effect instance missing")?;
    if *name != MATCH_NAME {
        return Err("effect instance mismatch".into());
    }
    let descriptor = properties::unique_list(run, *b"sspc").map_err(|e| e.to_string())?;
    let controls = properties::unique_list(descriptor, *b"tdgp").map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    for (name, run) in properties::runs(controls).map_err(|e| e.to_string())? {
        if name == "ADBE Group End" {
            continue;
        }
        if !seen.insert(name) {
            return Err(format!("duplicate control {name}"));
        }
        match name {
            RATE | "ADBE Posterize Time-0000" => {}
            "ADBE Effect Built In Params" => {
                let options = properties::unique_list(run, *b"tdgp").map_err(|e| e.to_string())?;
                if properties::runs(options)
                    .map_err(|e| e.to_string())?
                    .iter()
                    .any(|(name, _)| *name != "ADBE Group End")
                {
                    return Err("nonempty effect compositing options are unsupported".into());
                }
            }
            _ => return Err(format!("unknown explicit control {name}")),
        }
    }
    Ok(())
}

fn intervals(
    numeric: &properties::NumericProperty,
    duration: f64,
) -> Result<Vec<Interval>, String> {
    if !duration.is_finite() || duration <= 0.0 || duration > super::MAX_TIME_SECS {
        return Err("invalid composition duration".into());
    }
    if numeric.expression_present || numeric.expression_enabled || numeric.dimensions_separated {
        return Err("expressions and separated rate controls are unsupported".into());
    }
    let valid_rate = |values: &[f64]| {
        (values.len() == 1 && values[0].is_finite() && values[0] >= 1.0 && values[0] <= 60.0)
            .then(|| values[0])
    };
    let end = super::composition_duration(duration).as_millis();
    let mut points = Vec::new();
    if numeric.animated {
        if numeric.keyframes.is_empty() || numeric.keyframes.len() > MAX_INTERVALS {
            return Err("empty or excessive Hold rate schedule".into());
        }
        let mut previous = -1.0;
        for key in &numeric.keyframes {
            if !key.time_secs.is_finite()
                || key.time_secs < 0.0
                || key.time_secs >= duration
                || key.time_secs <= previous
                || key.in_interpolation != 3
                || key.out_interpolation != 3
            {
                return Err("requires strictly ordered in-span Hold rate keys".into());
            }
            let rate = valid_rate(&key.values).ok_or("invalid frame rate")?;
            // Numeric native times are seconds. The gate switches at the first
            // integer model input on/after the native boundary, never before it.
            points.push(((key.time_secs * 1_000.0).ceil() as u64, rate));
            previous = key.time_secs;
        }
        if points[0].0 != 0 {
            return Err("first rate key must start at zero".into());
        }
    } else {
        points.push((0, valid_rate(&numeric.values).ok_or("invalid frame rate")?));
    }
    let mut intervals = Vec::new();
    for (index, &(start, rate)) in points.iter().enumerate() {
        let stop = points.get(index + 1).map_or(end, |point| point.0);
        if start >= stop {
            return Err("rate windows collide in the millisecond clock".into());
        }
        intervals.push(Interval {
            start,
            end: stop,
            rate,
        });
    }
    Ok(intervals)
}

fn target_id(target: &PropertyTarget) -> u64 {
    target
        .layer_id()
        .map(u64::from)
        .or_else(|| target.effect_id().map(u64::from))
        .or_else(|| target.fx_item_id().map(u64::from))
        .expect("every property target has one identity namespace")
}

pub(super) fn animations_closed(entries: &[AnimationGraphEntry], ids: &Range<u64>) -> bool {
    entries.iter().all(|entry| {
        let owned = ids.contains(&target_id(&entry.target));
        if owned {
            entry.animator.keyframe_track().is_some()
                && entry.dependencies.is_empty()
                && entry.random_seed_target.is_none()
                && entry.layer_refs.is_empty()
        } else {
            !entry
                .dependencies
                .iter()
                .any(|target| ids.contains(&target_id(target)))
                && !entry
                    .random_seed_target
                    .as_ref()
                    .is_some_and(|target| ids.contains(&target_id(target)))
                && !entry
                    .layer_refs
                    .values()
                    .any(|reference| ids.contains(&u64::from(reference.layer_id)))
        }
    })
}

pub(super) fn collect_layers(
    layer: &FxLayer,
    ids: &mut HashSet<LayerId>,
    depth: usize,
) -> Result<(), String> {
    if depth >= MAX_GROUP_DEPTH || !ids.insert(layer.id()) {
        return Err("duplicate identity or additional hold depth exceeds limit".into());
    }
    if let Some(children) = layer.child_layers() {
        for child in children {
            collect_layers(child.data(), ids, depth + 1)?;
        }
    }
    Ok(())
}

pub(super) fn closed_visual(
    layer: &FxLayer,
    ids: &HashSet<LayerId>,
    root: LayerId,
    visual_only: bool,
) -> bool {
    let (matte, masks) = match layer {
        FxLayer::Group(x) => (&x.track_matte, &x.masks),
        FxLayer::Adjustment(x) => (&x.track_matte, &x.masks),
        FxLayer::Text(x) => {
            if x.path_options
                .as_ref()
                .is_some_and(|path| !ids.contains(&path.path_layer))
            {
                return false;
            }
            (&x.track_matte, &x.masks)
        }
        FxLayer::Shape(x) => (&x.track_matte, &x.masks),
        FxLayer::Rect(x) => (&x.track_matte, &x.masks),
        FxLayer::Image(x) => (&x.track_matte, &x.masks),
        FxLayer::BooleanOperation(x) => (&x.track_matte, &x.masks),
        FxLayer::Video(x) if !visual_only => (&x.track_matte, &x.masks),
        FxLayer::Audio(_) if !visual_only => {
            return layer
                .parent_id()
                .is_none_or(|id| id == root || ids.contains(&id));
        }
        // Sound and physical source clocks cannot be duplicated under a visual hold.
        _ => return false,
    };
    layer
        .parent_id()
        .is_none_or(|id| id == root || ids.contains(&id))
        && matte
            .as_ref()
            .is_none_or(|matte| ids.contains(&matte.layer))
        && masks
            .iter()
            .all(|mask| mask.layer.is_none_or(|id| ids.contains(&id)))
        && layer.effects().iter().all(|effect| {
            let payload = match effect.data() {
                EffectData::Identified { effect, .. } | EffectData::Legacy(effect) => effect,
            };
            match payload {
                EffectPayload::Unknown(_) | EffectPayload::Known(LayerEffect::Unsupported(_)) => {
                    false
                }
                EffectPayload::Known(LayerEffect::CustomShader { texture_inputs, .. }) => {
                    texture_inputs
                        .iter()
                        .all(|input| input.source_layer_id.is_none_or(|id| ids.contains(&id)))
                }
                _ => true,
            }
        })
        && layer.child_layers().is_none_or(|children| {
            children
                .iter()
                .all(|child| closed_visual(child.data(), ids, root, visual_only))
        })
}

pub(super) fn reparent(layer: &mut FxLayer, parent: LayerId) -> Result<(), String> {
    match layer {
        FxLayer::Group(group) => group.parent = Some(parent),
        FxLayer::Adjustment(adjustment) => adjustment.parent = Some(parent),
        _ => return Err("held stack requires native occurrence owners".into()),
    }
    Ok(())
}

impl Converter<'_> {
    pub(super) fn apply_posterize_time(
        &mut self,
        context: &LayerContext<'_>,
        source_indices: &[usize],
        emitted_end: u64,
        layers: &mut Vec<FxLayer>,
    ) -> Result<(), DocumentError> {
        let candidates: Vec<_> = source_indices
            .iter()
            .enumerate()
            .filter_map(|(index, &source)| {
                let layer = &context.comp.layers[source];
                (layer.record.flags().adjustment_layer && layer.record.flags().enabled).then(|| {
                    schedule(layer, context.comp.duration_secs)
                        .map(|value| value.map(|schedule| (index, schedule)))
                })
            })
            .collect();
        let mut found = Vec::new();
        let mut invalid = false;
        for candidate in candidates {
            match candidate {
                Ok(Some(value)) => found.push(value),
                Ok(None) => {}
                Err(message) => {
                    invalid = true;
                    self.warn(
                        Limitation::Timing,
                        Some(context.comp_id),
                        None,
                        format!("Posterize Time adjustment omitted: {message}"),
                    );
                }
            }
        }
        // Direct sibling matte helpers allocate outside the native occurrence
        // intervals. Nested helpers were already allocated inside each occurrence.
        if invalid
            || found.len() != 1
            || layers.len() != source_indices.len()
            || self.next_id != emitted_end
        {
            return Ok(());
        }
        let (index, intervals) = found.pop().expect("one schedule");
        let source = &context.comp.layers[source_indices[index]];
        let result = self.posterized_stack(
            context,
            source_indices,
            emitted_end,
            layers,
            index,
            &intervals,
        );
        match result {
            Ok(Some(replacement)) => {
                layers.splice(index.., replacement);
                self.diagnostics.retain(|note| !(note.composition_id == Some(context.comp_id)
                    && note.layer_id == Some(source.record.id())
                    && (note.message.contains(MATCH_NAME)
                        || note.message.starts_with("adjustment-layer cross-layer compositing has no existing FX equivalent")
                        || note.message.starts_with("native Adjustment lowered to a direct FX Adjustment sibling"))));
                self.warn(Limitation::Timing, Some(context.comp_id), Some(source.record.id()),
                    "Full-span Hold Posterize Time adjustment lowered to independent editable visual stacks with static source-zero hold grids and unheld interval gates; native switches round upward to milliseconds. Copies are independently editable; arbitrary submillisecond boundaries and general adjustment clocks remain unsupported".into());
            }
            Ok(None) => {}
            Err(message) => self.warn(
                Limitation::Timing,
                Some(context.comp_id),
                Some(source.record.id()),
                format!("Posterize Time adjustment omitted; original siblings retained: {message}"),
            ),
        }
        Ok(())
    }

    fn posterized_stack(
        &mut self,
        context: &LayerContext<'_>,
        source_indices: &[usize],
        emitted_end: u64,
        layers: &[FxLayer],
        index: usize,
        intervals: &[Interval],
    ) -> Result<Option<Vec<FxLayer>>, String> {
        let FxLayer::Adjustment(adjustment) = &layers[index] else {
            return Ok(None);
        };
        if adjustment.is_hidden
            || adjustment.transform.opacity.value() != 100.0
            || adjustment.effects.iter().any(|effect| {
                !matches!(
                    effect.data(),
                    EffectData::Identified {
                        enabled: true,
                        effect: EffectPayload::Known(LayerEffect::PosterizeTime { .. }),
                        ..
                    }
                )
            })
            || adjustment.effects.len() != 1
            || !adjustment.masks.is_empty()
            || adjustment.track_matte.is_some()
            || self
                .animations
                .iter()
                .any(|entry| entry.target.layer_id() == Some(adjustment.id))
            || index + 1 == layers.len()
        {
            return Err("masked, styled, animated-opacity or empty adjustment scope".into());
        }
        let mut removed = HashSet::from([u64::from(adjustment.id)]);
        for effect in &adjustment.effects {
            if let EffectData::Identified { id, .. } = effect.data() {
                removed.insert(u64::from(*id));
            }
        }
        if self.animations.iter().any(|entry| {
            removed.contains(&target_id(&entry.target))
                || entry
                    .dependencies
                    .iter()
                    .any(|target| removed.contains(&target_id(target)))
                || entry
                    .random_seed_target
                    .as_ref()
                    .is_some_and(|target| removed.contains(&target_id(target)))
                || entry
                    .layer_refs
                    .values()
                    .any(|reference| removed.contains(&u64::from(reference.layer_id)))
        }) {
            return Err("graph references the removed adjustment owner".into());
        }
        let native_ids: HashSet<_> = source_indices[index + 1..]
            .iter()
            .map(|&i| context.comp.layers[i].record.id())
            .collect();
        if context.comp.layers.iter().any(|layer| {
            let parent = layer.record.parent_id();
            parent != 0 && native_ids.contains(&layer.record.id()) != native_ids.contains(&parent)
        }) {
            return Err("native parenting crosses the held boundary".into());
        }
        let transforms =
            properties::read_transform(&context.comp.layers[source_indices[index]].content)
                .map_err(|e| e.to_string())?;
        if transforms.iter().any(|p| {
            p.match_name == "ADBE Opacity"
                && !p.numeric.as_ref().is_ok_and(|n| {
                    !n.animated
                        && !n.expression_present
                        && !n.expression_enabled
                        && n.values == [100.0]
                })
        }) {
            return Err("requires static authored 100 percent opacity".into());
        }
        let roots = properties::root_runs(&context.comp.layers[source_indices[index]].content)
            .map_err(|e| e.to_string())?;
        for (name, run) in roots {
            if name == "ADBE Time Remapping" {
                let leaf = properties::unique_list(run, *b"tdbs").map_err(|e| e.to_string())?;
                let remap = properties::read_numeric(leaf).map_err(|e| e.to_string())?;
                if remap.animated
                    || remap.expression_present
                    || remap.expression_enabled
                    || remap.values != [0.0]
                {
                    return Err("authored adjustment remap is unsupported".into());
                }
            }
            if name == "ADBE Mask Parade" {
                let group = properties::unique_list(run, *b"tdgp").map_err(|e| e.to_string())?;
                if !properties::runs(group)
                    .map_err(|e| e.to_string())?
                    .is_empty()
                {
                    return Err("native adjustment masks are unsupported".into());
                }
            }
        }
        let styles = crate::layer_styles::read(
            &context.comp.layers[source_indices[index]].content,
            [1.0, 1.0],
        );
        if !styles.styles.is_empty() || !styles.warnings.is_empty() {
            return Err("adjustment layer styles are unsupported".into());
        }
        let below = &layers[index + 1..];
        let mut ids = HashSet::new();
        for layer in below {
            collect_layers(layer, &mut ids, context.depth + 3)?;
        }
        let mut above_ids = HashSet::new();
        for layer in &layers[..index] {
            collect_layers(layer, &mut above_ids, context.depth + 1)?;
        }
        if !below
            .iter()
            .all(|layer| closed_visual(layer, &ids, context.parent, true))
            || !layers[..index]
                .iter()
                .all(|layer| closed_visual(layer, &above_ids, context.parent, false))
        {
            return Err(
                "scope contains media, unsupported layers or cross-boundary references".into(),
            );
        }
        // A native Transform wrapper can be allocated after its own child.
        // Start at the earliest retained layer identity, not the returned root.
        let generated = ids
            .iter()
            .copied()
            .map(u64::from)
            .min()
            .expect("nonempty held stack")..emitted_end;
        if generated.is_empty() {
            return Err("held identity interval is empty".into());
        }
        if !animations_closed(&self.animations, &generated) {
            return Err(
                "held scope has script, dependency, seed or cross-boundary graph references".into(),
            );
        }
        let id_checkpoint = self.next_id;
        let animation_checkpoint = self.animation_budget.checkpoint();
        let animation_start = self.animations.len();
        let diagnostics_start = self.diagnostics.len();
        let shape_checkpoint = self.shape_budget.checkpoint();
        let assets_start = self.assets.len();
        let visited = self.visited_compositions.clone();
        let stack = self.stack.clone();
        let overrides = self.overrides.clone();
        #[cfg(test)]
        let inline_bytes = self.committed_inline_remap_bytes;
        let denied = self.animation_budget.denials();
        let built = (|| {
            let duration = self.duration(context.comp_id, context.comp.duration_secs);
            let mut replacement = Vec::new();
            for (branch, interval) in intervals.iter().enumerate() {
                let mut gate = group(
                    self.allocate_id().map_err(|e| e.to_string())?,
                    "Posterize Time interval".into(),
                    Some(context.parent),
                    TimeRangeProperty::new(
                        Time::from_millis(interval.start),
                        fx_schema::Duration::from_millis(interval.end - interval.start),
                    ),
                );
                let mut held = group(
                    self.allocate_id().map_err(|e| e.to_string())?,
                    "Posterize Time source grid".into(),
                    Some(gate.id),
                    TimeRangeProperty::new(Time::ZERO, duration),
                );
                let start = Time::from_millis(interval.start);
                let end = Time::from_millis(interval.end);
                let playback = TimeRemapProperty::new(
                    [start, end]
                        .into_iter()
                        .enumerate()
                        .map(|(i, time)| TimeRemapKeyframe {
                            id: KeyframeId::new(format!("aep-posterize-{}-{i}", gate.id)),
                            time,
                            value: time,
                            easing: PropertyKeyframeEasing::Linear,
                        })
                        .collect(),
                    TimeRemapExtrapolation::Inactive,
                    TimeRemapExtrapolation::Inactive,
                )
                .map_err(|e| e.to_string())?;
                let mut estimate = super::animation_budget::TimeRemapEstimate::default();
                for (i, time) in [start, end].into_iter().enumerate() {
                    let id = super::animation_budget::GeneratedKeyframeIdSize::new(format_args!(
                        "aep-posterize-{}-{i}",
                        gate.id
                    ))
                    .map_err(|e| e.to_string())?;
                    estimate
                        .push_key(&id, time, time, PropertyKeyframeEasing::Linear)
                        .map_err(|e| e.to_string())?;
                }
                self.animation_budget
                    .reserve(
                        estimate
                            .reservation_bytes(
                                TimeRemapExtrapolation::Inactive,
                                TimeRemapExtrapolation::Inactive,
                            )
                            .map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
                self.note_committed_remap(&playback);
                gate.playback = remapped_playback(gate.playback.input_range(), playback);
                held.effects.push(
                    EffectRecord::from_data(&EffectData::Identified {
                        id: EffectId::new(
                            super::reserve_ids(&mut self.next_id, 1)
                                .ok_or("effect identity overflow")?,
                        ),
                        enabled: true,
                        effect: EffectPayload::Known(LayerEffect::PosterizeTime {
                            frame_rate: Some(interval.rate),
                        }),
                    })
                    .map_err(|e| e.to_string())?,
                );
                let mut children = if branch == 0 {
                    below.to_vec()
                } else {
                    let copy_first_id = self.next_id;
                    let sample = LayerContext {
                        parent: held.id,
                        depth: context.depth + 2,
                        ..*context
                    };
                    let mut copies = Vec::new();
                    let mut guides = Vec::new();
                    for &source_index in &source_indices[index + 1..] {
                        let native = &context.comp.layers[source_index];
                        if native.record.flags().adjustment_layer {
                            let (adjustment, mut extra) = self
                                .adjustment_layer(&sample, native)
                                .map_err(|e| e.to_string())?;
                            copies.push(FxLayer::Adjustment(adjustment));
                            guides.append(&mut extra);
                        } else {
                            copies.push(FxLayer::Group(
                                self.layer(&sample, native, LayerPurpose::Ordinary)
                                    .map_err(|e| e.to_string())?,
                            ));
                        }
                    }
                    self.apply_mattes(&sample, &source_indices[index + 1..], &mut copies)
                        .map_err(|e| e.to_string())?;
                    self.apply_set_mattes(&sample, &source_indices[index + 1..], &mut copies)
                        .map_err(|e| e.to_string())?;
                    self.apply_preserve_transparency(
                        &sample,
                        &source_indices[index + 1..],
                        &mut copies,
                    )
                    .map_err(|e| e.to_string())?;
                    if !guides.is_empty() || copies.len() != below.len() {
                        return Err("new sampled helpers cross the held boundary".into());
                    }
                    let mut fresh_ids = HashSet::new();
                    for layer in &copies {
                        collect_layers(layer, &mut fresh_ids, context.depth + 3)?;
                    }
                    if !copies
                        .iter()
                        .all(|layer| closed_visual(layer, &fresh_ids, held.id, true))
                    {
                        return Err("regenerated scope is not closed visual content".into());
                    }
                    if !animations_closed(&self.animations, &(copy_first_id..self.next_id)) {
                        return Err("regenerated animations are not independent keyframes".into());
                    }
                    copies
                };
                for child in &mut children {
                    reparent(child, held.id)?;
                }
                let split_context = LayerContext {
                    parent: held.id,
                    depth: context.depth + 2,
                    ..*context
                };
                self.apply_split2(&split_context, &source_indices[index + 1..], &mut children)
                    .map_err(|e| e.to_string())?;
                held.layers = super::stored_layers(children).map_err(|e| e.to_string())?;
                gate.layers.push(
                    fx_schema::Layer::from_data(&FxLayer::Group(held))
                        .map_err(|e| e.to_string())?,
                );
                replacement.push(FxLayer::Group(gate));
            }
            if self.animation_budget.denials() != denied
                || self.diagnostics[diagnostics_start..].iter().any(|note| {
                    note.limitation == Limitation::ExpansionLimit
                        || note.limitation == Limitation::Cycle
                })
            {
                return Err("held copy exceeded an import allowance".into());
            }
            Ok(replacement)
        })();
        if built.is_err() {
            self.next_id = id_checkpoint;
            self.animation_budget.rollback(animation_checkpoint);
            self.animations.truncate(animation_start);
            self.diagnostics.truncate(diagnostics_start);
            self.shape_budget.restore(shape_checkpoint);
            self.assets.truncate(assets_start);
            self.visited_compositions = visited;
            self.stack = stack;
            self.overrides = overrides;
            #[cfg(test)]
            {
                self.committed_inline_remap_bytes = inline_bytes;
            }
        }
        built.map(Some)
    }
}

#[cfg(test)]
mod tests;
