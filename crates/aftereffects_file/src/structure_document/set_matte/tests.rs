use super::*;

fn data(tag: &[u8; 4], bytes: impl Into<Vec<u8>>) -> Chunk {
    Chunk::data(*tag, bytes.into()).unwrap()
}

#[test]
fn layer_reference_separates_source_id_from_sampling_stage() {
    for (id, stage, expected) in [
        (2088_u32, 0_i32, MatteSampleStage::Source),
        (743, -1, MatteSampleStage::AllEffects),
    ] {
        let chunks = vec![
            data(b"cdat", vec![0; 40]),
            data(b"tdpi", id.to_be_bytes()),
            data(b"tdps", stage.to_be_bytes()),
        ];
        assert_eq!(
            layer_reference(&chunks).unwrap(),
            LayerReference {
                layer_id: id,
                stage: expected,
            }
        );
    }
}

#[test]
fn malformed_ambiguous_and_unsupported_references_are_rejected() {
    let valid = [
        data(b"tdpi", 743_u32.to_be_bytes()),
        data(b"tdps", (-1_i32).to_be_bytes()),
    ];
    for invalid in [
        vec![],
        vec![valid[0].clone()],
        vec![valid[0].clone(), valid[0].clone(), valid[1].clone()],
        vec![data(b"tdpi", [0; 3]), valid[1].clone()],
        vec![data(b"tdpi", [0; 4]), valid[1].clone()],
        vec![valid[0].clone(), data(b"tdps", (-2_i32).to_be_bytes())],
        vec![valid[0].clone(), data(b"tdps", 1_i32.to_be_bytes())],
    ] {
        assert!(layer_reference(&invalid).is_err());
    }
}

fn named(name: &str) -> Chunk {
    let mut bytes = name.as_bytes().to_vec();
    bytes.resize(40, 0);
    data(b"tdmn", bytes)
}

fn numeric_chunks(value: f64) -> Vec<Chunk> {
    let mut meta = vec![0; 124];
    meta[..2].copy_from_slice(&[0xdb, 0x99]);
    meta[3] = 1;
    vec![
        data(b"tdb4", meta),
        data(b"tdsb", [0, 0, 0, 1]),
        data(b"cdat", value.to_be_bytes()),
    ]
}

fn numeric_property(value: f64) -> Chunk {
    Chunk::list(*b"tdbs", numeric_chunks(value))
}

fn layer_reference_property(id: u32, stage: i32) -> Chunk {
    let mut children = numeric_chunks(0.0);
    children.extend([
        data(b"tdpi", id.to_be_bytes()),
        data(b"tdps", stage.to_be_bytes()),
    ]);
    Chunk::list(*b"tdbs", children)
}

fn synthetic_layer(
    enabled: bool,
    id: u32,
    stage: i32,
    channel: Option<f64>,
    extra: Vec<Chunk>,
    count: usize,
) -> Layer {
    let project = crate::structure::read_project(include_bytes!(
        "../../../tests/fixtures/compositing/trackMatteType.aep"
    ))
    .unwrap();
    let crate::structure::ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("comp")
    };
    let mut layer = comp.layers[0].clone();
    assert!(layer.record.flags().effects_active);
    let mut body = vec![
        data(b"tdsb", [0, 0, 0, u8::from(enabled)]),
        named("ADBE Set Matte3-0001"),
        layer_reference_property(id, stage),
    ];
    if let Some(channel) = channel {
        body.extend([named("ADBE Set Matte3-0002"), numeric_property(channel)]);
    }
    body.extend(extra);
    let mut effects = Vec::new();
    for _ in 0..count {
        effects.extend([
            named(MATCH_NAME),
            Chunk::list(
                *b"sspc",
                vec![
                    Chunk::list(*b"parT", vec![]),
                    Chunk::list(*b"tdgp", body.clone()),
                ],
            ),
        ]);
    }
    layer.content = vec![Chunk::list(
        *b"tdgp",
        vec![named("ADBE Effect Parade"), Chunk::list(*b"tdgp", effects)],
    )];
    layer
}

#[test]
fn review_audit_duplicate_set_matte_parades_are_rejected() {
    let mut layer = synthetic_layer(true, 743, 0, None, vec![], 1);
    let root = layer.content[0].children_mut().unwrap();
    root.extend(root.clone());
    let error = sources(&layer).unwrap_err();
    assert!(error.contains("duplicate"), "{error}");
}

