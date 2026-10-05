//! Bounded editable half-plane reflection of the pre-Mirror vector image.
use std::collections::HashMap;

use super::{MAX_GROUP_DEPTH, group, reserve_ids, stored_layers};
use crate::{effects::native, structure::Layer};
use fx_schema::{
    EffectData, EffectId, EffectRecord, FxItemId, GroupLayer, LayerData, LayerId,
    NonNegativeProperty, Position, PropertyTarget, ShapeContent, ShapePath, ShapePathCommand,
    animator::AnimationGraphEntry,
    layer::{MaskMode, PathMask, ShapeLayer},
};

pub(super) struct Context<'a> {
    pub size: [u16; 2],
    pub ordinals: &'a [usize],
    pub depth: usize,
    pub other_stages: bool,
}
pub(super) struct State<'a> {
    pub next: &'a mut u64,
    pub entries: &'a mut Vec<AnimationGraphEntry>,
    pub animations: &'a mut super::animation_budget::AnimationBudget,
    pub shapes: &'a mut super::shapes::OutputBudget,
}

fn control(
    effect: &native::DecodedEffect,
    name: &str,
    dimensions: usize,
) -> Result<Vec<f64>, String> {
    let numeric = effect
        .parameters
        .iter()
        .find(|p| p.match_name == name)
        .and_then(|p| p.numeric.as_ref().ok())
        .ok_or_else(|| format!("{name}: missing/malformed control"))?;
    if numeric.expression_enabled
        || numeric.animated
        || !numeric.keyframes.is_empty()
        || numeric.dimensions_separated
        || numeric.values.len() != dimensions
        || numeric
            .values
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
    {
        return Err(format!(
            "{name}: requires bounded finite static controls; active expressions/keys unsupported"
        ));
    }
    Ok(numeric.values.clone())
}

