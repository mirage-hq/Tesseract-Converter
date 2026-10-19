//! Bounded zero-fill inside strokes, without a new FX fill-opacity control.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum FillMode {
    StaticZero,
    InitialZeroHold,
}
use super::super::{
    MAX_GROUP_DEPTH, group, reserve_ids, shapes::OutputBudget, stored_layers, transform,
};
use crate::{
    layer_styles::NativeLayerStyle,
    properties,
    rifx::Chunk,
    structure::{Layer, SolidSource},
};
use fx_schema::{
    EffectData, EffectPayload, EffectRecord, GroupLayer, LayerData, LayerEffect, LayerId,
    LayerStrokePosition, PropertyTarget, TrackMatte, layer::TrackMatteType,
};

fn fill_opacity(content: &[Chunk]) -> Result<Option<properties::NumericProperty>, String> {
    fn visit(
        runs: &[(&str, &[Chunk])],
        found: &mut Vec<properties::NumericProperty>,
    ) -> Result<(), String> {
        for (name, run) in runs {
            if *name == "ADBE Layer Fill Opacity2" {
                found.push(
                    properties::read_numeric(
                        properties::unique_list(run, *b"tdbs").map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?,
                );
            } else if matches!(
                *name,
                "ADBE Layer Styles" | "ADBE Blend Options Group" | "ADBE Adv Blend Group"
            ) {
                visit(
                    &properties::runs(
                        properties::unique_list(run, *b"tdgp").map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?,
                    found,
                )?;
            }
        }
        Ok(())
    }
    let mut found = Vec::new();
    visit(
        &properties::root_runs(content).map_err(|e| e.to_string())?,
        &mut found,
    )?;
    match found.len() {
        0 => Ok(None),
        1 => Ok(found.pop()),
        _ => Err("duplicate native Fill Opacity controls".into()),
    }
}

pub(in super::super) fn apply(
    native: &Layer,
    owner: &mut GroupLayer,
    size: [u16; 2],
    depth: usize,
    next: &mut u64,
    entries: &mut [fx_schema::animator::AnimationGraphEntry],
    budget: &mut OutputBudget,
) -> Result<Option<FillMode>, String> {
    let Some(fill) = fill_opacity(&native.content)? else {
        return Ok(None);
    };
    if !fill.animated && !fill.expression_enabled && fill.values.as_slice() == [100.0] {
        return Ok(None);
    }
    let mode = zero_fill_mode(&fill)?;
    let flags = native.record.flags();
    if native.record.layer_type() != 0
        || flags.three_d_layer
        || flags.adjustment_layer
        || flags.collapse_transformation
    {
        return Err(
            "zero-fill Stroke requires a finite uncollapsed 2D AV source; source fill retained"
                .into(),
        );
    }
    let decoded = crate::layer_styles::read(&native.content, size.map(f64::from));
    let [NativeLayerStyle::Stroke(stroke)] = decoded.styles.as_slice() else {
        return Err(
            "zero Layer Fill Opacity requires an isolated Stroke style; source fill retained"
                .into(),
        );
    };
    if !stroke.enabled
        || stroke.position != LayerStrokePosition::Inside
        || stroke.blend_mode != fx_schema::BlendMode::Normal
    {
        return Err("zero Layer Fill Opacity requires an enabled Normal Inside Stroke; source fill retained".into());
    }
    lower(owner, size, depth, next, entries, budget, stroke.color)?;
    Ok(Some(mode))
}

fn zero_fill_mode(fill: &properties::NumericProperty) -> Result<FillMode, String> {
    if !fill.expression_enabled {
        if !fill.animated && fill.keyframes.is_empty() && fill.values.as_slice() == [0.0] {
            return Ok(FillMode::StaticZero);
        }
        if fill.animated
            && fill
                .keyframes
                .first()
                .is_some_and(|key| key.values.as_slice() == [0.0])
            && fill.keyframes.iter().all(|key| {
                key.time_secs.is_finite()
                    && matches!(key.values.as_slice(), [0.0] | [100.0])
                    && key.out_interpolation == 3
            })
            && fill
                .keyframes
                .windows(2)
                .all(|pair| pair[0].time_secs < pair[1].time_secs)
        {
            return Ok(FillMode::InitialZeroHold);
        }
    }
    Err("only constant zero or initial-zero binary Hold Layer Fill Opacity is represented; source fill retained".into())
}

fn lower(
    owner: &mut GroupLayer,
    size: [u16; 2],
    depth: usize,
    next: &mut u64,
    entries: &mut [fx_schema::animator::AnimationGraphEntry],
    budget: &mut OutputBudget,
    color: [f64; 4],
) -> Result<bool, String> {
    if depth.checked_add(5).is_none_or(|d| d >= MAX_GROUP_DEPTH)
        || size.contains(&0)
        || !owner.masks.is_empty()
        || owner.track_matte.is_some()
        || owner.playback != super::super::identity_playback(owner.playback.input_range())
    {
        return Err("zero-fill Stroke requires an isolated finite source clock/plane without owner masks or matte".into());
    }
    let Some(last) = owner.effects.last() else {
        return Err("mapped Stroke is absent".into());
    };
    let EffectData::Identified {
        id,
        enabled: true,
        effect: EffectPayload::Known(LayerEffect::Stroke(style)),
        compositing_options: None,
        ..
    } = last.data()
    else {
        return Err("mapped Stroke must be the final unqualified effect".into());
    };
    let stroke_id = *id;
    if entries.iter().any(|entry| {
        entry.target.effect_id() == Some(stroke_id)
            && entry.target != PropertyTarget::effect_param(stroke_id, "width")
            && entry.target != PropertyTarget::effect_param(stroke_id, "color")
    }) {
        return Err("Stroke animation has no compatible erosion target".into());
    }
    let width = style.width().value();
    let mut cursor = *next;
    let first = reserve_ids(&mut cursor, 6).ok_or("zero-fill Stroke helper IDs exhausted")?;
    let mut candidate = owner.clone();
    candidate.effects.pop();
    let range = owner.playback.input_range();
    let mut gate = group(
        LayerId::new(first),
        "Inside Stroke output".into(),
        Some(owner.id),
        range,
    );
    let mut provider = group(
        LayerId::new(first + 1),
        "Stroke source alpha".into(),
        Some(gate.id),
        range,
    );
    provider.effects = std::mem::take(&mut candidate.effects);
    let mut children: Vec<_> = candidate.layers.iter().map(|l| l.data().clone()).collect();
    for child in &mut children {
        let LayerData::Group(child) = child else {
            return Err("Stroke source must retain its editable content Group".into());
        };
        child.parent = Some(provider.id);
    }
    provider.layers = stored_layers(children).map_err(|e| e.to_string())?;
    let proxy = |id, rect_id, name: &str, color: [f32; 3]| -> Result<GroupLayer, String> {
        let mut proxy = group(LayerId::new(id), name.into(), Some(gate.id), range);
        proxy.track_matte = Some(TrackMatte {
            mode: TrackMatteType::Alpha,
            layer: provider.id,
        });
        let source = SolidSource {
            width: size[0],
            height: size[1],
            pixel_aspect: (1, 1),
            color,
        };
        let rect = transform::solid_rect(&source, &proxy, LayerId::new(rect_id), proxy.transform);
        proxy.layers = stored_layers(vec![LayerData::Rect(rect)]).map_err(|e| e.to_string())?;
        Ok(proxy)
    };
    let mut outer = proxy(
        first + 2,
        first + 3,
        "Stroke color",
        [color[0] as f32, color[1] as f32, color[2] as f32],
    )?;
    let animated_color = entries
        .iter()
        .any(|entry| entry.target == PropertyTarget::effect_param(stroke_id, "color"));
    outer.transform.opacity = fx_schema::PercentageProperty::new(if animated_color {
        100.0
    } else {
        color[3] * 100.0
    })
    .ok_or("invalid Stroke opacity")?;
    let mut inner = proxy(first + 4, first + 5, "Stroke eroded alpha", [1.; 3])?;
    inner.effects.push(
        EffectRecord::from_data(&EffectData::Identified {
            id: stroke_id,
            enabled: true,
            effect: EffectPayload::Known(LayerEffect::SimpleChoker { choke: Some(width) }),
            compositing_options: None,
            extensions: Default::default(),
        })
        .map_err(|e| e.to_string())?,
    );
    gate.track_matte = Some(TrackMatte {
        mode: TrackMatteType::AlphaInverted,
        layer: inner.id,
    });
    gate.layers = stored_layers(vec![
        LayerData::Group(outer),
        LayerData::Group(inner),
        LayerData::Group(provider),
    ])
    .map_err(|e| e.to_string())?;
    candidate.layers = stored_layers(vec![LayerData::Group(gate)]).map_err(|e| e.to_string())?;
    fx_schema::Layer::from_data(&LayerData::Group(candidate.clone())).map_err(|e| e.to_string())?;
    let checkpoint = budget.checkpoint();
    if !budget.reserve(&candidate) {
        budget.restore(checkpoint);
        return Err("zero-fill Stroke helper budget exhausted".into());
    }
    for entry in entries {
        if entry.target == PropertyTarget::effect_param(stroke_id, "width") {
            entry.target = PropertyTarget::effect_param(stroke_id, "choke");
        } else if entry.target == PropertyTarget::effect_param(stroke_id, "color") {
            entry.target =
                PropertyTarget::layer(LayerId::new(first + 3), fx_schema::PropType::FillColor);
        }
    }
    *next = cursor;
    *owner = candidate;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::{ItemKind, read_project};

    fn source(fill: f64) -> crate::structure::StructuralProject {
        let mut project = read_project(include_bytes!(
            "../../../tests/fixtures/layer_styles/styles_static_adobe.aep"
        ))
        .unwrap();
        let ItemKind::Composition(comp) =
            &mut project.items.iter_mut().find(|i| i.id == 114).unwrap().kind
        else {
            panic!()
        };
        let layer = comp
            .layers
            .iter_mut()
            .find(|l| l.record.id() == 127)
            .unwrap();
        let decoded = crate::layer_styles::read(&layer.content, [640., 360.]);
        assert!(matches!(
            decoded.styles.as_slice(),
            [NativeLayerStyle::Stroke(_)]
        ));
        fn find_size(chunks: &[Chunk]) -> Option<Vec<Chunk>> {
            if let Ok(runs) = properties::runs(chunks)
                && let Some((_, run)) = runs.iter().find(|(n, _)| *n == "frameFX/size")
            {
                return Some(run.to_vec());
            }
            chunks
                .iter()
                .filter_map(Chunk::children)
                .find_map(find_size)
        }
        let mut leaf = find_size(&layer.content).unwrap();
        fn set_value(chunks: &mut [Chunk], value: f64) {
            for chunk in chunks {
                if chunk.id() == *b"cdat" {
                    *chunk = Chunk::data(*b"cdat", value.to_be_bytes()).unwrap();
                } else if let Some(children) = chunk.children_mut() {
                    set_value(children, value);
                }
            }
        }
        set_value(&mut leaf, fill);
        fn insert(chunks: &mut [Chunk], leaf: &[Chunk]) -> bool {
            for i in 0..chunks.len() {
                if chunks[i].id() == *b"tdmn"
                    && chunks[i]
                        .data_payload()
                        .unwrap()
                        .starts_with(b"ADBE Adv Blend Group\0")
                {
                    let body = chunks[i + 1..]
                        .iter_mut()
                        .find(|c| c.list_kind() == Some(*b"tdgp"))
                        .unwrap()
                        .children_mut()
                        .unwrap();
                    let mut name = [0; 40];
                    name[..24].copy_from_slice(b"ADBE Layer Fill Opacity2");
                    let mut fields = vec![Chunk::data(*b"tdmn", name).unwrap()];
                    fields.extend_from_slice(leaf);
                    body.splice(0..0, fields);
                    return true;
                }
                if let Some(children) = chunks[i].children_mut()
                    && insert(children, leaf)
                {
                    return true;
                }
            }
            false
        }
        assert!(insert(&mut layer.content, &leaf));
        project
    }
    fn find_ring(layers: &[fx_schema::Layer]) -> Option<&GroupLayer> {
        for layer in layers {
            if let LayerData::Group(g) = layer.data() {
                if g.name == "Inside Stroke output" {
                    return Some(g);
                }
                if let Some(found) = find_ring(&g.layers) {
                    return Some(found);
                }
            }
        }
        None
    }
    #[test]
    fn initial_zero_hold_fill_retains_outline_with_explicit_animation_omission() {
        fn animate_fill(chunks: &mut [Chunk]) -> bool {
            for index in 0..chunks.len() {
                if chunks[index].id() == *b"tdmn"
                    && chunks[index]
                        .data_payload()
                        .unwrap()
                        .starts_with(b"ADBE Layer Fill Opacity2\0")
                {
                    let leaf = chunks[index + 1].children_mut().unwrap();
                    let meta = leaf
                        .iter()
                        .find(|c| c.id() == *b"tdb4")
                        .unwrap()
                        .data_payload()
                        .unwrap();
                    let timebase = i32::from_be_bytes(meta[12..16].try_into().unwrap());
                    let mut header = vec![0; 24];
                    header[10..12].copy_from_slice(&3_u16.to_be_bytes());
                    header[18..20].copy_from_slice(&48_u16.to_be_bytes());
                    header[23] = 4;
                    let mut data = Vec::new();
                    for (second, value) in [(0, 0.0_f64), (1, 100.0), (2, 0.0)] {
                        let mut key = vec![0; 48];
                        key[..4].copy_from_slice(&(second * timebase).to_be_bytes());
                        key[4] = 3;
                        key[5] = 3;
                        key[8..16].copy_from_slice(&value.to_be_bytes());
                        data.extend(key);
                    }
                    leaf.retain(|c| c.id() != *b"cdat");
                    leaf.push(Chunk::list(
                        *b"list",
                        vec![
                            Chunk::data(*b"lhd3", header).unwrap(),
                            Chunk::data(*b"ldat", data).unwrap(),
                        ],
                    ));
                    return true;
                }
                if let Some(children) = chunks[index].children_mut()
                    && animate_fill(children)
                {
                    return true;
                }
            }
            false
        }
        let mut project = source(0.);
        let ItemKind::Composition(comp) =
            &mut project.items.iter_mut().find(|i| i.id == 114).unwrap().kind
        else {
            panic!()
        };
        let layer = comp
            .layers
            .iter_mut()
            .find(|l| l.record.id() == 127)
            .unwrap();
        assert!(animate_fill(&mut layer.content));
        let numeric = fill_opacity(&layer.content).unwrap().unwrap();
        assert!(numeric.animated);
        assert_eq!(
            numeric
                .keyframes
                .iter()
                .map(|k| k.values[0])
                .collect::<Vec<_>>(),
            [0., 100., 0.]
        );
        let imported = super::super::super::to_structural_fx_document(&project, Some(114)).unwrap();
        assert!(
            find_ring(imported.document.composition().layers()).is_some(),
            "initial zero fill must not become a filled source"
        );
        assert!(
            imported
                .diagnostics
                .iter()
                .any(|d| d.message.contains("later fill toggles are omitted"))
        );
        let mut unsupported = numeric.clone();
        unsupported.keyframes[0].out_interpolation = 1;
        assert!(
            zero_fill_mode(&unsupported).is_err(),
            "do not discard continuous fades silently"
        );
        unsupported = numeric;
        unsupported.keyframes[0].values = vec![100.];
        assert!(
            zero_fill_mode(&unsupported).is_err(),
            "do not force an initially filled layer into an outline"
        );
    }

    #[test]
    fn inside_ring_keeps_width_keyframes_and_atomic_budget_failure() {
        use super::super::super::{
            animation::{self, NumericAnimationClock, NumericAnimationTarget},
            animation_budget::AnimationBudget,
        };
        let project = source(100.);
        let imported = super::super::super::to_structural_fx_document(&project, Some(114)).unwrap();
        fn stroke_owner(layers: &[fx_schema::Layer]) -> Option<&GroupLayer> {
            layers.iter().find_map(|layer| {
                let LayerData::Group(g) = layer.data() else {
                    return None;
                };
                if g.effects.iter().any(|e| {
                    matches!(
                        e.data(),
                        EffectData::Identified {
                            effect: EffectPayload::Known(LayerEffect::Stroke(_)),
                            ..
                        }
                    )
                }) {
                    Some(g)
                } else {
                    stroke_owner(&g.layers)
                }
            })
        }
        let mut owner = stroke_owner(imported.document.composition().layers())
            .unwrap()
            .clone();
        let EffectData::Identified { id, .. } = owner.effects.last().unwrap().data() else {
            panic!()
        };
        let id = *id;
        let ItemKind::Composition(comp) = &project.item(114).unwrap().kind else {
            panic!()
        };
        let native = comp.layers.iter().find(|l| l.record.id() == 127).unwrap();
        let key = |time_secs, value| crate::properties::NumericKeyframe {
            time_secs,
            values: vec![value],
            in_interpolation: 1,
            out_interpolation: 1,
            in_speed: vec![0.],
            out_speed: vec![0.],
            in_influence: vec![0.],
            out_influence: vec![0.],
            spatial_in: vec![],
            spatial_out: vec![],
        };
        let numeric = properties::NumericProperty {
            values: vec![8.],
            animated: true,
            expression_enabled: false,
            expression_present: false,
            dimensions_separated: false,
            value_kind: properties::NumericValueKind::Continuous,
            keyframes: vec![key(0., 8.), key(1., 2.)],
        };
        let (mut entries, warnings) = animation::numeric_entries(
            "frameFX/size",
            &numeric,
            &[NumericAnimationTarget::float(
                PropertyTarget::effect_param(id, "width"),
                0,
                1.,
            )],
            NumericAnimationClock::parent_identity(native).unwrap(),
            &mut AnimationBudget::default(),
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(entries.len(), 1);
        let mut opacity = numeric.clone();
        opacity.keyframes[0].values[0] = 0.0;
        opacity.keyframes[1].values[0] = 100.0;
        let (color_entries, warnings) = animation::numeric_entries(
            "frameFX/opacity",
            &opacity,
            &[NumericAnimationTarget::opacity_color(
                PropertyTarget::effect_param(id, "color"),
                [0.2, 0.4, 0.8],
            )
            .scaled(255.0 / 100.0)],
            NumericAnimationClock::parent_identity(native).unwrap(),
            &mut AnimationBudget::default(),
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(color_entries.len(), 1);
        entries.extend(color_entries);
        let original_entries = entries.clone();
        let original_owner = owner.clone();
        let mut next = 10000;
        assert!(
            lower(
                &mut owner,
                [320, 180],
                0,
                &mut next,
                &mut entries,
                &mut OutputBudget::with_limit(1),
                [1.; 4]
            )
            .is_err()
        );
        assert_eq!(owner, original_owner);
        assert_eq!(entries, original_entries);
        assert_eq!(next, 10000);
        assert_eq!(
            lower(
                &mut owner,
                [320, 180],
                0,
                &mut next,
                &mut entries,
                &mut OutputBudget::default(),
                [0.2, 0.4, 0.8, 0.0]
            ),
            Ok(true)
        );
        assert_eq!(entries[0].target, PropertyTarget::effect_param(id, "choke"));
        assert_eq!(entries[0].animator, original_entries[0].animator);
        let ring = find_ring(&owner.layers).unwrap();
        let LayerData::Group(paint) = ring.layers[0].data() else {
            panic!("stroke paint proxy")
        };
        assert_eq!(
            paint.transform.opacity.value(),
            100.0,
            "animated alpha owns opacity once"
        );
        assert_eq!(
            entries[1].target,
            PropertyTarget::layer(paint.layers[0].id(), fx_schema::PropType::FillColor)
        );
        assert_eq!(entries[1].animator, original_entries[1].animator);
    }

    #[test]
    fn zero_native_style_fill_becomes_shared_inside_alpha_ring_not_filled_source() {
        let imported =
            super::super::super::to_structural_fx_document(&source(0.), Some(114)).unwrap();
        let ring = find_ring(imported.document.composition().layers())
            .expect("zero Fill Opacity must suppress source interiors");
        assert_eq!(
            ring.track_matte.as_ref().unwrap().mode,
            TrackMatteType::AlphaInverted
        );
        let LayerData::Group(eroded) = ring.layers[1].data() else {
            panic!()
        };
        let LayerData::Group(provider) = ring.layers[2].data() else {
            panic!()
        };
        assert_eq!(ring.track_matte.as_ref().unwrap().layer, eroded.id);
        assert_eq!(eroded.track_matte.as_ref().unwrap().layer, provider.id);
        assert!(
            matches!(eroded.effects[0].data(), EffectData::Identified { effect: EffectPayload::Known(LayerEffect::SimpleChoker { choke: Some(v) }), .. } if *v > 0.)
        );
        assert!(!provider.layers.is_empty(), "retain editable native source");
        assert!(
            imported
                .diagnostics
                .iter()
                .any(|d| d.message.contains("Zero Layer Fill Opacity"))
        );
        let ordinary =
            super::super::super::to_structural_fx_document(&source(100.), Some(114)).unwrap();
        assert!(find_ring(ordinary.document.composition().layers()).is_none());
        let partial =
            super::super::super::to_structural_fx_document(&source(50.), Some(114)).unwrap();
        assert!(find_ring(partial.document.composition().layers()).is_none());
        assert!(
            partial
                .diagnostics
                .iter()
                .any(|d| d.message.contains("only constant zero"))
        );
    }
}
