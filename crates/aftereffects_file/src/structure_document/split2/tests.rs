use super::super::{to_structural_fx_document, to_structural_fx_document_with_animation_limit};
use super::*;
use crate::{
    schema::layer_records::LayerRecord,
    structure::{ItemKind, read_project},
};
fn data(id: [u8; 4], bytes: Vec<u8>) -> Chunk {
    Chunk::data(id, bytes).unwrap()
}
fn name(name: &str) -> Chunk {
    let mut b = name.as_bytes().to_vec();
    b.resize(40, 0);
    data(*b"tdmn", b)
}
fn numeric(values: &[f64]) -> Chunk {
    let mut meta = vec![0; 124];
    meta[..2].copy_from_slice(&[0xdb, 0x99]);
    meta[3] = values.len() as u8;
    Chunk::list(
        *b"tdbs",
        vec![
            data(*b"tdb4", meta),
            data(*b"tdsb", vec![0, 0, 0, 1]),
            data(
                *b"cdat",
                values.iter().flat_map(|v| v.to_be_bytes()).collect(),
            ),
        ],
    )
}
fn flat() -> Vec<u8> {
    [
        0_u32.to_le_bytes().to_vec(),
        vec![1_f32; 256]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect(),
        [0_u32, 1, 1, 12345]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect(),
    ]
    .concat()
}
fn controls(amounts: [f64; 2]) -> Vec<Chunk> {
    let mut controls = vec![];
    for (key, value) in [
        ("CC Split 2-0001", vec![10.0, 45.0]),
        ("CC Split 2-0002", vec![500.0, 45.0]),
        ("CC Split 2-0003", vec![amounts[0]]),
        ("CC Split 2-0004", vec![amounts[1]]),
    ] {
        controls.extend([name(key), numeric(&value)]);
    }
    vec![Chunk::list(
        *b"tdgp",
        vec![
            name("ADBE Effect Parade"),
            Chunk::list(
                *b"tdgp",
                vec![
                    name(MATCH_NAME),
                    Chunk::list(
                        *b"sspc",
                        vec![
                            Chunk::list(*b"parT", vec![]),
                            Chunk::list(*b"tdgp", controls),
                            data(*b"sdat", flat()),
                        ],
                    ),
                ],
            ),
        ],
    )]
}
fn project() -> crate::structure::StructuralProject {
    let mut project = read_project(include_bytes!(
        "../../../tests/fixtures/effects/shape_owner_gaussian.aep"
    ))
    .unwrap();
    let ItemKind::Composition(comp) =
        &mut project.items.iter_mut().find(|i| i.id == 1).unwrap().kind
    else {
        panic!()
    };
    comp.width = 320;
    comp.height = 180;
    comp.duration_secs = 4.75;
    let below = comp.layers[0].clone();
    let mut adjustment = below.clone();
    let mut record = adjustment.record.encode();
    record[..4].copy_from_slice(&91_u32.to_be_bytes());
    record[38] |= 2;
    record[131] = 0;
    record[40..44].copy_from_slice(&99_u32.to_be_bytes());
    for (offset, v) in [(12, 24_i32), (20, 0), (28, 384)] {
        record[offset..offset + 4].copy_from_slice(&v.to_be_bytes());
    }
    for offset in [16, 24, 32] {
        record[offset..offset + 4].copy_from_slice(&96_u32.to_be_bytes());
    }
    adjustment.record = LayerRecord::decode(&record).unwrap();
    adjustment.content = controls([20.0, 20.0]);
    adjustment.name = "Renamed Split".into();
    let mut above = below.clone();
    let mut record = above.record.encode();
    record[..4].copy_from_slice(&92_u32.to_be_bytes());
    above.record = LayerRecord::decode(&record).unwrap();
    above.name = "Above unchanged".into();
    comp.layers = vec![above, adjustment, below];
    project.items.push(crate::structure::ProjectItem {
        id: 99,
        name: "Generic Adjustment source".into(),
        parent_folder: None,
        kind: ItemKind::Footage,
        footage: None,
        media: None,
        native_media: None,
        solid: Some(Ok(crate::structure::SolidSource {
            width: 320,
            height: 180,
            pixel_aspect: (1, 1),
            color: [0.0; 3],
        })),
    });
    project
}
fn root(doc: &super::super::StructuralConversion) -> &GroupLayer {
    let FxLayer::Group(root) = doc.document.composition().layers()[0].data() else {
        panic!()
    };
    root
}
fn as_group(layer: &fx_schema::Layer) -> &GroupLayer {
    let FxLayer::Group(g) = layer.data() else {
        panic!()
    };
    g
}
#[test]
fn split2_editable_halves_preserve_above_and_original_support_with_independent_ids() {
    let converted = to_structural_fx_document(&project(), Some(1)).unwrap();
    let root = root(&converted);
    assert_eq!(root.layers.len(), 5, "{:?}", converted.diagnostics);
    assert_eq!(root.layers[0].data().name(), "Above unchanged");
    assert!(matches!(root.layers[1].data(), FxLayer::Adjustment(_)));
    let unchanged = as_group(&root.layers[2]);
    assert!(unchanged.masks.is_empty());
    assert_eq!(unchanged.transform.opacity.value(), 100.0);
    let upper = as_group(&root.layers[3]);
    let lower = as_group(&root.layers[4]);
    assert_eq!(upper.transform.position, Position::TwoD([0.0, -36.0]));
    assert_eq!(lower.transform.position, Position::TwoD([0.0, 36.0]));
    assert_eq!(upper.masks.len(), 1);
    assert_eq!(lower.masks.len(), 1);
    let mut sets = vec![];
    for g in [unchanged, upper, lower] {
        let mut ids = HashSet::new();
        collect_layers(&FxLayer::Group(g.clone()), &mut ids, 0).unwrap();
        sets.push(ids);
    }
    assert!(sets[0].is_disjoint(&sets[1]));
    assert!(sets[0].is_disjoint(&sets[2]));
    assert!(sets[1].is_disjoint(&sets[2]));
    assert!(
        converted
            .diagnostics
            .iter()
            .any(|n| n.message.contains("source height times amount/100"))
    );
    for g in [unchanged, upper, lower] {
        let entry = converted
            .document
            .composition()
            .dynamics()
            .entries()
            .iter()
            .find(|e| e.target == PropertyTarget::layer(g.id, fx_schema::PropType::Opacity))
            .unwrap();
        let track = entry.animator.keyframe_track().unwrap();
        assert_eq!(track.keyframes()[1].layer_time().as_millis(), 250);
        assert_eq!(track.keyframes()[2].layer_time().as_millis(), 4250);
    }
}
#[test]
fn split2_unknown_profile_and_budget_denial_retain_original_stack() {
    let limited = to_structural_fx_document_with_animation_limit(&project(), Some(1), 1).unwrap();
    assert_eq!(root(&limited).layers.len(), 3);
    assert!(
        limited
            .diagnostics
            .iter()
            .any(|n| n.message.contains("CC Split 2 omitted"))
    );
    let mut profile = flat();
    profile[4..8].copy_from_slice(&0.5_f32.to_le_bytes());
    assert!(validate_flat_profile(&profile).is_err());
    let mut profile = flat();
    profile[1028..1032].copy_from_slice(&2_u32.to_le_bytes());
    assert!(validate_flat_profile(&profile).is_err());
    let mut profile = flat();
    profile[1040..1044].copy_from_slice(&999_u32.to_le_bytes());
    assert!(
        validate_flat_profile(&profile).is_ok(),
        "allocation identity is not an admission whitelist"
    );
    let mut profile = flat();
    profile[4..8].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(validate_flat_profile(&profile).is_err());
}
#[test]
fn split2_rejects_return_to_zero_and_expression_amounts() {
    let mut value = gate(100, 200, true);
    assert!(validate_amount(&value).is_err());
    value.animated = false;
    value.keyframes.clear();
    value.values = vec![1.0];
    assert!(validate_amount(&value).is_ok());
    value.expression_enabled = true;
    assert!(validate_amount(&value).is_err());
}

