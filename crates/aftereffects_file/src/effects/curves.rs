//! Curves (`ADBE CurvesCustom`) packed state ↔ FX piecewise-linear curves.
//!
//! The arbitrary payload holds a version header, five 256-entry eight-bit
//! tables (RGB master, Red, Green, Blue, Alpha) and five editor point blocks
//! ([`editor`]). AE renders the table; the editor points are what its curve
//! panel draws and edits (the table is AE's natural cubic spline through them).

mod editor;

use fx_schema::{
    ColorCurve, ColorCurvePoint, ColorCurves, ColorCurvesSemanticVersion, LayerEffect,
    MAX_COLOR_CURVE_POINTS,
};

use crate::{
    properties::{self, PropertyError},
    rifx::Chunk,
};

pub(crate) const MATCH_NAME: &str = "ADBE CurvesCustom";
pub(crate) const CONTROL: &str = "ADBE CurvesCustom-0001";
const SAMPLES: usize = 256;
const CHANNELS: usize = 5;
const TABLES_AT: usize = HEADER.len();
const EDITOR_AT: usize = TABLES_AT + CHANNELS * SAMPLES;
const PAYLOAD_LEN: usize = EDITOR_AT + CHANNELS * editor::BLOCK;
const HEADER: [u8; 4] = [0, 1, 0, 1];
/// Channel names in payload order, as AE's Channel menu shows them.
const CHANNEL_NAMES: [&str; 4] = ["RGB", "Red", "Green", "Blue"];

fn identity_table() -> [u8; SAMPLES] {
    std::array::from_fn(|x| x as u8)
}

/// AE 26.5's default (identity) Curves arbitrary payload.
pub(crate) fn default_payload() -> Vec<u8> {
    let identity = identity_table();
    let diagonal = editor::encode(&editor::DIAGONAL);
    payload([&identity; 4], [&diagonal; 4])
}

/// Header, the four colour tables plus an identity alpha table, then the
/// four colour editor blocks plus the default alpha block.
fn payload(tables: [&[u8; SAMPLES]; 4], blocks: [&[u8; editor::BLOCK]; 4]) -> Vec<u8> {
    let identity = identity_table();
    let diagonal = editor::encode(&editor::DIAGONAL);
    let mut bytes = Vec::with_capacity(PAYLOAD_LEN);
    bytes.extend_from_slice(&HEADER);
    for table in tables.into_iter().chain([&identity]) {
        bytes.extend_from_slice(table);
    }
    for block in blocks.into_iter().chain([&diagonal]) {
        bytes.extend_from_slice(block);
    }
    debug_assert_eq!(bytes.len(), PAYLOAD_LEN);
    bytes
}

/// Piecewise-linear FX evaluation at `input` in 0..=1.
fn evaluate(curve: &ColorCurve, input: f64) -> f64 {
    let points = curve.points();
    points
        .windows(2)
        .find(|pair| input <= pair[1].x())
        .map_or_else(
            || points.last().map_or(input, |point| point.y()),
            |pair| {
                let span = pair[1].x() - pair[0].x();
                let fraction = if span > 0.0 {
                    (input - pair[0].x()) / span
                } else {
                    1.0
                };
                pair[0].y() + fraction * (pair[1].y() - pair[0].y())
            },
        )
}

