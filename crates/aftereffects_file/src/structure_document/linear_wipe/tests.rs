use super::super::{ItemKind, StructuralProject, to_structural_fx_document};
use super::*;
use crate::{rifx::Rifx, schema::layer_records::LayerRecord};
use fx_schema::LayerData as FxLayer;

fn native_project() -> StructuralProject {
    let mut project = crate::structure::read_project(include_bytes!(
        "../../../tests/fixtures/effects/shape_owner_gaussian.aep"
    ))
    .unwrap();
    let fixture = Rifx::parse_with(
        include_bytes!("../../../tests/fixtures/effects/native-linear-wipe-anchor-controls.rifx"),
        |_| false,
    )
    .unwrap();
    let ItemKind::Composition(comp) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        panic!()
    };
    let template = comp.layers[0].clone();
    comp.layers = fixture.chunks()[..3]
        .iter()
        .enumerate()
        .map(|(index, chunk)| {
            let mut layer = template.clone();
            layer.content = chunk.children().unwrap().to_vec();
            layer.record =
                LayerRecord::decode(crate::properties::data(&layer.content, *b"ldta").unwrap())
                    .unwrap();
            layer.name = format!("Renamed wipe occurrence {index}").into();
            layer
        })
        .collect();
    comp.width = 3840;
    comp.height = 2160;
    comp.duration_secs = 7.;
    let mut solid = project.items[0].clone();
    solid.id = 96;
    solid.kind = ItemKind::Footage;
    solid.media = None;
    solid.solid = Some(Ok(crate::structure::SolidSource {
        width: 3840,
        height: 2160,
        pixel_aspect: (1, 1),
        color: [1.; 3],
    }));
    project.items.push(solid);
    project
}
fn g(layer: &fx_schema::Layer) -> &GroupLayer {
    let FxLayer::Group(group) = layer.data() else {
        panic!()
    };
    group
}
fn masked(group: &GroupLayer) -> Option<&GroupLayer> {
    if !group.masks.is_empty() {
        return Some(group);
    }
    group.layers.iter().find_map(|layer| match layer.data() {
        FxLayer::Group(child) => masked(child),
        _ => None,
    })
}
#[test]
fn native_linear_wipe_masks_completion_keys_and_post_anchor_are_editable() {
    let project = native_project();
    let result = to_structural_fx_document(&project, Some(1)).unwrap();
    let root = g(&result.document.composition().layers()[0]);
    for (index, owner) in root.layers.iter().map(g).enumerate() {
        let mask_stage = masked(owner)
            .unwrap_or_else(|| panic!("native Wipes omitted: {:?}", result.diagnostics));
        assert_eq!(mask_stage.masks.len(), if index == 0 { 1 } else { 2 });
        assert!(
            mask_stage
                .masks
                .iter()
                .all(|mask| mask.feather == [0., 0.] && !mask.inverted)
        );
    }
}