fn adjustment_mut(project: &mut crate::structure::StructuralProject) -> &mut Layer {
    let ItemKind::Composition(comp) =
        &mut project.items.iter_mut().find(|i| i.id == 1).unwrap().kind
    else {
        panic!()
    };
    &mut comp.layers[1]
}
#[test]
fn split2_declines_unequal_sides_and_transformed_or_unknown_domains() {
    let mut unequal = project();
    adjustment_mut(&mut unequal).content = controls([20.0, 10.0]);
    assert_eq!(
        root(&to_structural_fx_document(&unequal, Some(1)).unwrap())
            .layers
            .len(),
        3
    );
    for (key, value) in [
        ("ADBE Position", vec![161.0, 90.0]),
        ("ADBE Anchor Point", vec![159.0, 90.0]),
        ("ADBE Scale", vec![1.1, 1.0]),
        ("ADBE Rotate Z", vec![1.0]),
    ] {
        let mut project = project();
        adjustment_mut(&mut project).content[0]
            .children_mut()
            .unwrap()
            .extend([
                name("ADBE Transform Group"),
                Chunk::list(*b"tdgp", vec![name(key), numeric(&value)]),
            ]);
        let converted = to_structural_fx_document(&project, Some(1)).unwrap();
        assert_eq!(
            root(&converted).layers.len(),
            3,
            "{key}: {:?}",
            converted.diagnostics
        );
    }
    for kind in ["animated", "separated", "expression"] {
        let mut project = project();
        let mut leaf = numeric(&[160.0, 90.0]);
        let children = leaf.children_mut().unwrap();
        let index = children
            .iter()
            .position(|c| {
                c.id()
                    == if kind == "separated" {
                        *b"tdsb"
                    } else {
                        *b"tdb4"
                    }
            })
            .unwrap();
        let mut bytes = children[index].data_payload().unwrap().to_vec();
        let id = children[index].id();
        if kind == "animated" {
            bytes[68] = 1;
        }
        if kind == "separated" {
            bytes[2] |= 8;
        }
        if kind == "expression" {
            bytes[120] |= 1;
        }
        children[index] = data(id, bytes);
        adjustment_mut(&mut project).content[0]
            .children_mut()
            .unwrap()
            .extend([
                name("ADBE Transform Group"),
                Chunk::list(*b"tdgp", vec![name("ADBE Position"), leaf]),
            ]);
        let converted = to_structural_fx_document(&project, Some(1)).unwrap();
        assert_eq!(
            root(&converted).layers.len(),
            3,
            "{kind}: {:?}",
            converted.diagnostics
        );
    }
    let mut project = project();
    project
        .items
        .iter_mut()
        .find(|i| i.id == 99)
        .unwrap()
        .solid
        .as_mut()
        .unwrap()
        .as_mut()
        .unwrap()
        .width = 319;
    assert_eq!(
        root(&to_structural_fx_document(&project, Some(1)).unwrap())
            .layers
            .len(),
        3
    );
}
fn conversion_state(
    project: &crate::structure::StructuralProject,
    shape_limit: usize,
) -> (
    Vec<FxLayer>,
    u64,
    usize,
    usize,
    Vec<crate::diagnostic::ImportDiagnostic>,
) {
    use super::super::{
        AssetNamespace, MediaResolution, animation_budget::AnimationBudget, shapes,
    };
    let expression_samples = Default::default();
    let mut resolver = |_: &super::super::MediaAssetRequest| MediaResolution::Unavailable;
    let mut converter = Converter {
        text_overrides: Default::default(),
        expression_samples: &expression_samples,
        expression_evaluations: Default::default(),
        items: project.items.iter().map(|i| (i.id, i)).collect(),
        camera_normalizations: Default::default(),
        diagnostics: vec![],
        next_id: 2,
        linked: false,
        asset_namespace: AssetNamespace::STANDALONE,
        stack: vec![],
        visited_compositions: HashSet::new(),
        animations: vec![],
        animation_budget: AnimationBudget::default(),
        committed_inline_remap_bytes: 0,
        unavailable_cutouts: 0,
        overrides: vec![],
        media_resolver: &mut resolver,
        assets: vec![],
        shape_budget: shapes::OutputBudget::with_limit(shape_limit),
        mapped_shape_expressions: Default::default(),
        root_progress: fx_conv::Progress::default().phase("test", "layers", 0),
    };
    let item = project.item(1).unwrap();
    let layers = converter
        .composition_layers(item, LayerId::new(1), 0)
        .unwrap();
    (
        layers,
        converter.next_id,
        converter.shape_budget.checkpoint(),
        converter.animations.len(),
        converter.diagnostics,
    )
}
#[test]
fn split2_generated_mask_budget_denial_rolls_back_copies_and_ids() {
    let project = project();
    let accepted = conversion_state(&project, usize::MAX);
    assert_eq!(accepted.0.len(), 5);
    let rejected = conversion_state(&project, accepted.2 - 1);
    let mut baseline = project.clone();
    adjustment_mut(&mut baseline).content = controls([20.0, 10.0]);
    let baseline = conversion_state(&baseline, usize::MAX);
    assert_eq!(rejected.0.len(), 3);
    assert_eq!(rejected.1, baseline.1, "copy identities rolled back");
    assert_eq!(rejected.2, baseline.2, "shape charges rolled back");
    assert_eq!(rejected.3, baseline.3, "animations rolled back");
    assert!(
        rejected
            .4
            .iter()
            .any(|d| d.message.contains("generated Split mask exceeds"))
    );
}
