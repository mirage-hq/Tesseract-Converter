//! Transparent self-input image contours approximated with existing editable mattes.
//! Native contour segmentation, phase and edge algorithms are not represented.
use std::collections::HashSet;

use super::{MAX_GROUP_DEPTH, group, reserve_ids, stored_layers, transform};
use crate::{
    properties,
    rifx::Chunk,
    structure::{Layer, SolidSource},
};
use fx_schema::{
    EffectData, EffectId, EffectPayload, EffectRecord, GroupLayer, LayerData, LayerEffect, LayerId,
    TrackMatte, layer::TrackMatteType,
};
const NAME: &str = "APC Vegas";
// Native parameter declarations are independent of instance values. Popup
// current slot56 is not a default; validated defaults are in slot62.
const DECLARATIONS: &[(u32, u32, u32, u16)] = &[
    (0, 0, 0, 0),
    (52, 7, 1, 2),
    (54, 13, 0, 0),
    (2, 0, 0, 0),
    (4, 4, 0, 0),
    (6, 7, 1, 2),
    (10, 7, 1, 9),
    (12, 2, 127 << 16, 0),
    (14, 2, 0, 0),
    (16, 2, 1 << 15, 0),
    (44, 7, 1, 2),
    (46, 1, 1, 0),
    (48, 7, 1, 2),
    (27, 14, 0, 0),
    (56, 13, 0, 0),
    (50, 12, 0, 0),
    (33, 14, 0, 0),
    (35, 13, 0, 0),
    (28, 1, 32, 0),
    (24, 2, 1 << 16, 0),
    (26, 7, 2, 2),
    (30, 3, 0, 0),
    (32, 4, 0, 0),
    (34, 1, 1, 0),
    (49, 14, 0, 0),
    (51, 13, 0, 0),
    (8, 7, 2, 4),
    (18, 5, 0xffffff00, 0),
    (20, 2, 2 << 16, 0),
    (22, 2, 0, 0),
    (36, 2, 1 << 16, 0),
    (38, 2, 0, 0),
    (40, 2, 1 << 15, 0),
    (42, 2, 0, 0),
    (69, 14, 0, 0),
];
struct Profile {
    ordinal: usize,
    width: f64,
    threshold: f64,
    color: [f32; 3],
}
fn controls(descriptor: &[Chunk]) -> Result<Vec<(&str, &[Chunk])>, String> {
    let table = properties::unique_list(descriptor, *b"parT").map_err(|e| e.to_string())?;
    if !table.is_empty() {
        let rows = properties::runs(table).map_err(|e| e.to_string())?;
        if rows.len() != DECLARATIONS.len() + 1 {
            return Err("requires complete native Vegas declarations or a sparse instance".into());
        }
        let mut seen = HashSet::new();
        for (name, run) in rows {
            if !seen.insert(name) {
                return Err("duplicate Vegas declaration".into());
            }
            let (kind, value, choices) = if name == "ADBE Effect Built In Params" {
                (9, 0, 0)
            } else {
                let (_, kind, value, choices) = DECLARATIONS
                    .iter()
                    .find(|(number, _, _, _)| name == format!("{NAME}-{number:04}"))
                    .ok_or("unknown Vegas declaration")?;
                (*kind, *value, *choices)
            };
            let p = properties::data(run, *b"pard").map_err(|e| e.to_string())?;
            if p.len() != 148
                || p[12..16] != kind.to_be_bytes()
                || if kind == 7 {
                    p[60..62] != choices.to_be_bytes() || p[62..64] != (value as u16).to_be_bytes()
                } else {
                    p[56..60] != value.to_be_bytes()
                }
            {
                return Err("Vegas native ABI/default declaration conflict".into());
            }
        }
    }
    let body = properties::unique_list(descriptor, *b"tdgp").map_err(|e| e.to_string())?;
    let rows = properties::runs(body).map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    for (name, run) in &rows {
        if !seen.insert(*name) {
            return Err("duplicate Vegas control".into());
        }
        if *name == "ADBE Group End" {
            continue;
        }
        if *name == "ADBE Effect Built In Params" {
            let options = properties::runs(
                properties::unique_list(run, *b"tdgp").map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            if options.iter().any(|(name, _)| *name != "ADBE Group End") {
                return Err("Vegas compositing options unsupported".into());
            }
        } else if !DECLARATIONS
            .iter()
            .any(|(number, _, _, _)| *name == format!("{NAME}-{number:04}"))
        {
            return Err(format!("unknown Vegas control {name}"));
        }
    }
    Ok(rows)
}
fn numeric(
    rows: &[(&str, &[Chunk])],
    number: u32,
) -> Result<Option<properties::NumericProperty>, String> {
    let name = format!("{NAME}-{number:04}");
    let Some((_, run)) = rows.iter().find(|(name_, _)| *name_ == name) else {
        return Ok(None);
    };
    let numeric = properties::read_numeric(
        properties::unique_list(run, *b"tdbs").map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if numeric.expression_present
        || numeric.expression_enabled
        || numeric.dimensions_separated
        || numeric
            .values
            .iter()
            .chain(numeric.keyframes.iter().flat_map(|key| key.values.iter()))
            .any(|v| !v.is_finite())
    {
        return Err(format!("{name}: nonfinite or expression-driven control"));
    }
    // Sweep animation is deliberately omitted by this full-outline approximation.
    if number != 30 && (numeric.animated || !numeric.keyframes.is_empty()) {
        return Err(format!(
            "{name}: animated contour controls are outside admitted profile"
        ));
    }
    Ok(Some(numeric))
}
fn scalar(rows: &[(&str, &[Chunk])], number: u32, default: f64) -> Result<f64, String> {
    let values = numeric(rows, number)?.map_or(vec![default], |p| p.values);
    let [value] = values.as_slice() else {
        return Err("Vegas scalar control dimensions differ".into());
    };
    Ok(*value)
}
fn profile(layer: &Layer) -> Result<Option<Profile>, String> {
    let root = properties::root_runs(&layer.content).map_err(|e| e.to_string())?;
    let mut parades = root
        .iter()
        .filter(|(name, _)| *name == "ADBE Effect Parade");
    let Some((_, parade)) = parades.next() else {
        return Ok(None);
    };
    if parades.next().is_some() {
        return Err("ambiguous Effect Parade".into());
    }
    let rows =
        properties::runs(properties::unique_list(parade, *b"tdgp").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut matches = rows.iter().enumerate().filter(|(_, p)| p.0 == NAME);
    let Some((index, (_, run))) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err("multiple Vegas stages unsupported".into());
    }
    let descriptor = properties::unique_list(run, *b"sspc").map_err(|e| e.to_string())?;
    let mut warnings = Vec::new();
    if !properties::group_enabled_or_warn(descriptor, NAME, &mut warnings) {
        return Ok(None);
    }
    if !warnings.is_empty() {
        return Err(warnings.join("; "));
    }
    let rows = controls(descriptor)?;
    // Validate even ignored source controls before applying an approximation.
    for (number, _, _, _) in DECLARATIONS {
        let _ = numeric(&rows, *number)?;
    }
    for (number, default, expected) in [
        (0, 0., 0.),
        (52, 1., 1.),
        (2, 0., 0.),
        (4, 0., 0.),
        (6, 1., 1.),
        (10, 1., 1.),
        (14, 0., 0.),
        (44, 1., 1.),
        (8, 2., 1.),
    ] {
        if scalar(&rows, number, default)? != expected {
            return Err(format!(
                "{NAME}-{number:04}: requires self Input, Intensity, unblurred Image Contours, all contours and Transparent blend"
            ));
        }
    }
    let width = scalar(&rows, 20, 2.)?;
    let threshold = scalar(&rows, 12, 127.)?;
    if !(0. ..=20.).contains(&width)
        || !(1. ..=255.).contains(&threshold)
        || threshold.fract() != 0.
    {
        return Err(
            "Vegas width must be 0..20 px and threshold an integer code value 1..255".into(),
        );
    }
    let color = numeric(&rows, 18)?.map_or(vec![1., 1., 0., 1.], |p| p.values);
    let [r, g, b, a] = color.as_slice() else {
        return Err("Vegas color dimensions differ".into());
    };
    if *a != 1. || [r, g, b].iter().any(|v| !(0. ..=1.).contains(*v)) {
        return Err("Vegas requires an opaque finite stroke color".into());
    }
    Ok(Some(Profile {
        ordinal: index + 1,
        width,
        threshold,
        color: [*r as f32, *g as f32, *b as f32],
    }))
}
pub(super) struct Context<'a> {
    pub size: [u16; 2],
    pub ordinals: &'a [usize],
    pub depth: usize,
    pub animations: &'a [fx_schema::animator::AnimationGraphEntry],
}
pub(super) struct State<'a> {
    pub next: &'a mut u64,
    pub budget: &'a mut super::shapes::OutputBudget,
}

