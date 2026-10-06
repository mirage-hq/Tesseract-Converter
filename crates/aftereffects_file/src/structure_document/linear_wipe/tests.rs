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

fn composition_source_project() -> StructuralProject {
    let mut project = native_project();
    let mut nested = match &project.item(1).unwrap().kind {
        ItemKind::Composition(composition) => (**composition).clone(),
        _ => panic!("fixture root must be a composition"),
    };
    nested.layers.truncate(1);
    let mut record = nested.layers[0].record.encode();
    record[0..4].copy_from_slice(&97_001_u32.to_be_bytes());
    record[40..44].copy_from_slice(&97_u32.to_be_bytes());
    record[132..136].copy_from_slice(&0_u32.to_be_bytes());
    record[39] &= !4;
    nested.layers[0].record = LayerRecord::decode(&record).unwrap();
    nested.layers[0].name = "Retained source paint".into();

    let mut solid = project.item(96).unwrap().clone();
    solid.id = 97;
    project.items.push(solid);
    let source = project.items.iter_mut().find(|item| item.id == 96).unwrap();
    source.kind = ItemKind::Composition(Box::new(nested));
    source.solid = None;

    project
}

fn evaluated_completion_project() -> StructuralProject {
    let mut project = composition_source_project();
    let ItemKind::Composition(composition) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        panic!()
    };
    let completion = leaf(&mut composition.layers[0], "ADBE Linear Wipe-0001");
    let mut metadata = properties::data(completion, *b"tdb4").unwrap().to_vec();
    metadata[68] = 0;
    metadata[119] &= !1;
    metadata[120] |= 1;
    completion.retain(|chunk| {
        chunk.list_kind() != Some(*b"list") && !matches!(&chunk.id(), b"cdat" | b"Utf8" | b"expr")
    });
    replace(completion, *b"tdb4", metadata);
    completion.push(Chunk::data(*b"cdat", 21_f64.to_be_bytes().to_vec()).unwrap());
    completion.push(Chunk::data(*b"Utf8", b"20 + Math.pow(time, 2);".to_vec()).unwrap());
    project
}

fn contains_rect(group: &GroupLayer) -> bool {
    group.layers.iter().any(|layer| match layer.data() {
        FxLayer::Rect(_) => true,
        FxLayer::Group(child) => contains_rect(child),
        _ => false,
    })
}

#[test]
fn composition_source_wipe_uses_evaluated_completion_and_keeps_editable_paint() {
    let project = evaluated_completion_project();
    let result = to_structural_fx_document(&project, Some(1)).unwrap();
    let root = g(&result.document.composition().layers()[0]);
    let owner = g(&root.layers[0]);
    let mask_stage = masked(owner)
        .unwrap_or_else(|| panic!("composition-source Wipe omitted: {:?}", result.diagnostics));
    assert_eq!(mask_stage.masks.len(), 1);
    assert!(contains_rect(mask_stage));
    assert!(owner.description.contains("source=96"));

    let guide = mask_stage
        .layers
        .iter()
        .find(|layer| matches!(layer.data(), FxLayer::Shape(_)))
        .unwrap();
    let completion = result
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .find(|entry| entry.target == PropertyTarget::layer(guide.id(), PropType::AnchorPointX))
        .unwrap();
    assert!(
        completion
            .animator
            .keyframe_track()
            .unwrap()
            .keyframes()
            .len()
            > 2
    );
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .to_string()
            .contains("converter-evaluated expression lowered")
    }));
}

