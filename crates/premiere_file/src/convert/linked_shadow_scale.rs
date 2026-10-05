//! Static uniform linked placement compensates screen-space pixel effect units.
//! DropShadow lengths and signed SimpleChoker radii share the placement factor.
//! GaussianBlur already inherits world scale in the renderer and is left intact.
use crate::schema::{PrAnimatedProperty, PrVideoOccurrence};
use fx_schema::{
    animator::{
        AnimationGraphEntry, AnimatorData, PropertyAnimator, PropertyKeyframe,
        PropertyKeyframeTrack,
    },
    EffectData, EffectId, EffectPayload, EffectRecord, Layer, LayerData, LayerEffect, LayerId,
    NonNegativeProperty, PropType, PropertyValue, Transform,
};
use std::collections::{HashMap, HashSet};

pub(super) fn placement(
    clip: &PrVideoOccurrence,
    transform: &Transform,
    parented: bool,
    transform_stage: bool,
) -> std::result::Result<f64, &'static str> {
    if parented
        || transform_stage
        || transform.rotation != 0.
        || transform.rotation_x != 0.
        || transform.rotation_y != 0.
        || transform.skew != 0.
        || transform.orientation != [0.; 3]
        || transform.scale[0] != transform.scale[1]
        || !transform.scale[0].is_finite()
        || transform.scale[0] <= 0.
        || clip.animations.iter().any(|animation| {
            matches!(
                animation.property(),
                PrAnimatedProperty::Rotation | PrAnimatedProperty::UniformScale
            )
        })
    {
        return Err("screen-pixel placement compensation requires unparented static uniform Motion with no rotation or Transform effect stage");
    }
    Ok(transform.scale[0] / 100.)
}

fn length_param(name: &str) -> bool {
    matches!(name, "offset" | "blurRadius" | "spreadRadius" | "choke")
}
fn scaled(
    value: &PropertyValue,
    factor: f64,
    vector: bool,
    choke: bool,
) -> Result<PropertyValue, String> {
    match value {
        PropertyValue::Vector2(v) if vector && v.iter().all(|v| (v * factor).is_finite()) => {
            Ok(PropertyValue::Vector2(v.map(|v| v * factor)))
        }
        PropertyValue::Float(v)
            if !vector
                && (choke || *v >= 0.)
                && (v * factor).is_finite()
                && (!choke || (v * factor).abs() <= 10.) =>
        {
            Ok(PropertyValue::Float(v * factor))
        }
        _ => Err("screen-pixel track has unsupported value shape/range".into()),
    }
}
fn scale_animator(
    animator: &PropertyAnimator,
    factor: f64,
    vector: bool,
    choke: bool,
) -> Result<PropertyAnimator, String> {
    let data = match animator.data() {
        AnimatorData::Constant { value } => AnimatorData::Constant {
            value: scaled(value, factor, vector, choke)?,
        },
        AnimatorData::Keyframes {
            track,
            enabled,
            disabled_value,
        } => AnimatorData::Keyframes {
            track: PropertyKeyframeTrack::new(
                track
                    .keyframes()
                    .iter()
                    .map(|key| {
                        Ok(PropertyKeyframe::new(
                            key.id().clone(),
                            key.layer_time(),
                            scaled(key.value(), factor, vector, choke)?,
                            key.easing(),
                        )
                        .with_spatial_tangents(
                            key.spatial_in_tangent().map(|v| v * factor),
                            key.spatial_out_tangent().map(|v| v * factor),
                        ))
                    })
                    .collect::<Result<_, String>>()?,
            )
            .map_err(|e| e.to_string())?,
            enabled: *enabled,
            disabled_value: disabled_value
                .as_ref()
                .map(|value| scaled(value, factor, vector, choke))
                .transpose()?,
        },
        AnimatorData::JsScript { .. } => {
            return Err(
                "script-driven effect pixels are outside static placement compensation".into(),
            )
        }
    };
    PropertyAnimator::from_data(&data).map_err(|e| e.to_string())
}
fn layer_parts(layer: &mut LayerData) -> Option<(&Transform, &mut Vec<EffectRecord>)> {
    match layer {
        LayerData::Group(x) => Some((&x.transform, &mut x.effects)),
        LayerData::Text(x) => Some((&x.transform, &mut x.effects)),
        LayerData::Rect(x) => Some((&x.transform, &mut x.effects)),
        LayerData::Shape(x) => Some((&x.transform, &mut x.effects)),
        LayerData::Image(x) => Some((&x.transform, &mut x.effects)),
        LayerData::Video(x) => Some((&x.transform, &mut x.effects)),
        LayerData::BooleanOperation(x) => Some((&x.transform, &mut x.effects)),
        LayerData::Adjustment(x) => Some((&x.transform, &mut x.effects)),
        _ => None,
    }
}
pub(super) struct Report {
    pub compensated: usize,
    pub declined: usize,
}

