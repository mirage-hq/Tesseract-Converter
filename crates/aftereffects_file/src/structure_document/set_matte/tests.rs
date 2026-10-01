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
        layer_id: 743,
        stage: MatteSampleStage::AllEffects,
        mode: TrackMatteType::Alpha,
    };
    assert_eq!(
        sources(&synthetic_layer(true, 743, -1, None, vec![], 2)).unwrap(),
        vec![all_effects_alpha, all_effects_alpha]
    );
    assert_eq!(
        sources(&synthetic_layer(true, 2088, 0, Some(2.0), vec![], 1)).unwrap(),
        vec![MatteSource {
            layer_id: 2088,
            stage: MatteSampleStage::Source,
            mode: TrackMatteType::Luma,
        }]
    );
    assert_eq!(
        sources(&synthetic_layer(true, 743, -1, Some(1.0), vec![], 1)).unwrap(),
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
    assert!(sources(&synthetic_layer(true, 2088, 0, Some(3.0), vec![], 1)).is_err());
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
            layer_id: 42,
            stage: MatteSampleStage::Source,
            mode: TrackMatteType::Alpha,
        }]
    );
    for (slot, value) in [(2, 5), (3, 1), (4, 0), (5, 0), (6, 0)] {
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

#[test]
#[ignore = "requires licensed AEP_INTRO_IMPORT_SOURCE; no redistributable native fixture"]
fn pinned_intro_alpha_defaults_match_independent_adobe_readback() {
    use sha2::{Digest, Sha256};
    let bytes =
        std::fs::read(std::env::var_os("AEP_INTRO_IMPORT_SOURCE").expect("source path")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "75bb7d70238e23ffaafdeacf952217de1fcded8f86875bee39c2e91bda7804d9"
    );
    let project = crate::structure::read_project(&bytes).unwrap();
    let crate::structure::ItemKind::Composition(comp) = &project.item(3).unwrap().kind else {
        panic!("SH01 composition")
    };
    // Adobe 26.5 readback: Alpha=4, Invert=0, Stretch=1, Composite=1,
    // Premultiply=1. One instance stores parT defaults; the other is sparse.
    for id in [2360, 2217] {
        let layer = comp
            .layers
            .iter()
            .find(|layer| layer.record.id() == id)
            .unwrap();
        assert_eq!(
            sources(layer).unwrap(),
            vec![MatteSource {
                layer_id: 2218,
                stage: MatteSampleStage::Source,
                mode: TrackMatteType::Alpha,
            }]
        );
    }
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

#[test]
#[ignore = "requires local licensed AEP_SET_MATTE_SOURCE; source cannot be redistributed"]
fn local_external_source_restores_two_intersection_matte_stacks() {
    use sha2::{Digest, Sha256};
    let bytes =
        std::fs::read(std::env::var_os("AEP_SET_MATTE_SOURCE").expect("licensed source path"))
            .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "28bbce1b8c9f9625105d632504a97c598394a4753d6b0d923fb19942e302bb5d"
    );
    let project = crate::structure::read_project(&bytes).unwrap();
    let crate::structure::ItemKind::Composition(comp) = &project.item(724).unwrap().kind else {
        panic!("composition")
    };
    for (id, expected) in [(739, [743, 742]), (740, [741, 746])] {
        let source = comp
            .layers
            .iter()
            .find(|layer| layer.record.id() == id)
            .unwrap();
        assert_eq!(
            sources(source).unwrap(),
            expected
                .map(|layer_id| MatteSource {
                    layer_id,
                    stage: MatteSampleStage::AllEffects,
                    mode: TrackMatteType::Alpha,
                })
                .to_vec()
        );
    }
    let result = super::super::to_structural_fx_document(&project, Some(724)).unwrap();
    let mut all = Vec::new();
    groups(result.document.composition().layers(), &mut all);
    for name in ["Intersect-Left", "Intesect-Right"] {
        let wrappers: Vec<_> = all
            .iter()
            .filter(|group| group.name == format!("{name} (Set Matte)"))
            .collect();
        assert_eq!(wrappers.len(), 2, "{name}: {:?}", result.diagnostics);
        for wrapper in wrappers {
            let matte = wrapper.track_matte.as_ref().expect("alpha gate");
            assert_eq!(matte.mode, TrackMatteType::Alpha);
            assert_eq!(wrapper.layers.len(), 2);
            assert!(wrapper.playback.time_remap().is_none());
            assert_eq!(
                wrapper.transform.position,
                fx_schema::Position::TwoD([0.0, 0.0])
            );
            let helper = wrapper
                .layers
                .iter()
                .find(|layer| layer.id() == matte.layer)
                .expect("direct-child provider");
            let FxLayer::Group(helper) = helper.data() else {
                panic!("helper")
            };
            assert_eq!(helper.parent, Some(wrapper.id));
            assert!(helper.name.ends_with(" (Set Matte sample)"));
            assert!(!helper.is_hidden);
            assert_ne!(wrapper.layers[0].id(), helper.id);
        }
    }
    // Sampling duplicates, rather than consumes, the ordinary source paint.
    for id in [741, 742, 743, 746] {
        let name = &comp
            .layers
            .iter()
            .find(|layer| layer.record.id() == id)
            .unwrap()
            .name;
        let paint = all
            .iter()
            .find(|group| group.name.as_str() == name.as_ref())
            .expect("original paint copy");
        assert!(!all.iter().any(|group| {
            group
                .track_matte
                .as_ref()
                .is_some_and(|matte| matte.layer == paint.id)
        }));
    }
    result.document.to_json_vec().unwrap();
}

fn has_native_effect(layer: &Layer, expected: &str) -> bool {
    let roots = properties::root_runs(&layer.content).unwrap();
    let (_, parade) = roots
        .iter()
        .find(|(name, _)| *name == "ADBE Effect Parade")
        .expect("native Effect Parade");
    let instances = properties::unique_list(parade, *b"tdgp").unwrap();
    properties::runs(instances)
        .unwrap()
        .iter()
        .any(|(name, _)| *name == expected)
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

fn enable_drop_shadow_style(layer: &mut Layer) {
    let root = property_root_children_mut(layer);
    let styles = named_list_children_mut(root, "ADBE Layer Styles", *b"tdgp");
    let drop_shadow = named_list_children_mut(styles, "dropShadow/enabled", *b"tdgp");
    let flags = drop_shadow
        .iter_mut()
        .find(|chunk| chunk.id() == *b"tdsb")
        .expect("drop-shadow enable flags");
    *flags = data(b"tdsb", [0, 0, 0, 1]);
}

fn set_set_matte_stage(layer: &mut Layer, stage: i32) {
    let root = property_root_children_mut(layer);
    let effects = named_list_children_mut(root, "ADBE Effect Parade", *b"tdgp");
    let descriptor = named_list_children_mut(effects, MATCH_NAME, *b"sspc");
    let controls = unique_list_children_mut(descriptor, *b"tdgp");
    let property = named_list_children_mut(controls, "ADBE Set Matte3-0001", *b"tdbs");
    let encoded = property
        .iter_mut()
        .find(|chunk| chunk.id() == *b"tdps")
        .expect("Set Matte sampling stage");
    *encoded = data(b"tdps", stage.to_be_bytes());
}

fn add_occurrence_pipeline_probe(project: &mut crate::structure::StructuralProject) {
    let supported_effects = {
        let crate::structure::ItemKind::Composition(comp) = &project.item(705).unwrap().kind else {
            panic!("composition")
        };
        let layer = comp
            .layers
            .iter()
            .find(|layer| layer.record.id() == 720)
            .unwrap();
        cloned_root_run(layer, "ADBE Effect Parade")
    };
    let mask_run = {
        let crate::structure::ItemKind::Composition(comp) = &project.item(1197).unwrap().kind
        else {
            panic!("composition")
        };
        let layer = comp
            .layers
            .iter()
            .find(|layer| layer.record.id() == 1204)
            .unwrap();
        cloned_root_run(layer, "ADBE Mask Parade")
    };
    let item = project
        .items
        .iter_mut()
        .find(|item| item.id == 705)
        .unwrap();
    let crate::structure::ItemKind::Composition(comp) = &mut item.kind else {
        panic!("composition")
    };
    let provider = comp
        .layers
        .iter_mut()
        .find(|layer| layer.record.id() == 2088)
        .unwrap();
    let root = property_root_children_mut(provider);
    replace_named_run(root, "ADBE Effect Parade", supported_effects);
    append_before_group_end(root, mask_run);
    enable_drop_shadow_style(provider);
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
#[ignore = "requires the unchanged licensed local Intro source via AEP_INTRO_IMPORT_SOURCE"]
fn pinned_intro_noise_consumers_keep_both_alpha_gates() {
    use sha2::{Digest, Sha256};
    let bytes =
        std::fs::read(std::env::var_os("AEP_INTRO_IMPORT_SOURCE").expect("source path")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "75bb7d70238e23ffaafdeacf952217de1fcded8f86875bee39c2e91bda7804d9"
    );
    let project = crate::structure::read_project(&bytes).unwrap();
    let crate::structure::ItemKind::Composition(comp) = &project.item(3).unwrap().kind else {
        panic!("SH01")
    };
    for (consumer, provider) in [(2414, 2233), (2370, 2219)] {
        let native = comp
            .layers
            .iter()
            .find(|layer| layer.record.id() == consumer)
            .unwrap();
        assert_eq!(native.record.track_matte_type(), 1);
        assert_eq!(
            sources(native).unwrap(),
            vec![MatteSource {
                layer_id: provider,
                stage: MatteSampleStage::Source,
                mode: TrackMatteType::Alpha
            }]
        );
    }
    let result = super::super::to_structural_fx_document(&project, Some(3)).unwrap();
    let mut all = Vec::new();
    groups(result.document.composition().layers(), &mut all);
    for native_id in [2414, 2370] {
        let original = all
            .iter()
            .find(|group| {
                group
                    .description
                    .contains(&format!("AEP comp=3 layer={native_id} kind="))
            })
            .unwrap();
        let wrapper = all
            .iter()
            .find(|group| Some(group.id) == original.parent)
            .unwrap();
        assert!(
            wrapper.name.ends_with("(Set Matte)"),
            "native {native_id} has no Set Matte gate"
        );
        let effect_gate = wrapper.track_matte.as_ref().unwrap();
        let native_gate = original.track_matte.as_ref().unwrap();
        assert_eq!(effect_gate.mode, TrackMatteType::Alpha);
        assert_eq!(native_gate.mode, TrackMatteType::Alpha);
        assert_ne!(effect_gate.layer, native_gate.layer);
        assert!(
            wrapper
                .layers
                .iter()
                .any(|child| child.id() == effect_gate.layer)
        );
        assert!(all.iter().any(|group| group.id == native_gate.layer));
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

#[test]
#[ignore = "requires local licensed AEP_SET_MATTE_SOURCE; source cannot be redistributed"]
fn local_external_source_lowers_noise_from_source_stage_luma() {
    use sha2::{Digest, Sha256};
    let bytes =
        std::fs::read(std::env::var_os("AEP_SET_MATTE_SOURCE").expect("licensed source path"))
            .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "28bbce1b8c9f9625105d632504a97c598394a4753d6b0d923fb19942e302bb5d"
    );
    let mut project = crate::structure::read_project(&bytes).unwrap();
    let crate::structure::ItemKind::Composition(comp) = &project.item(705).unwrap().kind else {
        panic!("composition")
    };
    let target = comp
        .layers
        .iter()
        .find(|layer| layer.record.id() == 2098)
        .unwrap();
    assert_eq!(target.record.layer_type(), 0);
    assert_eq!(target.record.source_id(), 165);
    assert_eq!(target.record.parent_id(), 0);
    assert!(!target.record.flags().three_d_layer);
    assert_eq!(
        sources(target).unwrap(),
        vec![MatteSource {
            layer_id: 2088,
            stage: MatteSampleStage::Source,
            mode: TrackMatteType::Luma,
        }]
    );

    let provider = comp
        .layers
        .iter()
        .find(|layer| layer.record.id() == 2088)
        .unwrap();
    assert_eq!(provider.name.as_ref(), "SH-09");
    assert_eq!(provider.record.layer_type(), 0);
    assert_eq!(provider.record.source_id(), 724);
    assert_eq!(provider.record.parent_id(), 0);
    assert!(!provider.record.flags().three_d_layer);
    assert!(matches!(
        &project.item(724).unwrap().kind,
        crate::structure::ItemKind::Composition(_)
    ));
    assert!(has_native_effect(provider, "Keylight 906"));

    add_occurrence_pipeline_probe(&mut project);
    let crate::structure::ItemKind::Composition(comp) = &project.item(705).unwrap().kind else {
        panic!("composition")
    };
    let provider = comp
        .layers
        .iter()
        .find(|layer| layer.record.id() == 2088)
        .unwrap();
    assert!(has_native_effect(provider, "ADBE Tint"));
    assert!(
        !crate::layer_styles::read(
            &provider.content,
            [f64::from(comp.width), f64::from(comp.height)],
        )
        .styles
        .is_empty()
    );

    let mut all_effects_project = project.clone();
    let item = all_effects_project
        .items
        .iter_mut()
        .find(|item| item.id == 705)
        .unwrap();
    let crate::structure::ItemKind::Composition(comp) = &mut item.kind else {
        panic!("composition")
    };
    let target = comp
        .layers
        .iter_mut()
        .find(|layer| layer.record.id() == 2098)
        .unwrap();
    set_set_matte_stage(target, -1);
    assert_eq!(
        sources(target).unwrap(),
        vec![MatteSource {
            layer_id: 2088,
            stage: MatteSampleStage::AllEffects,
            mode: TrackMatteType::Luma,
        }]
    );

    let result = super::super::to_structural_fx_document(&project, Some(705)).unwrap();
    let mut all = Vec::new();
    groups(result.document.composition().layers(), &mut all);
    let wrappers: Vec<_> = all
        .iter()
        .filter(|group| group.name == "Noise (Set Matte)")
        .filter(|group| {
            group.layers.iter().any(|layer| {
                matches!(layer.data(), FxLayer::Group(child) if child.description.contains("comp=705 layer=2098 "))
            })
        })
        .collect();
    let [wrapper] = wrappers.as_slice() else {
        panic!(
            "expected one layer-2098 Set Matte wrapper: {:?}",
            result.diagnostics
        )
    };
    let matte = wrapper.track_matte.as_ref().expect("Luma gate");
    assert_eq!(matte.mode, TrackMatteType::Luma);
    let helper = wrapper
        .layers
        .iter()
        .find(|layer| layer.id() == matte.layer)
        .expect("direct-child provider");
    let FxLayer::Group(helper) = helper.data() else {
        panic!("helper")
    };
    assert_eq!(helper.parent, Some(wrapper.id));
    assert_eq!(helper.name, "SH-09 (Set Matte sample)");
    assert!(helper.description.contains("comp=705 layer=2088 "));
    // The pinned provider's native Keylight was asserted before the probe.
    // Because Keylight itself is not mapped, the probe substitutes mapped Tint/
    // Levels effects and adds a mask/style so SOURCE omission is discriminating.
    assert!(helper.effects.is_empty());
    assert!(helper.masks.is_empty());
    assert!(!helper.layers.is_empty());
    let mut nested = Vec::new();
    groups(&helper.layers, &mut nested);
    assert!(nested.iter().any(|group| {
        group.description.contains("comp=724 layer=747 ") && !group.effects.is_empty()
    }));

    // The independently visible provider retains its occurrence pipeline.
    let visible: Vec<_> = all
        .iter()
        .filter(|group| group.name == "SH-09")
        .filter(|group| group.description.contains("comp=705 layer=2088 "))
        .collect();
    let [visible] = visible.as_slice() else {
        panic!("expected one independently visible SH-09 provider")
    };
    assert!(!visible.is_hidden);
    assert!(!visible.effects.is_empty());
    assert!(!visible.masks.is_empty());
    assert!(has_effect_type(visible, "tintTritone"));
    assert!(has_effect_type(visible, "dropShadow"));

    // ALL_EFFECTS remains the old behavior and imports the same occurrence
    // Effects, mask, and Layer Style into its independent helper.
    let all_effects =
        super::super::to_structural_fx_document(&all_effects_project, Some(705)).unwrap();
    let mut all_groups = Vec::new();
    groups(all_effects.document.composition().layers(), &mut all_groups);
    let wrappers: Vec<_> = all_groups
        .iter()
        .filter(|group| group.name == "Noise (Set Matte)")
        .filter(|group| {
            group.layers.iter().any(|layer| {
                matches!(layer.data(), FxLayer::Group(child) if child.description.contains("comp=705 layer=2098 "))
            })
        })
        .collect();
    let [all_effects_wrapper] = wrappers.as_slice() else {
        panic!("expected one all-effects layer-2098 Set Matte wrapper")
    };
    let matte = all_effects_wrapper.track_matte.as_ref().expect("Luma gate");
    assert_eq!(matte.mode, TrackMatteType::Luma);
    let helper = all_effects_wrapper
        .layers
        .iter()
        .find(|layer| layer.id() == matte.layer)
        .expect("direct-child all-effects provider");
    let FxLayer::Group(helper) = helper.data() else {
        panic!("helper")
    };
    assert!(!helper.effects.is_empty());
    assert!(!helper.masks.is_empty());
    assert!(has_effect_type(helper, "tintTritone"));
    assert!(has_effect_type(helper, "dropShadow"));
    let mut nested = Vec::new();
    groups(&helper.layers, &mut nested);
    assert!(nested.iter().any(|group| {
        group.description.contains("comp=724 layer=747 ") && !group.effects.is_empty()
    }));

    let json = String::from_utf8(result.document.to_json_vec().unwrap()).unwrap();
    assert!(!json.contains("JsScript"));
}

#[test]
#[ignore = "requires local licensed AEP_CAPTION_SOURCE, which cannot be redistributed"]
fn local_external_caption_source_matte_keeps_geometry_without_effect_fade() {
    use fx_schema::{PropType, PropertyTarget};
    use sha2::{Digest, Sha256};
    let bytes =
        std::fs::read(std::env::var_os("AEP_CAPTION_SOURCE").expect("local native source path"))
            .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "0026b99dcc616687f05eac377f09f59a7fa4ef0ac73933a27342ef7a8b1205c7"
    );
    let original = crate::structure::read_project(&bytes).unwrap();
    for stage in [0, -1] {
        let mut project = original.clone();
        let mut root = project.item(474).unwrap().clone();
        root.id = 9000;
        let crate::structure::ItemKind::Composition(comp) = &mut root.kind else {
            panic!()
        };
        // Keep text immediately before its native Pill so index-1 stays valid.
        let provider = comp.layers[1].record.id();
        let mut target = synthetic_layer(true, provider, stage, None, vec![], 1);
        patch_layer_identity(&mut target, 9001, 474);
        let mut record = target.record.encode();
        record[107] = 0;
        record[160..164].fill(0);
        target.record = crate::schema::layer_records::LayerRecord::decode(&record).unwrap();
        target.name = "Caption Matte Consumer".into();
        comp.layers.push(target);
        project.items.push(root);
        let result = super::super::to_structural_fx_document(&project, Some(9000)).unwrap();
        let mut all = Vec::new();
        groups(result.document.composition().layers(), &mut all);
        let wrapper = all
            .iter()
            .copied()
            .find(|g| g.name == "Caption Matte Consumer (Set Matte)")
            .unwrap_or_else(|| panic!("missing consumer: {:?}", result.diagnostics));
        let matte_id = wrapper.track_matte.as_ref().unwrap().layer;
        let helper = all.iter().copied().find(|g| g.id == matte_id).unwrap();
        let visible = all
            .iter()
            .copied()
            .find(|g| {
                g.description
                    .contains(&format!("comp=9000 layer={provider} "))
            })
            .unwrap();
        let entries = result.document.composition().dynamics().entries();
        for (owner, fade_expected) in [(helper, stage == -1), (visible, true)] {
            let mut nested = Vec::new();
            groups(&owner.layers, &mut nested);
            let paint = nested
                .iter()
                .copied()
                .find(|g| {
                    g.layers
                        .iter()
                        .filter(|l| matches!(l.data(), FxLayer::Rect(_)))
                        .count()
                        == 2
                })
                .unwrap_or_else(|| panic!("missing paint group at stage {stage}"));
            let rectangles: Vec<_> = paint
                .layers
                .iter()
                .filter_map(|l| match l.data() {
                    FxLayer::Rect(r) => Some(r),
                    _ => None,
                })
                .collect();
            assert!(
                rectangles
                    .iter()
                    .any(|r| r.rect.fill_enabled && r.transform.opacity.value() == 50.0)
            );
            assert!(
                rectangles
                    .iter()
                    .any(|r| r.rect.stroke_enabled && r.transform.opacity.value() == 100.0)
            );
            let owned: Vec<_> = entries
                .iter()
                .filter(|e| {
                    e.target
                        .layer_id()
                        .is_some_and(|id| id == paint.id || rectangles.iter().any(|r| r.id == id))
                })
                .collect();
            assert_eq!(owned.len(), if fade_expected { 6 } else { 5 });
            let opacity = PropertyTarget::layer(paint.id, PropType::Opacity);
            assert_eq!(
                owned.iter().filter(|e| e.target == opacity).count(),
                usize::from(fade_expected)
            );
            for entry in owned {
                let keys = entry.animator.keyframe_track().unwrap().keyframes();
                assert_eq!(keys[0].layer_time().as_millis(), 11600);
                assert_eq!(
                    keys[1].layer_time().as_millis(),
                    if entry.target == opacity {
                        11801
                    } else {
                        12200
                    }
                );
            }
        }
        assert_eq!(
            helper.playback.input_range(),
            visible.playback.input_range()
        );
        assert_eq!(helper.transform, visible.transform);
    }
}