#[test]
fn keyed_integer_continuous_completion_expression_lowers_to_editable_mask() {
    let mut project = composition_source_project();
    let ItemKind::Composition(composition) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        panic!()
    };
    declare_wipe_controls(&mut composition.layers[0]);
    let completion = leaf(&mut composition.layers[0], "ADBE Linear Wipe-0001");
    let mut metadata = properties::data(completion, *b"tdb4").unwrap().to_vec();
    assert_ne!(metadata[59] & 4, 0, "fixture Completion must be integer");
    assert_ne!(metadata[68], 0, "fixture Completion must remain keyed");
    metadata[119] &= !1;
    metadata[120] |= 1;
    replace(completion, *b"tdb4", metadata);
    completion.push(Chunk::data(*b"Utf8", b"value;".to_vec()).unwrap());

    let result = to_structural_fx_document(&project, Some(1)).unwrap();
    let root = g(&result.document.composition().layers()[0]);
    let owner = g(&root.layers[0]);
    let mask_stage = masked(owner)
        .unwrap_or_else(|| panic!("integer Completion Wipe omitted: {:?}", result.diagnostics));
    assert!(contains_rect(mask_stage));
    assert!(owner.description.contains("source=96"));
    let guide = mask_stage
        .layers
        .iter()
        .find(|layer| matches!(layer.data(), FxLayer::Shape(_)))
        .unwrap();
    let completion = result
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .find(|entry| entry.target == PropertyTarget::layer(guide.id(), PropType::AnchorPointX))
        .unwrap();
    assert!(
        completion
            .animator
            .keyframe_track()
            .unwrap()
            .keyframes()
            .len()
            > 2
    );
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .to_string()
            .contains("converter-evaluated expression lowered")
    }));
    assert!(!result.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .to_string()
            .contains("Linear Wipe source profile not lowered")
    }));
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
fn named_run(chunks: &[Chunk], name: &str) -> Vec<Chunk> {
    let start = chunks
        .iter()
        .position(|chunk| {
            chunk.id() == *b"tdmn"
                && chunk
                    .data_payload()
                    .is_some_and(|bytes| bytes.starts_with(name.as_bytes()))
        })
        .unwrap();
    let end = chunks[start + 1..]
        .iter()
        .position(|chunk| chunk.id() == *b"tdmn")
        .map_or(chunks.len(), |index| start + 1 + index);
    chunks[start..end].to_vec()
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
fn effect_parade(layer: &mut Layer) -> &mut Vec<Chunk> {
    named_mut(
        list_mut(&mut layer.content, *b"tdgp"),
        "ADBE Effect Parade",
        *b"tdgp",
    )
}

fn plugin<'a>(layer: &'a mut Layer, name: &str) -> &'a mut Vec<Chunk> {
    named_mut(effect_parade(layer), name, *b"sspc")
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
fn declare_wipe_controls(layer: &mut Layer) {
    let mut declarations = Vec::new();
    for (name, kind, value) in std::iter::once(("ADBE Effect Built In Params".to_owned(), 9, 0))
        .chain(
            WIPE_DEFAULTS
                .iter()
                .map(|(number, kind, value)| (format!("{WIPE}-{number:04}"), *kind, *value)),
        )
    {
        let mut bytes = vec![0; 148];
        bytes[12..16].copy_from_slice(&kind.to_be_bytes());
        if kind == 7 {
            bytes[60..62].copy_from_slice(&2_u16.to_be_bytes());
            bytes[62..64].copy_from_slice(&(value as u16).to_be_bytes());
        } else {
            bytes[56..60].copy_from_slice(&value.to_be_bytes());
        }
        let mut name = name.into_bytes();
        name.resize(40, 0);
        declarations.push(Chunk::data(*b"tdmn", name).unwrap());
        declarations.push(Chunk::data(*b"pard", bytes).unwrap());
    }
    *list_mut(plugin(layer, WIPE), *b"parT") = declarations;
}
fn shape_source_project() -> StructuralProject {
    let wipe_project = native_project();
    let mut wipe_layer = source(&wipe_project, 0).clone();
    let wipe_effects = effect_parade(&mut wipe_layer).clone();
    let scale_project = crate::structure::read_project(include_bytes!(
        "../../../tests/fixtures/effects/shadow-static-source-plane.aep"
    ))
    .unwrap();
    let ItemKind::Composition(scale_composition) = &scale_project.item(1).unwrap().kind else {
        panic!("static source-plane fixture must contain its composition")
    };
    let mut scale_layer = scale_composition.layers[0].clone();
    let scale_transform = named_mut(
        list_mut(&mut scale_layer.content, *b"tdgp"),
        "ADBE Transform Group",
        *b"tdgp",
    );
    let scale_run = named_run(scale_transform, "ADBE Scale");

    let mut project = crate::structure::read_project(include_bytes!(
        "../../../tests/fixtures/effects/shape_owner_gaussian.aep"
    ))
    .unwrap();
    let ItemKind::Composition(composition) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        panic!()
    };
    composition.layers.truncate(1);
    composition.width = 3840;
    composition.height = 2160;
    composition.duration_secs = 7.;
    let layer = &mut composition.layers[0];
    assert_eq!(layer.record.layer_type(), 4);
    assert_eq!(layer.record.source_id(), 0);
    assert!(layer.record.flags().collapse_transformation);
    layer.name = "MASK composition-sized Shape".into();
    *effect_parade(layer) = wipe_effects;
    scalar(layer, "ADBE Linear Wipe-0002", 90.);
    let transform = named_mut(
        list_mut(&mut layer.content, *b"tdgp"),
        "ADBE Transform Group",
        *b"tdgp",
    );
    transform.splice(transform.len() - 1..transform.len() - 1, scale_run);
    let scale = named_mut(transform, "ADBE Scale", *b"tdbs");
    replace(
        scale,
        *b"cdat",
        [2.0_f64, 2.0, 1.0]
            .into_iter()
            .flat_map(f64::to_be_bytes)
            .collect(),
    );
    for (name, values) in [
        ("ADBE Anchor Point", vec![0.0, 0.0, 0.0]),
        ("ADBE Position_0", vec![1920.0]),
        ("ADBE Position_1", vec![1080.0]),
    ] {
        replace(
            named_mut(transform, name, *b"tdbs"),
            *b"cdat",
            values.into_iter().flat_map(f64::to_be_bytes).collect(),
        );
    }
    project
}