/// Stages the fresh occurrence tree and its tracks; cached AE pictures are untouched.
pub(super) fn apply(
    root: &mut Layer,
    entries: &mut Vec<AnimationGraphEntry>,
    factor: f64,
) -> Result<Report, String> {
    if !factor.is_finite() || factor <= 0. {
        return Err("invalid linked placement factor".into());
    }
    let dynamic: HashSet<LayerId> = entries
        .iter()
        .filter_map(|entry| entry.target.as_property())
        .filter(|property| {
            matches!(
                property.property_type(),
                PropType::ScaleX | PropType::ScaleY
            )
        })
        .map(|property| property.layer_id())
        .collect();
    let mut owned = HashMap::<EffectId, Vec<usize>>::new();
    for (index, entry) in entries.iter().enumerate() {
        if let fx_schema::PropertyTarget::EffectProperty(target) = &entry.target {
            if length_param(target.param_name()) {
                owned.entry(target.effect_id()).or_default().push(index);
            }
        }
    }
    let referenced: HashSet<EffectId> = entries
        .iter()
        .flat_map(|entry| {
            entry
                .dependencies
                .iter()
                .chain(entry.random_seed_target.iter())
        })
        .filter_map(fx_schema::PropertyTarget::effect_id)
        .collect();
    struct Walk<'a> {
        dynamic: &'a HashSet<LayerId>,
        owned: &'a HashMap<EffectId, Vec<usize>>,
        referenced: &'a HashSet<EffectId>,
        entries: &'a [AnimationGraphEntry],
        factor: f64,
        tracks: Vec<(usize, PropertyAnimator)>,
        report: Report,
    }
    impl Walk<'_> {
        fn visit(
            &mut self,
            layer: &mut LayerData,
            ancestors_neutral: bool,
            depth: usize,
        ) -> Result<(), String> {
            if depth >= 64 {
                return Err("linked pixel placement exceeds bounded tree depth".into());
            }
            let id = layer.id();
            let mut neutral = ancestors_neutral && !self.dynamic.contains(&id);
            if let Some((transform, effects)) = layer_parts(layer) {
                neutral &= transform.scale == [100., 100.];
                for record in effects {
                    let EffectData::Identified {
                        id,
                        enabled,
                        effect: EffectPayload::Known(effect),
                    } = record.data()
                    else {
                        continue;
                    };
                    if !matches!(
                        effect,
                        LayerEffect::DropShadow(_) | LayerEffect::SimpleChoker { .. }
                    ) {
                        continue;
                    }
                    let affected = self.owned.get(id).map_or(&[][..], Vec::as_slice);
                    let updates = affected
                        .iter()
                        .map(|index| {
                            let entry = &self.entries[*index];
                            if !entry.dependencies.is_empty()
                                || entry.random_seed_target.is_some()
                                || !entry.layer_refs.is_empty()
                            {
                                return Err("dependent effect pixel animation".into());
                            }
                            let fx_schema::PropertyTarget::EffectProperty(target) = &entry.target
                            else {
                                unreachable!()
                            };
                            if !matches!(
                                (effect, target.param_name()),
                                (
                                    LayerEffect::DropShadow(_),
                                    "offset" | "blurRadius" | "spreadRadius"
                                ) | (LayerEffect::SimpleChoker { .. }, "choke")
                            ) {
                                return Err("pixel target does not belong to its effect".into());
                            }
                            Ok((
                                *index,
                                scale_animator(
                                    &entry.animator,
                                    self.factor,
                                    target.param_name() == "offset",
                                    target.param_name() == "choke",
                                )?,
                            ))
                        })
                        .collect::<Result<Vec<_>, String>>();
                    if !neutral || self.referenced.contains(id) || updates.is_err() {
                        self.report.declined += 1;
                        continue;
                    }
                    let effect = match effect {
                        LayerEffect::DropShadow(shadow) => {
                            let mut shadow = shadow.clone();
                            shadow.offset = shadow.offset.map(|v| v * self.factor);
                            shadow.blur_radius =
                                NonNegativeProperty::new(shadow.blur_radius.value() * self.factor)
                                    .ok_or("scaled shadow blur is nonfinite")?;
                            shadow.spread_radius = NonNegativeProperty::new(
                                shadow.spread_radius.value() * self.factor,
                            )
                            .ok_or("scaled shadow spread is nonfinite")?;
                            LayerEffect::DropShadow(shadow)
                        }
                        LayerEffect::SimpleChoker { choke } => {
                            let value = Some(choke.unwrap_or(1.) * self.factor);
                            if value.is_some_and(|value| !value.is_finite() || value.abs() > 10.) {
                                self.report.declined += 1;
                                continue;
                            }
                            LayerEffect::SimpleChoker { choke: value }
                        }
                        _ => unreachable!("pixel effect admitted above"),
                    };
                    *record = EffectRecord::from_data(&EffectData::Identified {
                        id: *id,
                        enabled: *enabled,
                        effect: EffectPayload::Known(effect),
                    })
                    .map_err(|e| e.to_string())?;
                    self.tracks.extend(updates.expect("updates admitted above"));
                    self.report.compensated += 1;
                }
            }
            let children = match layer {
                LayerData::Group(x) => Some(&mut x.layers),
                LayerData::BooleanOperation(x) => Some(&mut x.layers),
                LayerData::AiEdit(x) => Some(&mut x.layers),
                _ => None,
            };
            if let Some(children) = children {
                for stored in children {
                    let mut child = stored.data().clone();
                    self.visit(&mut child, neutral, depth + 1)?;
                    *stored = Layer::from_data(&child).map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        }
    }
    let mut candidate = root.data().clone();
    let mut walk = Walk {
        dynamic: &dynamic,
        owned: &owned,
        referenced: &referenced,
        entries,
        factor,
        tracks: Vec::new(),
        report: Report {
            compensated: 0,
            declined: 0,
        },
    };
    walk.visit(&mut candidate, true, 0)?;
    let candidate = Layer::from_data(&candidate).map_err(|e| e.to_string())?;
    let Walk { tracks, report, .. } = walk;
    for (index, animator) in tracks {
        entries[index].animator = animator;
    }
    *root = candidate;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aftereffects_file::{
        rifx::{Chunk, Rifx},
        schema::layer_records::LayerRecord,
        structure::{read_project, ItemKind, SolidSource},
    };
    fn list_mut(chunks: &mut [Chunk], kind: [u8; 4]) -> &mut Vec<Chunk> {
        chunks
            .iter_mut()
            .find(|c| c.list_kind() == Some(kind))
            .unwrap()
            .children_mut()
            .unwrap()
    }
    fn named_mut<'a>(chunks: &'a mut [Chunk], name: &str, kind: [u8; 4]) -> &'a mut Vec<Chunk> {
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
    fn native_picture() -> (Layer, Vec<AnimationGraphEntry>) {
        let mut project = read_project(include_bytes!(
            "../../../aftereffects_file/tests/fixtures/effects/shape_owner_gaussian.aep"
        ))
        .unwrap();
        let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
            panic!()
        };
        let template = comp.layers[0].clone();
        let parsed = Rifx::parse_with(
            include_bytes!("../../tests/fixtures/linked-shadow-placement-controls.rifx"),
            |_| false,
        )
        .unwrap();
        let table = parsed
            .chunks()
            .iter()
            .find(|c| c.list_kind() == Some(*b"parT"))
            .unwrap()
            .children()
            .unwrap();
        let ItemKind::Composition(comp) =
            &mut project.items.iter_mut().find(|i| i.id == 1).unwrap().kind
        else {
            panic!()
        };
        comp.width = 3840;
        comp.height = 2160;
        comp.duration_secs = 6.;
        comp.layers = parsed
            .chunks()
            .iter()
            .filter(|c| c.list_kind() == Some(*b"Layr"))
            .map(|chunk| {
                let mut layer = template.clone();
                layer.content = chunk.children().unwrap().to_vec();
                layer.record = LayerRecord::decode(
                    layer
                        .content
                        .iter()
                        .find(|c| c.id() == *b"ldta")
                        .unwrap()
                        .data_payload()
                        .unwrap(),
                )
                .unwrap();
                layer.name = format!("Independent native door {}", layer.record.id()).into();
                layer
            })
            .collect();
        let mut defaults = project.item(1).unwrap().clone();
        defaults.id = 2;
        let ItemKind::Composition(comp) = &mut defaults.kind else {
            panic!()
        };
        let parade = named_mut(
            list_mut(&mut comp.layers[0].content, *b"tdgp"),
            "ADBE Effect Parade",
            *b"tdgp",
        );
        *list_mut(named_mut(parade, "ADBE Geometry2", *b"sspc"), *b"parT") = table.to_vec();
        project.items.push(defaults);
        let mut solid = project.items[0].clone();
        solid.id = 96;
        solid.kind = ItemKind::Footage;
        solid.media = None;
        solid.solid = Some(Ok(SolidSource {
            width: 3840,
            height: 2160,
            pixel_aspect: (1, 1),
            color: [1.; 3],
        }));
        project.items.push(solid);
        let result =
            aftereffects_file::structure_document::to_structural_fx_document(&project, Some(1))
                .unwrap();
        (
            result.document.composition().layers()[0].clone(),
            result.document.composition().dynamics().entries().to_vec(),
        )
    }
    fn shadows(layer: &Layer, out: &mut Vec<(EffectId, fx_schema::DropShadow)>) {
        for effect in layer.effects() {
            if let EffectData::Identified {
                id,
                effect: EffectPayload::Known(LayerEffect::DropShadow(shadow)),
                ..
            } = effect.data()
            {
                out.push((*id, shadow.clone()));
            }
        }
        if let Some(children) = layer.child_layers() {
            for child in children {
                shadows(child, out);
            }
        }
    }
    #[test]
    fn native_four_doors_uniform_placement_scales_pixels_and_keys_without_double_rotation() {
        let (mut root, mut entries) = native_picture();
        let before = root.clone();
        let original = entries.clone();
        let mut old = Vec::new();
        shadows(&root, &mut old);
        assert_eq!(old.len(), 8, "all four native doors have two shadows");
        assert!(
            old.iter().any(|(_, s)| s.offset[1] > 0.) && old.iter().any(|(_, s)| s.offset[1] < 0.),
            "post-effect half-turn offsets already have both signs"
        );
        let report = apply(&mut root, &mut entries, 0.5).unwrap();
        assert_eq!(report.compensated, 8);
        assert_eq!(report.declined, 0);
        let mut new = Vec::new();
        shadows(&root, &mut new);
        for ((old_id, old), (new_id, new)) in old.iter().zip(&new) {
            assert_eq!(old_id, new_id);
            assert_eq!(new.offset, old.offset.map(|v| v * 0.5));
            assert_eq!(new.blur_radius.value(), old.blur_radius.value() * 0.5);
            assert_eq!(new.spread_radius.value(), old.spread_radius.value() * 0.5);
        }
        for (old, new) in original.iter().zip(&entries) {
            if let fx_schema::PropertyTarget::EffectProperty(target) = &old.target {
                if target.param_name() == "offset" {
                    assert_eq!(
                        new.animator,
                        scale_animator(&old.animator, 0.5, true, false).unwrap()
                    );
                    continue;
                }
            }
            assert_eq!(old, new);
        }
        fn geometry(layer: &Layer) -> Vec<(LayerId, Transform)> {
            let mut out = Vec::new();
            if let LayerData::Group(g) = layer.data() {
                out.push((g.id, g.transform));
                for child in &g.layers {
                    out.extend(geometry(child));
                }
            }
            out
        }
        assert_eq!(geometry(&before), geometry(&root));
    }
    #[test]
    fn placement_declines_dynamic_internal_scale_atomically_for_affected_subtree() {
        let (mut root, mut entries) = native_picture();
        let before = root.clone();
        let mut data = root.data().clone();
        let LayerData::Group(g) = &mut data else {
            panic!()
        };
        g.transform.scale = [50., 50.];
        root = Layer::from_data(&data).unwrap();
        let staged = root.clone();
        let tracks = entries.clone();
        let report = apply(&mut root, &mut entries, 0.5).unwrap();
        assert_eq!(report.compensated, 0);
        assert_eq!(report.declined, 8);
        assert_eq!(root, staged);
        assert_eq!(entries, tracks);
        assert!(apply(&mut root, &mut entries, f64::INFINITY).is_err());
        assert_eq!(root, staged);
        let LayerData::Group(mut group) = before.data().clone() else {
            panic!()
        };
        group.effects.push(
            EffectRecord::from_data(&EffectData::Identified {
                id: EffectId::new(900000),
                enabled: true,
                effect: EffectPayload::Known(LayerEffect::GaussianBlur {
                    blurriness: NonNegativeProperty::new(23.).unwrap(),
                    repeat_edge_pixels: Some(false),
                    layer_size: None,
                }),
            })
            .unwrap(),
        );
        root = Layer::from_data(&LayerData::Group(group)).unwrap();
        let blur = root.effects().last().unwrap().clone();
        apply(&mut root, &mut entries, 0.5).unwrap();
        assert_eq!(root.effects().last().unwrap(), &blur);
    }
    #[test]
    fn static_placement_guards_and_dependent_pixels_retain_the_source() {
        let mut clip = PrVideoOccurrence {
            id: None,
            media: crate::format::MediaId("different".into()),
            start_ticks: 0,
            end_ticks: 100,
            in_ticks: 0,
            out_ticks: 100,
            playback_rate: 1.,
            frame_blending: None,
            opacity: 100.,
            blend_mode: crate::schema::PrBlendMode::Normal,
            transform: Default::default(),
            crop: Default::default(),
            animations: Vec::new(),
            effects: Vec::new(),
            effects_above_mask: 0,
            stroke: None,
            active_transforms: 0,
            source_effects: None,
            time_remap: None,
            linear_wipe: None,
            opacity_mask: None,
            track_matte: None,
            enabled: true,
        };
        let mut transform = super::super::background::identity_transform();
        transform.scale = [75.; 2];
        assert_eq!(placement(&clip, &transform, false, false), Ok(0.75));
        assert!(placement(&clip, &transform, true, false).is_err());
        assert!(placement(&clip, &transform, false, true).is_err());
        transform.rotation = 180.;
        assert!(placement(&clip, &transform, false, false).is_err());
        transform.rotation = 0.;
        transform.scale = [75., 76.];
        assert!(placement(&clip, &transform, false, false).is_err());
        transform.scale = [75.; 2];
        clip.animations
            .push(crate::schema::PrPropertyAnimation::UniformScale(vec![]));
        assert!(placement(&clip, &transform, false, false).is_err());
        let (mut root, mut entries) = native_picture();
        let mut old = Vec::new();
        shadows(&root, &mut old);
        let id = old[0].0;
        let target = fx_schema::PropertyTarget::effect_param(id, "offset");
        let index = entries
            .iter()
            .position(|entry| entry.target == target)
            .unwrap();
        entries[index].dependencies.push(target.clone());
        let original = entries[index].clone();
        let report = apply(&mut root, &mut entries, 0.5).unwrap();
        assert_eq!(report.declined, 1);
        assert_eq!(report.compensated, 7);
        assert_eq!(entries[index], original);
        let mut after = Vec::new();
        shadows(&root, &mut after);
        assert_eq!(after.iter().find(|(key, _)| *key == id).unwrap(), &old[0]);
    }

    #[test]
    fn signed_morphology_pixels_and_keys_scale_but_clamped_or_scripted_tracks_decline() {
        let (mut root, mut entries) = native_picture();
        let mut data = root.data().clone();
        let LayerData::Group(group) = &mut data else {
            panic!()
        };
        for (id, value) in [(900001, -3.), (900002, 3.)] {
            group.effects.push(
                EffectRecord::from_data(&EffectData::Identified {
                    id: EffectId::new(id),
                    enabled: true,
                    effect: EffectPayload::Known(LayerEffect::SimpleChoker { choke: Some(value) }),
                })
                .unwrap(),
            );
        }
        root = Layer::from_data(&data).unwrap();
        let target = fx_schema::PropertyTarget::effect_param(EffectId::new(900001), "choke");
        let animator = PropertyAnimator::keyframes(
            PropertyKeyframeTrack::new(vec![
                PropertyKeyframe::new(
                    fx_schema::animator::KeyframeId::new("negative"),
                    fx_schema::TimeOffset::ZERO,
                    PropertyValue::Float(-3.),
                    fx_schema::animator::PropertyKeyframeEasing::Linear,
                ),
                PropertyKeyframe::new(
                    fx_schema::animator::KeyframeId::new("positive"),
                    fx_schema::TimeOffset::from_millis(500),
                    PropertyValue::Float(3.),
                    fx_schema::animator::PropertyKeyframeEasing::Linear,
                ),
            ])
            .unwrap(),
        );
        entries.push(AnimationGraphEntry {
            target: target.clone(),
            animator: animator.clone(),
            dependencies: Vec::new(),
            random_seed_target: None,
            layer_refs: Default::default(),
        });
        let report = apply(&mut root, &mut entries, 0.5).unwrap();
        assert_eq!(report.compensated, 10);
        assert_eq!(report.declined, 0);
        for (record, expected) in root.effects().iter().zip([-1.5, 1.5]) {
            let EffectData::Identified {
                effect: EffectPayload::Known(LayerEffect::SimpleChoker { choke: Some(value) }),
                ..
            } = record.data()
            else {
                panic!()
            };
            assert_eq!(*value, expected);
        }
        assert_eq!(
            entries.last().unwrap().animator,
            scale_animator(&animator, 0.5, false, true).unwrap()
        );
        let original = root.clone();
        let graph = entries.clone();
        let report = apply(&mut root, &mut entries, 10.).unwrap();
        assert_eq!(report.declined, 2);
        assert_eq!(root.effects(), original.effects());
        assert_eq!(entries.last(), graph.last());
        let mut script = entries.last().unwrap().clone();
        script.animator = PropertyAnimator::from_data(&AnimatorData::JsScript {
            code: None,
            layer_time_js_code: Some("return 3;".into()),
        })
        .unwrap();
        *entries.last_mut().unwrap() = script.clone();
        let report = apply(&mut root, &mut entries, 0.5).unwrap();
        assert_eq!(report.declined, 1);
        assert_eq!(entries.last(), Some(&script));
    }

    #[test]
    fn implicit_morphology_default_materializes_scaled_renderer_radius() {
        let (mut root, mut entries) = native_picture();
        let mut data = root.data().clone();
        let LayerData::Group(group) = &mut data else {
            panic!()
        };
        group.effects.push(
            EffectRecord::from_data(&EffectData::Identified {
                id: EffectId::new(900009),
                enabled: true,
                effect: EffectPayload::Known(LayerEffect::SimpleChoker { choke: None }),
            })
            .unwrap(),
        );
        root = Layer::from_data(&data).unwrap();
        let report = apply(&mut root, &mut entries, 0.5).unwrap();
        assert_eq!(report.compensated, 9);
        assert!(matches!(
            root.effects()[0].data(),
            EffectData::Identified {
                effect: EffectPayload::Known(LayerEffect::SimpleChoker { choke: Some(0.5) }),
                ..
            }
        ));
    }
}
