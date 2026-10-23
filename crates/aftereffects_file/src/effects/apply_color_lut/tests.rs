//! Apply Color LUT sequence data against the human-authored AE 26.5 fixture
//! `color_ae_apply_color_lut.aep` (chart layer, Apply Color LUT with
//! `color_warm_tint.cube`, defaults otherwise).

use super::*;
use crate::structure::{ItemKind, read_project};

const FIXTURE: &[u8] = include_bytes!("../../../tests/fixtures/color_ae_apply_color_lut.aep");
/// The `.cube` the user picked (committed beside the Premiere LUT fixtures).
const CUBE: &str = include_str!("../../../../premiere_file/tests/fixtures/color_warm_tint.cube");

fn fixture_sequence_data() -> Vec<u8> {
    let project = read_project(FIXTURE).unwrap();
    let layer = project
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Composition(composition) => composition.layers.first(),
            _ => None,
        })
        .expect("chart layer");
    instance_sequence_data(&layer.content, 1).unwrap().to_vec()
}

/// ICC tag `signature` of the profile inside `sequence_data`.
fn tag(sequence_data: &[u8], signature: &[u8; 4]) -> Vec<u8> {
    let profile = &sequence_data[HEADER_LEN..];
    let count = be_u32(profile, PROFILE_HEADER_LEN).unwrap() as usize;
    (0..count)
        .map(|index| &profile[PROFILE_HEADER_LEN + 4 + 12 * index..][..12])
        .find(|entry| &entry[..4] == signature)
        .map(|entry| {
            let offset = u32::from_be_bytes(entry[4..8].try_into().unwrap()) as usize;
            let size = u32::from_be_bytes(entry[8..12].try_into().unwrap()) as usize;
            profile[offset..offset + size].to_vec()
        })
        .expect("tag present")
}

#[test]
fn fixture_embeds_the_picked_cube_as_a_16_bit_device_link() {
    let data = fixture_sequence_data();
    assert_eq!(data.len(), 30_256);
    let embedded = decode_sequence_data(&data).unwrap();
    assert_eq!(embedded.file_name, "color_warm_tint.cube");
    let picked = Lut3d::parse_cube(CUBE).unwrap();
    assert_eq!(embedded.lut.size, 17);
    assert_eq!(picked.size, 17);
    let worst = embedded
        .lut
        .samples
        .iter()
        .zip(&picked.samples)
        .flat_map(|(native, cube)| native.iter().zip(cube).map(|(a, b)| (a - b).abs()))
        .fold(0.0, f64::max);
    assert!(worst <= 0.5 / 65_535.0 + 1e-12, "max deviation {worst}");
    // Red varies fastest in the decoded .cube order, as in the picked file.
    assert_eq!(picked.samples[1], [0.15625, 0.0, 0.0]);
    assert!((embedded.lut.samples[1][0] - 0.15625).abs() < 1e-5);
}

#[test]
fn fixture_lut_becomes_a_content_addressed_look_transform() {
    let project = read_project(FIXTURE).unwrap();
    let layer = project
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Composition(composition) => composition.layers.first(),
            _ => None,
        })
        .unwrap();
    let (effect, embedded) = lower(&layer.content, 1).unwrap();
    let value = serde_json::to_value(&effect).unwrap();
    assert_eq!(value["type"], "lookTransform");
    assert_eq!(value["mix"], 1.0);
    assert_eq!(value["transform"]["type"], "lut3d");
    assert_eq!(
        value["transform"]["inputEncoding"],
        SDR_REC709_DISPLAY_ENCODING_ID
    );
    assert_eq!(
        value["transform"]["outputEncoding"],
        SDR_REC709_DISPLAY_ENCODING_ID
    );
    assert_eq!(value["transform"]["interpolation"], "trilinear");
    let id = embedded.asset_id();
    assert_eq!(value["transform"]["assetId"], id.as_str());
    assert!(id.as_str().starts_with("aep-lut-"));
    assert_eq!(embedded.package_file_name(), "color_warm_tint.cube");
    let text = embedded.cube_text();
    assert!(text.starts_with("TITLE \"color_warm_tint\"\nLUT_3D_SIZE 17\n"));
    assert_eq!(Lut3d::parse_cube(&text).unwrap().size, 17);
    assert!(matches!(lower(&layer.content, 2), Err(reason) if reason.contains("occurrence")));
}