#[test]
fn sparse_alpha_and_explicit_luma_stages_are_distinguished() {
    let all_effects_alpha = MatteSource {
        projection: None,
        layer_id: 743,
        stage: MatteSampleStage::AllEffects,
        mode: TrackMatteType::Alpha,
    };
    assert_eq!(
        sources(&synthetic_layer(true, 743, -1, None, vec![], 2)).unwrap(),
        vec![all_effects_alpha, all_effects_alpha]
    );
    assert_eq!(
        sources(&synthetic_layer(true, 2088, 0, Some(5.0), vec![], 1)).unwrap(),
        vec![MatteSource {
            projection: None,
            layer_id: 2088,
            stage: MatteSampleStage::Source,
            mode: TrackMatteType::Luma,
        }]
    );
    assert_eq!(
        sources(&synthetic_layer(true, 743, -1, Some(4.0), vec![], 1)).unwrap(),
        vec![all_effects_alpha]
    );
    assert!(
        sources(&synthetic_layer(false, 743, -1, None, vec![], 2))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn unknown_controls_and_channels_are_rejected_but_long_stacks_are_retained() {
    assert!(sources(&synthetic_layer(true, 2088, 0, Some(9.0), vec![], 1)).is_err());
    assert!(
        sources(&synthetic_layer(
            true,
            2088,
            0,
            Some(2.0),
            vec![named("ADBE Set Matte3-0002"), numeric_property(2.0)],
            1,
        ))
        .is_err()
    );
    assert!(
        sources(&synthetic_layer(
            true,
            2088,
            0,
            None,
            vec![named("ADBE Set Matte3-0003"), numeric_property(0.0)],
            1,
        ))
        .is_err()
    );
    let stack = sources(&synthetic_layer(true, 743, -1, None, vec![], 5)).unwrap();
    assert_eq!(stack.len(), 5);
    assert!(
        stack
            .iter()
            .all(|source| source.mode == TrackMatteType::Alpha)
    );
}

#[test]
fn only_shapes_and_composition_occurrences_are_independently_duplicable() {
    assert!(independently_duplicable_kind(4, false));
    assert!(independently_duplicable_kind(0, true));
    assert!(!independently_duplicable_kind(0, false));
    assert!(!independently_duplicable_kind(3, true));
}

fn canonical_defaults(layer: &mut Layer, changed: Option<(usize, u32)>) {
    let root = property_root_children_mut(layer);
    let effects = named_list_children_mut(root, "ADBE Effect Parade", *b"tdgp");
    let descriptor = named_list_children_mut(effects, MATCH_NAME, *b"sspc");
    let table = unique_list_children_mut(descriptor, *b"parT");
    for (index, (kind, default)) in [
        (0_u32, 0_u32),
        (0, 0),
        (7, 4),
        (4, 0),
        (4, 1),
        (4, 1),
        (4, 1),
        (9, 0),
    ]
    .into_iter()
    .enumerate()
    {
        let mut bytes = vec![0; 148];
        bytes[12..16].copy_from_slice(&kind.to_be_bytes());
        let default = changed
            .filter(|(slot, _)| *slot == index)
            .map_or(default, |(_, value)| value);
        bytes[56..60].copy_from_slice(&default.to_be_bytes());
        let name = if index == 7 {
            "ADBE Effect Built In Params".to_owned()
        } else {
            format!("ADBE Set Matte3-{index:04}")
        };
        table.extend([named(&name), data(b"pard", bytes)]);
    }
}

#[test]
fn canonical_alpha_defaults_preserve_source_and_reject_changed_profiles() {
    let mut layer = synthetic_layer(true, 42, 0, None, vec![], 1);
    canonical_defaults(&mut layer, None);
    assert_eq!(
        sources(&layer).unwrap(),
        vec![MatteSource {
            projection: None,
            layer_id: 42,
            stage: MatteSampleStage::Source,
            mode: TrackMatteType::Alpha,
        }]
    );
    for (slot, value) in [(2, 6), (3, 1), (4, 0), (5, 0), (6, 0)] {
        let mut changed = synthetic_layer(true, 42, 0, None, vec![], 1);
        canonical_defaults(&mut changed, Some((slot, value)));
        assert!(
            sources(&changed).is_err(),
            "unproved default {slot}={value}"
        );
    }
    let mut explicit_channel = synthetic_layer(true, 42, 0, Some(2.0), vec![], 1);
    canonical_defaults(&mut explicit_channel, None);
    assert!(sources(&explicit_channel).is_err());
}

fn groups<'a>(layers: &'a [fx_schema::Layer], out: &mut Vec<&'a GroupLayer>) {
    for layer in layers {
        if let FxLayer::Group(group) = layer.data() {
            out.push(group);
            groups(&group.layers, out);
        }
    }
}

fn patch_layer_identity(layer: &mut Layer, id: u32, source_id: u32) {
    let mut bytes = layer.record.encode();
    bytes[..4].copy_from_slice(&id.to_be_bytes());
    bytes[40..44].copy_from_slice(&source_id.to_be_bytes());
    layer.record = crate::schema::layer_records::LayerRecord::decode(&bytes).unwrap();
}

fn set_matte_project(
    mut project: crate::structure::StructuralProject,
    source_comp_id: u32,
    mut provider: Layer,
    stage: i32,
) -> crate::structure::StructuralProject {
    const ROOT_ID: u32 = 9_000;
    const PROVIDER_ID: u32 = 901;
    const TARGET_ID: u32 = 902;

    patch_layer_identity(&mut provider, PROVIDER_ID, source_comp_id);
    provider.name = "Visible Provider".into();
    let mut target = synthetic_layer(true, PROVIDER_ID, stage, None, vec![], 1);
    patch_layer_identity(&mut target, TARGET_ID, source_comp_id);
    let mut target_record = target.record.encode();
    target_record[107] = 0;
    target_record[160..164].fill(0);
    target.record = crate::schema::layer_records::LayerRecord::decode(&target_record).unwrap();
    target.name = "Target".into();

    let mut root = project.item(source_comp_id).unwrap().clone();
    root.id = ROOT_ID;
    let crate::structure::ItemKind::Composition(comp) = &mut root.kind else {
        panic!("source must be a composition")
    };
    comp.layers = vec![target, provider];
    project.items.push(root);
    project
}

fn audio_layer_ids(group: &GroupLayer, output: &mut Vec<fx_schema::LayerId>) {
    for layer in &group.layers {
        match layer.data() {
            FxLayer::Group(child) => audio_layer_ids(child, output),
            FxLayer::Audio(audio) => output.push(audio.id),
            FxLayer::Video(video) if video.volume.is_some() => output.push(video.id),
            _ => {}
        }
    }
}

#[test]
fn precomposition_set_matte_helper_mutes_audio_and_releases_volume_animation() {
    let project = crate::structure::read_project(include_bytes!(
        "../../../tests/fixtures/media/import_audio_media_controls.aep"
    ))
    .unwrap();
    let provider = {
        let fixture = crate::structure::read_project(include_bytes!(
            "../../../tests/fixtures/layer_styles/styles_static_adobe.aep"
        ))
        .unwrap();
        let crate::structure::ItemKind::Composition(comp) = &fixture.item(1).unwrap().kind else {
            panic!("provider fixture composition")
        };
        comp.layers[0].clone()
    };
    let project = set_matte_project(project, 63, provider, -1);
    let result =
        super::super::to_structural_fx_document_with_assets(&project, Some(9_000), &mut |_| true)
            .unwrap();
    let mut all = Vec::new();
    groups(result.document.composition().layers(), &mut all);
    let wrapper = all
        .iter()
        .copied()
        .find(|group| group.name == "Target (Set Matte)")
        .unwrap_or_else(|| panic!("Set Matte wrapper: {:?}", result.diagnostics));
    let matte = wrapper.track_matte.as_ref().expect("matte gate");
    let helper = wrapper
        .layers
        .iter()
        .find(|layer| layer.id() == matte.layer)
        .and_then(|layer| match layer.data() {
            FxLayer::Group(group) => Some(group),
            _ => None,
        })
        .expect("Set Matte helper");
    assert!(!super::super::has_enabled_audio(helper));
    let mut helper_audio_ids = Vec::new();
    audio_layer_ids(helper, &mut helper_audio_ids);
    assert!(!helper_audio_ids.is_empty());
    let entries = result.document.composition().dynamics().entries();
    assert!(helper_audio_ids.iter().all(|id| {
        !entries.iter().any(|entry| {
            entry.target == fx_schema::PropertyTarget::layer(*id, fx_schema::PropType::AudioVolume)
        })
    }));

    let visible = all
        .iter()
        .copied()
        .find(|group| {
            group.name == "Visible Provider" && group.description.contains("comp=9000 layer=901 ")
        })
        .expect("independently visible provider");
    assert!(super::super::has_enabled_audio(visible));
    let mut visible_audio_ids = Vec::new();
    audio_layer_ids(visible, &mut visible_audio_ids);
    assert!(visible_audio_ids.iter().any(|id| {
        entries.iter().any(|entry| {
            entry.target == fx_schema::PropertyTarget::layer(*id, fx_schema::PropType::AudioVolume)
        })
    }));
}

fn chunk_match_name(chunk: &Chunk) -> Option<&str> {
    (chunk.id() == *b"tdmn").then_some(())?;
    let bytes = chunk.data_payload()?;
    (bytes.len() == 40).then_some(())?;
    let end = bytes
        .iter()
        .rposition(|byte| *byte != 0)
        .map_or(0, |i| i + 1);
    std::str::from_utf8(&bytes[..end]).ok()
}

fn named_run_bounds(children: &[Chunk], expected: &str) -> (usize, usize) {
    let starts: Vec<_> = children
        .iter()
        .enumerate()
        .filter_map(|(index, chunk)| chunk_match_name(chunk).map(|name| (index, name)))
        .collect();
    let position = starts
        .iter()
        .position(|(_, name)| *name == expected)
        .unwrap_or_else(|| panic!("missing native property run {expected}"));
    let start = starts[position].0;
    let end = starts
        .get(position + 1)
        .map_or(children.len(), |(index, _)| *index);
    (start, end)
}

fn unique_list_children_mut(run: &mut [Chunk], kind: [u8; 4]) -> &mut Vec<Chunk> {
    let matches: Vec<_> = run
        .iter()
        .enumerate()
        .filter_map(|(index, chunk)| (chunk.list_kind() == Some(kind)).then_some(index))
        .collect();
    let [index] = matches.as_slice() else {
        panic!("expected one native {kind:?} list")
    };
    run[*index].children_mut().expect("parsed native list")
}

fn named_list_children_mut<'a>(
    children: &'a mut [Chunk],
    name: &str,
    kind: [u8; 4],
) -> &'a mut Vec<Chunk> {
    let (start, end) = named_run_bounds(children, name);
    unique_list_children_mut(&mut children[start..end], kind)
}