fn neutral_effect(effect: &EffectRecord) -> bool {
    matches!(
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
    )
}
fn neutral_input(
    root: &LayerData,
    entries: &[fx_schema::animator::AnimationGraphEntry],
    depth: usize,
) -> Result<(), String> {
    let mut providers = HashSet::new();
    let mut pending = vec![(root, depth)];
    let mut max_depth = depth;
    while let Some((layer, depth)) = pending.pop() {
        if depth >= MAX_GROUP_DEPTH {
            return Err("Vegas source depth exceeds allowance".into());
        }
        max_depth = max_depth.max(depth);
        match layer {
            LayerData::Group(g) => {
                if let Some(matte) = &g.track_matte {
                    providers.insert(matte.layer);
                }
                for mask in &g.masks {
                    providers.extend(mask.layer);
                }
            }
            LayerData::Rect(r) => {
                if let Some(matte) = &r.track_matte {
                    providers.insert(matte.layer);
                }
            }
            _ => {}
        }
        if let Some(children) = layer.child_layers() {
            pending.extend(children.iter().map(|child| (child.data(), depth + 1)));
        }
    }
    if max_depth
        .checked_add(2)
        .is_none_or(|d| d >= MAX_GROUP_DEPTH)
        || depth.checked_add(5).is_none_or(|d| d >= MAX_GROUP_DEPTH)
    {
        return Err("Vegas helper plus source depth exceeds allowance".into());
    }
    let mut visible = HashSet::new();
    let mut pending = vec![root];
    while let Some(layer) = pending.pop() {
        if providers.contains(&layer.id()) {
            continue;
        }
        visible.insert(layer.id());
        if layer.effects().iter().any(|effect| !neutral_effect(effect)) {
            return Err("Vegas input has unproven RGB-changing effects".into());
        }
        match layer {
            LayerData::Group(g)=>pending.extend(g.layers.iter().map(|child|child.data())),
            LayerData::Rect(r) if r.rect.fill_paint.is_none() && (!r.rect.fill_enabled||r.rect.fill_color[0]==r.rect.fill_color[1]&&r.rect.fill_color[1]==r.rect.fill_color[2]) && (!r.rect.stroke_enabled||r.rect.stroke_color.is_some_and(|c|c[0]==c[1]&&c[1]==c[2]))=>{},
            LayerData::Audio(_)=>{},
            _=>return Err("Vegas Intensity approximation requires structurally grayscale Rect/Group paint; media, arbitrary shaders and intrinsic colored text glyphs remain outside admitted profile".into()),
        }
    }
    if entries.iter().any(|entry| {
        entry.target.as_property().is_some_and(|property| {
            visible.contains(&property.layer_id())
                && matches!(
                    property.property_type(),
                    fx_schema::PropType::FillColor | fx_schema::PropType::StrokeColor
                )
        })
    }) {
        return Err("Vegas grayscale input has animated paint colors".into());
    }
    Ok(())
}

