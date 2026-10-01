//! Approximate one opaque-solid self-inverse Masks-stage profile with existing FX.
//! Separate groups force mask -> shadows -> inverse mask -> Glow/styles order;
//! the renderer puts a group's own masks after leading decoration effects.
use std::collections::{HashMap, HashSet};

use super::{
    MAX_GROUP_DEPTH, animation_budget::AnimationBudget, control_links::unique_run, group,
    identity_playback, masks, reserve_ids, set_matte, stored_layers,
};
use crate::{
    properties,
    rifx::Chunk,
    structure::{Layer, ProjectItem},
};
use fx_schema::{
    EffectData, EffectPayload, FxItemId, GroupLayer, LayerData, LayerEffect, LayerId,
    animator::AnimationGraphEntry,
};
const MATTE: &str = "ADBE Set Matte3";
pub(super) struct Context<'a> {
    pub source: Option<&'a ProjectItem>,
    pub items: &'a HashMap<u32, &'a ProjectItem>,
    pub depth: usize,
}

#[derive(Clone, Copy)]
pub(super) enum Profile {
    SelfLayer { mask_supported: bool },
    ForeignLayer { provider: u32 },
}

pub(super) fn static_values(run: &[Chunk], expected: &[f64]) -> Result<(), String> {
    let property = properties::unique_list(run, *b"tdbs").map_err(|e| e.to_string())?;
    let n = properties::read_numeric(property).map_err(|e| e.to_string())?;
    if n.animated
        || !n.keyframes.is_empty()
        || n.expression_present
        || n.expression_enabled
        || n.values != expected
    {
        return Err("requires static native controls with no expressions".into());
    }
    Ok(())
}
fn raw_profile(layer: &Layer, profile: Profile) -> Result<bool, String> {
    let roots = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    if !roots.iter().any(|(name, _)| *name == "ADBE Effect Parade") {
        return Ok(false);
    }
    let parade = unique_run(&roots, "ADBE Effect Parade").map_err(|e| e.to_string())?;
    let instances = properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?;
    let mut active = Vec::new();
    for (name, run) in properties::runs(instances).map_err(|e| e.to_string())? {
        let descriptor = properties::unique_list(run, *b"sspc").map_err(|e| e.to_string())?;
        let mut warnings = Vec::new();
        let enabled = properties::group_enabled_or_warn(descriptor, name, &mut warnings);
        if !warnings.is_empty() {
            return Err(warnings.join("; "));
        }
        if enabled {
            active.push((name, descriptor));
        }
    }
    if !active.iter().any(|(name, _)| *name == MATTE) {
        return Ok(false);
    }
    let names: Vec<_> = active.iter().map(|(n, _)| *n).collect();
    let self_names = names.strip_suffix(&["ADBE Geometry2"]).unwrap_or(&names);
    if matches!(profile, Profile::SelfLayer { .. })
        && self_names != ["ADBE Drop Shadow", "ADBE Drop Shadow", MATTE, "ADBE Glo2"]
        && self_names
            != [
                "ADBE Drop Shadow",
                "ADBE Drop Shadow",
                MATTE,
                "ADBE Glo2",
                "ADBE Linear Wipe",
                "ADBE Linear Wipe",
            ]
    {
        return Err(
            "requires two shadows, self Set Matte, Glow, optional opposed Wipes and optional final Transform in source order"
                .into(),
        );
    }
    if matches!(profile, Profile::ForeignLayer { .. })
        && (!names.ends_with(&["ADBE Geometry2"])
            || (self_names != ["ADBE Drop Shadow", "ADBE Drop Shadow", MATTE, "ADBE Glo2"]
                && self_names
                    != [
                        "ADBE Drop Shadow",
                        "ADBE Drop Shadow",
                        MATTE,
                        "ADBE Glo2",
                        "ADBE Linear Wipe",
                        "ADBE Linear Wipe",
                    ]))
    {
        return Err("requires two shadows, foreign Set Matte, Glow, optional opposed Wipes and final Transform in source order".into());
    }
    for (name, descriptor) in &active {
        let controls = properties::unique_list(descriptor, *b"tdgp").map_err(|e| e.to_string())?;
        let rows = properties::runs(controls).map_err(|e| e.to_string())?;
        let mut seen = HashSet::new();
        for (control, run) in &rows {
            if *control == "ADBE Group End" {
                continue;
            }
            if !seen.insert(*control) {
                return Err(format!("duplicate {name} control {control}"));
            }
            if *control == "ADBE Effect Built In Params" {
                let options = properties::unique_list(run, *b"tdgp").map_err(|e| e.to_string())?;
                if properties::runs(options)
                    .map_err(|e| e.to_string())?
                    .iter()
                    .any(|(n, _)| *n != "ADBE Group End")
                {
                    return Err("nonempty effect compositing options".into());
                }
            } else {
                let supported = match *name {
                    MATTE => true,
                    "ADBE Geometry2" => {
                        matches!(*control, "ADBE Geometry2-0000" | "ADBE Geometry2-0007")
                    }
                    "ADBE Linear Wipe" => matches!(
                        *control,
                        "ADBE Linear Wipe-0000"
                            | "ADBE Linear Wipe-0001"
                            | "ADBE Linear Wipe-0002"
                            | "ADBE Linear Wipe-0003"
                    ),
                    _ => crate::effects::definitions::definition(name)
                        .is_some_and(|d| d.parameters.iter().any(|p| p.match_name == *control)),
                };
                if !supported {
                    return Err(format!("unknown {name} control {control}"));
                }
            }
        }
        if *name != MATTE {
            continue;
        }
        let table = properties::unique_list(descriptor, *b"parT").map_err(|e| e.to_string())?;
        if !table.is_empty() {
            set_matte::validate_alpha_defaults(table)?;
        }
        // Sparse instances use the separately pinned native Alpha/composite
        // default profile, not a nonexistent entry in the generic effect catalog.
        for (control, run) in rows {
            match control {
                "ADBE Set Matte3-0000" => static_values(run, &[0.])?,
                "ADBE Set Matte3-0001" => {
                    static_values(run, &[0.])?;
                    let leaf = properties::unique_list(run, *b"tdbs").map_err(|e| e.to_string())?;
                    let id = properties::data(leaf, *b"tdpi").map_err(|e| e.to_string())?;
                    let stage = properties::data(leaf, *b"tdps").map_err(|e| e.to_string())?;
                    let provider = match profile {
                        Profile::SelfLayer { .. } => layer.record.id(),
                        Profile::ForeignLayer { provider } => provider,
                    };
                    if id != provider.to_be_bytes() || stage != (-2_i32).to_be_bytes() {
                        return Err("requires the same native layer and stage -2".into());
                    }
                }
                "ADBE Set Matte3-0002" => static_values(run, &[4.])?,
                "ADBE Set Matte3-0003" => static_values(run, &[1.])?,
                "ADBE Set Matte3-0004" | "ADBE Set Matte3-0005" | "ADBE Set Matte3-0006" => {
                    static_values(run, &[1.])?
                }
                "ADBE Effect Built In Params" | "ADBE Group End" => {}
                _ => return Err(format!("unknown explicit Set Matte control {control}")),
            }
        }
        if !seen.contains("ADBE Set Matte3-0001") || !seen.contains("ADBE Set Matte3-0003") {
            return Err("requires explicit self provider and inversion".into());
        }
    }
    Ok(true)
}
pub(super) fn raw_mask(layer: &Layer) -> Result<(), String> {
    let roots = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    let parade = unique_run(&roots, "ADBE Mask Parade").map_err(|e| e.to_string())?;
    let atoms = properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?;
    let rows = properties::runs(atoms).map_err(|e| e.to_string())?;
    let [("ADBE Mask Atom", atom)] = rows.as_slice() else {
        return Err("requires one native mask".into());
    };
    let info = properties::data(atom, *b"mkif").map_err(|e| e.to_string())?;
    if info.len() != 48 || info[0] != 0 || info[3] != 0 || info[6..8] != 1_u16.to_be_bytes() {
        return Err("requires ordinary noninverted Add mask".into());
    }
    let controls = properties::unique_list(atom, *b"tdgp").map_err(|e| e.to_string())?;
    let rows = properties::runs(controls).map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    for (name, run) in rows {
        if !seen.insert(name) {
            return Err("duplicate mask control".into());
        }
        match name {
            "ADBE Mask Shape" => {
                if masks::mask_roto_bezier(run).map_err(|e| e.to_string())? == Some(true)
                    || masks::has_variable_feather(run).map_err(|e| e.to_string())?
                {
                    return Err("unsupported native mask path form".into());
                }
                let outline = properties::unique_list(run, *b"om-s").map_err(|e| e.to_string())?;
                let leaf = properties::unique_list(outline, *b"tdbs").map_err(|e| e.to_string())?;
                let meta = properties::read_path_metadata(leaf).map_err(|e| e.to_string())?;
                if meta.expression_present || meta.expression_enabled {
                    return Err("mask path expressions are unsupported".into());
                }
            }
            "ADBE Mask Feather" => static_values(run, &[0., 0.])?,
            "ADBE Mask Opacity" => static_values(run, &[1.])?,
            "ADBE Mask Offset" => static_values(run, &[0.])?,
            _ => return Err(format!("unsupported mask control {name}")),
        }
    }
    if !seen.contains("ADBE Mask Shape") {
        return Err("requires an explicit editable mask path".into());
    }
    Ok(())
}
fn known(effect: &fx_schema::EffectRecord) -> Result<&LayerEffect, String> {
    match effect.data() {
        EffectData::Identified {
            enabled: true,
            effect: EffectPayload::Known(effect),
            ..
        } => Ok(effect),
        _ => Err("requires enabled identified known effects".into()),
    }
}