fn property_root_children_mut(layer: &mut Layer) -> &mut Vec<Chunk> {
    let roots: Vec<_> = layer
        .content
        .iter()
        .enumerate()
        .filter_map(|(index, chunk)| (chunk.list_kind() == Some(*b"tdgp")).then_some(index))
        .collect();
    let [index] = roots.as_slice() else {
        panic!("expected one layer property root")
    };
    layer.content[*index]
        .children_mut()
        .expect("parsed layer property root")
}

fn append_before_group_end(children: &mut Vec<Chunk>, chunks: Vec<Chunk>) {
    let index = children
        .iter()
        .rposition(|chunk| chunk_match_name(chunk) == Some("ADBE Group End"))
        .unwrap_or(children.len());
    children.splice(index..index, chunks);
}

fn replace_named_run(children: &mut Vec<Chunk>, name: &str, replacement: Vec<Chunk>) {
    let (start, end) = named_run_bounds(children, name);
    children.splice(start..end, replacement);
}

fn cloned_root_run(layer: &Layer, expected: &str) -> Vec<Chunk> {
    let mut chunks = vec![named(expected)];
    let (_, run) = properties::root_runs(&layer.content)
        .unwrap()
        .into_iter()
        .find(|(name, _)| *name == expected)
        .unwrap_or_else(|| panic!("missing native root run {expected}"));
    chunks.extend_from_slice(run);
    chunks
}

fn has_effect_type(group: &GroupLayer, expected: &str) -> bool {
    serde_json::to_value(&group.effects)
        .unwrap()
        .as_array()
        .is_some_and(|effects| {
            effects.iter().any(|effect| {
                effect
                    .get("effect")
                    .and_then(|effect| effect.get("type"))
                    .and_then(serde_json::Value::as_str)
                    == Some(expected)
            })
        })
}

fn redistributable_stage_project(stage: i32) -> crate::structure::StructuralProject {
    let project = crate::structure::read_project(include_bytes!(
        "../../../tests/fixtures/layer_styles/styles_static_adobe.aep"
    ))
    .unwrap();
    let mut provider = {
        let crate::structure::ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
            panic!("style composition")
        };
        comp.layers
            .iter()
            .find(|layer| layer.record.id() == 15)
            .unwrap()
            .clone()
    };
    let effects = {
        let fixture = crate::structure::read_project(include_bytes!(
            "../../../tests/fixtures/effects/shape_owner_gaussian.aep"
        ))
        .unwrap();
        let crate::structure::ItemKind::Composition(comp) = &fixture.item(1).unwrap().kind else {
            panic!("effect composition")
        };
        cloned_root_run(&comp.layers[0], "ADBE Effect Parade")
    };
    let mask = {
        let fixture = crate::structure::read_project(include_bytes!(
            "../../../tests/fixtures/masks/mask.aep"
        ))
        .unwrap();
        let layer = fixture
            .items
            .iter()
            .find_map(|item| match &item.kind {
                crate::structure::ItemKind::Composition(comp) => comp.layers.first(),
                _ => None,
            })
            .unwrap();
        cloned_root_run(layer, "ADBE Mask Parade")
    };
    let root = property_root_children_mut(&mut provider);
    append_before_group_end(root, effects);
    append_before_group_end(root, mask);
    set_matte_project(project, 1, provider, stage)
}

#[test]
fn source_stage_omits_only_direct_occurrence_pipeline() {
    let source =
        super::super::to_structural_fx_document(&redistributable_stage_project(0), Some(9_000))
            .unwrap();
    let all_effects =
        super::super::to_structural_fx_document(&redistributable_stage_project(-1), Some(9_000))
            .unwrap();

    let inspect = |result: &super::super::StructuralConversion| {
        let mut all = Vec::new();
        groups(result.document.composition().layers(), &mut all);
        let wrapper = all
            .iter()
            .copied()
            .find(|group| group.name == "Target (Set Matte)")
            .unwrap_or_else(|| panic!("Set Matte wrapper: {:?}", result.diagnostics));
        let matte = wrapper.track_matte.as_ref().unwrap();
        let helper = wrapper
            .layers
            .iter()
            .find(|layer| layer.id() == matte.layer)
            .and_then(|layer| match layer.data() {
                FxLayer::Group(group) => Some(group),
                _ => None,
            })
            .unwrap();
        let visible = all
            .iter()
            .copied()
            .find(|group| {
                group.name == "Visible Provider"
                    && group.description.contains("comp=9000 layer=901 ")
            })
            .unwrap();
        (helper.clone(), visible.clone())
    };
    let (source_helper, source_visible) = inspect(&source);
    let (all_helper, all_visible) = inspect(&all_effects);

    assert!(source_helper.effects.is_empty());
    assert!(source_helper.masks.is_empty());
    assert!(all_helper.effects.len() >= 2);
    assert!(!all_helper.masks.is_empty());
    for helper in [&source_helper, &all_helper] {
        let mut nested = Vec::new();
        groups(&helper.layers, &mut nested);
        assert!(nested.iter().any(|group| !group.effects.is_empty()));
    }
    for visible in [&source_visible, &all_visible] {
        assert!(visible.effects.len() >= 2);
        assert!(!visible.masks.is_empty());
    }
    assert_eq!(source_helper.transform, source_visible.transform);
    assert_eq!(source_helper.playback, source_visible.playback);
    assert_eq!(all_helper.transform, all_visible.transform);
    assert_eq!(all_helper.playback, all_visible.playback);
    for result in [&source, &all_effects] {
        let json = String::from_utf8(result.document.to_json_vec().unwrap()).unwrap();
        assert!(!json.contains("JsScript"));
    }
}