fn source(project: &StructuralProject, index: usize) -> &Layer {
    let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!()
    };
    &comp.layers[index]
}
fn baseline(project: &StructuralProject, index: usize) -> GroupLayer {
    let mut disabled = project.clone();
    let ItemKind::Composition(comp) =
        &mut disabled.items.iter_mut().find(|i| i.id == 1).unwrap().kind
    else {
        panic!()
    };
    for layer in &mut comp.layers {
        let mut bytes = layer.record.encode();
        bytes[39] &= !4;
        layer.record = LayerRecord::decode(&bytes).unwrap();
    }
    let converted = to_structural_fx_document(&disabled, Some(1)).unwrap();
    g(&g(&converted.document.composition().layers()[0]).layers[index]).clone()
}
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
        .position(|c| c.id() == *b"tdmn" && c.data_payload().unwrap().starts_with(name.as_bytes()))
        .unwrap();
    let end = chunks[start + 1..]
        .iter()
        .position(|c| c.id() == *b"tdmn")
        .map_or(chunks.len(), |i| start + 1 + i);
    list_mut(&mut chunks[start..end], kind)
}
fn plugin<'a>(layer: &'a mut Layer, name: &str) -> &'a mut Vec<Chunk> {
    named_mut(
        named_mut(
            list_mut(&mut layer.content, *b"tdgp"),
            "ADBE Effect Parade",
            *b"tdgp",
        ),
        name,
        *b"sspc",
    )
}
fn leaf<'a>(layer: &'a mut Layer, name: &str) -> &'a mut Vec<Chunk> {
    let effect = name.rsplit_once('-').unwrap().0;
    named_mut(list_mut(plugin(layer, effect), *b"tdgp"), name, *b"tdbs")
}
fn replace(chunks: &mut [Chunk], tag: [u8; 4], data: Vec<u8>) {
    *chunks.iter_mut().find(|c| c.id() == tag).unwrap() = Chunk::data(tag, data).unwrap();
}
fn ensure_scalar(layer: &mut Layer, name: &str) {
    let effect = name.rsplit_once('-').unwrap().0;
    let static_leaf = leaf(layer, "ADBE Linear Wipe-0002").clone();
    let body = list_mut(plugin(layer, effect), *b"tdgp");
    if !body
        .iter()
        .any(|c| c.id() == *b"tdmn" && c.data_payload().unwrap().starts_with(name.as_bytes()))
    {
        body.push(Chunk::data(*b"tdmn", format!("{name}\0").into_bytes()).unwrap());
        body.push(Chunk::list(*b"tdbs", static_leaf));
    }
}
fn scalar(layer: &mut Layer, name: &str, value: f64) {
    ensure_scalar(layer, name);
    replace(leaf(layer, name), *b"cdat", value.to_be_bytes().to_vec());
}
fn translated(transform: Transform, p: [f64; 2]) -> [f64; 2] {
    let angle = transform.rotation.to_radians();
    let q = [
        p[0] - transform.anchor_point[0],
        p[1] - transform.anchor_point[1],
    ];
    let Position::TwoD(position) = transform.position else {
        panic!()
    };
    [
        position[0] + angle.cos() * q[0] - angle.sin() * q[1],
        position[1] + angle.sin() * q[0] + angle.cos() * q[1],
    ]
}
#[test]
fn cardinal_endpoints_and_alternate_angled_completions_use_canvas_projection() {
    for angle in [0_f64, 90., 180., 270., 24., -37.] {
        let d = [angle.to_radians().sin(), angle.to_radians().cos()];
        for completion in [0., 17., 63., 100.] {
            let (transform, span) = guide([800, 300], angle, completion);
            let point = translated(transform, [0., 0.]);
            let projected = (point[0] - 400.) * d[0] + (point[1] - 150.) * d[1];
            assert!((projected - (completion / 100. - 0.5) * span).abs() < 1e-9);
            if completion == 0. || completion == 100. {
                for corner in [[0., 0.], [800., 0.], [0., 300.], [800., 300.]] {
                    let inside = (corner[0] - point[0]) * d[0] + (corner[1] - point[1]) * d[1];
                    assert!(if completion == 0. {
                        inside >= -1e-9
                    } else {
                        inside <= 1e-9
                    });
                }
            }
        }
    }
    let project = native_project();
    let profile = profile(source(&project, 1)).unwrap().unwrap();
    assert_eq!(profile.wipes[0].completion.values, [33.]);
    assert_eq!(profile.wipes[1].completion.values, [33.]);
    assert_eq!(profile.wipes[1].angle, profile.wipes[0].angle + 180.);
    assert_eq!(
        alias("effect('Other Name')('ADBE Linear Wipe-0002') + 180;", 2),
        Some(("Other Name", true))
    );
    for text in [
        "effect('x')('Completion')",
        "effect('x')('ADBE Linear Wipe-0001')+180",
        "effect('x')('ADBE Linear Wipe-0002')+90",
        "effect('x')('ADBE Linear Wipe-0002');evil()",
    ] {
        assert!(alias(text, if text.contains("0001") { 1 } else { 2 }).is_none());
    }
}
#[test]
fn native_wipe_keys_keep_source_clock_and_anchor_phase() {
    let project = native_project();
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    let root = g(&converted.document.composition().layers()[0]);
    let entries = converted.document.composition().dynamics().entries();
    for (index, layer) in root.layers.iter().enumerate() {
        let mask = masked(g(layer)).unwrap();
        let guide = mask
            .layers
            .iter()
            .find(|l| matches!(l.data(), LayerData::Shape(_)))
            .unwrap();
        let source = source(&project, index);
        let clock = NumericAnimationClock::parent_identity(source).unwrap();
        if index == 0 {
            let entry = entries
                .iter()
                .find(|e| e.target == PropertyTarget::layer(guide.id(), PropType::AnchorPointX))
                .unwrap();
            let keys = entry.animator.keyframe_track().unwrap().keyframes();
            let (_, span) = guide_fn([3840, 2160], -66., 100.);
            assert_eq!(keys[0].value(), &fx_schema::PropertyValue::Float(-span));
            assert_eq!(keys[1].value(), &fx_schema::PropertyValue::Float(0.));
            assert_eq!(
                keys[0].layer_time().as_millis(),
                (clock.seconds(0.) * 1000.).round() as i64
            );
            assert_eq!(
                keys[1].layer_time().as_millis(),
                (clock.seconds(1.) * 1000.).round() as i64
            );
        } else {
            let geometry = g(&g(layer).layers[0]);
            assert_eq!(geometry.transform.position, Position::xy(1920., 1080.));
            let entry = entries
                .iter()
                .find(|e| e.target == PropertyTarget::layer(geometry.id, PropType::AnchorPointX))
                .unwrap();
            let keys = entry.animator.keyframe_track().unwrap().keyframes();
            let native = profile(source).unwrap().unwrap().anchor.unwrap();
            assert_eq!(
                keys[0].value(),
                &fx_schema::PropertyValue::Float(native.keyframes[0].values[0] * 3840.)
            );
            assert_eq!(
                keys[1].layer_time().as_millis(),
                (clock.seconds(native.keyframes[1].time_secs) * 1000.).round() as i64
            );
        }
    }
}
fn guide_fn(size: [u16; 2], angle: f64, completion: f64) -> (Transform, f64) {
    guide(size, angle, completion)
}
#[test]
fn reversed_nonzero_occurrence_clock_and_late_budget_failures_are_atomic() {
    let project = native_project();
    let mut layer = source(&project, 0).clone();
    let mut bytes = layer.record.encode();
    bytes[8..12].copy_from_slice(&(-1_i32).to_be_bytes());
    bytes[12..16].copy_from_slice(&24576_i32.to_be_bytes());
    bytes[16..20].copy_from_slice(&24576_u32.to_be_bytes());
    bytes[108..112].copy_from_slice(&1_u32.to_be_bytes());
    layer.record = LayerRecord::decode(&bytes).unwrap();
    // The source start is +1s and stretch is -1; local 0..1 reverses to parent 1..0.
    let mut owner = baseline(&project, 0);
    let mut cursor = 90000;
    let mut budget = AnimationBudget::default();
    let mut output = OutputBudget::default();
    let entries = apply(
        &layer,
        &mut owner,
        Context {
            source: project.item(96),
            size: [3840, 2160],
            depth: 0,
            planar: true,
        },
        State {
            next: &mut cursor,
            animations: &mut budget,
            shapes: &mut output,
        },
    )
    .unwrap()
    .unwrap();
    let keys = entries[0].animator.keyframe_track().unwrap().keyframes();
    assert_eq!(keys[0].layer_time().as_millis(), 0);
    assert_eq!(keys[1].layer_time().as_millis(), 1000);
    for case in 0..3 {
        let mut owner = baseline(&project, 0);
        let before = owner.clone();
        let mut cursor = 90000;
        let mut budget = if case == 0 {
            AnimationBudget::with_limit(1)
        } else {
            AnimationBudget::default()
        };
        let mut output = if case == 1 {
            OutputBudget::with_limit(1)
        } else {
            OutputBudget::default()
        };
        let depth = if case == 2 { MAX_GROUP_DEPTH - 2 } else { 0 };
        let used = budget.used();
        let remaining = output.checkpoint();
        assert!(
            apply(
                source(&project, 0),
                &mut owner,
                Context {
                    source: project.item(96),
                    size: [3840, 2160],
                    depth,
                    planar: true
                },
                State {
                    next: &mut cursor,
                    animations: &mut budget,
                    shapes: &mut output
                }
            )
            .is_err()
        );
        assert_eq!(owner, before);
        assert_eq!(cursor, 90000);
        assert_eq!(budget.used(), used);
        assert_eq!(output.checkpoint(), remaining);
    }
}
#[test]
fn unsafe_controls_and_geometry_leave_source_owner_and_ids_unchanged() {
    let project = native_project();
    for case in 0..9 {
        let mut layer = source(&project, 1).clone();
        let mut owner = baseline(&project, 1);
        let before = owner.clone();
        let mut cursor = 90000;
        match case {
            0 => scalar(&mut layer, "ADBE Linear Wipe-0003", 1.),
            1 | 2 => {
                let name = if case == 1 {
                    "ADBE Linear Wipe-0002"
                } else {
                    "ADBE Linear Wipe-0003"
                };
                ensure_scalar(&mut layer, name);
                let leaf = leaf(&mut layer, name);
                let mut metadata = properties::data(leaf, *b"tdb4").unwrap().to_vec();
                metadata[68] = 1;
                replace(leaf, *b"tdb4", metadata);
            }
            3 => scalar(&mut layer, "ADBE Geometry2-0007", 1.),
            4 => {
                let root = list_mut(&mut layer.content, *b"tdgp");
                let start = root
                    .iter()
                    .position(|c| {
                        c.id() == *b"tdmn"
                            && c.data_payload().unwrap().starts_with(b"ADBE Effect Parade")
                    })
                    .unwrap();
                let duplicate = root[start..].to_vec();
                root.extend(duplicate);
            }
            5 => {
                let body = list_mut(plugin(&mut layer, WIPE), *b"tdgp");
                body.push(Chunk::data(*b"tdmn", b"Unknown-0001\0".to_vec()).unwrap());
            }
            6 => {
                let body = list_mut(plugin(&mut layer, WIPE), *b"tdgp");
                let start = body
                    .iter()
                    .position(|c| {
                        c.id() == *b"tdmn"
                            && c.data_payload()
                                .unwrap()
                                .starts_with(b"ADBE Linear Wipe-0002")
                    })
                    .unwrap();
                let copy = body[start..start + 2].to_vec();
                body.extend(copy);
            }
            7 => cursor = u64::MAX - 2,
            8 => {
                owner.track_matte = Some(fx_schema::TrackMatte {
                    mode: fx_schema::layer::TrackMatteType::Alpha,
                    layer: owner.id,
                })
            }
            _ => unreachable!(),
        }
        let before = if case == 8 { owner.clone() } else { before };
        let initial = cursor;
        let mut budget = AnimationBudget::default();
        assert!(
            apply(
                &layer,
                &mut owner,
                Context {
                    source: project.item(96),
                    size: [3840, 2160],
                    depth: 0,
                    planar: true
                },
                State {
                    next: &mut cursor,
                    animations: &mut budget,
                    shapes: &mut OutputBudget::default()
                }
            )
            .is_err(),
            "case {case}"
        );
        assert_eq!(owner, before);
        assert_eq!(cursor, initial);
        assert_eq!(budget.used(), 0);
    }
    let mut disabled = source(&project, 0).clone();
    let mut bytes = disabled.record.encode();
    bytes[39] &= !4;
    disabled.record = LayerRecord::decode(&bytes).unwrap();
    assert!(profile(&disabled).unwrap().is_none());
    let descriptor = plugin(&mut disabled, WIPE);
    replace(list_mut(descriptor, *b"tdgp"), *b"tdsb", vec![0; 4]);
    let mut bytes = disabled.record.encode();
    bytes[39] |= 4;
    disabled.record = LayerRecord::decode(&bytes).unwrap();
    assert!(profile(&disabled).unwrap().is_none());
}

#[test]
fn curved_or_diagonal_post_anchor_is_declined() {
    let project = native_project();
    let p = profile(source(&project, 1)).unwrap().unwrap();
    let mut anchor = p.anchor.unwrap();
    anchor.keyframes[1].values[1] += 0.2;
    assert!(curve(&anchor, 2, false).is_err());
    anchor.keyframes[1].values[1] -= 0.2;
    anchor.keyframes[1].spatial_in[0] = 0.1;
    assert!(curve(&anchor, 2, false).is_err());
}