fn set_static_transform_value(layer: &mut Layer, name: &str, values: &[f64]) {
    let transform = named_mut(
        list_mut(&mut layer.content, *b"tdgp"),
        "ADBE Transform Group",
        *b"tdgp",
    );
    replace(
        named_mut(transform, name, *b"tdbs"),
        *b"cdat",
        values.iter().copied().flat_map(f64::to_be_bytes).collect(),
    );
}

fn parented_shape_source_project() -> StructuralProject {
    let mut project = shape_source_project();
    let ItemKind::Composition(composition) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        panic!()
    };
    let child = &mut composition.layers[0];
    let mut parent = child.clone();
    let mut child_record = child.record.encode();
    child_record[132..136].copy_from_slice(&999_u32.to_be_bytes());
    child.record = LayerRecord::decode(&child_record).unwrap();

    let mut parent_record = parent.record.encode();
    parent_record[0..4].copy_from_slice(&999_u32.to_be_bytes());
    parent_record[132..136].copy_from_slice(&0_u32.to_be_bytes());
    parent_record[39] &= !4;
    parent.record = LayerRecord::decode(&parent_record).unwrap();
    parent.name = "Static planar Wipe parent".into();
    effect_parade(&mut parent).clear();
    for (name, values) in [
        ("ADBE Anchor Point", &[120.0, 80.0, 0.0][..]),
        ("ADBE Scale", &[0.75, 1.25, 1.0][..]),
    ] {
        set_static_transform_value(&mut parent, name, values);
    }
    composition.layers.push(parent);
    project
}