fn set_provider_opacity(project: &mut crate::structure::StructuralProject, animated: bool) {
    let transform = if animated {
        let fixture = crate::structure::read_project(include_bytes!(
            "../../../tests/fixtures/properties/property_1D_opacity.aep"
        ))
        .unwrap();
        let crate::structure::ItemKind::Composition(comp) = &fixture.item(1).unwrap().kind else {
            panic!("opacity composition")
        };
        cloned_root_run(&comp.layers[0], "ADBE Transform Group")
    } else {
        vec![
            named("ADBE Transform Group"),
            Chunk::list(*b"tdgp", vec![named("ADBE Opacity"), numeric_property(0.0)]),
        ]
    };
    let item = project
        .items
        .iter_mut()
        .find(|item| item.id == 9_000)
        .unwrap();
    let crate::structure::ItemKind::Composition(comp) = &mut item.kind else {
        panic!("test composition")
    };
    let provider = comp
        .layers
        .iter_mut()
        .find(|layer| layer.record.id() == 901)
        .unwrap();
    replace_named_run(
        property_root_children_mut(provider),
        "ADBE Transform Group",
        transform,
    );
}

fn stage_helper_and_visible(
    result: &super::super::StructuralConversion,
) -> (GroupLayer, GroupLayer) {
    let mut all = Vec::new();
    groups(result.document.composition().layers(), &mut all);
    let wrapper = all
        .iter()
        .copied()
        .find(|group| group.name == "Target (Set Matte)")
        .unwrap();
    let matte = wrapper.track_matte.as_ref().unwrap();
    let helper = wrapper
        .layers
        .iter()
        .find(|layer| layer.id() == matte.layer)
        .and_then(|layer| match layer.data() {
            FxLayer::Group(group) => Some(group.clone()),
            _ => None,
        })
        .unwrap();
    let visible = all
        .iter()
        .copied()
        .find(|group| {
            group.name == "Visible Provider" && group.description.contains("comp=9000 layer=901 ")
        })
        .unwrap()
        .clone();
    (helper, visible)
}

#[test]
fn source_stage_ignores_only_owner_opacity_static_and_animated() {
    use fx_schema::{PropType, PropertyTarget};

    for animated in [false, true] {
        let mut source_project = redistributable_stage_project(0);
        set_provider_opacity(&mut source_project, animated);
        let source = super::super::to_structural_fx_document(&source_project, Some(9_000)).unwrap();
        let (source_helper, source_visible) = stage_helper_and_visible(&source);
        assert_eq!(source_helper.transform.opacity.value(), 100.0);
        let entries = source.document.composition().dynamics().entries();
        assert!(!entries.iter().any(|entry| {
            entry.target == PropertyTarget::layer(source_helper.id, PropType::Opacity)
        }));
        assert_eq!(
            entries.iter().any(|entry| {
                entry.target == PropertyTarget::layer(source_visible.id, PropType::Opacity)
            }),
            animated
        );

        let mut all_effects_project = redistributable_stage_project(-1);
        set_provider_opacity(&mut all_effects_project, animated);
        let all_effects =
            super::super::to_structural_fx_document(&all_effects_project, Some(9_000)).unwrap();
        let (all_helper, all_visible) = stage_helper_and_visible(&all_effects);
        assert_eq!(all_helper.transform.opacity, all_visible.transform.opacity);
        let entries = all_effects.document.composition().dynamics().entries();
        assert_eq!(
            entries.iter().any(|entry| {
                entry.target == PropertyTarget::layer(all_helper.id, PropType::Opacity)
            }),
            animated
        );
    }
}

fn track_matte_provider_with_set_matte(
    incompatible_dependency: bool,
) -> crate::structure::StructuralProject {
    let mut project = redistributable_stage_project(-1);
    let item = project
        .items
        .iter_mut()
        .find(|item| item.id == 9_000)
        .unwrap();
    let crate::structure::ItemKind::Composition(comp) = &mut item.kind else {
        panic!("composition")
    };
    // Both Set Matte endpoints must be supported shapes/precompositions, not
    // the solid footage in trackMatteType.aep (which intentionally is rejected).
    let mut consumer = comp.layers[0].clone();
    patch_layer_identity(&mut consumer, 904, 1);
    consumer.content.clear();
    consumer.name = "Track matte consumer".into();
    let mut record = consumer.record.encode();
    record[107] = 1;
    record[160..164].copy_from_slice(&902_u32.to_be_bytes());
    consumer.record = crate::schema::layer_records::LayerRecord::decode(&record).unwrap();
    if incompatible_dependency {
        let nested = synthetic_layer(true, 902, -1, None, vec![], 1);
        replace_named_run(
            property_root_children_mut(&mut comp.layers[1]),
            "ADBE Effect Parade",
            cloned_root_run(&nested, "ADBE Effect Parade"),
        );
    }
    comp.layers.insert(0, consumer);
    project
}

#[test]
fn track_matte_provider_clone_keeps_its_bounded_set_matte_gate_and_links() {
    let result = super::super::to_structural_fx_document(
        &track_matte_provider_with_set_matte(false),
        Some(9_000),
    )
    .unwrap();
    let FxLayer::Group(root) = result.document.composition().layers()[0].data() else {
        panic!("composition root")
    };
    let layers = &root.layers;
    let FxLayer::Group(target) = layers[0].data() else {
        panic!("track matte target")
    };
    let helper_id = target.track_matte.as_ref().unwrap().layer;
    let helper = layers
        .iter()
        .find(|layer| layer.id() == helper_id)
        .and_then(|layer| match layer.data() {
            FxLayer::Group(group) => Some(group),
            _ => None,
        })
        .unwrap();
    let nested_matte = helper
        .track_matte
        .as_ref()
        .unwrap_or_else(|| panic!("provider Set Matte gate: {:?}", result.diagnostics));
    let nested_provider = helper
        .layers
        .iter()
        .find(|layer| layer.id() == nested_matte.layer)
        .and_then(|layer| match layer.data() {
            FxLayer::Group(group) => Some(group),
            _ => None,
        })
        .expect("direct-child Set Matte provider");
    assert_eq!(nested_provider.parent, Some(helper.id));
}

#[test]
fn copied_provider_with_native_track_matte_does_not_gain_a_set_matte_wrapper() {
    let mut project = track_matte_provider_with_set_matte(false);
    let item = project
        .items
        .iter_mut()
        .find(|item| item.id == 9_000)
        .unwrap();
    let crate::structure::ItemKind::Composition(comp) = &mut item.kind else {
        panic!("composition")
    };
    let mut record = comp.layers[1].record.encode();
    record[107] = 1;
    record[160..164].copy_from_slice(&901_u32.to_be_bytes());
    comp.layers[1].record = crate::schema::layer_records::LayerRecord::decode(&record).unwrap();
    let result = super::super::to_structural_fx_document(&project, Some(9_000)).unwrap();
    let FxLayer::Group(root) = result.document.composition().layers()[0].data() else {
        panic!("composition root")
    };
    let FxLayer::Group(consumer) = root.layers[0].data() else {
        panic!("consumer")
    };
    let helper_id = consumer.track_matte.as_ref().unwrap().layer;
    let helper = root
        .layers
        .iter()
        .find(|layer| layer.id() == helper_id)
        .unwrap();
    assert!(!helper.data().name().contains("(Set Matte)"));
    let FxLayer::Group(helper) = helper.data() else {
        panic!("provider")
    };
    assert!(
        root.layers
            .iter()
            .any(|layer| layer.id() == helper.track_matte.as_ref().unwrap().layer)
    );
    result.document.to_json_vec().unwrap();
}

