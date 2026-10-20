//! Code-built effect controls on a public native precomposition fixture.
use super::*;
use crate::rifx::Chunk;
use fx_schema::LayerData;

fn named(name: &str) -> Chunk {
    let mut bytes = name.as_bytes().to_vec();
    bytes.resize(40, 0);
    Chunk::data(*b"tdmn", bytes).unwrap()
}

fn scalar(name: &str, value: f64) -> Vec<Chunk> {
    let mut meta = vec![0; 124];
    meta[..4].copy_from_slice(&[0xdb, 0x99, 0, 1]);
    vec![
        named(name),
        Chunk::list(
            *b"tdbs",
            vec![
                Chunk::data(*b"tdb4", meta).unwrap(),
                Chunk::data(*b"tdsb", [0, 0, 0, 1]).unwrap(),
                Chunk::data(*b"cdat", value.to_be_bytes()).unwrap(),
            ],
        ),
    ]
}

fn white_fill() -> Vec<Chunk> {
    let mut meta = vec![0; 124];
    meta[..4].copy_from_slice(&[0xdb, 0x99, 0, 4]);
    meta[59] = 1;
    effect(
        "ADBE Fill",
        [
            vec![
                named("ADBE Fill-0002"),
                Chunk::list(
                    *b"tdbs",
                    vec![
                        Chunk::data(*b"tdb4", meta).unwrap(),
                        Chunk::data(*b"tdsb", [0, 0, 0, 1]).unwrap(),
                        Chunk::data(
                            *b"cdat",
                            [255_f64; 4]
                                .into_iter()
                                .flat_map(f64::to_be_bytes)
                                .collect::<Vec<_>>(),
                        )
                        .unwrap(),
                    ],
                ),
            ],
            scalar("ADBE Fill-0005", 1.),
        ]
        .concat(),
        true,
    )
}

fn effect(name: &str, mut controls: Vec<Chunk>, enabled: bool) -> Vec<Chunk> {
    controls.insert(
        0,
        Chunk::data(*b"tdsb", [0, 0, 0, u8::from(enabled)]).unwrap(),
    );
    vec![
        named(name),
        Chunk::list(
            *b"sspc",
            vec![
                Chunk::list(*b"parT", vec![]),
                Chunk::list(*b"tdgp", controls),
            ],
        ),
    ]
}

fn project(mode: f64, opacity: f64, enabled: bool) -> StructuralProject {
    let mut project = fixture("outPoint_clamp.aep");
    let source_id = composition(&project, 13).layers[0].record.source_id();
    let shape = read_project(include_bytes!(
        "../../../tests/fixtures/effects/shape_owner_gaussian.aep"
    ))
    .unwrap();
    composition_mut(&mut project, source_id).layers = composition(&shape, 1).layers.clone();
    let layer = &mut composition_mut(&mut project, 13).layers[0];
    set_native_clock(layer, [10000, 0, 50000], (2, 1));
    stack(layer, mode, opacity, enabled);
    project
}

fn stack(layer: &mut Layer, mode: f64, opacity: f64, enabled: bool) {
    let gaussian = effect(
        "ADBE Gaussian Blur 2",
        [
            scalar("ADBE Gaussian Blur 2-0001", 5.),
            scalar("ADBE Gaussian Blur 2-0002", 1.),
            scalar("ADBE Gaussian Blur 2-0003", 0.),
        ]
        .concat(),
        true,
    );
    let composite = effect(
        "CC Composite",
        [
            scalar("CC Composite-0000", 0.),
            scalar("CC Composite-0001", opacity),
            scalar("CC Composite-0002", mode),
            scalar("CC Composite-0003", 1.),
        ]
        .concat(),
        enabled,
    );
    let root = layer
        .content
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .unwrap()
        .children_mut()
        .unwrap();
    root.extend([
        named("ADBE Effect Parade"),
        Chunk::list(*b"tdgp", [white_fill(), gaussian, composite].concat()),
    ]);
}