#[test]
fn composition_sized_shape_wipe_preserves_vector_paint_and_native_clock() {
    let project = shape_source_project();
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    let root = g(&converted.document.composition().layers()[0]);
    let owner = g(&root.layers[0]);
    let mask_stage =
        masked(owner).unwrap_or_else(|| panic!("Shape Wipe omitted: {:?}", converted.diagnostics));
    assert_eq!(mask_stage.masks.len(), 1);
    assert!(contains_rect(mask_stage));
    assert!(owner.description.contains("kind=4 source=0"));
    assert_eq!(owner.transform.anchor_point, [0.0, 0.0]);
    assert_eq!(owner.transform.position, Position::xy(1920.0, 1080.0));
    assert_eq!(owner.transform.scale, [200.0, 200.0]);

    let guide = mask_stage
        .layers
        .iter()
        .find(|layer| matches!(layer.data(), FxLayer::Shape(_)))
        .unwrap();
    let entry = converted
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .find(|entry| entry.target == PropertyTarget::layer(guide.id(), PropType::AnchorPointX))
        .unwrap();
    let keys = entry.animator.keyframe_track().unwrap().keyframes();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].value(), &fx_schema::PropertyValue::Float(-3840.));
    assert_eq!(keys[1].value(), &fx_schema::PropertyValue::Float(0.));
    let FxLayer::Shape(guide) = guide.data() else {
        panic!("Wipe guide must remain editable Shape paint")
    };
    let boundaries: Vec<_> = keys
        .iter()
        .map(|key| {
            let fx_schema::PropertyValue::Float(anchor_x) = key.value() else {
                panic!("Completion target must remain scalar")
            };
            let mut transform = guide.transform;
            transform.anchor_point[0] = *anchor_x;
            translated(owner.transform, translated(transform, [0.0, 0.0]))[0]
        })
        .collect();
    assert_eq!(boundaries, [3840.0, 0.0]);
    let [paint_min_x, paint_min_y, paint_max_x, paint_max_y] = rect_bounds(owner);
    assert!(paint_min_x >= boundaries[1] && paint_max_x <= boundaries[0]);
    assert!(paint_min_y >= 0.0 && paint_max_y <= 2160.0);
    let clock = NumericAnimationClock::parent_identity(source(&project, 0)).unwrap();
    assert_eq!(
        keys[0].layer_time().as_millis(),
        (clock.seconds(0.) * 1000.).round() as i64
    );
    assert_eq!(
        keys[1].layer_time().as_millis(),
        (clock.seconds(1.) * 1000.).round() as i64
    );
}

#[test]
fn composition_sized_shape_wipe_matches_the_emitted_parent_wrapper() {
    let project = parented_shape_source_project();
    let converted = to_structural_fx_document(&project, Some(1)).unwrap();
    let root = g(&converted.document.composition().layers()[0]);
    let wrapper = root
        .layers
        .iter()
        .map(g)
        .find(|group| group.description.contains("transform-only parent copy 999"))
        .expect("child must use the emitted static parent wrapper");
    assert_eq!(wrapper.transform.anchor_point, [120.0, 80.0]);
    assert_eq!(wrapper.transform.position, Position::xy(1920.0, 1080.0));
    assert_eq!(wrapper.transform.scale, [75.0, 125.0]);
    assert_eq!(wrapper.transform.rotation, 0.0);

    let owner = g(&wrapper.layers[0]);
    let mask_stage = masked(owner)
        .unwrap_or_else(|| panic!("parented Shape Wipe omitted: {:?}", converted.diagnostics));
    let guide = mask_stage
        .layers
        .iter()
        .find(|layer| matches!(layer.data(), FxLayer::Shape(_)))
        .expect("editable Wipe guide");
    let FxLayer::Shape(guide) = guide.data() else {
        unreachable!()
    };
    let entry = converted
        .document
        .composition()
        .dynamics()
        .entries()
        .iter()
        .find(|entry| entry.target == PropertyTarget::layer(guide.id, PropType::AnchorPointX))
        .expect("Completion track");
    let keys = entry.animator.keyframe_track().unwrap().keyframes();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].value(), &fx_schema::PropertyValue::Float(-3840.0));
    assert_eq!(keys[1].value(), &fx_schema::PropertyValue::Float(0.0));

    for (key, expected_x) in keys.iter().zip([3840.0, 0.0]) {
        let fx_schema::PropertyValue::Float(anchor_x) = key.value() else {
            panic!("Completion target must remain scalar")
        };
        let mut guide_transform = guide.transform;
        guide_transform.anchor_point[0] = *anchor_x;
        let world = TestAffine::transform(wrapper.transform)
            .then(TestAffine::transform(owner.transform))
            .then(TestAffine::transform(mask_stage.transform))
            .then(TestAffine::transform(guide_transform));
        for (point, expected_y) in [([0.0, 0.0], 1080.0), ([0.0, 100.0], 1180.0)] {
            let actual = world.point(point);
            assert!((actual[0] - expected_x).abs() < 1e-9, "{actual:?}");
            assert!((actual[1] - expected_y).abs() < 1e-9, "{actual:?}");
        }
    }
}

