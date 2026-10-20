use super::*;
use crate::rifx::Chunk;

fn marker(layer: &mut Layer, flags: u32, ticks: i32) {
    let mut name = b"ADBE Marker".to_vec();
    name.resize(40, 0);
    let mut meta = vec![0; 124];
    meta[..8].copy_from_slice(&[0xdb, 0x99, 0, 1, 0, 0, 0, 0]);
    meta[12..16].copy_from_slice(&24000_u32.to_be_bytes());
    let mut header = vec![0; 52];
    header[..4].copy_from_slice(&[0, 0xd0, 0x0b, 0xee]);
    header[11] = 1;
    header[19] = 16;
    header[23] = 4;
    let mut keys = vec![0; 16];
    keys[..4].copy_from_slice(&ticks.to_be_bytes());
    let mut value = vec![0; 20];
    value[..4].copy_from_slice(&flags.to_be_bytes());
    value[8..12].copy_from_slice(&48000_i32.to_be_bytes());
    value[12..16].copy_from_slice(&24000_u32.to_be_bytes());
    let root = layer
        .content
        .iter_mut()
        .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
        .unwrap()
        .children_mut()
        .unwrap();
    root.extend([
        Chunk::data(*b"tdmn", name).unwrap(),
        Chunk::list(
            *b"mrst",
            vec![
                Chunk::list(
                    *b"tdbs",
                    vec![
                        Chunk::data(*b"tdb4", meta).unwrap(),
                        Chunk::list(
                            *b"list",
                            vec![
                                Chunk::data(*b"lhd3", header).unwrap(),
                                Chunk::data(*b"ldat", keys).unwrap(),
                            ],
                        ),
                    ],
                ),
                Chunk::list(
                    *b"mrky",
                    vec![Chunk::list(
                        *b"Nmrd",
                        vec![
                            Chunk::data(*b"NmHd", value).unwrap(),
                            Chunk::data(*b"Utf8", b"Arbitrary label".to_vec()).unwrap(),
                        ],
                    )],
                ),
            ],
        ),
    ]);
}

fn project(flags: u32, stretch: (i32, u32)) -> StructuralProject {
    project_at(flags, stretch, 0)
}

fn project_at(flags: u32, stretch: (i32, u32), marker_ticks: i32) -> StructuralProject {
    // Supplementary code mutation of the public native precomposition fixture;
    // this is not an independently Adobe-authored protected-region oracle.
    let mut project = fixture("outPoint_clamp.aep");
    let layer = &mut composition_mut(&mut project, 13).layers[0];
    set_native_clock(layer, [10000, 0, 50000], stretch);
    marker(layer, flags, marker_ticks);
    project
}

fn imported(flags: u32, stretch: (i32, u32)) -> StructuralConversion {
    to_structural_fx_document(&project(flags, stretch), Some(13)).unwrap()
}

#[test]
fn leading_protected_region_keeps_unit_speed_in_stretched_editable_source_clock() {
    let converted = imported(2, (2, 1));
    let (clock, authored) = remap_clocks(&converted);
    assert!(authored.is_none());
    assert_eq!(clock.playback.input_range().start.as_millis(), 1000);
    assert_eq!(clock.playback.input_range().end().as_millis(), 11000);
    assert_eq!(
        key_millis(playback(clock)),
        [(1000, 0), (3000, 2000), (11000, 5000)]
    );
    assert!(
        playback(clock)
            .keyframes()
            .iter()
            .all(|key| key.easing == PropertyKeyframeEasing::Linear)
    );
    assert!(!has(&converted, Limitation::Placeholder));
    let readback =
        EditableFxCompositionDocument::from_json_slice(&converted.document.to_json_vec().unwrap())
            .unwrap();
    let root = as_group(&readback.composition().layers()[0]);
    let owner = as_group(&root.layers[0]);
    assert_eq!(
        key_millis(playback(as_group(&owner.layers[0]))),
        key_millis(playback(clock))
    );
}

#[test]
fn nonleading_protected_marker_times_keep_diagnosed_affine_clock() {
    for ticks in [-24000, 1, 24000] {
        let converted = to_structural_fx_document(&project_at(2, (2, 1), ticks), Some(13)).unwrap();
        let (clock, _) = remap_clocks(&converted);
        assert_eq!(key_millis(playback(clock)), [(1000, 0), (11000, 5000)]);
        assert!(!has(&converted, Limitation::Placeholder));
        assert!(converted.diagnostics.iter().any(|diagnostic| {
            diagnostic.message.contains("nonleading protected marker")
                && diagnostic.message.contains("affine timing retained")
        }));
    }
}

#[test]
fn ordinary_marker_and_unit_stretch_keep_the_affine_clock() {
    for (flags, stretch, end) in [(0, (2, 1), 11000), (2, (1, 1), 6000)] {
        let converted = imported(flags, stretch);
        let (clock, _) = remap_clocks(&converted);
        assert_eq!(key_millis(playback(clock)), [(1000, 0), (end, 5000)]);
    }
}

#[test]
fn unsupported_protected_clocks_keep_editable_siblings_and_diagnose_affine_fallback() {
    for case in 0..4 {
        let mut project = project(if case == 0 { 3 } else { 2 }, (2, 1));
        let layer = &mut composition_mut(&mut project, 13).layers[0];
        match case {
            0 => {}                   // Unknown protected flags.
            1 => marker(layer, 2, 0), // Duplicate marker property.
            2 => set_native_clock(layer, [10000, 10000, 50000], (2, 1)),
            3 => set_native_clock(layer, [10000, 0, 50000], (1, 4)),
            _ => unreachable!(),
        }
        let converted = to_structural_fx_document(&project, Some(13)).unwrap();
        let (clock, _) = remap_clocks(&converted);
        assert_eq!(playback(clock).keyframes().len(), 2);
        assert!(!has(&converted, Limitation::Placeholder));
        assert!(converted.diagnostics.iter().any(|diagnostic| {
            diagnostic.message.contains("protected-region source clock")
                && diagnostic.message.contains("affine timing retained")
        }));
    }
}
