use super::*;
use crate::{
    rifx::Chunk,
    structure::{ItemKind, read_project},
};

fn named(name: &str) -> Chunk {
    let mut bytes = vec![0; 40];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    Chunk::data(*b"tdmn", bytes).unwrap()
}

fn owner() -> Layer {
    let project = read_project(include_bytes!(
        "../../../tests/fixtures/effects/shape_owner_gaussian.aep"
    ))
    .unwrap();
    let ItemKind::Composition(comp) = &project.item(1).unwrap().kind else {
        panic!("composition")
    };
    let mut layer = comp.layers[0].clone();
    let mut bytes = vec![0, 1, 0, 1];
    for channel in 0..5 {
        bytes.extend((0..=255_u16).map(|x| if channel == 0 { (x / 2) as u8 } else { x as u8 }));
    }
    bytes.resize(1644, 0);
    let mut meta = vec![0; 124];
    meta[..4].copy_from_slice(&[0xdb, 0x99, 0, 1]);
    meta[57] = 1;
    meta[59] = 8;
    layer.content = vec![Chunk::list(
        *b"tdgp",
        vec![
            named("ADBE Effect Parade"),
            Chunk::list(
                *b"tdgp",
                vec![
                    named("ADBE CurvesCustom"),
                    Chunk::list(
                        *b"sspc",
                        vec![
                            Chunk::list(*b"parT", vec![]),
                            Chunk::list(
                                *b"tdgp",
                                vec![
                                    named("ADBE CurvesCustom-0001"),
                                    Chunk::list(
                                        *b"tdbs",
                                        vec![
                                            Chunk::data(*b"tdb4", meta).unwrap(),
                                            Chunk::data(*b"tdsb", [0, 0, 0, 1]).unwrap(),
                                            Chunk::data(*b"cdat", [0; 4]).unwrap(),
                                        ],
                                    ),
                                    Chunk::list(
                                        *b"aRbs",
                                        vec![Chunk::data(*b"aRbp", bytes).unwrap()],
                                    ),
                                ],
                            ),
                        ],
                    ),
                ],
            ),
        ],
    )];
    layer
}

#[test]
fn packed_curves_import_keeps_editable_master_lut() {
    let layer = owner();
    let imported = import(
        &Default::default(),
        1,
        &layer,
        [1920, 1080],
        [1920, 1080],
        &mut 999,
        &mut AnimationBudget::default(),
    );
    assert_eq!(imported.effects.len(), 1, "{:?}", imported.warnings);
    let data = imported.effects[0].data();
    let EffectData::Identified {
        effect: EffectPayload::Known(LayerEffect::ColorCurves { curves, .. }),
        ..
    } = data
    else {
        panic!("editable ColorCurves")
    };
    assert!(curves.red.is_identity() && curves.green.is_identity() && curves.blue.is_identity());
    let points = curves.master.points();
    assert_eq!(points[0].y(), 0.0);
    assert_eq!(points.last().unwrap().y(), 127.0 / 255.0);
    assert!(imported.animations.is_empty());
    assert_eq!(imported.native_ordinals, [1]);
    assert!(
        imported
            .warnings
            .iter()
            .any(|w| w.contains("8-bit") && w.contains("approx"))
    );
}