#[test]
fn static_planar_parent_transform_keeps_the_wipe_on_the_composition_plane() {
    let project = shape_source_project();
    let mut layer = source(&project, 0).clone();
    let mut bytes = layer.record.encode();
    bytes[132..136].copy_from_slice(&999_u32.to_be_bytes());
    layer.record = LayerRecord::decode(&bytes).unwrap();
    let mut parent = source(&project, 0).clone();
    let mut bytes = parent.record.encode();
    bytes[0..4].copy_from_slice(&999_u32.to_be_bytes());
    bytes[132..136].copy_from_slice(&0_u32.to_be_bytes());
    bytes[39] &= !4;
    parent.record = LayerRecord::decode(&bytes).unwrap();
    let parent_transform = Transform {
        anchor_point: [120.0, 80.0],
        position: Position::xy(500.0, 300.0),
        scale: [75.0, 125.0],
        rotation: 18.0,
        skew: 12.0,
        skew_axis: -23.0,
        rotation_x: 0.0,
        rotation_y: 0.0,
        orientation: [0.0; 3],
        opacity: fx_schema::PercentageProperty::new(100.0).unwrap(),
    };
    let ancestors = [AncestorTransform {
        layer: &parent,
        transform: parent_transform,
    }];
    let mut owner = baseline(&project, 0);
    let mut next = 90_000;
    let mut animations = AnimationBudget::default();
    let mut shapes = OutputBudget::default();
    let lowered = apply(
        &layer,
        &mut owner,
        Context {
            source: None,
            size: [3840, 2160],
            depth: 0,
            planar: true,
            composition_id: 1,
            composition_offset: None,
            ancestors: &ancestors,
            evaluations: &ExpressionSamples::default(),
        },
        State {
            next: &mut next,
            animations: &mut animations,
            shapes: &mut shapes,
        },
    )
    .expect("static planar parent chain is representable")
    .expect("Shape Wipe is present");
    let mask_stage = masked(&owner).expect("parented Shape retains its Wipe mask");
    let guide = mask_stage
        .layers
        .iter()
        .find(|layer| matches!(layer.data(), FxLayer::Shape(_)))
        .expect("editable Wipe guide");
    let FxLayer::Shape(guide) = guide.data() else {
        unreachable!()
    };
    let keys = lowered.entries[0]
        .animator
        .keyframe_track()
        .expect("Completion track")
        .keyframes();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].value(), &fx_schema::PropertyValue::Float(-3840.0));
    assert_eq!(keys[1].value(), &fx_schema::PropertyValue::Float(0.0));
    for (key, expected) in keys.iter().zip([
        [[3840.0, 1080.0], [3840.0, 1180.0]],
        [[0.0, 1080.0], [0.0, 1180.0]],
    ]) {
        let fx_schema::PropertyValue::Float(anchor_x) = key.value() else {
            panic!("Completion target must remain scalar")
        };
        let mut transform = guide.transform;
        transform.anchor_point[0] = *anchor_x;
        let world = TestAffine::transform(parent_transform)
            .then(TestAffine::transform(owner.transform))
            .then(TestAffine::transform(transform));
        for (point, expected) in [[0.0, 0.0], [0.0, 100.0]].into_iter().zip(expected) {
            let actual = world.point(point);
            assert!(
                actual
                    .iter()
                    .zip(expected)
                    .all(|(actual, expected)| (actual - expected).abs() < 1e-9),
                "{actual:?} != {expected:?}"
            );
        }
    }

    let mut declined = baseline(&project, 0);
    let before = declined.clone();
    let mut declined_next = 90_000;
    let mut declined_animations = AnimationBudget::default();
    let mut declined_shapes = OutputBudget::default();
    let animation_checkpoint = declined_animations.used();
    let shape_checkpoint = declined_shapes.checkpoint();
    let error = match apply(
        &layer,
        &mut declined,
        Context {
            source: None,
            size: [3840, 2160],
            depth: 0,
            planar: true,
            composition_id: 1,
            composition_offset: None,
            ancestors: &[],
            evaluations: &ExpressionSamples::default(),
        },
        State {
            next: &mut declined_next,
            animations: &mut declined_animations,
            shapes: &mut declined_shapes,
        },
    ) {
        Err(error) => error,
        Ok(_) => panic!("incomplete parent chain must remain declined"),
    };
    assert!(error.contains("complete parent chain"));
    assert_eq!(declined, before);
    assert_eq!(declined_next, 90_000);
    assert_eq!(declined_animations.used(), animation_checkpoint);
    assert_eq!(declined_shapes.checkpoint(), shape_checkpoint);
}