fn combined_alpha_project(stage: i32, resolved: bool) -> crate::structure::StructuralProject {
    let mut project = redistributable_stage_project(stage);
    let item = project
        .items
        .iter_mut()
        .find(|item| item.id == 9_000)
        .unwrap();
    let crate::structure::ItemKind::Composition(comp) = &mut item.kind else {
        panic!("composition")
    };
    let mut matte = comp.layers[1].clone();
    patch_layer_identity(&mut matte, 903, 1);
    matte.name = "Independent native matte".into();
    matte.content.clear();
    for (index, layer) in comp.layers.iter_mut().enumerate() {
        let id = if index == 0 && !resolved {
            999_u32
        } else {
            903
        };
        let mut record = layer.record.encode();
        record[107] = 1;
        record[160..164].copy_from_slice(&id.to_be_bytes());
        layer.record = crate::schema::layer_records::LayerRecord::decode(&record).unwrap();
    }
    comp.layers.push(matte);
    project
}

#[test]
fn combined_alpha_set_matte_preserves_resolved_native_gate() {
    let project = combined_alpha_project(0, true);
    let result = super::super::to_structural_fx_document(&project, Some(9_000)).unwrap();
    let FxLayer::Group(root) = result.document.composition().layers()[0].data() else {
        panic!("root")
    };
    let FxLayer::Group(wrapper) = root.layers[0].data() else {
        panic!("wrapper")
    };
    assert!(
        wrapper.name.ends_with("(Set Matte)"),
        "{:?}",
        result.diagnostics
    );
    let gate = wrapper.track_matte.as_ref().unwrap();
    assert_eq!(gate.mode, TrackMatteType::Alpha);
    assert!(wrapper.layers.iter().any(|layer| layer.id() == gate.layer));
    let FxLayer::Group(original) = wrapper.layers[0].data() else {
        panic!("original")
    };
    assert_eq!(original.parent, Some(wrapper.id));
    assert!(original.description.contains("layer=902 "));
    let native = original.track_matte.as_ref().unwrap();
    assert_eq!(native.mode, TrackMatteType::Alpha);
    assert_ne!(native.layer, gate.layer);
    assert!(root.layers.iter().any(|layer| layer.id() == native.layer));
    let mut sampled = Vec::new();
    groups(&wrapper.layers[1..], &mut sampled);
    let provider = sampled
        .iter()
        .find(|layer| layer.description.contains("layer=901 "))
        .unwrap();
    assert!(
        provider.track_matte.is_none(),
        "Source excludes provider composition matte"
    );
    result.document.to_json_vec().unwrap();
}

#[test]
fn combined_alpha_set_matte_rejects_all_effects_and_unresolved_native_gate() {
    for (stage, resolved) in [(-1, true), (0, false)] {
        let result = super::super::to_structural_fx_document(
            &combined_alpha_project(stage, resolved),
            Some(9_000),
        )
        .unwrap();
        let FxLayer::Group(root) = result.document.composition().layers()[0].data() else {
            panic!("root")
        };
        let FxLayer::Group(target) = root.layers[0].data() else {
            panic!("target")
        };
        assert!(!target.name.contains("(Set Matte)"));
        assert_eq!(target.track_matte.is_some(), resolved);
        assert!(result.diagnostics.iter().any(|d| {
            d.message
                .contains("unsupported dependency retained without lowering")
        }));
    }
}

#[test]
fn track_matte_provider_clone_retains_content_when_set_matte_dependency_is_incompatible() {
    let result = super::super::to_structural_fx_document(
        &track_matte_provider_with_set_matte(true),
        Some(9_000),
    )
    .unwrap();
    let FxLayer::Group(root) = result.document.composition().layers()[0].data() else {
        panic!("composition root")
    };
    let layers = &root.layers;
    let FxLayer::Group(target) = layers[0].data() else {
        panic!("track matte target")
    };
    let helper_id = target.track_matte.as_ref().unwrap().layer;
    let helper = layers
        .iter()
        .find(|layer| layer.id() == helper_id)
        .and_then(|layer| match layer.data() {
            FxLayer::Group(group) => Some(group),
            _ => None,
        })
        .unwrap();
    assert!(helper.track_matte.is_none());
    assert!(!helper.layers.is_empty());
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("unsupported dependency retained without lowering")
    }));
}

const RED_NATIVE: &[u8] = include_bytes!("../../../tests/fixtures/effects/set_matte_red.aep");