pub(super) fn apply(
    layer: &Layer,
    owner: &mut GroupLayer,
    context: Context<'_>,
    state: State<'_>,
) -> Result<bool, String> {
    let Context {
        size,
        ordinals,
        depth,
        animations,
    } = context;
    let Some(profile) = profile(layer)? else {
        return Ok(false);
    };
    if !layer.record.flags().effects_active {
        return Ok(false);
    }
    if layer.record.flags().three_d_layer
        || layer.record.flags().adjustment_layer
        || size.contains(&0)
        || owner.layers.len() != 1
        || !owner.masks.is_empty()
        || owner.track_matte.is_some()
        || owner.playback != super::identity_playback(owner.playback.input_range())
    {
        return Err("Vegas requires isolated enabled 2D content, with owner masks/matte outside the effect stage".into());
    }
    if ordinals.len() > owner.effects.len() || ordinals.windows(2).any(|p| p[0] > p[1]) {
        return Err("Vegas effect ordinal metadata is inconsistent".into());
    }
    if depth.checked_add(5).is_none_or(|d| d >= MAX_GROUP_DEPTH) {
        return Err("Vegas helper depth exceeds allowance".into());
    }
    let prefix = ordinals
        .iter()
        .take_while(|ordinal| **ordinal < profile.ordinal)
        .count();
    if owner
        .effects
        .iter()
        .take(prefix)
        .any(|effect| !neutral_effect(effect))
    {
        return Err("Vegas prefix effects must preserve structural grayscale RGB".into());
    }
    neutral_input(owner.layers[0].data(), animations, depth + 1)?;
    let mut candidate = owner.clone();
    let mut cursor = *state.next;
    let first = reserve_ids(&mut cursor, 11).ok_or("Vegas helper identity allocation exhausted")?;
    let range = owner.playback.input_range();
    let mut gate = group(
        LayerId::new(first),
        "Transparent contour output".into(),
        Some(owner.id),
        range,
    );
    let mut provider = group(
        LayerId::new(first + 1),
        "Vegas source intensity provider".into(),
        Some(gate.id),
        range,
    );
    provider.effects = candidate.effects.drain(..prefix).collect();
    provider.effects.push(
        EffectRecord::from_data(&EffectData::Identified {
            id: EffectId::new(first + 8),
            compositing_options: None,
            extensions: Default::default(),
            enabled: true,
            effect: EffectPayload::Known(LayerEffect::LumaKey {
                threshold: Some((profile.threshold - 0.5) / 255.),
                softness: Some(0.5 / 255.),
                invert: Some(0.),
            }),
        })
        .map_err(|e| e.to_string())?,
    );
    let mut original = candidate.layers[0].data().clone();
    let LayerData::Group(content) = &mut original else {
        return Err("Vegas requires editable source content Group".into());
    };
    content.parent = Some(provider.id);
    provider.layers = stored_layers(vec![original]).map_err(|e| e.to_string())?;
    let rect = |color, parent: &GroupLayer, id| {
        let source = SolidSource {
            width: size[0],
            height: size[1],
            pixel_aspect: (1, 1),
            color,
        };
        let mut rect = transform::solid_rect(&source, parent, id, parent.transform);
        rect.active_range = range;
        rect
    };
    let branch = |id, proxy_id, rect_id, effect_id, choke, color| -> Result<GroupLayer, String> {
        let mut branch = group(
            LayerId::new(id),
            "Vegas contour morphology".into(),
            Some(gate.id),
            range,
        );
        branch.effects.push(
            EffectRecord::from_data(&EffectData::Identified {
                id: EffectId::new(effect_id),
                compositing_options: None,
                extensions: Default::default(),
                enabled: true,
                effect: EffectPayload::Known(LayerEffect::SimpleChoker { choke: Some(choke) }),
            })
            .map_err(|e| e.to_string())?,
        );
        let mut proxy = group(
            LayerId::new(proxy_id),
            "Vegas source alpha proxy".into(),
            Some(branch.id),
            range,
        );
        proxy.track_matte = Some(TrackMatte {
            mode: TrackMatteType::Alpha,
            layer: provider.id,
        });
        proxy.layers = stored_layers(vec![LayerData::Rect(rect(
            color,
            &proxy,
            LayerId::new(rect_id),
        ))])
        .map_err(|e| e.to_string())?;
        branch.layers = stored_layers(vec![LayerData::Group(proxy)]).map_err(|e| e.to_string())?;
        Ok(branch)
    };
    let mut outer = branch(
        first + 2,
        first + 3,
        first + 4,
        first + 9,
        -profile.width / 2.,
        profile.color,
    )?;
    if profile.width == 0. {
        outer.transform.opacity =
            fx_schema::PercentageProperty::new(0.).expect("zero opacity is valid");
    }
    let inner = branch(
        first + 5,
        first + 6,
        first + 7,
        first + 10,
        profile.width / 2.,
        [1.; 3],
    )?;
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
    let checkpoint = state.budget.checkpoint();
    if !state.budget.reserve(&candidate) {
        state.budget.restore(checkpoint);
        return Err("Vegas helper output serialization failed or its byte count overflowed".into());
    }
    *owner = candidate;
    *state.next = cursor;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        rifx::Rifx,
        schema::layer_records::LayerRecord,
        structure::{ItemKind, read_project},
    };
    fn native() -> (crate::structure::StructuralProject, Layer) {
        let mut project = read_project(include_bytes!(
            "../../tests/fixtures/effects/shape_owner_gaussian.aep"
        ))
        .unwrap();
        let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
            panic!()
        };
        let mut layer = comp.layers[0].clone();
        let parsed = Rifx::parse_with(
            include_bytes!("../../tests/fixtures/effects/native-vegas-contour-controls.rifx"),
            |_| false,
        )
        .unwrap();
        layer.content = parsed.chunks()[0].children().unwrap().to_vec();
        layer.record =
            LayerRecord::decode(properties::data(&layer.content, *b"ldta").unwrap()).unwrap();
        layer.name = "Arbitrary contour owner".into();
        let ItemKind::Composition(comp) =
            &mut project.items.iter_mut().find(|i| i.id == 1).unwrap().kind
        else {
            panic!()
        };
        comp.width = 3840;
        comp.height = 2160;
        comp.duration_secs = 6.;
        comp.layers = vec![layer.clone()];
        let mut source = project.items[0].clone();
        source.id = 984;
        source.kind = ItemKind::Footage;
        source.media = None;
        source.solid = Some(Ok(SolidSource {
            width: 3840,
            height: 2160,
            pixel_aspect: (1, 1),
            color: [1.; 3],
        }));
        project.items.push(source);
        (project, layer)
    }
    fn g(layer: &fx_schema::Layer) -> &GroupLayer {
        let LayerData::Group(g) = layer.data() else {
            panic!()
        };
        g
    }
    fn plugin(layer: &mut Layer) -> &mut Vec<Chunk> {
        fn list(chunks: &mut [Chunk], kind: [u8; 4]) -> &mut Vec<Chunk> {
            chunks
                .iter_mut()
                .find(|c| c.list_kind() == Some(kind))
                .unwrap()
                .children_mut()
                .unwrap()
        }
        fn named<'a>(chunks: &'a mut [Chunk], name: &str) -> &'a mut Vec<Chunk> {
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
            list(
                &mut chunks[start..end],
                if name == NAME { *b"sspc" } else { *b"tdgp" },
            )
        }
        named(
            named(list(&mut layer.content, *b"tdgp"), "ADBE Effect Parade"),
            NAME,
        )
    }
    fn scalar(layer: &mut Layer, number: u32, value: f64) {
        let body = plugin(layer)
            .iter_mut()
            .find(|c| c.list_kind() == Some(*b"tdgp"))
            .unwrap()
            .children_mut()
            .unwrap();
        let name = format!("{NAME}-{number:04}");
        let index = body
            .iter()
            .position(|c| {
                c.id() == *b"tdmn" && c.data_payload().unwrap().starts_with(name.as_bytes())
            })
            .unwrap();
        let leaf = body[index + 1..]
            .iter_mut()
            .find(|c| c.list_kind() == Some(*b"tdbs"))
            .unwrap()
            .children_mut()
            .unwrap();
        *leaf.iter_mut().find(|c| c.id() == *b"cdat").unwrap() =
            Chunk::data(*b"cdat", value.to_be_bytes()).unwrap();
    }
    fn original_owner(layer: &Layer) -> GroupLayer {
        let (mut project, _) = native();
        let mut source = layer.clone();
        scalar(&mut source, 8, 2.);
        let ItemKind::Composition(comp) =
            &mut project.items.iter_mut().find(|i| i.id == 1).unwrap().kind
        else {
            panic!()
        };
        comp.layers = vec![source];
        let result = super::super::to_structural_fx_document(&project, Some(1)).unwrap();
        g(&g(&result.document.composition().layers()[0]).layers[0]).clone()
    }
    #[test]
    fn native_transparent_vegas_builds_centered_contour_and_shared_editable_provider() {
        let (project, source) = native();
        let before = original_owner(&source);
        let result = super::super::to_structural_fx_document(&project, Some(1)).unwrap();
        let owner = g(&g(&result.document.composition().layers()[0]).layers[0]);
        let gate = g(&owner.layers[0]);
        let inverse = gate
            .track_matte
            .as_ref()
            .unwrap_or_else(|| panic!("{:?}", result.diagnostics));
        assert_eq!(inverse.mode, TrackMatteType::AlphaInverted);
        let outer = g(&gate.layers[0]);
        let inner = g(&gate.layers[1]);
        let provider = g(&gate.layers[2]);
        assert_eq!(inverse.layer, inner.id);
        for (branch, choke) in [(outer, -3.), (inner, 3.)] {
            let EffectData::Identified {
                effect:
                    EffectPayload::Known(LayerEffect::SimpleChoker {
                        choke: Some(actual),
                    }),
                ..
            } = branch.effects[0].data()
            else {
                panic!()
            };
            assert_eq!(*actual, choke);
            let proxy = g(&branch.layers[0]);
            assert_eq!(
                proxy.track_matte,
                Some(TrackMatte {
                    mode: TrackMatteType::Alpha,
                    layer: provider.id
                })
            );
        }
        let EffectData::Identified {
            effect:
                EffectPayload::Known(LayerEffect::LumaKey {
                    threshold: Some(t),
                    softness: Some(s),
                    ..
                }),
            ..
        } = provider.effects.last().unwrap().data()
        else {
            panic!()
        };
        assert_eq!(*t, 254.5 / 255.);
        assert_eq!(*s, 0.5 / 255.);
        assert!(*s > 0.);
        let original = g(&provider.layers[0]);
        let old = g(&before.layers[0]);
        assert_eq!(original.id, old.id);
        assert_eq!(original.layers, old.layers);
        assert_eq!(original.playback, old.playback);
        assert_eq!(owner.transform, before.transform);
        assert!(owner.effects.is_empty());
        assert_eq!(
            &provider.effects[..before.effects.len()],
            before.effects.as_slice()
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.to_string().contains("animated sweep/rotation"))
        );
        assert_eq!(
            result.document.composition().dynamics().entries(),
            super::super::to_structural_fx_document(
                &{
                    let (mut p, _) = native();
                    let ItemKind::Composition(c) =
                        &mut p.items.iter_mut().find(|i| i.id == 1).unwrap().kind
                    else {
                        panic!()
                    };
                    scalar(&mut c.layers[0], 8, 2.);
                    p
                },
                Some(1)
            )
            .unwrap()
            .document
            .composition()
            .dynamics()
            .entries()
        );
    }
    #[test]
    fn zero_width_is_transparent_and_control_output_failures_are_atomic() {
        let (_, source) = native();
        let original = original_owner(&source);
        for case in 0..7 {
            let mut native = source.clone();
            let mut owner = original.clone();
            let mut cursor = 100000;
            let mut budget = super::super::shapes::OutputBudget::default();
            match case {
                0 => scalar(&mut native, 20, 0.),
                1 => scalar(&mut native, 20, 21.),
                2 => scalar(&mut native, 8, 2.),
                3 => scalar(&mut native, 12, 254.5),
                4 => cursor = u64::MAX - 2,
                5 => budget = super::super::shapes::OutputBudget::with_limit(1),
                6 => {
                    let mut data = owner.layers[0].data().clone();
                    let LayerData::Group(g) = &mut data else {
                        panic!()
                    };
                    let mut rect = g.layers[0].data().clone();
                    let LayerData::Rect(r) = &mut rect else {
                        panic!()
                    };
                    r.rect.fill_color = [1., 0., 0., 1.];
                    g.layers[0] = fx_schema::Layer::from_data(&rect).unwrap();
                    owner.layers[0] = fx_schema::Layer::from_data(&data).unwrap();
                }
                _ => unreachable!(),
            }
            let before = owner.clone();
            let next = cursor;
            let bytes = budget.checkpoint();
            let result = apply(
                &native,
                &mut owner,
                Context {
                    size: [3840, 2160],
                    ordinals: &[],
                    depth: 0,
                    animations: &[],
                },
                State {
                    next: &mut cursor,
                    budget: &mut budget,
                },
            );
            if case == 0 {
                assert_eq!(result, Ok(true));
                assert_eq!(
                    g(&g(&owner.layers[0]).layers[0]).transform.opacity.value(),
                    0.
                );
            } else {
                assert!(result.is_err(), "case{case}: {result:?}");
                assert_eq!(owner, before);
                assert_eq!(cursor, next);
                assert_eq!(budget.checkpoint(), bytes);
            }
        }
        let mut layer = source.clone();
        let rows = plugin(&mut layer);
        let table = rows
            .iter_mut()
            .find(|c| c.list_kind() == Some(*b"parT"))
            .unwrap()
            .children_mut()
            .unwrap();
        let start = table
            .iter()
            .position(|c| {
                c.id() == *b"tdmn" && c.data_payload().unwrap().starts_with(b"APC Vegas-0008")
            })
            .unwrap();
        let mut bytes = table[start + 1].data_payload().unwrap().to_vec();
        bytes[62..64].copy_from_slice(&1_u16.to_be_bytes());
        table[start + 1] = Chunk::data(*b"pard", bytes).unwrap();
        assert!(
            profile(&layer).is_err(),
            "cached1 cannot replace native default2"
        );
    }
    #[test]
    fn actual_source_depth_and_animated_paints_are_rejected_without_mutation() {
        let (_, source) = native();
        let mut owner = original_owner(&source);
        let mut next = 100000;
        let original = owner.clone();
        let mut budget = super::super::shapes::OutputBudget::default();
        assert!(
            apply(
                &source,
                &mut owner,
                Context {
                    size: [3840, 2160],
                    ordinals: &[],
                    depth: MAX_GROUP_DEPTH - 5,
                    animations: &[]
                },
                State {
                    next: &mut next,
                    budget: &mut budget
                }
            )
            .is_err()
        );
        assert_eq!(owner, original);
        assert_eq!(next, 100000);
        let content = g(&owner.layers[0]);
        let id = content.layers[0].id();
        let entry = fx_schema::animator::AnimationGraphEntry {
            target: fx_schema::PropertyTarget::layer(id, fx_schema::PropType::FillColor),
            animator: fx_schema::animator::PropertyAnimator::constant(
                fx_schema::PropertyValue::Color([1.; 4]),
            )
            .unwrap(),
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        };
        assert!(
            apply(
                &source,
                &mut owner,
                Context {
                    size: [3840, 2160],
                    ordinals: &[],
                    depth: 0,
                    animations: &[entry]
                },
                State {
                    next: &mut next,
                    budget: &mut budget
                }
            )
            .is_err()
        );
        assert_eq!(owner, original);
    }
}