#[test]
fn composition_normalization_offset_translates_the_shape_wipe_plane_before_inversion() {
    let project = shape_source_project();
    let offset = [300.0, -100.0];
    let mut owner = baseline(&project, 0);
    let Position::TwoD(position) = &mut owner.transform.position else {
        panic!("fixture owner must be planar")
    };
    position[0] += offset[0];
    position[1] += offset[1];
    let mut next = 90_000;
    let mut animations = AnimationBudget::default();
    let mut shapes = OutputBudget::default();
    let lowered = apply(
        source(&project, 0),
        &mut owner,
        Context {
            source: None,
            size: [3840, 2160],
            depth: 0,
            planar: true,
            composition_id: 1,
            composition_offset: Some(offset),
            ancestors: &[],
            evaluations: &ExpressionSamples::default(),
        },
        State {
            next: &mut next,
            animations: &mut animations,
            shapes: &mut shapes,
        },
    )
    .expect("normalized Shape plane is representable")
    .expect("Shape Wipe is present");
    let mask_stage = masked(&owner).expect("normalized Shape retains its Wipe mask");
    let guide = mask_stage
        .layers
        .iter()
        .find(|layer| matches!(layer.data(), FxLayer::Shape(_)))
        .expect("editable Wipe guide");
    let FxLayer::Shape(guide) = guide.data() else {
        unreachable!()
    };
    let keys = lowered.entries[0]
        .animator
        .keyframe_track()
        .expect("Completion track")
        .keyframes();
    let boundaries: Vec<_> = keys
        .iter()
        .map(|key| {
            let fx_schema::PropertyValue::Float(anchor_x) = key.value() else {
                panic!("Completion target must remain scalar")
            };
            let mut transform = guide.transform;
            transform.anchor_point[0] = *anchor_x;
            TestAffine::transform(owner.transform)
                .then(TestAffine::transform(transform))
                .point([0.0, 0.0])
        })
        .collect();
    assert_eq!(boundaries, [[4140.0, 980.0], [300.0, 980.0]]);
}

#[derive(Clone, Copy)]
struct TestAffine {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl TestAffine {
    fn transform(transform: Transform) -> Self {
        let Position::TwoD(position) = transform.position else {
            panic!()
        };
        let multiply = |left: [f64; 4], right: [f64; 4]| {
            [
                left[0] * right[0] + left[2] * right[1],
                left[1] * right[0] + left[3] * right[1],
                left[0] * right[2] + left[2] * right[3],
                left[1] * right[2] + left[3] * right[3],
            ]
        };
        let rotation = |degrees: f64| {
            let (sin, cos) = degrees.to_radians().sin_cos();
            [cos, sin, -sin, cos]
        };
        let scale = transform.scale.map(|value| value / 100.0);
        let linear = multiply(
            rotation(transform.rotation - transform.skew_axis),
            multiply(
                [1.0, 0.0, -transform.skew.to_radians().tan(), 1.0],
                multiply(
                    rotation(transform.skew_axis),
                    [scale[0], 0.0, 0.0, scale[1]],
                ),
            ),
        );
        let [a, b, c, d] = linear;
        Self {
            a,
            b,
            c,
            d,
            e: position[0] - a * transform.anchor_point[0] - c * transform.anchor_point[1],
            f: position[1] - b * transform.anchor_point[0] - d * transform.anchor_point[1],
        }
    }