#[test]
fn native_set_matte_red_import_keeps_public_editable_channel_projection() {
    use sha2::{Digest, Sha256};
    assert_eq!(
        format!("{:x}", Sha256::digest(RED_NATIVE)),
        "f14eef956f06ce4072d2b273f39db2e6ee57da54075e4bcb5a8f51031340dffc"
    );
    let project = crate::structure::read_project(RED_NATIVE).unwrap();
    let ItemKind::Composition(comp) = &project.item(20).unwrap().kind else {
        panic!("native target")
    };
    let source = comp.layers.iter().find(|l| l.record.id() == 47).unwrap();
    assert_eq!(
        sources(source).unwrap(),
        vec![MatteSource {
            layer_id: 32,
            stage: MatteSampleStage::Source,
            mode: TrackMatteType::Luma,
            projection: Some(ChannelSource::Red)
        }]
    );
    let result = super::super::to_structural_fx_document(&project, Some(20)).unwrap();
    let mut all = Vec::new();
    groups(result.document.composition().layers(), &mut all);
    let helper = all
        .iter()
        .find(|g| has_effect_type(g, "shiftChannels"))
        .unwrap_or_else(|| panic!("{:?}", result.diagnostics));
    let effects = serde_json::to_value(&helper.effects).unwrap();
    assert_eq!(effects[0]["effect"]["takeRedFrom"], "red");
    assert_eq!(effects[0]["effect"]["takeGreenFrom"], "fullOff");
    assert_eq!(effects[0]["effect"]["takeBlueFrom"], "fullOff");
    assert_eq!(effects[1]["effect"]["type"], "hueSaturation");
    assert_eq!(effects[1]["effect"]["saturation"], -100.0);
    assert_eq!(helper.layers.len(), 2);
    let FxLayer::Rect(backing) = helper.layers[1].data() else {
        panic!("opaque black backing")
    };
    assert_eq!(backing.rect.fill_color, [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(backing.rect.size, [320.0, 180.0]);
    assert_eq!(backing.parent, Some(helper.id));
    assert_eq!(backing.transform.opacity.value(), 100.0);
    let mut colors = Vec::new();
    for group in &all {
        for layer in &group.layers {
            if let FxLayer::Rect(rect) = layer.data() {
                colors.push(rect.rect.fill_color);
            }
        }
    }
    for expected in [
        [0.8, 0.2, 0.1, 1.0],
        [0.1, 0.7, 0.3, 1.0],
        [0.2, 0.4, 0.9, 1.0],
    ] {
        assert!(colors.iter().any(|color| {
            color
                .iter()
                .zip(expected)
                .all(|(actual, expected)| (actual - expected).abs() < 1e-6)
        }));
    }

    let wrapper = all
        .iter()
        .find(|g| g.track_matte.as_ref().is_some_and(|m| m.layer == helper.id))
        .unwrap();
    assert_eq!(
        wrapper.track_matte.as_ref().unwrap().mode,
        TrackMatteType::Luma
    );
    assert_eq!(helper.parent, wrapper.parent);
    assert_ne!(helper.id, wrapper.id);
    assert!(
        all.iter()
            .any(|g| g.name == "Cell 1" && g.transform.opacity.value() == 50.0)
    );
    assert!(
        all.iter()
            .any(|g| g.name == "Cell 2" && g.transform.opacity.value() == 0.0)
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("premultiplied Red"))
    );
}

#[test]
fn native_set_matte_red_bypass_retains_original_picture() {
    let mut project = crate::structure::read_project(RED_NATIVE).unwrap();
    let item = project.items.iter_mut().find(|i| i.id == 20).unwrap();
    let ItemKind::Composition(comp) = &mut item.kind else {
        panic!("composition")
    };
    let layer = comp
        .layers
        .iter_mut()
        .find(|l| l.record.id() == 47)
        .unwrap();
    // Explicit supplementary mutation, not an independently authored bypass case.
    let root = property_root_children_mut(layer);
    let parade = named_list_children_mut(root, "ADBE Effect Parade", *b"tdgp");
    let descriptor = named_list_children_mut(parade, MATCH_NAME, *b"sspc");
    let body = descriptor
        .iter_mut()
        .find(|c| c.list_kind() == Some(*b"tdgp"))
        .unwrap()
        .children_mut()
        .unwrap();
    if let Some(enabled) = body.iter_mut().find(|c| c.id() == *b"tdsb") {
        *enabled = data(b"tdsb", [0, 0, 0, 0]);
    } else {
        body.insert(0, data(b"tdsb", [0, 0, 0, 0]));
    }
    assert!(sources(layer).unwrap().is_empty());
    let result = super::super::to_structural_fx_document(&project, Some(20)).unwrap();
    let mut all = Vec::new();
    groups(result.document.composition().layers(), &mut all);
    assert!(!all.iter().any(|g| has_effect_type(g, "shiftChannels")));
    assert!(
        all.iter()
            .any(|g| g.name == "Target source" && !g.is_hidden)
    );
    let exported = crate::export_document::to_aep_with_fps(&result.document, 30.0).unwrap();
    let native = crate::structure::read_project(&exported.bytes).unwrap();
    let ItemKind::Composition(root) = &native.item(1).unwrap().kind else {
        panic!("output composition")
    };
    assert!(root.layers.iter().any(|layer| layer.record.flags().enabled));
}

#[test]
fn native_set_matte_red_current_green_edit_exports_native_controls() {
    let project = crate::structure::read_project(RED_NATIVE).unwrap();
    let result = super::super::to_structural_fx_document(&project, Some(20)).unwrap();
    let mut value: serde_json::Value =
        serde_json::from_slice(&result.document.to_json_vec().unwrap()).unwrap();
    fn edit(value: &mut serde_json::Value) -> usize {
        let mut count = 0;
        if value["type"] == "shiftChannels" {
            value["takeRedFrom"] = serde_json::json!("fullOff");
            value["takeGreenFrom"] = serde_json::json!("green");
            count += 1;
        }
        match value {
            serde_json::Value::Object(fields) => {
                for child in fields.values_mut() {
                    count += edit(child);
                }
            }
            serde_json::Value::Array(values) => {
                for child in values {
                    count += edit(child);
                }
            }
            _ => {}
        }
        count
    }
    assert_eq!(edit(&mut value), 1);
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let output = crate::export_document::to_aep_with_fps(&document, 30.0).unwrap();
    let native = crate::structure::read_project(&output.bytes).unwrap();
    let mut found = 0;
    let mut luma = false;
    for item in &native.items {
        let ItemKind::Composition(comp) = &item.kind else {
            continue;
        };
        for layer in &comp.layers {
            luma |= layer.record.track_matte_type() == 3;
            let (effects, _) = crate::effects::native::read_effects(&layer.content, [320.0, 180.0]);
            for effect in effects
                .iter()
                .filter(|e| e.match_name == "ADBE Shift Channels")
            {
                found += 1;
                for (name, value) in [
                    ("ADBE Shift Channels-0002", 10.0),
                    ("ADBE Shift Channels-0003", 3.0),
                    ("ADBE Shift Channels-0004", 10.0),
                ] {
                    let p = effect
                        .parameters
                        .iter()
                        .find(|p| p.match_name == name)
                        .unwrap();
                    assert_eq!(p.numeric.as_ref().unwrap().values, [value]);
                }
            }
        }
    }
    assert_eq!(found, 1, "{:?}", output.diagnostics);
    assert!(luma, "{:?}", output.diagnostics);
}

#[test]
fn native_set_matte_red_edited_graph_bypass_keeps_picture() {
    let project = crate::structure::read_project(RED_NATIVE).unwrap();
    let imported = super::super::to_structural_fx_document(&project, Some(20)).unwrap();
    let mut value = imported.document.to_json_value().unwrap();
    fn unbind(value: &mut serde_json::Value) -> Option<serde_json::Value> {
        if value["trackMatte"]["mode"] == "luma" {
            let id = value["trackMatte"]["layer"].clone();
            value.as_object_mut().unwrap().remove("trackMatte");
            return Some(id);
        }
        match value {
            serde_json::Value::Object(fields) => fields.values_mut().find_map(unbind),
            serde_json::Value::Array(values) => values.iter_mut().find_map(unbind),
            _ => None,
        }
    }
    let helper = unbind(&mut value).unwrap();
    fn hide_helper(value: &mut serde_json::Value, id: &serde_json::Value) -> usize {
        let mut count = 0;
        if value["type"] == "Group" && value["id"] == *id {
            value["isHidden"] = serde_json::json!(true);
            count += 1;
        }
        match value {
            serde_json::Value::Object(fields) => {
                for child in fields.values_mut() {
                    count += hide_helper(child, id);
                }
            }
            serde_json::Value::Array(values) => {
                for child in values {
                    count += hide_helper(child, id);
                }
            }
            _ => {}
        }
        count
    }
    assert_eq!(hide_helper(&mut value, &helper), 1);
    let edited = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let mut all = Vec::new();
    groups(edited.composition().layers(), &mut all);
    assert!(
        all.iter()
            .any(|g| g.name == "Target source" && !g.is_hidden && g.track_matte.is_none())
    );
    let output = crate::export_document::to_aep_with_fps(&edited, 30.0).unwrap();
    let native = crate::structure::read_project(&output.bytes).unwrap();
    let ItemKind::Composition(root) = &native.item(1).unwrap().kind else {
        panic!("output composition")
    };
    assert!(root.layers.iter().any(|layer| layer.record.flags().enabled));
    for item in &native.items {
        if let ItemKind::Composition(comp) = &item.kind {
            assert!(!comp.layers.iter().any(|l| l.record.track_matte_type() == 3));
        }
    }
}

#[test]
fn native_set_matte_alpha_then_red_keeps_both_gates() {
    // Supplementary mutation of the pinned native source, not native-authored
    // stacked-gate or edited-output fidelity evidence.
    let mut project = crate::structure::read_project(RED_NATIVE).unwrap();
    let ItemKind::Composition(comp) =
        &mut project.items.iter_mut().find(|i| i.id == 20).unwrap().kind
    else {
        panic!("composition")
    };
    let mut alpha_provider = comp
        .layers
        .iter()
        .find(|l| l.record.id() == 32)
        .unwrap()
        .clone();
    patch_layer_identity(&mut alpha_provider, 900, 1);
    alpha_provider.name = "Independent Alpha provider".into();
    comp.layers.push(alpha_provider);
    let consumer = comp
        .layers
        .iter_mut()
        .find(|l| l.record.id() == 47)
        .unwrap();
    let alpha = synthetic_layer(true, 900, 0, None, vec![], 1);
    let mut effects = named_list_children_mut(
        property_root_children_mut(&mut alpha.clone()),
        "ADBE Effect Parade",
        *b"tdgp",
    )
    .clone();
    let parade = named_list_children_mut(
        property_root_children_mut(consumer),
        "ADBE Effect Parade",
        *b"tdgp",
    );
    effects.append(parade);
    *parade = effects;
    assert_eq!(
        sources(consumer)
            .unwrap()
            .iter()
            .map(|s| s.layer_id)
            .collect::<Vec<_>>(),
        [900, 32]
    );
    let imported = super::super::to_structural_fx_document(&project, Some(20)).unwrap();
    let mut all = Vec::new();
    groups(imported.document.composition().layers(), &mut all);
    let gates: Vec<_> = all
        .iter()
        .filter_map(|g| g.track_matte.as_ref().map(|m| (*g, m)))
        .collect();
    assert_eq!(
        gates.len(),
        2,
        "both live gate bindings must survive: {:?}",
        imported.diagnostics
    );
    for mode in [TrackMatteType::Alpha, TrackMatteType::Luma] {
        let (consumer, matte) = gates.iter().find(|(_, m)| m.mode == mode).unwrap();
        let provider = all.iter().find(|g| g.id == matte.layer).unwrap();
        assert_ne!(consumer.id, provider.id);
        assert!(provider.name.contains("Set Matte sample"));
        assert_eq!(consumer.parent, provider.parent);
        assert_eq!(consumer.layers.len(), 1);
        let FxLayer::Group(previous) = consumer.layers[0].data() else {
            panic!("preserved subtree")
        };
        assert_eq!(previous.parent, Some(consumer.id));
        let identity = group(
            consumer.id,
            String::new(),
            consumer.parent,
            TimeRangeProperty::new(Time::ZERO, fx_schema::Duration::from_secs(2.0)),
        );
        assert_eq!(consumer.transform, identity.transform);
        assert_eq!(consumer.playback, identity.playback);
        assert!(consumer.effects.is_empty() && consumer.masks.is_empty());

        println!(
            "matte scope: {:?} consumer={} parent={:?} provider={} parent={:?}",
            mode, consumer.id, consumer.parent, provider.id, provider.parent
        );
    }
    let picture = all.iter().find(|g| g.name == "Target source").unwrap();
    assert!(!picture.is_hidden);
    let baseline = super::super::to_structural_fx_document(
        &crate::structure::read_project(RED_NATIVE).unwrap(),
        Some(20),
    )
    .unwrap();
    let mut baseline_groups = Vec::new();
    groups(
        baseline.document.composition().layers(),
        &mut baseline_groups,
    );
    let original = baseline_groups
        .iter()
        .find(|g| g.name == "Target source")
        .unwrap();
    assert_eq!(picture.transform, original.transform);
    assert_eq!(picture.playback, original.playback);
    assert_eq!(picture.effects, original.effects);
    assert_eq!(picture.masks, original.masks);
    assert_eq!(picture.blend_mode, original.blend_mode);
    // Every generated sample stays consumed rather than joining ordinary paint.
    for helper in all
        .iter()
        .filter(|g| g.name.ends_with("(Set Matte sample)"))
    {
        assert!(gates.iter().any(|(_, m)| m.layer == helper.id));
    }
    let output = crate::export_document::to_aep_with_fps(&imported.document, 30.0).unwrap();
    let native = crate::structure::read_project(&output.bytes).unwrap();
    let mut modes = Vec::new();
    for item in &native.items {
        let ItemKind::Composition(comp) = &item.kind else {
            continue;
        };
        for layer in &comp.layers {
            if let Some(compositing::MatteLayer::Id(id)) = compositing::matte_layer(&layer.record) {
                modes.push(layer.record.track_matte_type());
                println!(
                    "native matte scope: composition={} consumer={} name={} mode={} provider={}",
                    item.id,
                    layer.record.id(),
                    layer.name,
                    layer.record.track_matte_type(),
                    id
                );

                let provider = comp
                    .layers
                    .iter()
                    .find(|p| p.record.id() == id)
                    .expect("same-composition matte provider");
                assert!(
                    !provider.record.flags().enabled,
                    "matte helper must not paint independently"
                );
            }
        }
    }
    modes.sort();
    assert_eq!(modes, [1, 3], "{:?}", output.diagnostics);
    let ItemKind::Composition(root) = &native.item(1).unwrap().kind else {
        panic!("root")
    };
    assert!(root.layers.iter().any(|l| l.record.flags().enabled));
    assert!(
        native.items.iter().any(|item| {
            let ItemKind::Composition(comp) = &item.kind else {
                return false;
            };
            comp.layers
                .iter()
                .any(|layer| layer.name.as_ref() == "Target source" && layer.record.flags().enabled)
        }),
        "original target picture must survive: {:?}",
        output.diagnostics
    );
    fn has_target_paint(chunks: &[Chunk]) -> bool {
        if let Ok(runs) = properties::runs(chunks) {
            for (name, run) in runs {
                if name == "ADBE Vector Fill Color" {
                    let color =
                        properties::read_numeric(properties::unique_list(run, *b"tdbs").unwrap())
                            .unwrap();
                    if color.values.len() == 4
                        && color
                            .values
                            .iter()
                            .zip([0.3, 0.6, 0.8, 1.0])
                            .all(|(actual, expected)| (actual - expected).abs() < 1e-6)
                    {
                        return true;
                    }
                }
            }
        }
        chunks
            .iter()
            .filter_map(Chunk::children)
            .any(has_target_paint)
    }
    assert!(
        native.items.iter().any(|item| {
            let ItemKind::Composition(comp) = &item.kind else {
                return false;
            };
            comp.layers
                .iter()
                .any(|layer| layer.record.flags().enabled && has_target_paint(&layer.content))
        }),
        "original target native vector paint must survive fresh export"
    );
}

#[test]
fn premiere_linked_green_blue_sources_keep_mixed_gate_scopes() {
    let bytes =
        include_bytes!("../../../../premiere_file/tests/fixtures/premiere_channel_matte/aep.aep");
    for (comp_id, consumer_id, provider_id, channel) in [
        (34, 47, 46, ChannelSource::Green),
        (48, 61, 60, ChannelSource::Blue),
    ] {
        let mut project = crate::structure::read_project(bytes).unwrap();
        let ItemKind::Composition(comp) = &mut project
            .items
            .iter_mut()
            .find(|i| i.id == comp_id)
            .unwrap()
            .kind
        else {
            panic!("native comp")
        };
        let consumer = comp
            .layers
            .iter()
            .find(|l| l.record.id() == consumer_id)
            .unwrap();
        assert_eq!(sources(consumer).unwrap()[0].projection, Some(channel));
        for (id, control) in [(900, None), (901, Some(5.0))] {
            let mut provider = comp
                .layers
                .iter()
                .find(|l| l.record.id() == provider_id)
                .unwrap()
                .clone();
            let source_id = provider.record.source_id();
            patch_layer_identity(&mut provider, id, source_id);
            comp.layers.push(provider);
            let mut gate = synthetic_layer(true, id, 0, control, vec![], 1);
            let mut effects = named_list_children_mut(
                property_root_children_mut(&mut gate),
                "ADBE Effect Parade",
                *b"tdgp",
            )
            .clone();
            let consumer = comp
                .layers
                .iter_mut()
                .find(|l| l.record.id() == consumer_id)
                .unwrap();
            let parade = named_list_children_mut(
                property_root_children_mut(consumer),
                "ADBE Effect Parade",
                *b"tdgp",
            );
            effects.append(parade);
            *parade = effects;
        }
        let imported = super::super::to_structural_fx_document(&project, Some(comp_id)).unwrap();
        let mut all = Vec::new();
        groups(imported.document.composition().layers(), &mut all);
        let gates: Vec<_> = all
            .iter()
            .filter_map(|g| g.track_matte.as_ref().map(|m| (*g, m)))
            .collect();
        assert_eq!(gates.len(), 3, "{:?}", imported.diagnostics);
        for (consumer, matte) in gates {
            let provider = all.iter().find(|g| g.id == matte.layer).unwrap();
            assert_eq!(consumer.parent, provider.parent);
            assert_eq!(consumer.layers.len(), 1);
            assert!(consumer.effects.is_empty() && consumer.masks.is_empty());
            assert_ne!(consumer.id, provider.id);
        }
    }
}

#[test]
fn native_rgb_source_sample_ignores_only_provider_occurrence_blend() {
    use fx_schema::BlendMode;

    fn set_blend(layer: &mut Layer, blend: u8) {
        let mut bytes = layer.record.encode();
        bytes[99] = blend;
        layer.record = crate::schema::layer_records::LayerRecord::decode(&bytes).unwrap();
    }
    // Derived from the pinned Premiere-linked source, not another native save
    // or an executed pixel comparison. Multiply over the projection's opaque
    // black gives zero; Source sampling must instead retain the source channel.
    let bytes =
        include_bytes!("../../../../premiere_file/tests/fixtures/premiere_channel_matte/aep.aep");
    for (comp_id, provider_id) in [(34, 46), (48, 60)] {
        let mut baseline = crate::structure::read_project(bytes).unwrap();
        let ItemKind::Composition(inner) =
            &mut baseline.items.iter_mut().find(|i| i.id == 2).unwrap().kind
        else {
            panic!("source composition")
        };
        set_blend(
            inner
                .layers
                .iter_mut()
                .find(|l| l.record.id() == 20)
                .unwrap(),
            6,
        );
        let mut changed = baseline.clone();
        let ItemKind::Composition(comp) = &mut changed
            .items
            .iter_mut()
            .find(|i| i.id == comp_id)
            .unwrap()
            .kind
        else {
            panic!("occurrence composition")
        };
        set_blend(
            comp.layers
                .iter_mut()
                .find(|l| l.record.id() == provider_id)
                .unwrap(),
            5,
        );
        let mut samples = Vec::new();
        for (project, occurrence_blend) in [
            (&baseline, BlendMode::Normal),
            (&changed, BlendMode::Multiply),
        ] {
            let imported = super::super::to_structural_fx_document(project, Some(comp_id)).unwrap();
            let mut all = Vec::new();
            groups(imported.document.composition().layers(), &mut all);
            let projection = all
                .iter()
                .find(|g| has_effect_type(g, "shiftChannels"))
                .unwrap();
            let FxLayer::Group(sample) = projection.layers[0].data() else {
                panic!("sample over black")
            };
            let FxLayer::Rect(backing) = projection.layers[1].data() else {
                panic!("black backing")
            };
            assert_eq!(backing.rect.fill_color, [0.0, 0.0, 0.0, 1.0]);
            let identity = format!("comp={comp_id} layer={provider_id} ");
            let original = all
                .iter()
                .find(|g| g.id != sample.id && g.description.contains(&identity))
                .unwrap();
            assert_eq!(
                original.blend_mode, occurrence_blend,
                "original occurrence is not normalized"
            );
            let mut inside = Vec::new();
            groups(&sample.layers, &mut inside);
            let cell = inside
                .iter()
                .find(|g| g.description.contains("comp=2 layer=20 "))
                .unwrap();
            assert_eq!(
                cell.blend_mode,
                BlendMode::Screen,
                "source composition blend is not normalized"
            );
            assert_eq!(
                sample.blend_mode,
                BlendMode::Normal,
                "Source-stage channel sample must not multiply against opaque black"
            );
            samples.push(serde_json::to_value(sample).unwrap());
        }
        assert_eq!(
            samples[0], samples[1],
            "changing only the provider occurrence blend cannot change the sampled source graph"
        );
    }
    // Other sample-stage semantics are unchanged by the Source-only boundary.
    let mut project = redistributable_stage_project(-1);
    let ItemKind::Composition(comp) = &mut project
        .items
        .iter_mut()
        .find(|i| i.id == 9_000)
        .unwrap()
        .kind
    else {
        panic!("AllEffects composition")
    };
    set_blend(
        comp.layers
            .iter_mut()
            .find(|l| l.record.id() == 901)
            .unwrap(),
        5,
    );
    let imported = super::super::to_structural_fx_document(&project, Some(9_000)).unwrap();
    let (sample, original) = stage_helper_and_visible(&imported);
    assert_eq!(sample.blend_mode, BlendMode::Multiply);
    assert_eq!(original.blend_mode, BlendMode::Multiply);
}