/// Plugin parameters exclude AE's built-in per-effect compositing subtree.
/// Inspect the exact occurrence, not another Mirror or its parT declaration.
fn compositing_enabled(layer: &Layer, ordinal: usize) -> Result<bool, String> {
    use crate::properties;
    let roots = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    let parades: Vec<_> = roots
        .iter()
        .filter(|(name, _)| *name == "ADBE Effect Parade")
        .collect();
    let [(_, parade)] = parades.as_slice() else {
        return Err("ambiguous Mirror Effect Parade".into());
    };
    let occurrences =
        properties::runs(properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let Some(("ADBE Mirror", run)) = occurrences.get(ordinal - 1) else {
        return Err("Mirror occurrence identity differs".into());
    };
    let descriptor = properties::unique_list(run, *b"sspc").map_err(|e| e.to_string())?;
    let controls =
        properties::runs(properties::unique_list(descriptor, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let builtins: Vec<_> = controls
        .iter()
        .filter(|(name, _)| *name == "ADBE Effect Built In Params")
        .collect();
    if builtins.is_empty() {
        return Ok(true);
    }
    let [(_, run)] = builtins.as_slice() else {
        return Err("duplicate Mirror Compositing Options".into());
    };
    let options =
        properties::runs(properties::unique_list(run, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    if options.iter().any(|(name, _)| !seen.insert(*name)) {
        return Err("duplicate Mirror compositing control".into());
    }
    // Independent Adobe catalog readback names this ADBE Effect Mask Opacity,
    // not the superficially plausible ADBE Effect Opacity. Zero is identity
    // regardless of native percentage storage normalization.
    if let Some((_, run)) = options
        .iter()
        .find(|(name, _)| *name == "ADBE Effect Mask Opacity")
    {
        let numeric = properties::read_numeric(
            properties::unique_list(run, *b"tdbs")
                .map_err(|e| format!("Mirror Effect Opacity: {e}"))?,
        )
        .map_err(|e| format!("Mirror Effect Opacity: {e}"))?;
        if numeric.expression_enabled
            || numeric.animated
            || !numeric.keyframes.is_empty()
            || numeric.dimensions_separated
            || numeric.values.len() != 1
            || !numeric.values[0].is_finite()
        {
            return Err(
                "Mirror Effect Opacity requires a finite static scalar without live expressions"
                    .into(),
            );
        }
        if numeric.values == [0.] {
            return Ok(false);
        }
        return Err("Mirror Effect Opacity: explicit nonzero wet/dry compositing is outside the bounded reflection graph; default absent opacity and static zero are supported".into());
    }
    for (name, run) in options {
        match name {
            "ADBE Group End" => {}
            "ADBE Effect Mask Parade" => {
                let masks = properties::runs(
                    properties::unique_list(run, *b"tdgp")
                        .map_err(|e| format!("Mirror Effect Masks: {e}"))?,
                )
                .map_err(|e| format!("Mirror Effect Masks: {e}"))?;
                if masks.iter().any(|(name, _)| *name != "ADBE Group End") {
                    return Err(
                        "Mirror Effect Masks: nonempty per-effect mask references are unsupported"
                            .into(),
                    );
                }
            }
            _ => {
                return Err(format!(
                    "Mirror Compositing Options / {name}: unsupported explicit control"
                ));
            }
        }
    }
    Ok(true)
}

/// This clone admits ordinary vector subtrees only. Each copied track receives a
/// new target and new key identities through the existing shape binding copier.
fn copy_vector(
    data: &LayerData,
    parent: LayerId,
    next: &mut u64,
    layers: &mut HashMap<LayerId, LayerId>,
    effects: &mut HashMap<EffectId, EffectId>,
    depth: usize,
) -> Result<LayerData, String> {
    if depth >= MAX_GROUP_DEPTH {
        return Err("Mirror helper depth exceeds allowance".into());
    }
    let mut copy = data.clone();
    let (id, parent_slot, masks, matte, stack) = match &mut copy {
        LayerData::Group(v) => (
            &mut v.id,
            &mut v.parent,
            &v.masks,
            &v.track_matte,
            &mut v.effects,
        ),
        LayerData::Rect(v) => (
            &mut v.id,
            &mut v.parent,
            &v.masks,
            &v.track_matte,
            &mut v.effects,
        ),
        LayerData::Shape(v) => (
            &mut v.id,
            &mut v.parent,
            &v.masks,
            &v.track_matte,
            &mut v.effects,
        ),
        _ => return Err("Mirror currently requires editable Group/Rect/Shape content".into()),
    };
    if !masks.is_empty() || matte.is_some() {
        return Err(
            "Mirror source masks/matte references are outside the bounded vector-copy route".into(),
        );
    }
    let new = LayerId::new(reserve_ids(next, 1).ok_or("Mirror identity allocation exhausted")?);
    if layers.insert(*id, new).is_some() {
        return Err("Mirror duplicate source identity".into());
    }
    *id = new;
    *parent_slot = Some(parent);
    for record in stack {
        if let EffectData::Identified {
            id,
            enabled,
            effect,
        } = record.data()
        {
            let new = EffectId::new(
                reserve_ids(next, 1).ok_or("Mirror effect identity allocation exhausted")?,
            );
            if effects.insert(*id, new).is_some() {
                return Err("Mirror duplicate effect identity".into());
            }
            *record = EffectRecord::from_data(&EffectData::Identified {
                id: new,
                enabled: *enabled,
                effect: effect.clone(),
            })
            .map_err(|e| e.to_string())?;
        }
    }
    if let LayerData::Group(v) = &mut copy {
        v.layers = stored_layers(
            v.layers
                .iter()
                .map(|child| copy_vector(child.data(), new, next, layers, effects, depth + 1))
                .collect::<Result<Vec<_>, _>>()?,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(copy)
}

fn guide(id: LayerId, parent: &GroupLayer, center: [f64; 2], angle: f64, extent: f64) -> LayerData {
    let line = |x, y| ShapePathCommand::LineTo {
        x,
        y,
        mirror: None,
        corner_radius: None,
    };
    let mut transform = parent.transform;
    transform.position = Position::xy(center[0], center[1]);
    transform.rotation = angle;
    LayerData::Shape(ShapeLayer {
        id,
        parent: Some(parent.id),
        name: "Mirror retained half-plane guide".into(),
        description: String::new(),
        is_hidden: false,
        blend_mode: Default::default(),
        track_matte: None,
        masks: vec![],
        active_range: parent.playback.input_range(),
        effects: vec![],
        motion_blur: false,
        transform,
        shape: ShapeContent {
            path: ShapePath {
                commands: vec![
                    ShapePathCommand::MoveTo {
                        x: -extent,
                        y: -extent,
                        mirror: None,
                        corner_radius: None,
                    },
                    line(0., -extent),
                    line(0., extent),
                    line(-extent, extent),
                    ShapePathCommand::Close,
                ],
            },
            fills: vec![],
            strokes: vec![],
            round_corners: None,
            offset_paths: None,
            trim: None,
            poly_star: None,
            ellipse: None,
        },
    })
}

pub(super) fn apply(
    layer: &Layer,
    owner: &mut GroupLayer,
    context: Context<'_>,
    state: State<'_>,
) -> Result<bool, String> {
    let (native, _) = native::read_effects(&layer.content, context.size.map(f64::from));
    let mirrors: Vec<_> = native
        .iter()
        .filter(|e| e.match_name == "ADBE Mirror" && e.enabled)
        .collect();
    if mirrors.is_empty() || !layer.record.flags().effects_active {
        return Ok(false);
    }
    if mirrors.len() != 1
        || context.other_stages
        || native.iter().any(|effect| {
            effect.enabled
                && matches!(
                    effect.match_name.as_str(),
                    "ADBE Geometry2" | "ADBE Radial Wipe" | "CC Split 2" | "APC Vegas"
                )
        })
    {
        return Err("one Mirror without other graph-staged native effects is required".into());
    }
    let mirror = mirrors[0];
    if !compositing_enabled(layer, mirror.index)? {
        return Ok(false);
    }
    let center = control(mirror, "ADBE Mirror-0001", 2)?;
    let angle = control(mirror, "ADBE Mirror-0002", 1)?[0].rem_euclid(360.);
    let flags = layer.record.flags();
    if flags.three_d_layer
        // Native Shape layers use this flag for continuous rasterization,
        // not collapsed precomposition transform semantics.
        || (flags.collapse_transformation && layer.record.layer_type() != 4)
        || flags.preserve_transparency
        || flags.adjustment_layer
        || context.size.contains(&0)
        || owner.layers.len() != 1
        || !owner.masks.is_empty()
        || owner.track_matte.is_some()
        || owner.playback != super::identity_playback(owner.playback.input_range())
    {
        return Err("requires one planar vector content Group, identity occurrence clock and no owner masks/matte".into());
    }
    if context.ordinals.len() > owner.effects.len()
        || context.ordinals.windows(2).any(|p| p[0] > p[1])
    {
        return Err("inconsistent effect ordinals".into());
    }
    let prefix = context
        .ordinals
        .iter()
        .take_while(|ordinal| **ordinal < mirror.index)
        .count();
    let mut candidate = owner.clone();
    let mut cursor = *state.next;
    let first = reserve_ids(&mut cursor, 8).ok_or("Mirror helper identity allocation exhausted")?;
    let mut output = group(
        LayerId::new(first + 5),
        "Mirror recombined finite plane".into(),
        Some(owner.id),
        owner.playback.input_range(),
    );
    let mut retained = group(
        LayerId::new(first),
        "Mirror retained source half".into(),
        Some(output.id),
        owner.playback.input_range(),
    );
    retained.effects = candidate.effects.drain(..prefix).collect();
    // Clip AFTER prefix effects: the native Mirror samples their combined result.
    let mut input = group(
        LayerId::new(first + 1),
        "Source before Mirror".into(),
        Some(retained.id),
        owner.playback.input_range(),
    );
    input.effects = std::mem::take(&mut retained.effects);
    let mut original = candidate.layers.remove(0).data().clone();
    let LayerData::Group(content) = &mut original else {
        return Err("requires editable source content Group".into());
    };
    content.parent = Some(input.id);
    input.layers = stored_layers(vec![original]).map_err(|e| e.to_string())?;
    let mut layers = HashMap::new();
    let mut effects = HashMap::new();
    let mut reflected = group(
        LayerId::new(first + 2),
        "Mirror reflected half".into(),
        Some(output.id),
        owner.playback.input_range(),
    );
    let copied = copy_vector(
        &LayerData::Group(input.clone()),
        reflected.id,
        &mut cursor,
        &mut layers,
        &mut effects,
        context.depth + 4,
    )?;
    // Reflection about n·(p-center)=0, n=(cos(angle),sin(angle)).
    // R(2*angle) diag(-1,1) = I - 2 n nᵀ.
    reflected.transform.anchor_point = [center[0], center[1]];
    reflected.transform.position = Position::xy(center[0], center[1]);
    reflected.transform.rotation = 2. * angle;
    reflected.transform.scale = [-100., 100.];
    let extent = 2.
        * (f64::from(context.size[0]).hypot(f64::from(context.size[1]))
            + center[0].hypot(center[1]))
        + 1.;
    for (branch, content, offset) in [
        (&mut retained, LayerData::Group(input), 3_u64),
        (&mut reflected, copied, 4),
    ] {
        let guide_id = LayerId::new(first + offset);
        // Guides live in the branch's pre-transform coordinates.
        let mut neutral = branch.clone();
        neutral.transform = super::group(
            LayerId::new(0),
            String::new(),
            None,
            branch.playback.input_range(),
        )
        .transform;
        branch.layers = stored_layers(vec![
            content,
            guide(guide_id, &neutral, [center[0], center[1]], angle, extent),
        ])
        .map_err(|e| e.to_string())?;
        branch.masks.push(PathMask {
            id: FxItemId::new(
                reserve_ids(&mut cursor, 1).ok_or("Mirror mask identity allocation exhausted")?,
            ),
            mode: MaskMode::Add,
            inverted: false,
            layer: Some(guide_id),
            legacy_path: None,
            feather: [0., 0.],
            expansion: 0.,
            opacity: NonNegativeProperty::new(1.).expect("one is nonnegative"),
        });
    }
    let checkpoint = state.animations.checkpoint();
    let result = (|| {
        let mut copies = Vec::new();
        for entry in state.entries.iter() {
            let target = match &entry.target {
                PropertyTarget::LayerProperty(p) if layers.contains_key(&p.layer_id()) => Some(
                    PropertyTarget::layer(layers[&p.layer_id()], p.property_type()),
                ),
                PropertyTarget::EffectProperty(p) if effects.contains_key(&p.effect_id()) => Some(
                    PropertyTarget::effect_param(effects[&p.effect_id()], p.param_name()),
                ),
                _ => None,
            };
            let Some(target) = target else {
                continue;
            };
            if entry.animator.is_js_script()
                || !entry.dependencies.is_empty()
                || entry.random_seed_target.is_some()
                || !entry.layer_refs.is_empty()
            {
                return Err(
                    "Mirror copied tracks require independent controls without graph references"
                        .into(),
                );
            }
            let animator =
                super::shapes::bindings::copy_animator(&entry.animator, &target, state.animations)
                    .map_err(|e| e.to_string())?
                    .ok_or("Mirror animation allowance exhausted")?;
            let mut copy = entry.clone();
            copy.target = target;
            copy.animator = animator;
            copies.push(copy);
        }
        let canvas_id = LayerId::new(first + 6);
        let mut canvas = guide(canvas_id, &output, [0., 0.], 0., 1.);
        let LayerData::Shape(shape) = &mut canvas else {
            unreachable!("guide constructs a Shape")
        };
        shape.name = "Mirror finite canvas guide".into();
        let [width, height] = context.size.map(f64::from);
        let line = |x, y| ShapePathCommand::LineTo {
            x,
            y,
            mirror: None,
            corner_radius: None,
        };
        shape.shape.path.commands = vec![
            ShapePathCommand::MoveTo {
                x: 0.,
                y: 0.,
                mirror: None,
                corner_radius: None,
            },
            line(width, 0.),
            line(width, height),
            line(0., height),
            ShapePathCommand::Close,
        ];
        output.layers = stored_layers(vec![
            LayerData::Group(reflected),
            LayerData::Group(retained),
            canvas,
        ])
        .map_err(|e| e.to_string())?;
        output.masks.push(PathMask {
            id: FxItemId::new(first + 7),
            mode: MaskMode::Add,
            inverted: false,
            layer: Some(canvas_id),
            legacy_path: None,
            feather: [0., 0.],
            expansion: 0.,
            opacity: NonNegativeProperty::new(1.).expect("one is nonnegative"),
        });
        candidate.layers =
            stored_layers(vec![LayerData::Group(output)]).map_err(|e| e.to_string())?;
        fx_schema::Layer::from_data(&LayerData::Group(candidate.clone()))
            .map_err(|e| e.to_string())?;
        if !state.shapes.reserve(&candidate) {
            return Err("Mirror helper output exceeds allowance".into());
        }
        Ok(copies)
    })();
    match result {
        Ok(copies) => {
            state.entries.extend(copies);
            *state.next = cursor;
            *owner = candidate;
            Ok(true)
        }
        Err(error) => {
            state.animations.rollback(checkpoint);
            Err(error)
        }
    }
}