    fn then(self, child: Self) -> Self {
        Self {
            a: self.a * child.a + self.c * child.b,
            b: self.b * child.a + self.d * child.b,
            c: self.a * child.c + self.c * child.d,
            d: self.b * child.c + self.d * child.d,
            e: self.a * child.e + self.c * child.f + self.e,
            f: self.b * child.e + self.d * child.f + self.f,
        }
    }

    fn point(self, point: [f64; 2]) -> [f64; 2] {
        [
            self.a * point[0] + self.c * point[1] + self.e,
            self.b * point[0] + self.d * point[1] + self.f,
        ]
    }
}

fn translated(transform: Transform, point: [f64; 2]) -> [f64; 2] {
    TestAffine::transform(transform).point(point)
}

fn rect_bounds(group: &GroupLayer) -> [f64; 4] {
    fn visit(layer: &fx_schema::Layer, parent: TestAffine, points: &mut Vec<[f64; 2]>) {
        match layer.data() {
            FxLayer::Group(group) => {
                let matrix = parent.then(TestAffine::transform(group.transform));
                for layer in &group.layers {
                    visit(layer, matrix, points);
                }
            }
            FxLayer::Rect(rect) => {
                let matrix = parent.then(TestAffine::transform(rect.transform));
                let [x, y] = rect.rect.position;
                let [width, height] = rect.rect.size;
                points.extend(
                    [
                        [x, y],
                        [x + width, y],
                        [x, y + height],
                        [x + width, y + height],
                    ]
                    .map(|point| matrix.point(point)),
                );
            }
            _ => {}
        }
    }
    let mut points = Vec::new();
    for layer in &group.layers {
        visit(layer, TestAffine::transform(group.transform), &mut points);
    }
    assert!(
        !points.is_empty(),
        "fixture must retain editable Rect paint"
    );
    points.iter().fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |[min_x, min_y, max_x, max_y], point| {
            [
                min_x.min(point[0]),
                min_y.min(point[1]),
                max_x.max(point[0]),
                max_y.max(point[1]),
            ]
        },
    )
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
    let mut evaluations = ExpressionSamples::default();
    evaluations.properties.push(EvaluatedProperty {
        composition_id: 1,
        layer_id: source(&project, 1).record.id(),
        property: PropertyIdentity::Effect {
            index: 2,
            match_name: "ADBE Linear Wipe-0001".into(),
        },
        start_ms: 0,
        sample_times_seconds: vec![0.],
        values: vec![vec![99.]],
        frame_sampled: true,
    });
    let profile = profile(source(&project, 1), 1, &evaluations)
        .unwrap()
        .unwrap();
    let Completion::Native(first) = &profile.wipes[0].completion else {
        panic!("fixture completion is native")
    };
    let Completion::Native(second) = &profile.wipes[1].completion else {
        panic!("fixture completion is native")
    };
    assert_eq!(first.values, [33.]);
    assert_eq!(second.values, [33.]);
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
            let evaluations = ExpressionSamples::default();
            let native = profile(source, 1, &evaluations)
                .unwrap()
                .unwrap()
                .anchor
                .unwrap();
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
            composition_id: 1,
            composition_offset: None,
            ancestors: &[],
            evaluations: &ExpressionSamples::default(),
        },
        State {
            next: &mut cursor,
            animations: &mut budget,
            shapes: &mut output,
        },
    )
    .unwrap()
    .unwrap()
    .entries;
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
                    planar: true,
                    composition_id: 1,
                    composition_offset: None,
                    ancestors: &[],
                    evaluations: &ExpressionSamples::default(),
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
                    planar: true,
                    composition_id: 1,
                    composition_offset: None,
                    ancestors: &[],
                    evaluations: &ExpressionSamples::default(),
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
    assert!(
        profile(&disabled, 1, &ExpressionSamples::default())
            .unwrap()
            .is_none()
    );
    let descriptor = plugin(&mut disabled, WIPE);
    replace(list_mut(descriptor, *b"tdgp"), *b"tdsb", vec![0; 4]);
    let mut bytes = disabled.record.encode();
    bytes[39] |= 4;
    disabled.record = LayerRecord::decode(&bytes).unwrap();
    assert!(
        profile(&disabled, 1, &ExpressionSamples::default())
            .unwrap()
            .is_none()
    );
}