fn find<'a>(group: &'a GroupLayer, name: &str) -> Option<&'a GroupLayer> {
    if group.name == name {
        return Some(group);
    }
    group.layers.iter().find_map(|layer| match layer.data() {
        LayerData::Group(child) => find(child, name),
        _ => None,
    })
}

#[test]
fn cc_composite_restores_independent_original_precomp_before_owner_transform() {
    let converted = to_structural_fx_document(&project(3., 100., true), Some(13)).unwrap();
    let owner = as_group(&root(&converted).layers[0]);
    let output = find(owner, "CC Composite output").expect("editable original-compositing stage");
    assert!(
        owner.effects.is_empty(),
        "prefix must not recolor the original"
    );
    assert_eq!(output.layers.len(), 2);
    let original = as_group(&output.layers[0]);
    let processed = as_group(&output.layers[1]);
    assert_eq!(original.name, "CC Composite original source clock");
    assert!(original.effects.is_empty());
    assert_eq!(processed.name, "CC Composite processed input");
    assert_eq!(processed.effects.len(), 2);
    let fill = serde_json::to_value(&processed.effects[0]).unwrap();
    assert_eq!(fill["effect"]["type"], "tintTritone");
    assert_eq!(fill["effect"]["blackR"], 1.);
    assert_eq!(fill["effect"]["whiteR"], 1.);
    assert!(matches!(
        processed.effects[1].data(),
        fx_schema::EffectData::Identified {
            effect: fx_schema::EffectPayload::Known(fx_schema::LayerEffect::GaussianBlur { .. }),
            ..
        }
    ));
    let processed_clock = as_group(&processed.layers[0]);
    assert_eq!(
        original.playback.input_range(),
        processed_clock.playback.input_range()
    );
    assert_eq!(
        key_millis(playback(original)),
        key_millis(playback(processed_clock))
    );
    for (a, b) in playback(original)
        .keyframes()
        .iter()
        .zip(playback(processed_clock).keyframes())
    {
        assert_ne!(
            a.id, b.id,
            "copied source clocks need independent key identities"
        );
    }
    assert_eq!(key_millis(playback(original)), [(1000, 0), (11000, 5000)]);
    assert!(!original.layers.is_empty());
    assert_eq!(original.layers.len(), processed_clock.layers.len());
    assert_eq!(original.transform, output.transform);
    assert_eq!(processed.transform, output.transform);
    fn ids(layer: &fx_schema::Layer, seen: &mut HashSet<LayerId>) {
        assert!(
            seen.insert(layer.id()),
            "copies need independent layer identities"
        );
        if let Some(children) = layer.data().child_layers() {
            for child in children {
                ids(child, seen);
            }
        }
    }
    let mut seen = HashSet::new();
    for layer in &output.layers {
        ids(layer, &mut seen);
    }
    EditableFxCompositionDocument::from_json_slice(&converted.document.to_json_vec().unwrap())
        .expect("editable graph readback");
    assert!(
        converted
            .diagnostics
            .iter()
            .any(|d| d.message.contains("CC Composite") && d.message.contains("alpha"))
    );
}

#[test]
fn cc_composite_retains_authored_remap_and_parent_visibility_gate() {
    let mut project = cycled_remap_project("value", false);
    stack(
        &mut composition_mut(&mut project, 125).layers[0],
        3.,
        100.,
        true,
    );
    let converted = to_structural_fx_document(&project, Some(125)).unwrap();
    let output = find(root(&converted), "CC Composite output").unwrap();
    let original = as_group(&output.layers[0]);
    let processed = as_group(&output.layers[1]);
    let processed_gate = as_group(&processed.layers[0]);
    assert_eq!(
        key_millis(playback(original)),
        key_millis(playback(processed_gate))
    );
    let original_source = as_group(&original.layers[0]);
    let processed_source = as_group(&processed_gate.layers[0]);
    assert_eq!(original_source.name, "Authored source remap");
    assert_eq!(
        key_millis(playback(original_source)),
        key_millis(playback(processed_source))
    );
    assert_eq!(
        playback(original_source).before(),
        playback(processed_source).before()
    );
    assert_eq!(
        playback(original_source).after(),
        playback(processed_source).after()
    );
    for (a, b) in playback(original_source)
        .keyframes()
        .iter()
        .zip(playback(processed_source).keyframes())
    {
        assert_ne!(a.id, b.id);
        assert_eq!(a.easing, b.easing);
    }
    EditableFxCompositionDocument::from_json_slice(&converted.document.to_json_vec().unwrap())
        .unwrap();
}

