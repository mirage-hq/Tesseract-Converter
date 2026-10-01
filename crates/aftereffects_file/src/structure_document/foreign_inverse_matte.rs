//! One-hop native mask-path alias, copied independently before a bounded inverse stage.
//! This never samples an emitted provider or evaluates JavaScript.

use super::{
    animation::NumericAnimationClock,
    animation_budget::AnimationBudget,
    control_links::{display_name, expression, finished, quoted, token, unique_run},
    self_inverse_matte::{self, Profile, static_values},
    shapes::path,
    transform,
};
use crate::{
    properties,
    rifx::Chunk,
    structure::{Composition, Layer, ProjectItem},
};
use fx_schema::{GroupLayer, LayerData, PropType, PropertyTarget, animator::AnimationGraphEntry};

pub(super) struct Context<'a> {
    pub comp: &'a Composition,
    pub items: &'a std::collections::HashMap<u32, &'a ProjectItem>,
    pub depth: usize,
}
fn alias(mut text: &str) -> Option<(&str, &str)> {
    if text.len() > 16 * 1024 {
        return None;
    }
    for part in ["thisComp", ".", "layer", "("] {
        token(&mut text, part)?;
    }
    let layer = quoted(&mut text)?;
    for part in [")", ".", "mask", "("] {
        token(&mut text, part)?;
    }
    let mask = quoted(&mut text)?;
    for part in [")", ".", "maskPath"] {
        token(&mut text, part)?;
    }
    finished(text).then_some((layer, mask))
}
fn mask_atom(layer: &Layer) -> Result<&[Chunk], String> {
    let root = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    let parade = unique_run(&root, "ADBE Mask Parade").map_err(|e| e.to_string())?;
    let atoms =
        properties::runs(properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let [("ADBE Mask Atom", atom)] = atoms.as_slice() else {
        return Err("requires exactly one native mask".into());
    };
    Ok(atom)
}
fn mask_path(layer: &Layer) -> Result<&[Chunk], String> {
    let controls =
        properties::unique_list(mask_atom(layer)?, *b"tdgp").map_err(|e| e.to_string())?;
    unique_run(
        &properties::runs(controls).map_err(|e| e.to_string())?,
        "ADBE Mask Shape",
    )
    .map_err(|e| e.to_string())
}
fn list_mut(chunks: &mut [Chunk], kind: [u8; 4]) -> Result<&mut Vec<Chunk>, String> {
    let mut indexes = chunks
        .iter()
        .enumerate()
        .filter(|(_, c)| c.list_kind() == Some(kind))
        .map(|(i, _)| i);
    let i = indexes.next().ok_or("missing property list")?;
    if indexes.next().is_some() {
        return Err("duplicate property list".into());
    }
    chunks[i]
        .children_mut()
        .ok_or("missing property children".into())
}
fn named_list_mut<'a>(
    chunks: &'a mut [Chunk],
    name: &str,
    kind: [u8; 4],
) -> Result<&'a mut Vec<Chunk>, String> {
    let start = chunks
        .iter()
        .position(|c| {
            c.id() == *b"tdmn"
                && c.data_payload().is_some_and(|b| {
                    b[..b.iter().rposition(|v| *v != 0).map_or(0, |i| i + 1)] == *name.as_bytes()
                })
        })
        .ok_or("missing named property")?;
    let end = chunks[start + 1..]
        .iter()
        .position(|c| c.id() == *b"tdmn")
        .map_or(chunks.len(), |i| start + 1 + i);
    list_mut(&mut chunks[start + 1..end], kind)
}
fn resolved(layer: &Layer, provider: &Layer) -> Result<Layer, String> {
    let mut copy = layer.clone();
    let root = list_mut(&mut copy.content, *b"tdgp")?;
    let parade = named_list_mut(root, "ADBE Mask Parade", *b"tdgp")?;
    let atom = named_list_mut(parade, "ADBE Mask Atom", *b"tdgp")?;
    let outline = named_list_mut(atom, "ADBE Mask Shape", *b"om-s")?;
    *outline = properties::unique_list(mask_path(provider)?, *b"om-s")
        .map_err(|e| e.to_string())?
        .to_vec();
    Ok(copy)
}
fn native_provider(layer: &Layer) -> Result<Option<u32>, String> {
    let roots = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    let Some((_, parade)) = roots.iter().find(|(name, _)| *name == "ADBE Effect Parade") else {
        return Ok(None);
    };
    let effects =
        properties::runs(properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut mattes = effects
        .iter()
        .filter(|(name, _)| *name == "ADBE Set Matte3");
    let Some((_, matte)) = mattes.next() else {
        return Ok(None);
    };
    if mattes.next().is_some() {
        return Err("ambiguous native Set Matte provider".into());
    }
    let descriptor = properties::unique_list(matte, *b"sspc").map_err(|e| e.to_string())?;
    let controls =
        properties::runs(properties::unique_list(descriptor, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let run = unique_run(&controls, "ADBE Set Matte3-0001").map_err(|e| e.to_string())?;
    let leaf = properties::unique_list(run, *b"tdbs").map_err(|e| e.to_string())?;
    let bytes = properties::data(leaf, *b"tdpi").map_err(|e| e.to_string())?;
    let id = u32::from_be_bytes(
        bytes
            .try_into()
            .map_err(|_| "invalid native matte provider ID")?,
    );
    Ok((id != layer.record.id()).then_some(id))
}

fn equal_authored_paths(a: &Layer, b: &Layer) -> Result<(), String> {
    self_inverse_matte::raw_mask(a)?;
    self_inverse_matte::raw_mask(b)?;
    let a = properties::unique_list(mask_path(a)?, *b"om-s").map_err(|e| e.to_string())?;
    let b = properties::unique_list(mask_path(b)?, *b"om-s").map_err(|e| e.to_string())?;
    let metadata = |run| {
        properties::unique_list(run, *b"tdbs")
            .and_then(properties::read_path_metadata)
            .map_err(|e| e.to_string())
    };
    if metadata(a)? != metadata(b)?
        || properties::unique_list(a, *b"omks").map_err(|e| e.to_string())?
            != properties::unique_list(b, *b"omks").map_err(|e| e.to_string())?
    {
        return Err("foreign provider and destination authored mask controls differ".into());
    }
    Ok(())
}

fn provider<'a>(
    layer: &Layer,
    context: &'a Context<'_>,
) -> Result<Option<(&'a Layer, bool)>, String> {
    let path = match mask_path(layer) {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let outline = properties::unique_list(path, *b"om-s").map_err(|e| e.to_string())?;
    let leaf = properties::unique_list(outline, *b"tdbs").map_err(|e| e.to_string())?;
    let meta = properties::read_path_metadata(leaf).map_err(|e| e.to_string())?;
    let provider = if meta.expression_enabled {
        let (name, mask) = alias(expression(leaf).map_err(|e| e.to_string())?)
            .ok_or("unsupported mask alias expression")?;
        let mut matches = context
            .comp
            .layers
            .iter()
            .filter(|l| l.name.as_ref() == name);
        let provider = matches.next().ok_or("mask provider absent")?;
        if matches.next().is_some() || provider.record.id() == layer.record.id() {
            return Err("ambiguous or self mask alias".into());
        }
        if display_name(
            properties::unique_list(mask_atom(provider)?, *b"tdgp").map_err(|e| e.to_string())?,
        ) != Some(mask)
        {
            return Err("mask alias name is missing or ambiguous".into());
        }
        provider
    } else {
        let Some(id) = native_provider(layer)? else {
            return Ok(None);
        };
        let mut matches = context.comp.layers.iter().filter(|l| l.record.id() == id);
        let provider = matches.next().ok_or("native mask provider absent")?;
        if matches.next().is_some() {
            return Err("ambiguous native mask provider".into());
        }
        equal_authored_paths(layer, provider)?;
        provider
    };
    self_inverse_matte::raw_mask(provider)?;
    let r = &provider.record;
    let f = r.flags();
    if !f.enabled
        || !f.effects_active
        || f.three_d_layer
        || f.adjustment_layer
        || f.preserve_transparency
        || r.parent_id() != 0
        || r.track_matte_type() != 0
        || r.layer_type() != 0
        || r.blend_mode() != 2
        || r.stretch() != Some(1.)
        || layer.record.stretch() != Some(1.)
        || r.start_time_fraction() != layer.record.start_time_fraction()
    {
        return Err("provider clock/geometry is not independent identity2D".into());
    }
    let source = context
        .items
        .get(&r.source_id())
        .and_then(|s| s.solid.as_ref())
        .and_then(|s| s.as_ref().ok())
        .ok_or("provider is not a decoded solid")?;
    let destination = context
        .items
        .get(&layer.record.source_id())
        .and_then(|s| s.solid.as_ref())
        .and_then(|s| s.as_ref().ok())
        .ok_or("destination is not a decoded solid")?;
    if source != destination || source.pixel_aspect != (1, 1) {
        return Err("provider/destination source bounds or solid differ".into());
    }
    let native = properties::read_transform(&provider.content).map_err(|e| e.to_string())?;
    if native.iter().any(|p| {
        !p.numeric.as_ref().is_ok_and(|n| {
            !n.animated && n.keyframes.is_empty() && !n.expression_present && !n.expression_enabled
        })
    }) {
        return Err("provider Transform is not static".into());
    }
    let (t, _) = transform::static_transform(provider, [source.width, source.height], context.comp);
    if t.anchor_point != [t.position.x(), t.position.y()]
        || t.scale != [100., 100.]
        || t.opacity.value() != 100.
        || t.rotation != 0.
        || t.rotation_x != 0.
        || t.rotation_y != 0.
        || t.skew != 0.
        || t.orientation != [0., 0., 0.]
    {
        return Err("provider Transform is not identity affine".into());
    }
    for (name, run) in properties::root_runs(&provider.content).map_err(|e| e.to_string())? {
        if name == "ADBE Time Remapping" {
            static_values(run, &[0.])?;
        }
    }
    Ok(Some((provider, meta.expression_enabled)))
}

pub(super) fn apply(
    context: Context<'_>,
    layer: &Layer,
    owner: &mut GroupLayer,
    animations: &mut [AnimationGraphEntry],
    next_id: &mut u64,
    budget: &mut AnimationBudget,
) -> Result<Option<Vec<AnimationGraphEntry>>, String> {
    let Some((provider, expression_alias)) = provider(layer, &context)? else {
        return Ok(None);
    };
    let source = context.items.get(&layer.record.source_id()).copied();
    if !expression_alias {
        let applied = self_inverse_matte::apply_profile(
            layer,
            self_inverse_matte::Context {
                source,
                items: context.items,
                depth: context.depth,
            },
            owner,
            animations,
            next_id,
            Profile::ForeignLayer {
                provider: provider.record.id(),
            },
            budget,
        )?;
        return Ok(applied.then(Vec::new));
    }
    let resolved = resolved(layer, provider)?;
    let size = source
        .and_then(|s| s.solid.as_ref())
        .and_then(|s| s.as_ref().ok())
        .map(|s| [f64::from(s.width), f64::from(s.height)])
        .ok_or("missing solid bounds")?;
    let mut candidate = owner.clone();
    let mut id = *next_id;
    if candidate.layers.len() != 2 {
        return Err("unexpected foreign mask topology".into());
    }
    let LayerData::Shape(mut guide) = candidate.layers[1].data().clone() else {
        return Err("missing destination mask guide".into());
    };
    if animations
        .iter()
        .any(|e| e.target.layer_id() == Some(guide.id))
    {
        return Err("destination guide already has authored animation".into());
    }
    let (shape, _) = path::decode_first(mask_path(provider)?, size).map_err(|e| e.to_string())?;
    guide.shape.path = shape;
    candidate.layers[1] =
        fx_schema::Layer::from_data(&LayerData::Shape(guide.clone())).map_err(|e| e.to_string())?;
    let checkpoint = budget.checkpoint();
    let built = (|| {
        let clock = NumericAnimationClock::parent_identity(layer)?;
        let (entries, warnings) = path::entries(
            mask_path(provider)?,
            size,
            PropertyTarget::layer(guide.id, PropType::ShapePath),
            clock,
            budget,
        );
        if !warnings.is_empty() {
            return Err(warnings.join("; "));
        }
        if !self_inverse_matte::apply_profile(
            &resolved,
            self_inverse_matte::Context {
                source,
                items: context.items,
                depth: context.depth,
            },
            &mut candidate,
            animations,
            &mut id,
            Profile::ForeignLayer {
                provider: provider.record.id(),
            },
            budget,
        )? {
            return Err("foreign effect profile absent".into());
        }
        Ok(entries)
    })();
    match built {
        Ok(entries) => {
            *owner = candidate;
            *next_id = id;
            Ok(Some(entries))
        }
        Err(error) => {
            budget.rollback(checkpoint);
            Err(error)
        }
    }
}
#[cfg(test)]
mod tests;