#[test]
fn leading_geometry_retains_wipe_and_reports_geometry_omission() {
    let mut project = composition_source_project();
    let mut layer = source(&project, 1).clone();
    let parade = effect_parade(&mut layer);
    let starts: Vec<_> = parade
        .iter()
        .enumerate()
        .filter(|(_, chunk)| chunk.id() == *b"tdmn")
        .map(|(index, _)| index)
        .collect();
    let geometry = starts
        .iter()
        .position(|start| {
            parade[*start]
                .data_payload()
                .unwrap()
                .starts_with(GEOMETRY.as_bytes())
        })
        .unwrap();
    let start = starts[geometry];
    let end = starts.get(geometry + 1).copied().unwrap_or(parade.len());
    let moved: Vec<_> = parade.drain(start..end).collect();
    let first = parade
        .iter()
        .position(|chunk| chunk.id() == *b"tdmn")
        .unwrap();
    parade.splice(first..first, moved);

    let layer_id = layer.record.id();
    let ItemKind::Composition(composition) = &mut project
        .items
        .iter_mut()
        .find(|item| item.id == 1)
        .unwrap()
        .kind
    else {
        panic!()
    };
    composition.layers[1] = layer;

    let result = to_structural_fx_document(&project, Some(1)).unwrap();
    let root = g(&result.document.composition().layers()[0]);
    let owner = g(&root.layers[1]);
    assert_eq!(masked(owner).unwrap().masks.len(), 2);
    assert!(owner.description.contains("source=96"));
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(layer_id)
            && diagnostic
                .to_string()
                .contains("Geometry2: post-layer mapping requires a planar Shape")
    }));
    assert!(!result.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .to_string()
            .contains("preceding Geometry2 remains a separate editable stage")
    }));
}

#[test]
fn finite_completion_easing_and_more_than_sixty_four_keys_are_retained() {
    let key = |time_secs, value, in_speed, out_speed| crate::properties::NumericKeyframe {
        time_secs,
        values: vec![value],
        in_interpolation: 2,
        out_interpolation: 2,
        in_speed: vec![in_speed],
        in_influence: vec![100.],
        out_speed: vec![out_speed],
        out_influence: vec![100.],
        spatial_in: vec![],
        spatial_out: vec![],
    };
    let completion = NumericProperty {
        values: vec![],
        animated: true,
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes: vec![key(0., 100., 0., 0.), key(1., 0., 1., 0.)],
        value_kind: NumericValueKind::Continuous,
    };
    curve(&completion, 1, true).unwrap();
    let minimum = minimum_completion(&Completion::Native(completion.clone())).unwrap();
    assert!((minimum + 1.0).abs() < 1e-9);
    assert!((path_extent([3840, 2160], 90.0, minimum) - 3878.4).abs() < 1e-9);

    let many_keys = NumericProperty {
        keyframes: (0..65)
            .map(|index| key(f64::from(index), f64::from(index), 0., 0.))
            .collect(),
        ..completion
    };
    curve(&many_keys, 1, true).unwrap();
}

#[test]
fn curved_or_diagonal_post_anchor_is_declined() {
    let project = native_project();
    let evaluations = ExpressionSamples::default();
    let p = profile(source(&project, 1), 1, &evaluations)
        .unwrap()
        .unwrap();
    let mut anchor = p.anchor.unwrap();
    anchor.keyframes[1].values[1] += 0.2;
    assert!(curve(&anchor, 2, false).is_err());
    anchor.keyframes[1].values[1] -= 0.2;
    anchor.keyframes[1].spatial_in[0] = 0.1;
    assert!(curve(&anchor, 2, false).is_err());
}
