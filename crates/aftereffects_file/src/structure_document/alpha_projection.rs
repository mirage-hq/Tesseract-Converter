//! Bounded native black-composite, Lightness-alpha, black-unmatte pipeline.

use super::{MAX_GROUP_DEPTH, group, reserve_ids, stored_layers, transform};
use crate::{
    properties,
    rifx::Chunk,
    structure::{Layer, SolidSource},
};
use fx_schema::animator::{AnimationGraphEntry, AnimatorData};
use fx_schema::{
    EffectData, EffectPayload, EffectRecord, GroupLayer, LayerData, LayerEffect, LayerId,
    PropertyTarget, TrackMatte, layer::TrackMatteType,
};
use std::collections::HashSet;

const SOLID: &str = "ADBE Solid Composite";
const SHIFT: &str = "ADBE Shift Channels";
const REMOVE: &str = "ADBE Remove Color Matting";

struct Profile {
    last: usize,
}

fn descriptor<'a>(name: &str, run: &'a [Chunk]) -> Result<Vec<(&'a str, &'a [Chunk])>, String> {
    let descriptor = properties::unique_list(run, *b"sspc").map_err(|e| e.to_string())?;
    let table = properties::unique_list(descriptor, *b"parT").map_err(|e| e.to_string())?;
    let defaults: &[(u32, u32, u32, u16)] = match name {
        SOLID => &[
            (0, 0, 0, 0),
            (1, 2, 100 << 16, 0),
            (2, 5, u32::MAX, 0),
            (3, 2, 100 << 16, 0),
            (4, 7, 1, 21),
        ],
        SHIFT => &[
            (0, 0, 0, 0),
            (1, 7, 1, 10),
            (2, 7, 2, 10),
            (3, 7, 3, 10),
            (4, 7, 4, 10),
        ],
        REMOVE => &[(0, 0, 0, 0), (1, 5, 0, 0), (2, 4, 1, 0)],
        _ => unreachable!(),
    };
    if !table.is_empty() {
        let rows = properties::runs(table).map_err(|e| e.to_string())?;
        if rows.len() != defaults.len() + 1 {
            return Err(format!(
                "{name}: requires complete native declarations or a sparse instance"
            ));
        }
        let mut seen = HashSet::new();
        for (parameter, run) in rows {
            if !seen.insert(parameter) {
                return Err("duplicate alpha-pipeline declaration".into());
            }
            let bytes = properties::data(run, *b"pard").map_err(|e| e.to_string())?;
            if bytes.len() != 148 {
                return Err("unsupported alpha-pipeline declaration length".into());
            }
            let (kind, value, choices) = if parameter == "ADBE Effect Built In Params" {
                (9, 0, 0)
            } else {
                let (_, kind, value, choices) = defaults
                    .iter()
                    .find(|(suffix, _, _, _)| parameter == format!("{name}-{suffix:04}"))
                    .ok_or("unknown alpha-pipeline declaration")?;
                (*kind, *value, *choices)
            };
            if bytes[12..16] != kind.to_be_bytes() {
                return Err("alpha-pipeline declaration kind conflict".into());
            }
            // Popup slot56 stores a current value; slot62 stores its default.
            let valid = if kind == 7 {
                bytes[60..62] == choices.to_be_bytes()
                    && bytes[62..64] == (value as u16).to_be_bytes()
            } else {
                bytes[56..60] == value.to_be_bytes()
            };
            if !valid {
                return Err("alpha-pipeline native defaults conflict".into());
            }
        }
    }
    let rows =
        properties::runs(properties::unique_list(descriptor, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    for (parameter, run) in &rows {
        if !seen.insert(*parameter) {
            return Err("duplicate alpha-pipeline control".into());
        }
        if *parameter == "ADBE Group End" {
            continue;
        }
        if *parameter == "ADBE Effect Built In Params" {
            let options = properties::runs(
                properties::unique_list(run, *b"tdgp").map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            if options.iter().any(|(name, _)| *name != "ADBE Group End") {
                return Err("alpha-pipeline compositing options unsupported".into());
            }
        } else if !defaults
            .iter()
            .any(|(suffix, _, _, _)| *parameter == format!("{name}-{suffix:04}"))
        {
            return Err(format!("unsupported alpha-pipeline control {parameter}"));
        }
    }
    Ok(rows)
}
fn numeric(
    rows: &[(&str, &[Chunk])],
    name: &str,
) -> Result<Option<properties::NumericProperty>, String> {
    let Some((_, run)) = rows.iter().find(|(parameter, _)| *parameter == name) else {
        return Ok(None);
    };
    let numeric = properties::read_numeric(
        properties::unique_list(run, *b"tdbs").map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if numeric.animated
        || !numeric.keyframes.is_empty()
        || numeric.expression_present
        || numeric.expression_enabled
        || numeric.dimensions_separated
        || !numeric.values.iter().all(|v| v.is_finite())
    {
        return Err(format!(
            "{name}: requires static finite controls without expressions"
        ));
    }
    Ok(Some(numeric))
}
fn scalar(
    rows: &[(&str, &[Chunk])],
    name: &str,
    default: f64,
    expected: f64,
) -> Result<(), String> {
    let values = numeric(rows, name)?.map_or(vec![default], |n| n.values);
    if values != [expected] {
        return Err(format!("{name}: nondefault alpha-pipeline control"));
    }
    Ok(())
}
fn black(rows: &[(&str, &[Chunk])], name: &str, explicit: bool) -> Result<(), String> {
    let value = numeric(rows, name)?;
    if value.is_none() && explicit {
        return Err("Solid Composite must explicitly author an opaque black background".into());
    }
    let values = value.map_or(vec![0.; 4], |n| n.values);
    if values.len() != 4 || values[..3] != [0.; 3] || (explicit && values[3] != 1.) {
        return Err(format!(
            "{name}: requires black RGB{}",
            if explicit { " and full alpha" } else { "" }
        ));
    }
    Ok(())
}
fn profile(layer: &Layer) -> Result<Option<Profile>, String> {
    let roots = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    let Some((_, parade)) = roots.iter().find(|(name, _)| *name == "ADBE Effect Parade") else {
        return Ok(None);
    };
    let rows =
        properties::runs(properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut triples = rows
        .windows(3)
        .enumerate()
        .filter(|(_, p)| p[0].0 == SOLID && p[1].0 == SHIFT && p[2].0 == REMOVE);
    let Some((index, triple)) = triples.next() else {
        return Ok(None);
    };
    if triples.next().is_some() {
        return Err("multiple alpha pipelines unsupported".into());
    }
    for (name, run) in triple {
        let plugin = properties::unique_list(run, *b"sspc").map_err(|e| e.to_string())?;
        let mut warnings = Vec::new();
        if !properties::group_enabled_or_warn(plugin, name, &mut warnings) || !warnings.is_empty() {
            return Err("requires enabled native alpha-pipeline stages".into());
        }
        let controls = descriptor(name, run)?;
        scalar(&controls, &format!("{name}-0000"), 0., 0.)?;
        match *name {
            SOLID => {
                scalar(&controls, "ADBE Solid Composite-0001", 100., 100.)?;
                black(&controls, "ADBE Solid Composite-0002", true)?;
                scalar(&controls, "ADBE Solid Composite-0003", 100., 100.)?;
                scalar(&controls, "ADBE Solid Composite-0004", 1., 1.)?;
            }
            SHIFT => {
                for (suffix, default, expected) in
                    [(1, 1., 7.), (2, 2., 2.), (3, 3., 3.), (4, 4., 4.)]
                {
                    scalar(
                        &controls,
                        &format!("{SHIFT}-{suffix:04}"),
                        default,
                        expected,
                    )?;
                }
            }
            REMOVE => {
                black(&controls, "ADBE Remove Color Matting-0001", false)?;
                scalar(&controls, "ADBE Remove Color Matting-0002", 1., 0.)?;
            }
            _ => unreachable!(),
        }
    }
    Ok(Some(Profile { last: index + 3 }))
}

pub(super) struct PaintState<'a> {
    pub next: &'a mut u64,
    pub animations: &'a mut Vec<fx_schema::animator::AnimationGraphEntry>,
    pub budget: &'a mut super::animation_budget::AnimationBudget,
}

fn gray(color: &[f64]) -> bool {
    color.len() >= 3
        && color.iter().all(|v| v.is_finite())
        && color[0] == color[1]
        && color[1] == color[2]
}
fn same_color_track(a: &AnimationGraphEntry, b: &AnimationGraphEntry) -> bool {
    if !a.dependencies.is_empty()
        || !b.dependencies.is_empty()
        || a.random_seed_target.is_some()
        || b.random_seed_target.is_some()
        || !a.layer_refs.is_empty()
        || !b.layer_refs.is_empty()
    {
        return false;
    }
    match (a.animator.data(), b.animator.data()) {
        (AnimatorData::Constant { value: a }, AnimatorData::Constant { value: b }) => a == b,
        (
            AnimatorData::Keyframes {
                track: a,
                enabled: true,
                disabled_value: None,
            },
            AnimatorData::Keyframes {
                track: b,
                enabled: true,
                disabled_value: None,
            },
        ) => {
            a.keyframes().len() == b.keyframes().len()
                && a.keyframes().iter().zip(b.keyframes()).all(|(a, b)| {
                    a.layer_time() == b.layer_time()
                        && a.value() == b.value()
                        && a.easing() == b.easing()
                        && a.spatial_in_tangent() == b.spatial_in_tangent()
                        && a.spatial_out_tangent() == b.spatial_out_tangent()
                })
        }
        _ => false,
    }
}
fn full_gray_tint(effect: &EffectRecord, entries: &[AnimationGraphEntry]) -> bool {
    let EffectData::Identified {
        id,
        enabled: true,
        effect:
            EffectPayload::Known(LayerEffect::TintTritone {
                black_r,
                black_g,
                black_b,
                white_r,
                white_g,
                white_b,
                amount,
            }),
        ..
    } = effect.data()
    else {
        return false;
    };
    if amount.unwrap_or(100.) != 100.
        || !gray(&[
            black_r.unwrap_or(0.),
            black_g.unwrap_or(0.),
            black_b.unwrap_or(0.),
        ])
        || !gray(&[
            white_r.unwrap_or(1.),
            white_g.unwrap_or(1.),
            white_b.unwrap_or(1.),
        ])
    {
        return false;
    }
    let tracks: Vec<_> = entries
        .iter()
        .filter(|entry| entry.target.effect_id() == Some(*id))
        .collect();
    if tracks.iter().any(|entry| !matches!(&entry.target,PropertyTarget::EffectProperty(target) if matches!(target.param_name(),"blackR"|"blackG"|"blackB"|"whiteR"|"whiteG"|"whiteB"))) {return false;}
    for names in [
        ["blackR", "blackG", "blackB"],
        ["whiteR", "whiteG", "whiteB"],
    ] {
        let channel = |name| {
            tracks
                .iter()
                .copied()
                .filter(|e| e.target == PropertyTarget::effect_param(*id, name))
                .collect::<Vec<_>>()
        };
        let [r, g, b] = names.map(channel);
        if (!r.is_empty() || !g.is_empty() || !b.is_empty())
            && (r.len() != 1
                || g.len() != 1
                || b.len() != 1
                || !same_color_track(r[0], g[0])
                || !same_color_track(r[0], b[0]))
        {
            return false;
        }
    }
    true
}
fn gray_effects(
    mut neutral: bool,
    effects: &[EffectRecord],
    entries: &[AnimationGraphEntry],
) -> bool {
    for effect in effects {
        if full_gray_tint(effect, entries) {
            neutral = true;
            continue;
        }
        if matches!(effect.data(),EffectData::Identified{id,effect:EffectPayload::Known(LayerEffect::ShiftChannels{take_red_from:fx_schema::effect::ChannelSource::Red,take_green_from:fx_schema::effect::ChannelSource::Green,take_blue_from:fx_schema::effect::ChannelSource::Blue}),..} if !entries.iter().any(|e|e.target.effect_id()==Some(*id)))
        {
            continue;
        }
        if !matches!(
            effect.data(),
            EffectData::Identified { enabled: false, .. }
                | EffectData::Identified {
                    effect: EffectPayload::Known(
                        LayerEffect::Levels { .. }
                            | LayerEffect::Exposure { .. }
                            | LayerEffect::GaussianBlur { .. }
                            | LayerEffect::SimpleChoker { .. }
                            | LayerEffect::LumaKey { .. }
                            | LayerEffect::TurbulentNoise { .. }
                    ),
                    ..
                }
        ) {
            neutral = false;
        }
    }
    neutral
}
fn gray_input(root: &LayerData, entries: &[AnimationGraphEntry], depth: usize) -> bool {
    if depth >= MAX_GROUP_DEPTH {
        return false;
    }
    // isHidden is a static scene flag, with no animatable visibility property.
    // Consumed matte/input branches are handled by the visible parent's proof.
    if match root {
        LayerData::Group(g) => g.is_hidden,
        LayerData::Rect(r) => r.is_hidden,
        LayerData::Text(t) => t.is_hidden,
        LayerData::Image(i) => i.is_hidden,
        LayerData::Video(v) => v.is_hidden,
        LayerData::Media(m) => m.is_hidden,
        LayerData::Shape(s) => s.is_hidden,
        LayerData::Pag(p) => p.is_hidden,
        LayerData::Adjustment(a) => a.is_hidden,
        LayerData::AiEdit(_) => false,
        LayerData::BooleanOperation(b) => b.is_hidden,
        LayerData::Audio(_) => true,
    } {
        return true;
    }
    let neutral = match root {
        LayerData::Group(g) => {
            let mut providers = HashSet::new();
            if let Some(matte) = &g.track_matte {
                providers.insert(matte.layer);
            }
            for child in &g.layers {
                match child.data() {
                    LayerData::Group(c) => {
                        if let Some(matte) = &c.track_matte {
                            providers.insert(matte.layer);
                        }
                    }
                    LayerData::Rect(c) => {
                        if let Some(matte) = &c.track_matte {
                            providers.insert(matte.layer);
                        }
                    }
                    LayerData::Text(c) => {
                        if let Some(matte) = &c.track_matte {
                            providers.insert(matte.layer);
                        }
                    }
                    _ => {}
                }
            }
            for mask in &g.masks {
                providers.extend(mask.layer);
            }
            g.layers
                .iter()
                .filter(|l| !providers.contains(&l.id()))
                .all(|l| gray_input(l.data(), entries, depth + 1))
        }
        LayerData::Rect(r) => {
            !entries.iter().any(|e| {
                e.target.as_property().is_some_and(|p| {
                    p.layer_id() == r.id
                        && matches!(
                            p.property_type(),
                            fx_schema::PropType::FillColor | fx_schema::PropType::StrokeColor
                        )
                })
            }) && r.rect.fill_paint.is_none()
                && (!r.rect.fill_enabled || gray(&r.rect.fill_color[..]))
                && (!r.rect.stroke_enabled
                    || r.rect.stroke_color.as_ref().is_some_and(|c| gray(&c[..])))
        }
        LayerData::Text(t) => {
            let text = &t.source_text;
            (!text.apply_fill || gray(&text.fill_color[..])) && (!text.apply_stroke || text.stroke_color.as_ref().is_some_and(|c|gray(&c[..]))) && t.animators.iter().all(|a|a.fill_color.as_ref().is_none_or(|c|gray(&c[..])) && a.stroke_color.as_ref().is_none_or(|c|gray(&c[..])))
                && !entries.iter().any(|entry|entry.target.as_property().is_some_and(|p|p.layer_id()==t.id && matches!(p.property_type(),fx_schema::PropType::FillColor|fx_schema::PropType::StrokeColor)) || matches!(&entry.target,PropertyTarget::FxItemProperty(target) if t.animators.iter().any(|a|a.id==target.item_id()) && matches!(target.property_name(),"fillColor"|"strokeColor")))
        }
        LayerData::Audio(_) => true,
        _ => false,
    };
    gray_effects(neutral, root.effects(), entries)
}
pub(super) fn apply(
    layer: &Layer,
    owner: &mut GroupLayer,
    size: [u16; 2],
    ordinals: &[usize],
    depth: usize,
    mut state: PaintState<'_>,
) -> Result<bool, String> {
    let checkpoint = state.budget.checkpoint();
    let result = apply_inner(layer, owner, size, ordinals, depth, &mut state);
    if result.is_err() {
        state.budget.rollback(checkpoint);
    }
    result
}

fn apply_inner(
    layer: &Layer,
    owner: &mut GroupLayer,
    size: [u16; 2],
    ordinals: &[usize],
    depth: usize,
    state: &mut PaintState<'_>,
) -> Result<bool, String> {
    let next = &mut *state.next;
    let animations = &mut *state.animations;
    let budget = &mut *state.budget;
    let Some(profile) = profile(layer)? else {
        return Ok(false);
    };
    if !layer.record.flags().effects_active
        || layer.record.flags().three_d_layer
        || layer.record.flags().adjustment_layer
        || size.contains(&0)
    {
        return Err("requires enabled 2D source-plane occurrence".into());
    }
    if owner.layers.len() != 1
        || !owner.masks.is_empty()
        || owner.track_matte.is_some()
        || owner.playback != super::identity_playback(owner.playback.input_range())
    {
        return Err(
            "requires isolated content with occurrence Transform outside alpha pipeline".into(),
        );
    }
    if ordinals.len() > owner.effects.len() || ordinals.windows(2).any(|p| p[0] > p[1]) {
        return Err("effect ordinal metadata is inconsistent".into());
    }
    if depth.checked_add(5).is_none_or(|d| d >= MAX_GROUP_DEPTH) {
        return Err("alpha-pipeline helper depth exceeds allowance".into());
    }
    let prefix = ordinals
        .iter()
        .take_while(|ordinal| **ordinal <= profile.last)
        .count();
    if !gray_effects(
        gray_input(owner.layers[0].data(), animations, depth + 1),
        &owner.effects[..prefix],
        animations,
    ) {
        return Err("Lightness alpha projection requires grayscale paint or a full-strength grayscale Tint; arbitrary media, colored paint and RGB-changing effects are outside the admitted profile".into());
    }
    let mut candidate = owner.clone();
    let mut cursor = *next;
    let first =
        reserve_ids(&mut cursor, 5).ok_or("alpha-pipeline helper identity allocation exhausted")?;
    let mut gate = group(
        LayerId::new(first),
        "Lightness alpha output".into(),
        Some(owner.id),
        owner.playback.input_range(),
    );
    let mut provider = group(
        LayerId::new(first + 1),
        "Opaque black alpha provider".into(),
        Some(gate.id),
        owner.playback.input_range(),
    );
    let mut input = group(
        LayerId::new(first + 2),
        "Source before black composite".into(),
        Some(provider.id),
        owner.playback.input_range(),
    );
    input.effects = candidate.effects.drain(..prefix).collect();
    let mut original = candidate.layers[0].data().clone();
    match &mut original {
        LayerData::Group(g) => g.parent = Some(input.id),
        _ => return Err("requires editable source content Group".into()),
    }
    let (copied, _reservation) =
        super::matte_text_paints::split(&mut original, animations, &mut cursor, depth + 4, budget)?;
    input.layers = stored_layers(vec![original]).map_err(|e| e.to_string())?;
    let rect = |color, parent: &GroupLayer, id| {
        let source = SolidSource {
            width: size[0],
            height: size[1],
            pixel_aspect: (1, 1),
            color,
        };
        let mut rect = transform::solid_rect(&source, parent, id, parent.transform);
        rect.active_range = owner.playback.input_range();
        rect
    };
    provider.layers = stored_layers(vec![
        LayerData::Group(input),
        LayerData::Rect(rect([0.; 3], &provider, LayerId::new(first + 3))),
    ])
    .map_err(|e| e.to_string())?;
    gate.track_matte = Some(TrackMatte {
        mode: TrackMatteType::Luma,
        layer: provider.id,
    });
    gate.layers = stored_layers(vec![
        LayerData::Rect(rect([1.; 3], &gate, LayerId::new(first + 4))),
        LayerData::Group(provider),
    ])
    .map_err(|e| e.to_string())?;
    candidate.layers = stored_layers(vec![LayerData::Group(gate)]).map_err(|e| e.to_string())?;
    fx_schema::Layer::from_data(&LayerData::Group(candidate.clone())).map_err(|e| e.to_string())?;
    animations.extend(copied);
    *owner = candidate;
    *next = cursor;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use crate::{
        rifx::Rifx,
        schema::layer_records::LayerRecord,
        structure::{ItemKind, read_project},
    };
    use fx_schema::{GroupLayer, LayerData};

    fn project(which: usize) -> crate::structure::StructuralProject {
        let mut p = read_project(include_bytes!(
            "../../tests/fixtures/effects/shape_owner_gaussian.aep"
        ))
        .unwrap();
        let ItemKind::Composition(comp) = &p.item(1).unwrap().kind else {
            panic!()
        };
        let template = comp.clone();
        let layer_template = comp.layers[0].clone();
        let fixture = Rifx::parse_with(
            include_bytes!("../../tests/fixtures/effects/native-lightness-alpha-pipeline.rifx"),
            |kind| kind == *b"btdk",
        )
        .unwrap();
        let mut layer = layer_template.clone();
        layer.content = fixture.chunks()[which].children().unwrap().to_vec();
        layer.record =
            LayerRecord::decode(crate::properties::data(&layer.content, *b"ldta").unwrap())
                .unwrap();
        layer.name = "Renamed native alpha occurrence".into();
        let source_id = layer.record.source_id();
        let ItemKind::Composition(comp) = &mut p.items.iter_mut().find(|i| i.id == 1).unwrap().kind
        else {
            panic!()
        };
        comp.width = 3840;
        comp.height = 2160;
        comp.duration_secs = 6.;
        comp.layers = vec![layer];
        let text = Rifx::parse_with(
            include_bytes!("../../tests/fixtures/text/point-scale-vertical-animators.rifx"),
            |kind| kind == *b"btdk",
        )
        .unwrap();
        let mut source = p.items[0].clone();
        source.id = source_id;
        source.name = "Unrelated source text".into();
        let mut content = template;
        content.width = 3840;
        content.height = 2160;
        content.duration_secs = 6.;
        content.layers = text
            .chunks()
            .iter()
            .map(|chunk| {
                let mut layer = layer_template.clone();
                layer.content = chunk.children().unwrap().to_vec();
                layer.record =
                    LayerRecord::decode(crate::properties::data(&layer.content, *b"ldta").unwrap())
                        .unwrap();
                layer
            })
            .collect();
        source.kind = ItemKind::Composition(content);
        p.items.push(source);
        p
    }
    fn group(layer: &fx_schema::Layer) -> &GroupLayer {
        let LayerData::Group(g) = layer.data() else {
            panic!()
        };
        g
    }

    fn list_mut(chunks: &mut [crate::rifx::Chunk], kind: [u8; 4]) -> &mut Vec<crate::rifx::Chunk> {
        chunks
            .iter_mut()
            .find(|c| c.list_kind() == Some(kind))
            .unwrap()
            .children_mut()
            .unwrap()
    }
    fn named_mut<'a>(
        chunks: &'a mut [crate::rifx::Chunk],
        name: &str,
        kind: [u8; 4],
    ) -> &'a mut Vec<crate::rifx::Chunk> {
        let start = chunks
            .iter()
            .position(|c| {
                c.id() == *b"tdmn" && c.data_payload().unwrap().starts_with(name.as_bytes())
            })
            .unwrap();
        let end = chunks[start + 1..]
            .iter()
            .position(|c| c.id() == *b"tdmn")
            .map_or(chunks.len(), |i| start + 1 + i);
        list_mut(&mut chunks[start..end], kind)
    }
    fn plugin<'a>(
        layer: &'a mut crate::structure::Layer,
        effect: &str,
    ) -> &'a mut Vec<crate::rifx::Chunk> {
        let root = list_mut(&mut layer.content, *b"tdgp");
        let parade = named_mut(root, "ADBE Effect Parade", *b"tdgp");
        named_mut(parade, effect, *b"sspc")
    }
    fn leaf<'a>(
        layer: &'a mut crate::structure::Layer,
        parameter: &str,
    ) -> &'a mut Vec<crate::rifx::Chunk> {
        let effect = parameter.rsplit_once('-').unwrap().0;
        named_mut(
            list_mut(plugin(layer, effect), *b"tdgp"),
            parameter,
            *b"tdbs",
        )
    }
    fn replace(chunks: &mut [crate::rifx::Chunk], tag: [u8; 4], data: Vec<u8>) {
        *chunks.iter_mut().find(|c| c.id() == tag).unwrap() =
            crate::rifx::Chunk::data(tag, data).unwrap();
    }
    fn scalar(layer: &mut crate::structure::Layer, name: &str, value: f64) {
        replace(leaf(layer, name), *b"cdat", value.to_be_bytes().to_vec());
    }
    fn main_layer(p: &mut crate::structure::StructuralProject) -> &mut crate::structure::Layer {
        let ItemKind::Composition(comp) = &mut p.items.iter_mut().find(|i| i.id == 1).unwrap().kind
        else {
            panic!()
        };
        &mut comp.layers[0]
    }

    #[test]
    fn native_lightness_alpha_pipeline_retains_editable_source_and_tint_before_black_composite() {
        for which in 0..2 {
            let converted =
                crate::structure_document::to_structural_fx_document(&project(which), Some(1))
                    .unwrap();
            let owner = group(&group(&converted.document.composition().layers()[0]).layers[0]);
            let gate = group(&owner.layers[0]);
            let matte = gate.track_matte.as_ref().unwrap_or_else(|| {
                panic!("missing Lightness alpha gate: {:?}", converted.diagnostics)
            });
            assert_eq!(matte.mode, fx_schema::layer::TrackMatteType::Luma);
            let provider = gate
                .layers
                .iter()
                .find(|l| l.id() == matte.layer)
                .map(group)
                .unwrap();
            assert_eq!(provider.layers.len(), 2);
            let input = group(&provider.layers[0]);
            assert!(!input.layers.is_empty());
            assert!(
                provider.effects.is_empty(),
                "Tint must not recolor the opaque black background"
            );
            let LayerData::Rect(background) = provider.layers[1].data() else {
                panic!()
            };
            assert_eq!(background.rect.fill_color, [0., 0., 0., 1.]);
            assert!(input.effects.len() >= usize::from(which == 1));
            let mut inhibited = project(which);
            scalar(main_layer(&mut inhibited), "ADBE Shift Channels-0001", 1.);
            let baseline =
                crate::structure_document::to_structural_fx_document(&inhibited, Some(1)).unwrap();
            let before = baseline.document.composition().dynamics().entries();
            let after = converted.document.composition().dynamics().entries();
            assert!(before.iter().all(|entry| after.contains(entry)));
            let copies: Vec<_> = after
                .iter()
                .filter(|entry| {
                    entry.animator.keyframe_track().is_some_and(|track| {
                        track
                            .keyframes()
                            .iter()
                            .all(|key| key.id().as_str().starts_with("aep-matte-paint-"))
                    })
                })
                .collect();
            assert!(
                !copies.is_empty(),
                "stroke copy retains mirrored text animation"
            );
            let copied_bytes: usize = copies
                .iter()
                .map(|entry| serde_json::to_vec(entry).unwrap().len() + 1)
                .sum();
            assert_eq!(
                converted.animation_budget_used - baseline.animation_budget_used,
                copied_bytes
            );
            fn identities(value: &serde_json::Value, seen: &mut std::collections::HashSet<u64>) {
                match value {
                    serde_json::Value::Object(object) => {
                        if let Some(id) = object.get("id").and_then(serde_json::Value::as_u64) {
                            assert!(seen.insert(id), "duplicate scene identity {id}");
                        }
                        for value in object.values() {
                            identities(value, seen);
                        }
                    }
                    serde_json::Value::Array(values) => {
                        for value in values {
                            identities(value, seen);
                        }
                    }
                    _ => {}
                }
            }
            identities(
                &converted.document.to_json_value().unwrap(),
                &mut Default::default(),
            );
            fn paints(layer: &fx_schema::Layer, found: &mut usize) {
                if let LayerData::Group(group) = layer.data() {
                    for pair in group.layers.windows(2) {
                        if let (LayerData::Text(first), LayerData::Text(second)) =
                            (pair[0].data(), pair[1].data())
                            && first.source_text.text == second.source_text.text
                        {
                            assert_eq!(
                                first.source_text.apply_stroke,
                                first.source_text.stroke_over_fill
                            );
                            assert_ne!(
                                first.source_text.apply_stroke,
                                second.source_text.apply_stroke
                            );
                            assert_ne!(first.source_text.apply_fill, second.source_text.apply_fill);
                            let mut a = first.source_text.clone();
                            let mut b = second.source_text.clone();
                            a.apply_fill = true;
                            a.apply_stroke = true;
                            b.apply_fill = true;
                            b.apply_stroke = true;
                            assert_eq!(
                                a, b,
                                "source text/font/geometry/color must remain unchanged"
                            );
                            *found += 1;
                        }
                    }
                    for child in &group.layers {
                        paints(child, found);
                    }
                } else if let LayerData::Text(text) = layer.data() {
                    assert!(!(text.source_text.apply_fill && text.source_text.apply_stroke));
                }
            }
            let mut found = 0;
            for child in &input.layers {
                paints(child, &mut found);
            }
            assert!(
                found > 0,
                "native provider contains ordered fill/stroke siblings"
            );
            if which == 1 {
                let tint =
                    input
                        .effects
                        .iter()
                        .find_map(|e| match e.data() {
                            fx_schema::EffectData::Identified {
                                id,
                                effect:
                                    fx_schema::EffectPayload::Known(
                                        fx_schema::LayerEffect::TintTritone { .. },
                                    ),
                                ..
                            } => Some(*id),
                            _ => None,
                        })
                        .unwrap();
                let red = fx_schema::PropertyTarget::effect_param(tint, "blackR");
                let track = converted
                    .document
                    .composition()
                    .dynamics()
                    .entries()
                    .iter()
                    .find(|e| e.target == red)
                    .unwrap()
                    .animator
                    .keyframe_track()
                    .unwrap();
                assert_eq!(track.keyframes().len(), 3);
                assert_eq!(track.keyframes()[1].layer_time().as_millis(), 667);
            }
        }
    }
    #[test]
    fn alpha_pipeline_control_and_topology_rejections_leave_owner_and_allocator_unchanged() {
        let p = project(1);
        let mut inhibited = p.clone();
        scalar(main_layer(&mut inhibited), "ADBE Shift Channels-0001", 1.);
        let baseline =
            crate::structure_document::to_structural_fx_document(&inhibited, Some(1)).unwrap();
        let original =
            group(&group(&baseline.document.composition().layers()[0]).layers[0]).clone();
        let mut source = p.clone();
        let original_layer = main_layer(&mut source).clone();
        let imported = super::super::effects::import(
            &Default::default(),
            1,
            &original_layer,
            [3840, 2160],
            [3840, 2160],
            &mut 10000,
            &mut Default::default(),
        );
        for case in 0..9 {
            let mut layer = original_layer.clone();
            let mut owner = original.clone();
            let mut cursor = 100000;
            match case {
                0 => replace(
                    leaf(&mut layer, "ADBE Solid Composite-0002"),
                    *b"cdat",
                    [255_f64, 0., 1., 0.]
                        .into_iter()
                        .flat_map(f64::to_be_bytes)
                        .collect(),
                ),
                1 => scalar(&mut layer, "ADBE Shift Channels-0001", 8.),
                2 => scalar(&mut layer, "ADBE Remove Color Matting-0002", 1.),
                3 => replace(
                    list_mut(plugin(&mut layer, "ADBE Shift Channels"), *b"tdgp"),
                    *b"tdsb",
                    vec![0; 4],
                ),
                4 | 5 => {
                    let leaf = leaf(&mut layer, "ADBE Shift Channels-0001");
                    let mut meta = crate::properties::data(leaf, *b"tdb4").unwrap().to_vec();
                    meta[if case == 4 { 68 } else { 120 }] = 1;
                    replace(leaf, *b"tdb4", meta);
                }
                6 => {
                    let options = named_mut(
                        list_mut(plugin(&mut layer, "ADBE Solid Composite"), *b"tdgp"),
                        "ADBE Effect Built In Params",
                        *b"tdgp",
                    );
                    options.push(
                        crate::rifx::Chunk::data(*b"tdmn", b"Unsupported Option\0".to_vec())
                            .unwrap(),
                    );
                }
                7 => {
                    owner.track_matte = Some(fx_schema::TrackMatte {
                        mode: fx_schema::layer::TrackMatteType::Alpha,
                        layer: owner.id,
                    })
                }
                8 => cursor = u64::MAX - 3,
                _ => unreachable!(),
            }
            let before = owner.clone();
            let initial = cursor;
            assert!(
                super::apply(
                    &layer,
                    &mut owner,
                    [3840, 2160],
                    &imported.native_ordinals,
                    0,
                    super::PaintState {
                        next: &mut cursor,
                        animations: &mut Vec::new(),
                        budget: &mut Default::default()
                    }
                )
                .is_err(),
                "case {case}"
            );
            assert_eq!(owner, before, "case {case}");
            assert_eq!(cursor, initial, "case {case}");
        }
        let mut owner = original.clone();
        let before = owner.clone();
        let mut cursor = 100000;
        assert!(
            super::apply(
                &original_layer,
                &mut owner,
                [3840, 2160],
                &imported.native_ordinals,
                super::MAX_GROUP_DEPTH - 5,
                super::PaintState {
                    next: &mut cursor,
                    animations: &mut Vec::new(),
                    budget: &mut Default::default()
                }
            )
            .is_err()
        );
        assert_eq!(owner, before);
        assert_eq!(cursor, 100000);
    }
    #[test]
    fn text_paint_late_failures_rollback_owner_ids_graph_and_reserved_bytes() {
        let mut native = project(1);
        let source = main_layer(&mut native).clone();
        let mut inhibited = native.clone();
        scalar(main_layer(&mut inhibited), "ADBE Shift Channels-0001", 1.);
        let baseline =
            crate::structure_document::to_structural_fx_document(&inhibited, Some(1)).unwrap();
        let original =
            group(&group(&baseline.document.composition().layers()[0]).layers[0]).clone();
        let imported = super::super::effects::import(
            &Default::default(),
            1,
            &source,
            [3840, 2160],
            [3840, 2160],
            &mut 10000,
            &mut Default::default(),
        );
        fn opacity(layer: &mut LayerData, index: &mut usize, wanted: usize) {
            match layer {
                LayerData::Text(text) => {
                    if *index == wanted {
                        text.transform.opacity = fx_schema::PercentageProperty::new(50.).unwrap();
                    }
                    *index += 1;
                }
                LayerData::Group(group) => {
                    for stored in &mut group.layers {
                        let mut child = stored.data().clone();
                        opacity(&mut child, index, wanted);
                        *stored = fx_schema::Layer::from_data(&child).unwrap();
                    }
                }
                _ => {}
            }
        }
        for case in 0..3 {
            let mut owner = original.clone();
            if case > 0 {
                let mut count = 0;
                for child in &mut owner.layers {
                    let mut data = child.data().clone();
                    opacity(&mut data, &mut count, case - 1);
                    *child = fx_schema::Layer::from_data(&data).unwrap();
                }
                assert!(count >= 2);
            }
            let before = owner.clone();
            let mut cursor = 100000;
            let mut animations = baseline
                .document
                .composition()
                .dynamics()
                .entries()
                .to_vec();
            let graph = animations.clone();
            let mut budget = if case == 0 {
                super::super::animation_budget::AnimationBudget::with_limit(16)
            } else {
                Default::default()
            };
            budget.reserve(8).unwrap();
            let used = budget.used();
            let error = super::apply(
                &source,
                &mut owner,
                [3840, 2160],
                &imported.native_ordinals,
                0,
                super::PaintState {
                    next: &mut cursor,
                    animations: &mut animations,
                    budget: &mut budget,
                },
            )
            .unwrap_err();
            assert!(
                error.contains(if case == 0 { "budget" } else { "opacity" }),
                "{error}"
            );
            assert_eq!(owner, before);
            assert_eq!(cursor, 100000);
            assert_eq!(animations, graph);
            assert_eq!(budget.used(), used);
        }
    }
    fn color_text(layer: &mut fx_schema::Layer) -> bool {
        let mut data = layer.data().clone();
        let changed = match &mut data {
            LayerData::Group(g) => g.layers.iter_mut().any(color_text),
            LayerData::Text(t) => {
                t.source_text.fill_color = [0.2, 0.5, 0.8, 1.];
                true
            }
            _ => false,
        };
        if changed {
            *layer = fx_schema::Layer::from_data(&data).unwrap();
        }
        changed
    }
    #[test]
    fn colored_paint_is_rejected_atomically_but_full_gray_tint_retains_native_profile() {
        for which in 0..2 {
            let p = project(which);
            let mut inhibited = p.clone();
            scalar(main_layer(&mut inhibited), "ADBE Shift Channels-0001", 1.);
            let baseline =
                crate::structure_document::to_structural_fx_document(&inhibited, Some(1)).unwrap();
            let mut owner =
                group(&group(&baseline.document.composition().layers()[0]).layers[0]).clone();
            assert!(color_text(&mut owner.layers[0]));
            let mut source = p.clone();
            let layer = main_layer(&mut source).clone();
            let imported = super::super::effects::import(
                &Default::default(),
                1,
                &layer,
                [3840, 2160],
                [3840, 2160],
                &mut 10000,
                &mut Default::default(),
            );
            let before = owner.clone();
            let mut next = 100000;
            let mut entries = baseline
                .document
                .composition()
                .dynamics()
                .entries()
                .to_vec();
            let initial = entries.clone();
            let mut budget = super::super::animation_budget::AnimationBudget::default();
            let result = super::apply(
                &layer,
                &mut owner,
                [3840, 2160],
                &imported.native_ordinals,
                0,
                super::PaintState {
                    next: &mut next,
                    animations: &mut entries,
                    budget: &mut budget,
                },
            );
            if which == 0 {
                assert!(result.is_err());
                assert_eq!(owner, before);
                assert_eq!(next, 100000);
                assert_eq!(entries, initial);
                assert_eq!(budget.used(), 0);
            } else {
                assert!(result.unwrap());
            }
        }
    }
    #[test]
    fn grayscale_tint_proof_rejects_partial_amount_and_mismatched_channel_motion() {
        use fx_schema::{
            EffectId, EffectRecord, LayerEffect, PropertyAnimator, PropertyTarget, PropertyValue,
        };
        let id = EffectId::new(7);
        let effect = |amount| {
            EffectRecord::from_data(&fx_schema::EffectData::Identified {
                id,
                enabled: true,
                effect: fx_schema::EffectPayload::Known(LayerEffect::TintTritone {
                    black_r: Some(0.2),
                    black_g: Some(0.2),
                    black_b: Some(0.2),
                    white_r: None,
                    white_g: None,
                    white_b: None,
                    amount: Some(amount),
                }),
                compositing_options: None,
                extensions: Default::default(),
            })
            .unwrap()
        };
        assert!(super::full_gray_tint(&effect(100.), &[]));
        assert!(!super::full_gray_tint(&effect(50.), &[]));
        let entry = |name, value| fx_schema::animator::AnimationGraphEntry {
            target: PropertyTarget::effect_param(id, name),
            animator: PropertyAnimator::constant(PropertyValue::Float(value)).unwrap(),
            dependencies: vec![],
            random_seed_target: None,
            layer_refs: Default::default(),
        };
        let entries = [
            entry("blackR", 0.4),
            entry("blackG", 0.4),
            entry("blackB", 0.4),
        ];
        assert!(super::full_gray_tint(&effect(100.), &entries));
        assert!(!super::full_gray_tint(&effect(100.), &entries[..2]));
        let mut changed = entries.clone();
        changed[2] = entry("blackB", 0.5);
        assert!(!super::full_gray_tint(&effect(100.), &changed));
    }

    #[test]
    fn grayscale_rect_with_colored_paint_track_is_declined_before_mutation() {
        let p = project(0);
        let mut inhibited = p.clone();
        scalar(main_layer(&mut inhibited), "ADBE Shift Channels-0001", 1.);
        let baseline =
            crate::structure_document::to_structural_fx_document(&inhibited, Some(1)).unwrap();
        let mut owner =
            group(&group(&baseline.document.composition().layers()[0]).layers[0]).clone();
        let mut content = group(&owner.layers[0]).clone();
        let solid = crate::structure::SolidSource {
            width: 3840,
            height: 2160,
            pixel_aspect: (1, 1),
            color: [0.5; 3],
        };
        let rect = super::transform::solid_rect(
            &solid,
            &content,
            fx_schema::LayerId::new(95000),
            content.transform,
        );
        content.layers = super::stored_layers(vec![LayerData::Rect(rect)]).unwrap();
        owner.layers = super::stored_layers(vec![LayerData::Group(content)]).unwrap();
        let mut source = p.clone();
        let layer = main_layer(&mut source).clone();
        let imported = super::super::effects::import(
            &Default::default(),
            1,
            &layer,
            [3840, 2160],
            [3840, 2160],
            &mut 10000,
            &mut Default::default(),
        );
        let entry = fx_schema::animator::AnimationGraphEntry {
            target: fx_schema::PropertyTarget::layer(
                fx_schema::LayerId::new(95000),
                fx_schema::PropType::FillColor,
            ),
            animator: fx_schema::PropertyAnimator::constant(fx_schema::PropertyValue::Color([
                1., 0., 0., 1.,
            ]))
            .unwrap(),
            dependencies: vec![],
            random_seed_target: None,
            layer_refs: Default::default(),
        };
        let mut entries = vec![entry];
        let before_entries = entries.clone();
        let before = owner.clone();
        let mut next = 100000;
        let mut budget = super::super::animation_budget::AnimationBudget::default();
        assert!(
            super::apply(
                &layer,
                &mut owner,
                [3840, 2160],
                &imported.native_ordinals,
                0,
                super::PaintState {
                    next: &mut next,
                    animations: &mut entries,
                    budget: &mut budget
                }
            )
            .is_err()
        );
        assert_eq!(owner, before);
        assert_eq!(entries, before_entries);
        assert_eq!(next, 100000);
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn hidden_colored_guides_do_not_taint_visible_grayscale_source() {
        let p = project(0);
        let mut inhibited = p.clone();
        scalar(main_layer(&mut inhibited), "ADBE Shift Channels-0001", 1.);
        let baseline =
            crate::structure_document::to_structural_fx_document(&inhibited, Some(1)).unwrap();
        let original =
            group(&group(&baseline.document.composition().layers()[0]).layers[0]).clone();
        let mut source = p.clone();
        let layer = main_layer(&mut source).clone();
        let imported = super::super::effects::import(
            &Default::default(),
            1,
            &layer,
            [3840, 2160],
            [3840, 2160],
            &mut 10000,
            &mut Default::default(),
        );
        for hidden in [false, true] {
            let mut owner = original.clone();
            let mut content = group(&owner.layers[0]).clone();
            let mut guide = super::group(
                fx_schema::LayerId::new(95000),
                "Unrelated colored guide".into(),
                Some(content.id),
                content.playback.input_range(),
            );
            guide.is_hidden = hidden;
            let solid = crate::structure::SolidSource {
                width: 3840,
                height: 2160,
                pixel_aspect: (1, 1),
                color: [0., 0.8, 1.],
            };
            let rect = super::transform::solid_rect(
                &solid,
                &guide,
                fx_schema::LayerId::new(95001),
                guide.transform,
            );
            guide.layers = super::stored_layers(vec![LayerData::Rect(rect)]).unwrap();
            content
                .layers
                .push(fx_schema::Layer::from_data(&LayerData::Group(guide)).unwrap());
            owner.layers = super::stored_layers(vec![LayerData::Group(content)]).unwrap();
            let before = owner.clone();
            let mut next = 100000;
            let mut entries = baseline
                .document
                .composition()
                .dynamics()
                .entries()
                .to_vec();
            let mut budget = super::super::animation_budget::AnimationBudget::default();
            let result = super::apply(
                &layer,
                &mut owner,
                [3840, 2160],
                &imported.native_ordinals,
                0,
                super::PaintState {
                    next: &mut next,
                    animations: &mut entries,
                    budget: &mut budget,
                },
            );
            if hidden {
                assert!(result.unwrap());
            } else {
                assert!(result.is_err());
                assert_eq!(owner, before);
                assert_eq!(next, 100000);
                assert_eq!(budget.used(), 0);
            }
        }
    }
}