fn table(curve: impl Fn(f64) -> f64) -> [u8; SAMPLES] {
    std::array::from_fn(|x| (curve(x as f64 / 255.0).clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The FX knots as AE editor points, when they fit AE's 16-point editor at
/// distinct eight-bit input codes; `None` keeps the default diagonal.
fn editor_points(curve: &ColorCurve) -> Option<Vec<editor::Point>> {
    let code = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u16;
    let points: Vec<_> = curve
        .points()
        .iter()
        .map(|point| [code(point.x()), code(point.y())])
        .collect();
    (points.len() <= editor::MAX_POINTS && points.windows(2).all(|pair| pair[0][0] < pair[1][0]))
        .then_some(points)
}

/// Encode FX curves as the native Curves payload read by [`lower`]. Each
/// table entry is the FX curve sampled at `x / 255`, rounded to one code
/// value, and AE renders that table. Each nonidentity curve's knots are also
/// written as AE editor points (up to 16 at distinct input codes) so AE's
/// curve panel shows the FX shape; AE draws its natural cubic spline through
/// them, and editing the curve in AE regenerates the table from that spline.
/// FX applies Master before the matching channel while AE applies each
/// channel table before the RGB table (measured render), so a Master present
/// beside a channel curve is folded into the channel tables (Master written
/// as identity, folded channels keep the default editor diagonal).
/// Returns the payload and its contextual approximations.
pub(crate) fn encode(curves: &ColorCurves) -> (Vec<u8>, Vec<String>) {
    let mut warnings = vec![
        "Curves tables sampled from the piecewise-linear FX curves at 256 eight-bit codes; AE renders these tables".to_owned(),
    ];
    let channels = [&curves.red, &curves.green, &curves.blue];
    let fold = !curves.master.is_identity() && channels.iter().any(|curve| !curve.is_identity());
    let master = if fold {
        warnings.push("Master curve folded into the Red/Green/Blue tables (FX applies Master before each channel, AE applies each channel before its RGB curve); native RGB is identity and the folded channels keep AE's default editor diagonal, so AE's curve panel does not show them".to_owned());
        identity_table()
    } else {
        table(|x| evaluate(&curves.master, x))
    };
    let [red, green, blue] = channels.map(|curve| {
        table(|x| {
            let x = if fold { evaluate(&curves.master, x) } else { x };
            evaluate(curve, x)
        })
    });
    let tables = [master, red, green, blue];
    let sources = [&curves.master, &curves.red, &curves.green, &curves.blue];
    let blocks: [[u8; editor::BLOCK]; 4] = std::array::from_fn(|channel| {
        let curve = sources[channel];
        let folded = fold && channel > 0;
        let shown = if curve.is_identity() || folded || (fold && channel == 0) {
            None
        } else if let Some(points) = editor_points(curve) {
            let spline = editor::spline_table(&points);
            warnings.push(format!(
                "Curves {}: the {} FX knots are written as AE editor points {}; AE's curve panel draws its natural cubic spline through them, which differs from the rendered piecewise-linear table by up to {} codes, and editing the curve in AE regenerates the table from that spline",
                CHANNEL_NAMES[channel],
                points.len(),
                editor::describe(&points),
                editor::max_deviation(&spline, &tables[channel]),
            ));
            Some(points)
        } else {
            warnings.push(format!(
                "Curves {}: the {} FX knots do not fit AE's 16-point editor at distinct eight-bit input codes; AE editor points stay at the default diagonal, so AE's curve panel does not show this curve (the rendered table still carries it)",
                CHANNEL_NAMES[channel],
                curve.points().len(),
            ));
            None
        };
        editor::encode(shown.as_deref().unwrap_or(&editor::DIAGONAL))
    });
    (
        payload(
            [&tables[0], &tables[1], &tables[2], &tables[3]],
            [&blocks[0], &blocks[1], &blocks[2], &blocks[3]],
        ),
        warnings,
    )
}

/// Lower the static Curves occurrence `ordinal` (one-based) of a layer to FX
/// curves plus contextual import approximations.
pub(crate) fn lower(
    content: &[Chunk],
    ordinal: usize,
) -> Result<(LayerEffect, Vec<String>), PropertyError> {
    let roots = properties::root_runs(content)?;
    let parade = roots
        .iter()
        .filter(|(name, _)| *name == "ADBE Effect Parade")
        .collect::<Vec<_>>();
    let [(_, parade)] = parade.as_slice() else {
        return Err(PropertyError::Layout("ambiguous Effect Parade"));
    };
    let instances = properties::runs(properties::unique_list(parade, *b"tdgp")?)?;
    let (name, instance) = instances
        .get(
            ordinal
                .checked_sub(1)
                .ok_or(PropertyError::Layout("invalid Curves ordinal"))?,
        )
        .ok_or(PropertyError::Layout("missing Curves occurrence"))?;
    if *name != MATCH_NAME {
        return Err(PropertyError::Layout("Curves occurrence identity mismatch"));
    }
    let plugin = properties::unique_list(instance, *b"sspc")?;
    let controls = properties::runs(properties::unique_list(plugin, *b"tdgp")?)?;
    let matches = controls
        .iter()
        .filter(|(name, _)| *name == CONTROL)
        .collect::<Vec<_>>();
    let [(_, run)] = matches.as_slice() else {
        return Err(PropertyError::Layout(
            "missing/duplicate packed Curves control",
        ));
    };
    let leaf = properties::unique_list(run, *b"tdbs")?;
    let metadata = properties::read_path_metadata(leaf)?;
    if metadata.animated || metadata.expression_present || !metadata.keyframes.is_empty() {
        return Err(PropertyError::Layout(
            "animated/expression Curves unsupported",
        ));
    }
    let meta = properties::data(leaf, *b"tdb4")?;
    if meta.len() < 60 || meta[2..4] != [0, 1] || meta[57] != 1 || meta[59] != 8 {
        return Err(PropertyError::Layout("unknown Curves property type"));
    }
    let arbitrary = properties::unique_list(run, *b"aRbs")?;
    if arbitrary.len() != 1 {
        return Err(PropertyError::Layout("ambiguous Curves arbitrary records"));
    }
    from_bytes(properties::data(arbitrary, *b"aRbp")?)
}

fn from_bytes(bytes: &[u8]) -> Result<(LayerEffect, Vec<String>), PropertyError> {
    // Version 1: five 256-byte tables followed by five editor point blocks.
    if bytes.len() != PAYLOAD_LEN || bytes[..4] != HEADER {
        return Err(PropertyError::Layout("unknown Curves table size/version"));
    }
    let tables: Vec<&[u8; SAMPLES]> = bytes[TABLES_AT..EDITOR_AT]
        .as_chunks::<SAMPLES>()
        .0
        .iter()
        .collect();
    let identity = |table: &[u8]| table.iter().enumerate().all(|(x, y)| usize::from(*y) == x);
    if !identity(tables[4]) {
        return Err(PropertyError::Layout(
            "nonidentity Curves alpha table unsupported",
        ));
    }
    let mut warnings = vec!["Effect ADBE CurvesCustom: static 8-bit tables approximated by editable piecewise-linear ColorCurves within one code value at every table sample (AE renders the table; its curve panel shows a spline through the editor points, which FX curves cannot represent); HDR and alpha fidelity unverified".to_owned()];
    // AE applies each channel table before the RGB table (measured render);
    // FX applies Master first, so a combined state folds into the channels.
    let fold = !identity(tables[0]) && tables[1..4].iter().any(|table| !identity(*table));
    let fx_tables: [[u8; SAMPLES]; 4] = std::array::from_fn(|channel| match (fold, channel) {
        (true, 0) => identity_table(),
        (true, _) => std::array::from_fn(|x| tables[0][usize::from(tables[channel][x])]),
        (false, _) => *tables[channel],
    });
    if fold {
        warnings.push("Effect ADBE CurvesCustom: AE applies each Red/Green/Blue curve before the RGB curve, the reverse of FX's Master-then-channel order, so the RGB table is folded into the Red/Green/Blue curves (FX Master identity); the RGB curve is no longer separately editable".to_owned());
    }
    let curves = ColorCurves {
        master: fit(&fx_tables[0])?,
        red: fit(&fx_tables[1])?,
        green: fit(&fx_tables[2])?,
        blue: fit(&fx_tables[3])?,
    };
    let fitted = [&curves.master, &curves.red, &curves.green, &curves.blue];
    let blocks = bytes[EDITOR_AT..].as_chunks::<{ editor::BLOCK }>().0;
    for (channel, name) in CHANNEL_NAMES.iter().enumerate() {
        if identity(tables[channel]) {
            continue;
        }
        let imported = if fold {
            "folded as described".to_owned()
        } else {
            format!(
                "imported as {} piecewise-linear FX knots fitted to the table",
                fitted[channel].points().len()
            )
        };
        warnings.push(match editor::decode(&blocks[channel]) {
            Ok(points) => {
                let deviation = editor::max_deviation(&editor::spline_table(&points), tables[channel]);
                let agreement = if deviation == 0 {
                    "the table equals AE's natural cubic spline through them".to_owned()
                } else {
                    format!("the table differs from AE's natural cubic spline through them by up to {deviation} codes (imported from the table)")
                };
                format!(
                    "Effect ADBE CurvesCustom: {name} editor points {} {imported}; {agreement}",
                    editor::describe(&points)
                )
            }
            Err(reason) => format!(
                "Effect ADBE CurvesCustom: {name} editor points unreadable ({reason}); {imported}"
            ),
        });
    }
    Ok((
        LayerEffect::ColorCurves {
            semantic_version: ColorCurvesSemanticVersion::V1,
            curves,
        },
        warnings,
    ))
}

/// Fit an eight-bit table with piecewise-linear knots at table samples, each
/// sample within one code value; fails above 32 knots.
pub(crate) fn fit(table: &[u8]) -> Result<ColorCurve, PropertyError> {
    let mut knots = vec![0, SAMPLES - 1];
    loop {
        let mut worst = None;
        for segment in knots.windows(2) {
            let [start, end] = [segment[0], segment[1]];
            for x in start + 1..end {
                let fraction = (x - start) as f64 / (end - start) as f64;
                let predicted = f64::from(table[start])
                    + fraction * (f64::from(table[end]) - f64::from(table[start]));
                let error = (predicted - f64::from(table[x])).abs();
                if error > 1.0 && worst.is_none_or(|(_, previous)| error > previous) {
                    worst = Some((x, error));
                }
            }
        }
        let Some((x, _)) = worst else {
            break;
        };
        if knots.len() == MAX_COLOR_CURVE_POINTS {
            return Err(PropertyError::Layout(
                "Curves table exceeds 32-point fit at one-code tolerance",
            ));
        }
        let index = knots
            .binary_search(&x)
            .expect_err("fitted knot lies strictly inside a segment");
        knots.insert(index, x);
    }
    ColorCurve::new(
        knots
            .into_iter()
            .map(|x| ColorCurvePoint::new(x as f64 / 255.0, f64::from(table[x]) / 255.0))
            .collect(),
    )
    .map_err(|_| PropertyError::Layout("Curves table fit is not representable"))
}

#[cfg(test)]
#[path = "curves/fixture_tests.rs"]
mod fixture_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> Vec<u8> {
        let mut bytes = vec![0, 1, 0, 1];
        for _ in 0..5 {
            bytes.extend(0..=255_u8);
        }
        bytes.resize(1644, 0);
        bytes
    }

    #[test]
    fn packed_curves_guards_unknown_alpha_and_unfittable_tables() {
        let original = payload();
        assert!(from_bytes(&original).is_ok());
        assert!(from_bytes(&original[..1643]).is_err());
        let mut bytes = original.clone();
        bytes[0] = 1;
        assert!(from_bytes(&bytes).is_err());
        let mut bytes = original.clone();
        bytes[4 + 4 * SAMPLES] = 1;
        assert!(from_bytes(&bytes).is_err());
        let mut bytes = original;
        for x in 0..SAMPLES {
            bytes[4 + x] = if x % 2 == 0 { 0 } else { 255 };
        }
        assert!(from_bytes(&bytes).is_err());
    }

    /// Every `aRbp` payload that follows a Curves control name in the native
    /// AE 26.5 colour catalog (definition table and instance records).
    fn catalog_payloads() -> Vec<&'static [u8]> {
        let bytes: &'static [u8] = include_bytes!("../../tests/fixtures/effects/color_catalog.aep");
        let mut payloads = Vec::new();
        let mut from = 0;
        while let Some(found) = bytes[from..]
            .windows(CONTROL.len())
            .position(|window| window == CONTROL.as_bytes())
        {
            let name = from + found;
            let tag = name
                + bytes[name..]
                    .windows(4)
                    .position(|window| window == b"aRbp")
                    .unwrap();
            let len = u32::from_be_bytes(bytes[tag + 4..tag + 8].try_into().unwrap()) as usize;
            payloads.push(&bytes[tag + 8..tag + 8 + len]);
            from = name + CONTROL.len();
        }
        payloads
    }

    #[test]
    fn curves_default_payload_matches_native_catalog_bytes() {
        let payloads = catalog_payloads();
        assert_eq!(payloads.len(), 4, "definition + instance, project + layer");
        for native in payloads {
            assert_eq!(native, default_payload().as_slice());
        }
    }

    #[test]
    fn curves_encode_round_trips_through_the_table_reader() {
        let curve = |points: &[(f64, f64)]| {
            ColorCurve::new(
                points
                    .iter()
                    .map(|(x, y)| ColorCurvePoint::new(*x, *y))
                    .collect(),
            )
            .unwrap()
        };
        let curves = ColorCurves {
            master: ColorCurve::identity(),
            red: curve(&[(0.0, 0.1), (0.5, 0.75), (1.0, 0.9)]),
            green: ColorCurve::identity(),
            blue: curve(&[(0.0, 0.0), (0.25, 0.1), (1.0, 1.0)]),
        };
        let (bytes, warnings) = encode(&curves);
        assert_eq!(bytes.len(), PAYLOAD_LEN);
        assert_eq!(
            warnings.len(),
            3,
            "table note + Red and Blue points: {warnings:?}"
        );
        assert_eq!(&bytes[4..4 + SAMPLES], &default_payload()[4..4 + SAMPLES]);
        assert_eq!(bytes[4 + SAMPLES], 26, "red(0) = 0.1 -> code 26");
        assert_eq!(bytes[4 + SAMPLES + 255], 230, "red(1) = 0.9 -> code 230");
        let LayerEffect::ColorCurves { curves: back, .. } = from_bytes(&bytes).unwrap().0 else {
            panic!("ColorCurves")
        };
        assert!(back.master.is_identity() && back.green.is_identity());
        let near = |a: &ColorCurve, b: &ColorCurve| {
            (0..SAMPLES).all(|x| {
                let input = x as f64 / 255.0;
                (evaluate(a, input) - evaluate(b, input)).abs() * 255.0 <= 1.5
            })
        };
        assert!(near(&back.red, &curves.red) && near(&back.blue, &curves.blue));
    }

    #[test]
    fn curves_encode_folds_master_into_present_channel_curves() {
        let darken = ColorCurve::new(vec![
            ColorCurvePoint::new(0.0, 0.0),
            ColorCurvePoint::new(1.0, 0.5),
        ])
        .unwrap();
        let invert = ColorCurve::new(vec![
            ColorCurvePoint::new(0.0, 1.0),
            ColorCurvePoint::new(1.0, 0.0),
        ])
        .unwrap();
        let curves = ColorCurves {
            master: darken.clone(),
            red: invert,
            green: ColorCurve::identity(),
            blue: ColorCurve::identity(),
        };
        let (bytes, warnings) = encode(&curves);
        assert!(
            warnings.iter().any(|w| w.contains("folded")),
            "{warnings:?}"
        );
        let LayerEffect::ColorCurves { curves: back, .. } = from_bytes(&bytes).unwrap().0 else {
            panic!("ColorCurves")
        };
        assert!(back.master.is_identity());
        // red(master(1)) = 1 - 0.5 and green(master(1)) = 0.5.
        assert_eq!(bytes[4 + SAMPLES + 255], 128);
        assert_eq!(bytes[4 + 2 * SAMPLES + 255], 128);
        assert_eq!(bytes[4 + SAMPLES], 255);
        assert!(!back.green.is_identity());
        // Master alone stays in the Master table.
        let (bytes, warnings) = encode(&ColorCurves {
            master: darken,
            red: ColorCurve::identity(),
            green: ColorCurve::identity(),
            blue: ColorCurve::identity(),
        });
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert_eq!(bytes[4 + 255], 128);
        assert_eq!(
            &bytes[4 + SAMPLES..4 + 4 * SAMPLES],
            &default_payload()[4 + SAMPLES..4 + 4 * SAMPLES]
        );
    }

    #[test]
    fn curves_encode_writes_fx_knots_as_editor_points_when_they_fit_the_native_editor() {
        let lift = ColorCurve::new(vec![
            ColorCurvePoint::new(0.0, 0.0),
            ColorCurvePoint::new(0.5, 0.65),
            ColorCurvePoint::new(1.0, 1.0),
        ])
        .unwrap();
        let (bytes, warnings) = encode(&ColorCurves {
            master: lift,
            red: ColorCurve::identity(),
            green: ColorCurve::identity(),
            blue: ColorCurve::identity(),
        });
        let block = |channel: usize| {
            &bytes[EDITOR_AT + channel * editor::BLOCK..EDITOR_AT + (channel + 1) * editor::BLOCK]
        };
        assert_eq!(
            editor::decode(block(0)).unwrap(),
            [[0, 0], [128, 166], [255, 255]]
        );
        for channel in 1..CHANNELS {
            assert_eq!(block(channel), editor::encode(&editor::DIAGONAL));
        }
        let deviation = editor::max_deviation(
            &editor::spline_table(&[[0, 0], [128, 166], [255, 255]]),
            &bytes[TABLES_AT..TABLES_AT + SAMPLES],
        );
        assert!(deviation > 0);
        assert!(
            warnings.iter().any(|warning| warning.contains(&format!(
                "Curves RGB: the 3 FX knots are written as AE editor points (0,0) (128,166) (255,255); AE's curve panel draws its natural cubic spline through them, which differs from the rendered piecewise-linear table by up to {deviation} codes"
            ))),
            "{warnings:?}"
        );

        // 17 knots exceed AE's 16-point editor: diagonal kept, table exact.
        let many = ColorCurve::new(
            (0..17)
                .map(|i| {
                    let x = f64::from(i) / 16.0;
                    ColorCurvePoint::new(x, x * x)
                })
                .collect(),
        )
        .unwrap();
        let (bytes, warnings) = encode(&ColorCurves {
            master: ColorCurve::identity(),
            red: ColorCurve::identity(),
            green: many.clone(),
            blue: ColorCurve::identity(),
        });
        assert_eq!(
            &bytes[EDITOR_AT + 2 * editor::BLOCK..EDITOR_AT + 3 * editor::BLOCK],
            editor::encode(&editor::DIAGONAL)
        );
        assert_eq!(
            bytes[TABLES_AT + 2 * SAMPLES + 128],
            (evaluate(&many, 128.0 / 255.0) * 255.0).round() as u8
        );
        assert!(
            warnings.iter().any(|warning| warning
                .contains("Curves Green: the 17 FX knots do not fit AE's 16-point editor")),
            "{warnings:?}"
        );
    }

    /// AE renders a Red table before the RGB table (adobe render of a
    /// Master x/2 + Red invert payload matched channel-then-RGB at MAD 0.73,
    /// FX's Master-then-channel at 42.7), so import folds RGB into channels.
    #[test]
    fn combined_native_rgb_and_channel_tables_fold_in_ae_order() {
        let mut bytes = payload();
        for x in 0..SAMPLES {
            bytes[TABLES_AT + x] = (x as u8).div_ceil(2);
            bytes[TABLES_AT + SAMPLES + x] = 255 - x as u8;
        }
        let (effect, warnings) = from_bytes(&bytes).unwrap();
        let LayerEffect::ColorCurves { curves, .. } = effect else {
            panic!("ColorCurves")
        };
        assert!(curves.master.is_identity());
        let at = |curve: &ColorCurve, code: u8| evaluate(curve, f64::from(code) / 255.0) * 255.0;
        // Red: RGB(255 - x); Green/Blue: RGB(x).
        assert!((at(&curves.red, 0) - 128.0).abs() <= 1.0);
        assert!(at(&curves.red, 255).abs() <= 1.0);
        assert!((at(&curves.green, 255) - 128.0).abs() <= 1.0);
        assert!((at(&curves.blue, 100) - 50.0).abs() <= 1.0);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("AE applies each Red/Green/Blue curve before the RGB curve")),
            "{warnings:?}"
        );
    }

    #[test]
    fn packed_curves_channel_fit_checks_every_native_sample() {
        let mut bytes = payload();
        for x in 0..SAMPLES {
            bytes[4 + SAMPLES + x] = ((x as f64 / 255.0).powi(2) * 255.0).round() as u8;
        }
        let LayerEffect::ColorCurves { curves, .. } = from_bytes(&bytes).unwrap().0 else {
            panic!("ColorCurves")
        };
        assert!(
            curves.master.is_identity() && curves.green.is_identity() && curves.blue.is_identity()
        );
        for x in 0..SAMPLES {
            let input = x as f64 / 255.0;
            let pair = curves
                .red
                .points()
                .windows(2)
                .find(|p| input >= p[0].x() && input <= p[1].x())
                .unwrap();
            let fraction = (input - pair[0].x()) / (pair[1].x() - pair[0].x());
            let output = pair[0].y() + fraction * (pair[1].y() - pair[0].y());
            assert!((output * 255.0 - f64::from(bytes[4 + SAMPLES + x])).abs() <= 1.000001);
        }
    }
}