#[test]
fn encoded_sequence_data_matches_the_native_profile_transform() {
    let native = fixture_sequence_data();
    let embedded = decode_sequence_data(&native).unwrap();
    let (written, clamped) = encode_sequence_data(&embedded.lut, &embedded.file_name);
    assert_eq!(clamped, 0);
    // Header prefix, transform, description and sequence tags are byte-equal;
    // only the copyright text, date, CMM/creator and profile ID differ.
    assert_eq!(written[..16], native[..16]);
    for signature in [b"A2B0", b"desc", b"pseq"] {
        assert_eq!(
            tag(&written, signature),
            tag(&native, signature),
            "{signature:?}"
        );
    }
    let (profile, native_profile) = (&written[HEADER_LEN..], &native[HEADER_LEN..]);
    assert_eq!(profile[8..24], native_profile[8..24]);
    assert_eq!(profile[36..40], native_profile[36..40]);
    assert_eq!(profile[64..80], native_profile[64..80]);
    assert_eq!(decode_sequence_data(&written).unwrap(), embedded);
}

#[test]
fn encoding_the_picked_cube_reproduces_the_native_clut() {
    let picked = Lut3d::parse_cube(CUBE).unwrap();
    let (written, _) = encode_sequence_data(&picked, "color_warm_tint.cube");
    assert_eq!(
        tag(&written, b"A2B0"),
        tag(&fixture_sequence_data(), b"A2B0"),
        "AE rounds each sample to the nearest 16-bit code"
    );
}

#[test]
fn out_of_range_samples_are_clamped_and_counted() {
    let mut lut = Lut3d::parse_cube(CUBE).unwrap();
    lut.samples[0] = [-0.25, 1.5, 0.5];
    let (written, clamped) = encode_sequence_data(&lut, "x.cube");
    assert_eq!(clamped, 2);
    let back = decode_sequence_data(&written).unwrap();
    assert_eq!(back.lut.samples[0][..2], [0.0, 1.0]);
    assert_eq!(back.file_name, "x.cube");
}

#[test]
fn cube_parser_accepts_only_unit_domain_3d_luts() {
    let identity = "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";
    let lut = Lut3d::parse_cube(&format!("# comment\nTITLE \"t\"\n{identity}")).unwrap();
    assert_eq!(lut.samples[1], [1.0, 0.0, 0.0]);
    for (text, reason) in [
        ("LUT_1D_SIZE 2\n0 0 0\n1 1 1\n", "LUT_1D_SIZE"),
        (&*format!("DOMAIN_MAX 2 2 2\n{identity}"), "unit domain"),
        ("LUT_3D_SIZE 2\n0 0 0\n", "1 samples"),
        ("LUT_3D_SIZE 1\n0 0 0\n", "LUT_3D_SIZE must be"),
        ("0 0 0\n", "no LUT_3D_SIZE"),
        ("LUT_3D_SIZE 2\nFOO 1\n", "unknown .cube keyword"),
    ] {
        let error = Lut3d::parse_cube(text).unwrap_err();
        assert!(error.contains(reason), "{text:?}: {error}");
    }
}

#[test]
fn foreign_profiles_are_reported_not_approximated() {
    let native = fixture_sequence_data();
    let mut broken = native.clone();
    broken[0] = 9;
    assert!(
        decode_sequence_data(&broken)
            .unwrap_err()
            .contains("header")
    );
    // A non-identity matrix (first element 2.0).
    let embedded = decode_sequence_data(&native).unwrap();
    let (mut written, _) = encode_sequence_data(&embedded.lut, "x.cube");
    let profile_start = HEADER_LEN;
    let a2b0 = written
        .windows(4)
        .position(|window| window == b"mAB ")
        .unwrap();
    let matrix = a2b0 + be_u32(&written, a2b0 + 16).unwrap() as usize;
    written[matrix..matrix + 4].copy_from_slice(&0x2_0000_i32.to_be_bytes());
    assert!(profile_start < matrix);
    assert!(
        decode_sequence_data(&written)
            .unwrap_err()
            .contains("matrix is not the identity")
    );
    // A gamma 2.2 parametric A curve.
    let (mut written, _) = encode_sequence_data(&embedded.lut, "x.cube");
    let a_curves = a2b0 + be_u32(&written, a2b0 + 28).unwrap() as usize;
    written[a_curves + 12..a_curves + 16].copy_from_slice(&0x2_3333_i32.to_be_bytes());
    assert!(
        decode_sequence_data(&written)
            .unwrap_err()
            .contains("A curves are not the identity")
    );
}