fn sparse_project(explicit: Vec<Chunk>) -> StructuralProject {
    let mut project = project(3., 100., true);
    let layer = &mut composition_mut(&mut project, 13).layers[0];
    let root = layer
        .content
        .iter_mut()
        .find(|c| c.list_kind() == Some(*b"tdgp"))
        .unwrap()
        .children_mut()
        .unwrap();
    let parade = root.last_mut().unwrap().children_mut().unwrap();
    let descriptor = parade.last_mut().unwrap().children_mut().unwrap();
    *descriptor.last_mut().unwrap() = Chunk::list(*b"tdgp", explicit);
    let declarations = [(1, 2_u32, 100_u32 * 65_536), (2, 7, 3), (3, 4, 1)]
        .into_iter()
        .flat_map(|(suffix, kind, value)| {
            let mut bytes = vec![0; 148];
            bytes[12..16].copy_from_slice(&kind.to_be_bytes());
            bytes[56..60].copy_from_slice(&value.to_be_bytes());
            [
                named(&format!("CC Composite-{suffix:04}")),
                Chunk::data(*b"pard", bytes).unwrap(),
            ]
        })
        .collect();
    project.effect_definitions = vec![Chunk::list(
        *b"EfdG",
        vec![Chunk::list(
            *b"EfDf",
            vec![
                named("CC Composite"),
                Chunk::list(*b"sspc", vec![Chunk::list(*b"parT", declarations)]),
            ],
        )],
    )];
    project
}

#[test]
fn cc_composite_sparse_defaults_restore_original_without_overriding_instance_controls() {
    let converted = to_structural_fx_document(&sparse_project(vec![]), Some(13)).unwrap();
    let output = find(root(&converted), "CC Composite output")
        .expect("shared native defaults retain original paint");
    let original = as_group(&output.layers[0]);
    let processed = as_group(&output.layers[1]);
    assert!(original.effects.is_empty());
    assert!(!original.layers.is_empty());
    assert_eq!(processed.effects.len(), 2);
    assert_eq!(key_millis(playback(original)), [(1000, 0), (11000, 5000)]);

    for explicit in [
        scalar("CC Composite-0001", 50.),
        scalar("CC Composite-0002", 2.),
    ] {
        let converted = to_structural_fx_document(&sparse_project(explicit), Some(13)).unwrap();
        assert!(find(root(&converted), "CC Composite output").is_none());
    }
    let mut ambiguous = sparse_project(vec![]);
    let entries = ambiguous.effect_definitions[0].children_mut().unwrap();
    let mut conflicting = entries[0].clone();
    let descriptor = conflicting
        .children_mut()
        .unwrap()
        .last_mut()
        .unwrap()
        .children_mut()
        .unwrap();
    descriptor[0] = Chunk::list(*b"parT", vec![named("CC Composite-0001")]);
    entries.push(conflicting);
    let converted = to_structural_fx_document(&ambiguous, Some(13)).unwrap();
    assert!(find(root(&converted), "CC Composite output").is_none());
}

#[test]
fn cc_composite_nondefault_and_disabled_profiles_keep_convertible_prefix() {
    for (mode, opacity, enabled) in [(2., 100., true), (3., 50., true), (3., 100., false)] {
        let converted =
            to_structural_fx_document(&project(mode, opacity, enabled), Some(13)).unwrap();
        let owner = as_group(&root(&converted).layers[0]);
        assert!(
            find(owner, "CC Composite output").is_none(),
            "mode={mode} opacity={opacity} enabled={enabled}"
        );
        assert_eq!(owner.effects.len(), 2, "convertible prefix retained");
    }
}
