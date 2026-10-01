//! Synthetic storage-layout regressions, not independent Adobe render proof.
use super::read_effects;
use crate::{properties, rifx::Chunk};

const HISTOGRAM: &str = "ADBE Easy Levels2-0002";

fn named(name: &str) -> Chunk {
    let mut bytes = vec![0; 40];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    Chunk::data(*b"tdmn", bytes).unwrap()
}

fn payload(master: [f32; 5]) -> Vec<u8> {
    let mut bytes = vec![0, 15, 16, 167, 0, 0, 0, 1];
    for channel in [
        master,
        [0., 1., 1., 0., 1.],
        [0., 1., 1., 0., 1.],
        [0., 1., 1., 0., 1.],
        [0., 1., 1., 0., 1.],
    ] {
        for value in channel {
            bytes.extend(value.to_be_bytes());
        }
    }
    bytes
}

fn leaf() -> Chunk {
    let mut meta = vec![0; 124];
    meta[..4].copy_from_slice(&[0xdb, 0x99, 0, 1]);
    meta[57] = 1;
    meta[59] = 8;
    Chunk::list(
        *b"tdbs",
        vec![
            Chunk::data(*b"tdb4", meta).unwrap(),
            Chunk::data(*b"tdsb", [0, 0, 0, 1]).unwrap(),
            Chunk::data(*b"cdat", [0; 4]).unwrap(),
        ],
    )
}

fn histogram(bytes: Vec<u8>) -> Vec<Chunk> {
    vec![
        named(HISTOGRAM),
        leaf(),
        Chunk::list(*b"aRbs", vec![Chunk::data(*b"aRbp", bytes).unwrap()]),
    ]
}

fn content(body: Vec<Chunk>) -> Vec<Chunk> {
    let plugin = |body| {
        Chunk::list(
            *b"sspc",
            vec![
                // A separate default arbitrary payload must never replace the authored body.
                Chunk::list(
                    *b"parT",
                    vec![
                        named(HISTOGRAM),
                        Chunk::data(*b"aRbp", payload([0., 1., 1., 0., 1.])).unwrap(),
                    ],
                ),
                Chunk::list(*b"tdgp", body),
            ],
        )
    };
    vec![Chunk::list(
        *b"tdgp",
        vec![
            named("ADBE Effect Parade"),
            Chunk::list(
                *b"tdgp",
                vec![
                    named("ADBE Easy Levels2"),
                    plugin(body),
                    named("ADBE Tint"),
                    Chunk::list(
                        *b"sspc",
                        vec![Chunk::list(*b"parT", vec![]), Chunk::list(*b"tdgp", vec![])],
                    ),
                ],
            ),
        ],
    )]
}

fn master(body: Vec<Chunk>) -> ([f64; 5], Vec<String>) {
    let (effects, warnings) = read_effects(&content(body), [3840., 1600.]);
    assert_eq!(effects.len(), 2, "{warnings:?}");
    let values = std::array::from_fn(|i| {
        let name = format!("ADBE Easy Levels2-{:04}", i + 3);
        effects[0]
            .parameters
            .iter()
            .find(|p| p.match_name == name)
            .unwrap()
            .numeric
            .as_ref()
            .unwrap()
            .values[0]
    });
    (values, warnings)
}

#[test]
fn packed_levels_master_overrides_sparse_defaults_and_ui_cache() {
    let expected = [48. / 255., 234. / 255., 1.5, 0.1, 0.9];
    let mut body = histogram(payload(expected));
    let mut meta = vec![0; 124];
    meta[..4].copy_from_slice(&[0xdb, 0x99, 0, 1]);
    body.extend([
        named("ADBE Easy Levels2-0003"),
        Chunk::list(
            *b"tdbs",
            vec![
                Chunk::data(*b"tdb4", meta).unwrap(),
                Chunk::data(*b"tdsb", [0, 0, 0, 1]).unwrap(),
                Chunk::data(*b"cdat", 0.75_f64.to_be_bytes()).unwrap(),
            ],
        ),
    ]);
    let (values, warnings) = master(body);
    assert_eq!(values, expected.map(f64::from));
    assert!(
        !warnings
            .iter()
            .any(|w| w.contains("not a supported numeric vector")),
        "{warnings:?}"
    );
}