#[test]
fn file_label_fits_the_declaration_label() {
    assert_eq!(
        file_label("color_warm_tint.cube"),
        "Using: color_warm_tint.cube"
    );
    let long = file_label(&"é".repeat(40));
    assert!(long.len() <= PARD_LABEL_BYTES && long.starts_with("Using: é"));
    assert_eq!(safe_cube_file_name("../x.cube"), "apply-color-lut.cube");
    assert_eq!(safe_cube_file_name("Warm Look.CUBE"), "Warm Look.CUBE");
    assert_eq!(safe_cube_file_name(".cube"), "apply-color-lut.cube");
}

/// The registry declaration (from the fixture's project-level `EfDf`) equals
/// every `pard` the fixture saved, in the definition and the layer instance.
#[test]
fn registry_declaration_matches_the_fixture_pards() {
    fn pards<'a>(chunks: &'a [Chunk], out: &mut Vec<Vec<(String, &'a [u8])>>) {
        for chunk in chunks {
            if chunk.list_kind() == Some(*b"parT") {
                let table = chunk.children().unwrap();
                let runs = properties::runs(table).unwrap();
                out.push(
                    runs.iter()
                        .map(|(name, run)| {
                            ((*name).to_owned(), properties::data(run, *b"pard").unwrap())
                        })
                        .collect(),
                );
            } else if let Some(children) = chunk.children() {
                pards(children, out);
            }
        }
    }
    let definition = crate::effects::definitions::definition(MATCH_NAME).unwrap();
    assert_eq!(definition.name, "Apply Color LUT");
    let kinds: Vec<_> = definition.parameters.iter().map(|p| p.kind).collect();
    assert_eq!(kinds, [0, 13, 14, 9]);
    let file = crate::rifx::Rifx::parse_with(FIXTURE, |kind| kind == *b"btdk").unwrap();
    let mut tables = Vec::new();
    pards(file.chunks(), &mut tables);
    assert_eq!(tables.len(), 2, "definition and instance tables");
    for table in tables {
        assert_eq!(table.len(), definition.parameters.len());
        for ((name, pard), parameter) in table.iter().zip(&definition.parameters) {
            assert_eq!(name, &parameter.match_name);
            let encoded = crate::effects::definitions::encode_parameter(parameter).unwrap();
            assert_eq!(encoded.data_payload().unwrap(), *pard, "{name}");
        }
    }
}

/// A small LUT whose output channels each depend on several inputs
/// non-linearly (`R' = g·b`, `G' = r`, `B' = (r + g + b) / 3` squared).
fn channel_mixing_lut(size: usize) -> Lut3d {
    let step = |index: usize| index as f64 / (size - 1) as f64;
    let samples = (0..size.pow(3))
        .map(|index| {
            let (r, g, b) = (
                step(index % size),
                step(index / size % size),
                step(index / size / size),
            );
            [g * b, r, ((r + g + b) / 3.0).powi(2)]
        })
        .collect();
    Lut3d { size, samples }
}