pub(super) fn apply(
    layer: &Layer,
    context: Context<'_>,
    owner: &mut GroupLayer,
    animations: &mut [AnimationGraphEntry],
    next_id: &mut u64,
    mask_supported: bool,
    budget: &mut AnimationBudget,
) -> Result<bool, String> {
    apply_profile(
        layer,
        context,
        owner,
        animations,
        next_id,
        Profile::SelfLayer { mask_supported },
        budget,
    )
}

pub(super) fn apply_profile(
    layer: &Layer,
    context: Context<'_>,
    owner: &mut GroupLayer,
    animations: &mut [AnimationGraphEntry],
    next_id: &mut u64,
    profile: Profile,
    budget: &mut AnimationBudget,
) -> Result<bool, String> {
    if !raw_profile(layer, profile)? {
        return Ok(false);
    }
    let rotated = super::inverse_matte_transform::rotation(layer, context.items)?;
    let record = &layer.record;
    let flags = record.flags();
    if !flags.enabled
        || !flags.effects_active
        || flags.three_d_layer
        || flags.adjustment_layer
        || flags.preserve_transparency
        || record.layer_type() != 0
        || record.parent_id() != 0
        || record.track_matte_type() != 0
        || record.stretch() != Some(1.)
        || record.blend_mode() != 2
    {
        return Err("requires enabled unparented unit-clock Normal 2D solid".into());
    }
    let solid = context
        .source
        .and_then(|s| s.solid.as_ref())
        .and_then(|s| s.as_ref().ok())
        .ok_or("requires a decoded opaque solid")?;
    if solid.width == 0
        || solid.height == 0
        || solid.pixel_aspect != (1, 1)
        || solid
            .color
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
    {
        return Err("invalid solid bounds/color".into());
    }
    let transforms = properties::read_transform(&layer.content).map_err(|e| e.to_string())?;
    if transforms.iter().any(|p| {
        !p.numeric.as_ref().is_ok_and(|n| {
            !n.animated && n.keyframes.is_empty() && !n.expression_present && !n.expression_enabled
        })
    }) {
        return Err("requires static occurrence Transform".into());
    }
    for (name, run) in properties::root_runs(&layer.content).map_err(|e| e.to_string())? {
        if name == "ADBE Time Remapping" {
            static_values(run, &[0.])?;
        }
    }
    let t = &owner.transform;
    let window = owner.playback.input_range();
    if window.start != fx_schema::Time::ZERO
        || window.duration.as_millis() == 0
        || owner.playback != identity_playback(window)
        || owner.track_matte.is_some()
        || owner.is_hidden
        || t.anchor_point != [t.position.x(), t.position.y()]
        || t.scale != [100., 100.]
        || t.opacity.value() != 100.
        || t.rotation != 0.
        || t.rotation_x != 0.
        || t.rotation_y != 0.
        || t.skew != 0.
        || t.orientation != [0., 0., 0.]
        || matches!(
            profile,
            Profile::SelfLayer {
                mask_supported: false
            }
        )
    {
        return Err("requires an identity-affine owner and fully supported mask import".into());
    }
    raw_mask(layer)?;
    if owner.layers.len() != 2
        || owner.masks.len() != 1
        || !(owner.effects.len() == 3 || owner.effects.len() == 4)
    {
        return Err("unexpected emitted owner topology".into());
    }
    if !matches!(known(&owner.effects[0])?, LayerEffect::DropShadow { .. })
        || !matches!(known(&owner.effects[1])?, LayerEffect::DropShadow { .. })
        || !matches!(known(&owner.effects[2])?, LayerEffect::Glow { .. })
        || owner
            .effects
            .get(3)
            .is_some_and(|e| !matches!(known(e), Ok(LayerEffect::InnerShadow { .. })))
    {
        return Err("unsupported emitted effect order".into());
    }
    let LayerData::Group(content) = owner.layers[0].data() else {
        return Err("requires source content Group".into());
    };
    let LayerData::Shape(guide) = owner.layers[1].data() else {
        return Err("requires original editable mask guide".into());
    };
    let mask = &owner.masks[0];
    if content.parent != Some(owner.id)
        || guide.parent != Some(owner.id)
        || mask.layer != Some(guide.id)
        || mask.mode != fx_schema::layer::MaskMode::Add
        || mask.inverted
        || mask.legacy_path.is_some()
        || mask.opacity.value() != 1.
        || mask.feather != [0., 0.]
        || mask.expansion != 0.
        || !guide.effects.is_empty()
        || !guide.masks.is_empty()
        || guide.track_matte.is_some()
        || !content.effects.is_empty()
        || !content.masks.is_empty()
        || content.track_matte.is_some()
        || content.layers.len() != 1
        || !matches!(content.layers[0].data(), LayerData::Rect(_))
    {
        return Err("requires one ordinary full-coverage Add mask on solid content".into());
    }
    if context
        .depth
        .checked_add(if rotated { 6 } else { 5 })
        .is_none_or(|d| d >= MAX_GROUP_DEPTH)
    {
        return Err("self-inverse helper depth exceeds import allowance".into());
    }
    let mut owned = HashSet::from([
        u64::from(owner.id),
        u64::from(content.id),
        u64::from(guide.id),
        u64::from(mask.id),
    ]);
    for e in &owner.effects {
        let EffectData::Identified { id, .. } = e.data() else {
            unreachable!()
        };
        owned.insert(u64::from(*id));
    }
    let referenced = |t: &fx_schema::PropertyTarget| {
        t.layer_id()
            .map(u64::from)
            .or_else(|| t.effect_id().map(u64::from))
            .or_else(|| t.fx_item_id().map(u64::from))
            .is_some_and(|id| owned.contains(&id))
    };
    for e in animations.iter() {
        if e.target.layer_id() == Some(owner.id)
            || e.dependencies.iter().any(referenced)
            || e.random_seed_target.as_ref().is_some_and(referenced)
            || e.layer_refs
                .values()
                .any(|r| owned.contains(&u64::from(r.layer_id)))
        {
            return Err("owner animation or graph linkage is unsupported".into());
        }
    }
    // All admission/serialization is staged; no animation tracks or budgets
    // change, and failed allocation leaves both owner and allocator untouched.
    let mut candidate_id = *next_id;
    let first = reserve_ids(&mut candidate_id, if rotated { 4 } else { 3 })
        .ok_or("helper identity allocation exhausted")?;
    let mut candidate = owner.clone();
    let mut source_content = content.clone();
    let mut shadows = group(
        LayerId::new(first),
        "Self-inverse matte shadow stage".into(),
        Some(owner.id),
        owner.playback.input_range(),
    );
    let mut masked = group(
        LayerId::new(first + 1),
        "Self-inverse matte source mask".into(),
        Some(shadows.id),
        owner.playback.input_range(),
    );
    source_content.parent = Some(masked.id);
    masked.masks = owner.masks.clone();
    masked.layers =
        stored_layers(vec![LayerData::Group(source_content)]).map_err(|e| e.to_string())?;
    shadows.effects = candidate.effects.drain(..2).collect();
    let mut rotated_shadow_ids = HashSet::new();
    if rotated {
        // Existing FX shadow offsets are screen-space. A native post-effect
        // half turn rotates the completed shadow, so rotate its vector too.
        for record in &mut shadows.effects {
            let mut data = record.data().clone();
            let EffectData::Identified {
                id,
                effect: EffectPayload::Known(LayerEffect::DropShadow(shadow)),
                ..
            } = &mut data
            else {
                unreachable!()
            };
            shadow.offset = shadow.offset.map(|v| if v == 0. { 0. } else { -v });
            rotated_shadow_ids.insert(*id);
            *record = fx_schema::EffectRecord::from_data(&data).map_err(|e| e.to_string())?;
        }
    }
    shadows.layers = stored_layers(vec![LayerData::Group(masked)]).map_err(|e| e.to_string())?;
    candidate.masks[0].id = FxItemId::new(first + 2);
    candidate.masks[0].inverted = true;
    candidate.layers = stored_layers(vec![
        LayerData::Group(shadows),
        LayerData::Shape(guide.clone()),
    ])
    .map_err(|e| e.to_string())?;
    if rotated {
        let mut rotation = group(
            LayerId::new(first + 3),
            if matches!(profile, Profile::ForeignLayer { .. }) {
                "Foreign matte post-Glow Transform"
            } else {
                "Self-inverse matte post-effect Transform"
            }
            .into(),
            Some(owner.id),
            owner.playback.input_range(),
        );
        rotation.transform.anchor_point =
            [f64::from(solid.width) / 2., f64::from(solid.height) / 2.];
        rotation.transform.position = fx_schema::Position::xy(
            rotation.transform.anchor_point[0],
            rotation.transform.anchor_point[1],
        );
        rotation.transform.rotation = 180.;
        rotation.effects = candidate.effects.drain(..1).collect();
        rotation.masks = std::mem::take(&mut candidate.masks);
        let mut children = candidate
            .layers
            .iter()
            .map(|x| x.data().clone())
            .collect::<Vec<_>>();
        for child in &mut children {
            match child {
                LayerData::Group(g) => g.parent = Some(rotation.id),
                LayerData::Shape(g) => g.parent = Some(rotation.id),
                _ => unreachable!(),
            }
        }
        rotation.layers = stored_layers(children).map_err(|e| e.to_string())?;
        candidate.layers =
            stored_layers(vec![LayerData::Group(rotation)]).map_err(|e| e.to_string())?;
    }
    fx_schema::Layer::from_data(&LayerData::Group(candidate.clone())).map_err(|e| e.to_string())?;
    let rotated_tracks = half_turn_shadow_tracks(animations, &rotated_shadow_ids, budget)?;
    for (index, animator) in rotated_tracks {
        animations[index].animator = animator;
    }
    *owner = candidate;
    *next_id = candidate_id;
    Ok(true)
}
/// Stage only the affected editable offset tracks, preserving identities,
/// clocks, eases and graph edges. Negation may change serialized number lengths;
/// reserve/release that exact delta before committing either graph or owner.
fn half_turn_shadow_tracks(
    animations: &[AnimationGraphEntry],
    effects: &HashSet<fx_schema::EffectId>,
    budget: &mut AnimationBudget,
) -> Result<Vec<(usize, fx_schema::animator::PropertyAnimator)>, String> {
    use fx_schema::{
        PropertyTarget, PropertyValue,
        animator::{AnimatorData, PropertyAnimator, PropertyKeyframe, PropertyKeyframeTrack},
    };
    let selected = animations.iter().enumerate().filter(|(_,entry)| matches!(&entry.target, PropertyTarget::EffectProperty(t) if effects.contains(&t.effect_id()) && t.param_name() == "offset"));
    let key = |source: &PropertyKeyframe| -> Result<PropertyKeyframe, String> {
        let PropertyValue::Vector2(vector) = source.value() else {
            return Err("post-effect shadow offset requires Vector2 keys".into());
        };
        if !vector.iter().all(|v| v.is_finite()) {
            return Err("nonfinite post-effect shadow offset".into());
        }
        Ok(PropertyKeyframe::new(
            source.id().clone(),
            source.layer_time(),
            PropertyValue::Vector2(vector.map(|v| if v == 0. { 0. } else { -v })),
            source.easing(),
        )
        .with_spatial_tangents(
            source.spatial_in_tangent().map(|v| -v),
            source.spatial_out_tangent().map(|v| -v),
        ))
    };
    let mut sizes = [0_usize; 2];
    for (_, entry) in selected.clone() {
        let AnimatorData::Keyframes {
            track,
            enabled: true,
            disabled_value: None,
        } = entry.animator.data()
        else {
            return Err("post-effect shadow offset requires enabled editable keyframes".into());
        };
        for source in track.keyframes() {
            let transformed = key(source)?;
            for (slot, value) in [source, &transformed].into_iter().enumerate() {
                sizes[slot] = sizes[slot]
                    .checked_add(value.serialized_json_size().map_err(|e| e.to_string())?)
                    .ok_or("rotated shadow track size overflow")?;
            }
        }
    }
    let checkpoint = budget.checkpoint();
    budget
        .reserve(sizes[1].saturating_sub(sizes[0]))
        .map_err(|e| e.to_string())?;
    let result = (|| {
        let mut replacements = Vec::new();
        for (index, entry) in selected {
            let AnimatorData::Keyframes { track, .. } = entry.animator.data() else {
                unreachable!()
            };
            let keys = track
                .keyframes()
                .iter()
                .map(&key)
                .collect::<Result<Vec<_>, _>>()?;
            let track = PropertyKeyframeTrack::new(keys).map_err(|e| e.to_string())?;
            replacements.push((index, PropertyAnimator::keyframes(track)));
        }
        Ok(replacements)
    })();
    match result {
        Ok(replacements) => {
            if let Err(error) = budget.release(sizes[0].saturating_sub(sizes[1])) {
                budget.rollback(checkpoint);
                return Err(error.to_string());
            }
            Ok(replacements)
        }
        Err(error) => {
            budget.rollback(checkpoint);
            Err(error)
        }
    }
}
#[cfg(test)]
mod tests;