#[test]
fn packed_levels_unknown_or_invalid_values_omit_only_the_effect() {
    let good = payload([0.2, 0.8, 1., 0., 1.]);
    let mut cases = vec![vec![], good[..107].to_vec()];
    let mut unknown = good.clone();
    unknown[7] = 2;
    cases.push(unknown);
    let mut extended = good.clone();
    extended.push(0);
    cases.push(extended);
    for values in [
        [f32::NAN, 0.8, 1., 0., 1.],
        [0., f32::INFINITY, 1., 0., 1.],
        [-0.1, 1., 1., 0., 1.],
        [0.8, 0.2, 1., 0., 1.],
        [0., 1., 0., 0., 1.],
        [0., 1., 1., 0., 2.],
    ] {
        cases.push(payload(values));
    }
    for bytes in cases {
        assert_omitted(histogram(bytes));
    }
}

fn assert_omitted(body: Vec<Chunk>) {
    let (effects, warnings) = read_effects(&content(body), [320., 180.]);
    assert_eq!(effects.len(), 1, "{warnings:?}");
    assert_eq!(effects[0].match_name, "ADBE Tint");
    assert_eq!(effects[0].index, 2);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("packed Levels") && w.contains("omitted")),
        "{warnings:?}"
    );
}

#[test]
fn packed_levels_rejects_animation_expression_and_ambiguous_records() {
    let good = || histogram(payload([0.2, 0.8, 1., 0., 1.]));
    for (index, value) in [(68, 1), (120, 1), (57, 0), (59, 0)] {
        let mut body = good();
        let leaf = body[1].children_mut().unwrap();
        let mut meta = properties::data(leaf, *b"tdb4").unwrap().to_vec();
        meta[index] = value;
        leaf[0] = Chunk::data(*b"tdb4", meta).unwrap();
        assert_omitted(body);
    }
    let mut body = good();
    body[1]
        .children_mut()
        .unwrap()
        .push(Chunk::data(*b"Utf8", b"arbitrary()".to_vec()).unwrap());
    assert_omitted(body);
    let mut body = good();
    body.extend(good());
    assert_omitted(body);
    let mut body = good();
    body.push(body[2].clone());
    assert_omitted(body);
    let mut body = good();
    let duplicate = body[2].children().unwrap()[0].clone();
    body[2].children_mut().unwrap().push(duplicate);
    assert_omitted(body);
}

#[test]
fn packed_levels_nonidentity_channel_is_diagnosed_but_master_survives() {
    let mut bytes = payload([0.2, 0.8, 1., 0., 1.]);
    bytes[28..32].copy_from_slice(&0.1_f32.to_be_bytes());
    let (values, warnings) = master(histogram(bytes));
    assert_eq!(values[0], f64::from(0.2_f32));
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("channel record 2") && w.contains("omitted")),
        "{warnings:?}"
    );
}

#[test]
fn packed_levels_ui_type_hint_cannot_round_master_fractions() {
    let mut content = content(histogram(payload([0.2, 0.8, 1., 0., 1.])));
    let root = content[0].children_mut().unwrap();
    let parade = root[1].children_mut().unwrap();
    let plugin = parade[1].children_mut().unwrap();
    let definitions = plugin[0].children_mut().unwrap();
    let mut descriptor = vec![0; 148];
    descriptor[12..16].copy_from_slice(&1_u32.to_be_bytes());
    definitions.extend([
        named("ADBE Easy Levels2-0003"),
        Chunk::data(*b"pard", descriptor).unwrap(),
    ]);
    let (effects, warnings) = read_effects(&content, [320., 180.]);
    assert_eq!(effects.len(), 2, "{warnings:?}");
    let black = effects[0]
        .parameters
        .iter()
        .find(|p| p.match_name == "ADBE Easy Levels2-0003")
        .unwrap();
    assert_eq!(
        black.numeric.as_ref().unwrap().values,
        vec![f64::from(0.2_f32)]
    );
}

#[test]
fn packed_levels_absence_preserves_existing_default_path() {
    let (values, _) = master(vec![]);
    assert_eq!(values, [0., 1., 1., 0., 1.]);
}