#[test]
fn channel_mixing_lut_round_trips_through_the_device_link_within_16_bit_rounding() {
    let lut = channel_mixing_lut(5);
    let (written, clamped) = encode_sequence_data(&lut, "mix.cube");
    assert_eq!(clamped, 0);
    let back = decode_sequence_data(&written).unwrap();
    assert_eq!(back.file_name, "mix.cube");
    assert_eq!(back.lut.size, 5);
    // Every output channel mixes the inputs, so a channel-order or
    // red-slowest/red-fastest swap in the CLUT would move samples by far more
    // than one 16-bit code.
    for (decoded, source) in back.lut.samples.iter().zip(&lut.samples) {
        for (decoded, source) in decoded.iter().zip(source) {
            assert!(
                (decoded - source).abs() <= 0.5 / 65535.0 + 1e-6,
                "{decoded} vs {source}"
            );
        }
    }
}

/// Value of one output channel of a 2³ LUT at `point`, trilinearly.
fn trilinear(corner: impl Fn(usize, usize, usize) -> f64, [r, g, b]: [f64; 3]) -> f64 {
    let mut value = 0.0;
    for (dr, wr) in [(0, 1.0 - r), (1, r)] {
        for (dg, wg) in [(0, 1.0 - g), (1, g)] {
            for (db, wb) in [(0, 1.0 - b), (1, b)] {
                value += wr * wg * wb * corner(dr, dg, db);
            }
        }
    }
    value
}

/// The same, tetrahedrally (for `r >= g >= b`: 000 → 100 → 110 → 111).
fn tetrahedral(corner: impl Fn(usize, usize, usize) -> f64, [r, g, b]: [f64; 3]) -> f64 {
    assert!(r >= g && g >= b);
    (1.0 - r) * corner(0, 0, 0)
        + (r - g) * corner(1, 0, 0)
        + (g - b) * corner(1, 1, 0)
        + b * corner(1, 1, 1)
}

/// FX declares (and its renderer uses) trilinear interpolation, the only v1
/// `lut3d` interpolation; AE and Premiere apply LUTs tetrahedrally at their
/// default LUT Interpolation (measured, see `LUT_INTERPOLATION`). The two
/// agree on per-channel affine tables and differ on channel-mixing ones, which
/// is why every Apply Color LUT import and export states the gap.
#[test]
fn lut_interpolation_is_trilinear_and_differs_from_tetrahedral_only_for_channel_mixing_luts() {
    let imported = fx_schema::ColorTransform::lut3d(
        AssetId::from_trusted("lut"),
        ColorEncodingId::new(SDR_REC709_DISPLAY_ENCODING_ID).unwrap(),
        ColorEncodingId::new(SDR_REC709_DISPLAY_ENCODING_ID).unwrap(),
    );
    let fx_schema::ColorTransform::Lut3d { interpolation, .. } = imported;
    assert_eq!(interpolation, fx_schema::ColorLutInterpolation::Trilinear);
    assert!(LUT_INTERPOLATION.contains("AE interpolates the LUT tetrahedrally"));
    assert!(LUT_INTERPOLATION.contains("FX lut3d is trilinear"));

    let point = [0.75, 0.5, 0.25];
    let corner = |lut: &Lut3d, channel: usize| {
        let lut = lut.clone();
        move |r: usize, g: usize, b: usize| lut.samples[r + 2 * g + 4 * b][channel]
    };
    let warm = Lut3d {
        size: 2,
        samples: (0..8)
            .map(|index| {
                let (r, g, b) = (
                    (index & 1) as f64,
                    ((index >> 1) & 1) as f64,
                    (index >> 2) as f64,
                );
                [0.1 + 0.9 * r, g, 0.8 * b]
            })
            .collect(),
    };
    for channel in 0..3 {
        let (a, b) = (
            trilinear(corner(&warm, channel), point),
            tetrahedral(corner(&warm, channel), point),
        );
        assert!(
            (a - b).abs() < 1e-12,
            "affine channel {channel}: {a} vs {b}"
        );
    }
    let mixing = channel_mixing_lut(2);
    let (a, b) = (
        trilinear(corner(&mixing, 0), point),
        tetrahedral(corner(&mixing, 0), point),
    );
    // g·b at (0.75, 0.5, 0.25): trilinear 0.125, tetrahedral 0.25 (32 codes).
    assert!(
        (a - 0.125).abs() < 1e-12 && (b - 0.25).abs() < 1e-12,
        "{a} vs {b}"
    );
}
