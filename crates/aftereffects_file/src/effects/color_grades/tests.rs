//! Synthetic decoded controls exercise the mapping contract; the pinned native
//! colour catalog test checks identities and defaults, not nondefault values.
use fx_schema::{ChannelMixer, PrimaryGrade, SelectiveColor};

use super::*;
use crate::{
    effects::native::{Declarations, DecodedParameter},
    properties::{NumericKeyframe, NumericValueKind},
};

fn scalar(values: Vec<f64>, keyframes: Vec<NumericKeyframe>) -> NumericProperty {
    NumericProperty {
        values,
        animated: !keyframes.is_empty(),
        expression_enabled: false,
        expression_present: false,
        dimensions_separated: false,
        keyframes,
        value_kind: NumericValueKind::Continuous,
    }
}

fn key(time_secs: f64, value: f64) -> NumericKeyframe {
    NumericKeyframe {
        time_secs,
        values: vec![value],
        in_interpolation: 1,
        out_interpolation: 1,
        in_speed: vec![0.0],
        in_influence: vec![1.0 / 6.0],
        out_speed: vec![0.0],
        out_influence: vec![1.0 / 6.0],
        spatial_in: Vec::new(),
        spatial_out: Vec::new(),
    }
}

fn parameter(match_name: &str, label: Option<&str>, numeric: NumericProperty) -> DecodedParameter {
    DecodedParameter {
        match_name: match_name.into(),
        declared_kind: Ok(Some(10)),
        declared_label: label.map(str::to_owned),
        numeric: Ok(numeric),
        unset_path: false,
    }
}

fn effect(match_name: &str, controls: &[(&str, f64)]) -> DecodedEffect {
    DecodedEffect {
        match_name: match_name.into(),
        index: 1,
        enabled: true,
        declarations: Declarations::Readable,
        parameters: controls
            .iter()
            .map(|(name, value)| parameter(name, None, scalar(vec![*value], Vec::new())))
            .collect(),
        compositing: Ok(Default::default()),
        arbitrary: Vec::new(),
    }
}

fn lumetri_controls() -> Vec<(&'static str, f64)> {
    vec![
        ("ADBE Lumetri-0003", 1.0),
        ("ADBE Lumetri-0007", 58.0),
        ("ADBE Lumetri-0008", 20.0),
        ("ADBE Lumetri-0011", 1.3),
        ("ADBE Lumetri-0012", 25.0),
        ("ADBE Lumetri-0013", -40.0),
        ("ADBE Lumetri-0014", 30.0),
        ("ADBE Lumetri-0015", 10.0),
        ("ADBE Lumetri-0016", -20.0),
        ("ADBE Lumetri-0020", 130.0),
        ("ADBE Lumetri-0023", 1.0),
        ("ADBE Lumetri-0030", 50.0),
        ("ADBE Lumetri-0031", 100.0),
    ]
}

fn grade(lowered: &LoweredGrade) -> PrimaryGrade {
    let LayerEffect::PrimaryGrade(grade) = lowered.effect else {
        panic!("primaryGrade expected, got {:?}", lowered.effect)
    };
    grade
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-12, "{actual} != {expected}");
}

#[test]
fn lumetri_basic_and_creative_scalars_map_to_one_primary_grade() {
    let lowered = lower(
        &effect(LUMETRI, &lumetri_controls()),
        ColorGradeDomain::Legacy,
    )
    .unwrap();
    let grade = grade(&lowered);
    // The fitted scales: Temperature/116, -Tint/100, Exposure/2.6,
    // Contrast/360, Highlights/240, Shadows/510, Whites/54, Blacks/124,
    // (Saturation-100)/110, Vibrance/167.
    close(grade.temperature, 0.5);
    close(grade.tint, -0.2);
    close(grade.exposure, 0.5);
    close(grade.contrast, 25.0 / 360.0);
    close(grade.highlights, -40.0 / 240.0);
    close(grade.shadows, 30.0 / 510.0);
    close(grade.whites, 10.0 / 54.0);
    close(grade.blacks, -20.0 / 124.0);
    close(grade.saturation, 30.0 / 110.0);
    close(grade.vibrance, 50.0 / 167.0);
    assert!(lowered.keyed.is_empty());
    // No saved curves, wheels, vignette or LUT state: no further sections.
    assert!(lowered.after.is_empty());
    assert!(
        !lowered
            .warnings
            .iter()
            .any(|w| w.contains("has no FX mapping"))
    );
    assert!(
        lowered
            .warnings
            .iter()
            .any(|w| w.contains("primaryGrade v1"))
    );
}

#[test]
fn lumetri_disabled_basic_section_keeps_creative_controls_only() {
    let mut controls = lumetri_controls();
    controls[0].1 = 0.0;
    let lowered = lower(&effect(LUMETRI, &controls), ColorGradeDomain::Legacy).unwrap();
    let grade = grade(&lowered);
    assert_eq!(
        (grade.temperature, grade.exposure, grade.saturation),
        (0.0, 0.0, 0.0)
    );
    close(grade.vibrance, 50.0 / 167.0);
    assert!(
        lowered
            .warnings
            .iter()
            .any(|w| w.contains("Basic Correction section disabled"))
    );
}

#[test]
fn lumetri_label_mismatch_omits_only_the_unconfirmed_control() {
    let mut source = effect(LUMETRI, &lumetri_controls());
    let tint = source
        .parameters
        .iter_mut()
        .find(|p| p.match_name == "ADBE Lumetri-0008")
        .unwrap();
    tint.declared_label = Some("Highlight Tint".into());
    source
        .parameters
        .iter_mut()
        .find(|p| p.match_name == "ADBE Lumetri-0007")
        .unwrap()
        .declared_label = Some("Temperature".into());
    let lowered = lower(&source, ColorGradeDomain::Legacy).unwrap();
    let grade = grade(&lowered);
    assert_eq!(grade.tint, 0.0);
    close(grade.temperature, 0.5);
    assert!(
        lowered
            .warnings
            .iter()
            .any(|w| w.contains("ADBE Lumetri-0008: declared label"))
    );
}

#[test]
fn lumetri_out_of_range_values_clamp_with_diagnostics() {
    let mut controls = lumetri_controls();
    controls[1].1 = 150.0; // Temperature: 150/116, FX maximum 1.
    controls[9].1 = 300.0; // Saturation 300%: 200/110, FX maximum 1.
    let lowered = lower(&effect(LUMETRI, &controls), ColorGradeDomain::Legacy).unwrap();
    let grade = grade(&lowered);
    assert_eq!((grade.temperature, grade.saturation), (1.0, 1.0));
    assert_eq!(
        lowered
            .warnings
            .iter()
            .filter(|w| w.contains("clamped"))
            .count(),
        2
    );
}

#[test]
fn lumetri_keys_bind_with_the_static_affine_map() {
    let mut source = effect(LUMETRI, &lumetri_controls());
    let exposure = source
        .parameters
        .iter_mut()
        .find(|p| p.match_name == "ADBE Lumetri-0011")
        .unwrap();
    exposure.numeric = Ok(scalar(Vec::new(), vec![key(0.0, 0.0), key(1.0, 2.6)]));
    let saturation = source
        .parameters
        .iter_mut()
        .find(|p| p.match_name == "ADBE Lumetri-0020")
        .unwrap();
    // 400% at the second key leaves the FX range: keys omitted, initial kept.
    saturation.numeric = Ok(scalar(Vec::new(), vec![key(0.0, 122.0), key(1.0, 400.0)]));
    let lowered = lower(&source, ColorGradeDomain::Legacy).unwrap();
    assert_eq!(lowered.keyed.len(), 1);
    assert_eq!(lowered.keyed[0].control.param, "exposure");
    close(lowered.keyed[0].control.scale, 1.0 / 2.6);
    let grade = grade(&lowered);
    close(grade.exposure, 0.0);
    close(grade.saturation, 0.2);
    assert!(
        lowered
            .warnings
            .iter()
            .any(|w| w.contains("keyed values leave the FX range"))
    );
}

/// An OCIO ACES project keeps the stops/2.2 Exposure scale for the static
/// value and its keys; every other control keeps its legacy fitted scale.
#[test]
fn aces_lumetri_exposure_keeps_the_aces_scale_for_static_values_and_keys() {
    let static_grade = lower(
        &effect(LUMETRI, &lumetri_controls()),
        ColorGradeDomain::Aces,
    )
    .unwrap();
    let grade = grade(&static_grade);
    close(grade.exposure, 1.3 / 2.2);
    close(grade.contrast, 25.0 / 360.0);
    assert!(
        static_grade
            .warnings
            .iter()
            .any(|w| w == LUMETRI_ACES_EXPOSURE_APPROXIMATION)
    );
    let legacy = lower(
        &effect(LUMETRI, &lumetri_controls()),
        ColorGradeDomain::Legacy,
    )
    .unwrap();
    assert!(
        !legacy
            .warnings
            .iter()
            .any(|w| w == LUMETRI_ACES_EXPOSURE_APPROXIMATION)
    );

    let mut source = effect(LUMETRI, &lumetri_controls());
    let exposure = source
        .parameters
        .iter_mut()
        .find(|p| p.match_name == "ADBE Lumetri-0011")
        .unwrap();
    exposure.numeric = Ok(scalar(Vec::new(), vec![key(0.0, 0.0), key(1.0, 2.2)]));
    let keyed = lower(&source, ColorGradeDomain::Aces).unwrap();
    assert_eq!(keyed.keyed.len(), 1);
    assert_eq!(keyed.keyed[0].control.param, "exposure");
    close(keyed.keyed[0].control.scale, 1.0 / 2.2);
}

/// A Lumetri instance with saved arbitrary-data payloads (RGB Curves, Color
/// Wheels, one Hue vs Sat curve) and scalar controls.
fn lumetri_with_data(scalars: &[(&str, f64)], data: Vec<(&str, Vec<u8>)>) -> DecodedEffect {
    let mut controls = lumetri_controls();
    controls.extend_from_slice(scalars);
    let mut source = effect(LUMETRI, &controls);
    source.arbitrary = data
        .into_iter()
        .map(|(name, bytes)| crate::effects::native::ArbitraryControl {
            match_name: name.to_owned(),
            payload: Ok(bytes),
        })
        .collect();
    source
}

fn rgb_curves(red: &[[f64; 2]]) -> Vec<u8> {
    let identity = vec![[0.0, 0.0], [1.0, 1.0]];
    [identity.clone(), red.to_vec(), identity.clone(), identity]
        .into_iter()
        .flat_map(|points| {
            crate::lumetri::LumetriCurve { kind: 0, points }
                .encode()
                .unwrap()
        })
        .collect()
}

fn wheels(midtones: [f64; 3]) -> Vec<u8> {
    [[0.0, 0.5, 0.0], midtones, [0.0, 0.5, 0.0], [0.0, 0.5, 0.0]]
        .iter()
        .flatten()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

#[test]
fn lumetri_curves_wheels_and_vignette_follow_the_grade_in_lumetri_order() {
    let source = lumetri_with_data(
        &[
            ("ADBE Lumetri-0037", 1.0),
            ("ADBE Lumetri-0046", 1.0),
            ("ADBE Lumetri-0050", 1.0),
            ("ADBE Lumetri-0051", -2.0),
            ("ADBE Lumetri-0052", 40.0),
            ("ADBE Lumetri-0054", 60.0),
        ],
        vec![
            (
                "ADBE Lumetri-0039",
                rgb_curves(&[[0.0, 0.0], [0.5, 0.25], [1.0, 1.0]]),
            ),
            ("ADBE Lumetri-0047", wheels([120.0, 0.5, 0.4])),
            (
                "ADBE Lumetri-0042",
                crate::writer::effects::lumetri_default_payload("ADBE Lumetri-0042").unwrap(),
            ),
        ],
    );
    let lowered = lower(&source, ColorGradeDomain::Legacy).unwrap();
    assert!(matches!(lowered.effect, LayerEffect::PrimaryGrade(_)));
    let [
        LayerEffect::ColorCurves { curves, .. },
        LayerEffect::ColorCurves { curves: wheels, .. },
        LayerEffect::Vignette {
            amount: Some(amount),
            radius: Some(radius),
            feather: Some(feather),
        },
    ] = lowered.after.as_slice()
    else {
        panic!("curves, wheels and vignette expected: {:?}", lowered.after)
    };
    // Red (0,0)(0.5,0.25)(1,1): Lumetri's natural spline darkens mid red.
    assert!(!curves.red.is_identity() && curves.green.is_identity());
    assert!(curves.master.is_identity());
    // A green midtone wheel lifts green.
    assert!(!wheels.green.is_identity() && wheels.master.is_identity());
    let fitted = crate::lumetri::vignette::to_fx([-2.0, 40.0, 60.0]);
    close(*amount, fitted[0]);
    close(*radius, fitted[1]);
    close(*feather, fitted[2]);
    let warned = |text: &str| lowered.warnings.iter().any(|w| w.contains(text));
    assert!(warned("Lumetri RGB Curves map to colorCurves"));
    assert!(warned("Lumetri Color Wheels map to colorCurves"));
    assert!(warned("Lumetri Vignette mapped to FX vignette by a fit"));
    assert!(
        !warned("Hue Saturation Curves"),
        "default payload is not reported"
    );
}

#[test]
fn lumetri_hue_saturation_curves_import_as_hue_curves_after_the_rgb_curves() {
    let curve = |kind: u32, points: &[[f64; 2]]| {
        crate::lumetri::LumetriCurve {
            kind,
            points: points.to_vec(),
        }
        .encode()
        .unwrap()
    };
    let source = lumetri_with_data(
        &[("ADBE Lumetri-0037", 1.0)],
        vec![
            (
                "ADBE Lumetri-0039",
                rgb_curves(&[[0.0, 0.0], [0.5, 0.25], [1.0, 1.0]]),
            ),
            (
                "ADBE Lumetri-0042",
                curve(
                    3,
                    &[
                        [0.0, 0.0],
                        [0.15, 0.0],
                        [0.25, -0.6],
                        [0.35, 0.0],
                        [1.0, 0.0],
                    ],
                ),
            ),
            ("ADBE Lumetri-0110", curve(2, &[[0.0, -0.5], [1.0, 0.0]])),
        ],
    );
    let lowered = lower(&source, ColorGradeDomain::Legacy).unwrap();
    let [LayerEffect::ColorCurves { .. }, LayerEffect::HueCurves(hue)] = lowered.after.as_slice()
    else {
        panic!("RGB curves then hue curves expected: {:?}", lowered.after)
    };
    assert!(hue.hue_vs_saturation.is_some() && hue.luminance_vs_saturation.is_some());
    assert!(hue.hue_vs_hue.is_none() && hue.saturation_vs_saturation.is_none());
    assert!(
        lowered
            .warnings
            .iter()
            .any(|w| w.contains("Lumetri Hue Saturation Curves and FX hueCurves are matched"))
    );
    // The Curves section off: diagnosed, not imported.
    let off = lumetri_with_data(
        &[("ADBE Lumetri-0037", 0.0)],
        vec![("ADBE Lumetri-0110", curve(2, &[[0.0, -0.5], [1.0, 0.0]]))],
    );
    let lowered = lower(&off, ColorGradeDomain::Legacy).unwrap();
    assert!(
        !lowered
            .after
            .iter()
            .any(|effect| matches!(effect, LayerEffect::HueCurves(_)))
    );
    assert!(lowered.warnings.iter().any(|w| {
        w.contains("Hue Saturation Curves not imported: its Lumetri Curves section is off")
    }));
}

#[test]
fn lumetri_built_in_look_is_omitted_by_its_saved_name() {
    let name: Vec<u8> = "SL BIG MINUS BLUE\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let source = lumetri_with_data(
        &[("ADBE Lumetri-0025", 10.0)],
        vec![("ADBE Lumetri-0096", name)],
    );
    let lowered = lower(&source, ColorGradeDomain::Legacy).unwrap();
    assert!(
        lowered.warnings.iter().any(|w| w.contains(
            "Look (ADBE Lumetri-0025): built-in LUT \"SL BIG MINUS BLUE\" (popup 10) omitted"
        )),
        "{:?}",
        lowered.warnings
    );
}

#[test]
fn lumetri_sections_off_or_unmapped_data_are_diagnosed() {
    let mut hue = crate::writer::effects::lumetri_default_payload("ADBE Lumetri-0106").unwrap();
    hue[4] = 2; // two points
    let source = lumetri_with_data(
        &[
            ("ADBE Lumetri-0037", 0.0),
            ("ADBE Lumetri-0050", 0.0),
            ("ADBE Lumetri-0051", 1.0),
            ("ADBE Lumetri-0025", 7.0),
        ],
        vec![
            ("ADBE Lumetri-0039", rgb_curves(&[[0.0, 0.1], [1.0, 1.0]])),
            ("ADBE Lumetri-0106", hue),
        ],
    );
    let lowered = lower(&source, ColorGradeDomain::Legacy).unwrap();
    assert!(lowered.after.is_empty(), "{:?}", lowered.after);
    let warned = |text: &str| lowered.warnings.iter().any(|w| w.contains(text));
    assert!(warned(
        "RGB Curves not imported: its Lumetri Curves section is off"
    ));
    assert!(warned("Vignette Amount 1 not imported"));
    assert!(warned(
        "Hue Saturation Curves not imported: its Lumetri Curves section is off"
    ));
    assert!(warned(
        "Look (ADBE Lumetri-0025): built-in LUT (name not saved) (popup 7) omitted"
    ));
}

#[test]
fn lumetri_creative_saturation_folds_or_carries_its_own_keys() {
    let mut controls = lumetri_controls();
    controls[12].1 = 150.0;
    let lowered = lower(&effect(LUMETRI, &controls), ColorGradeDomain::Legacy).unwrap();
    // Saturation 130% x Creative 150% = 195%: (195 - 100) / 110.
    close(grade(&lowered).saturation, 95.0 / 110.0);
    assert!(
        lowered
            .warnings
            .iter()
            .any(|w| w.contains("folded multiplicatively"))
    );
    controls[9].1 = 100.0;
    let lowered = lower(&effect(LUMETRI, &controls), ColorGradeDomain::Legacy).unwrap();
    close(grade(&lowered).saturation, 50.0 / 110.0);
}

#[test]
fn color_balance_lowers_to_per_channel_curves_with_static_balances() {
    let mut controls = vec![
        ("ADBE Color Balance 2-0001", 20.0),
        ("ADBE Color Balance 2-0002", 0.0),
        ("ADBE Color Balance 2-0003", -10.0),
        ("ADBE Color Balance 2-0004", 0.0),
        ("ADBE Color Balance 2-0005", 35.0),
        ("ADBE Color Balance 2-0006", 0.0),
        ("ADBE Color Balance 2-0007", 0.0),
        ("ADBE Color Balance 2-0008", 0.0),
        ("ADBE Color Balance 2-0009", -100.0),
        ("ADBE Color Balance 2-0010", 0.0),
    ];
    let mut source = effect(COLOR_BALANCE, &controls);
    source.parameters[0].numeric = Ok(scalar(Vec::new(), vec![key(0.0, 20.0), key(1.0, 60.0)]));
    let lowered = lower(&source, ColorGradeDomain::Legacy).unwrap();
    let LayerEffect::ColorCurves { curves, .. } = &lowered.effect else {
        panic!("colorCurves expected")
    };
    // The curves carry the measured transfer of the initial balances.
    let expected = color_balance::Balances {
        shadows: [20.0, 0.0, -10.0],
        midtones: [0.0, 35.0, 0.0],
        highlights: [0.0, 0.0, -100.0],
        preserve_luminosity: false,
    }
    .curves()
    .unwrap();
    assert_eq!(curves, &expected);
    assert!(curves.master.is_identity() && !curves.green.is_identity());
    assert!(lowered.keyed.is_empty());
    assert!(
        lowered
            .warnings
            .iter()
            .any(|w| w.contains("ADBE Color Balance 2-0001: keys omitted"))
    );
    assert!(
        lowered
            .warnings
            .contains(&COLOR_BALANCE_APPROXIMATION.to_owned())
    );

    controls[9].1 = 1.0;
    let lowered = lower(&effect(COLOR_BALANCE, &controls), ColorGradeDomain::Legacy).unwrap();
    let LayerEffect::ColorCurves {
        curves: preserved, ..
    } = &lowered.effect
    else {
        panic!("colorCurves expected")
    };
    assert_ne!(
        preserved, curves,
        "Preserve Luminosity changes the transfer"
    );
}

#[test]
fn channel_mixer_rows_constants_and_monochrome_map_statically() {
    let mut controls = vec![
        ("ADBE CHANNEL MIXER-0001", 0.0),
        ("ADBE CHANNEL MIXER-0002", 100.0),
        ("ADBE CHANNEL MIXER-0003", 0.0),
        ("ADBE CHANNEL MIXER-0004", 150.0),
        ("ADBE CHANNEL MIXER-0005", 100.0),
        ("ADBE CHANNEL MIXER-0006", 0.0),
        ("ADBE CHANNEL MIXER-0007", -50.0),
        ("ADBE CHANNEL MIXER-0008", 0.0),
        ("ADBE CHANNEL MIXER-0009", 0.0),
        ("ADBE CHANNEL MIXER-0010", 0.0),
        ("ADBE CHANNEL MIXER-0011", 200.0),
        ("ADBE CHANNEL MIXER-0012", -20.0),
    ];
    controls.push(("ADBE CHANNEL MIXER-0013", 1.0));
    let mut source = effect(CHANNEL_MIXER, &controls);
    source.parameters[1].numeric = Ok(scalar(Vec::new(), vec![key(0.0, 100.0), key(1.0, 50.0)]));
    let lowered = lower(&source, ColorGradeDomain::Legacy).unwrap();
    let LayerEffect::ChannelMixer(ChannelMixer {
        red,
        green,
        blue,
        monochrome,
        ..
    }) = lowered.effect
    else {
        panic!("channelMixer expected")
    };
    assert_eq!((red.red, red.green, red.constant), (0.0, 1.0, 1.0));
    assert_eq!((green.red, green.blue), (1.0, -0.5));
    assert_eq!((blue.blue, blue.constant), (2.0, -0.2));
    assert!(monochrome);
    assert!(lowered.keyed.is_empty());
    assert!(
        lowered
            .warnings
            .iter()
            .any(|w| w.contains("Red-Const") || w.contains("0004 150"))
    );
    assert!(
        lowered
            .warnings
            .iter()
            .any(|w| w.contains("FX control is static"))
    );
}

#[test]
fn selective_color_groups_and_method_map_statically() {
    let mut controls: Vec<(&str, f64)> = vec![
        (SELECTIVE_COLOR_METHOD, 2.0),
        (SELECTIVE_COLOR_SELECTED_RANGE, 1.0),
        ("ADBE SelectiveColor-0003", 0.0),
    ];
    // Reds.cyan (-0009), Neutrals.black (-0054), Blacks.yellow (-0059).
    controls.extend([
        ("ADBE SelectiveColor-0009", 20.0),
        ("ADBE SelectiveColor-0054", -50.0),
        ("ADBE SelectiveColor-0059", 100.0),
    ]);
    let lowered = lower(
        &effect(SELECTIVE_COLOR, &controls),
        ColorGradeDomain::Legacy,
    )
    .unwrap();
    let LayerEffect::SelectiveColor(selective) = lowered.effect else {
        panic!("selectiveColor expected")
    };
    let SelectiveColor {
        method,
        reds,
        neutrals,
        blacks,
        greens,
        ..
    } = *selective;
    assert_eq!(serde_json::to_value(method).unwrap(), "absolute");
    close(reds.cyan, 0.2);
    close(neutrals.black, -0.5);
    close(blacks.yellow, 1.0);
    assert_eq!(greens.magenta, 0.0);
    assert!(
        lowered
            .warnings
            .iter()
            .any(|w| w.contains("ADBE SelectiveColor-0003: visible slider"))
    );
    assert_eq!(
        SELECTIVE_COLOR_CONTROLS[35].native,
        "ADBE SelectiveColor-0060"
    );
    assert_eq!(SELECTIVE_COLOR_CONTROLS[35].param, "blacks.black");
}

#[test]
fn apply_color_lut_is_lowered_by_its_own_module() {
    assert!(!handles(crate::effects::apply_color_lut::MATCH_NAME) && handles(SELECTIVE_COLOR));
    assert!(!handles("ADBE CurvesCustom") && !handles("ADBE Selective Color"));
}

/// The pinned AE 26.5 colour catalog stores every effect at its defaults; each
/// must import as the neutral FX grade with every labelled identity confirmed.
#[test]
fn native_color_catalog_defaults_import_as_neutral_grades() {
    use crate::structure::{ItemKind, read_project};
    let project = read_project(include_bytes!(
        "../../../tests/fixtures/effects/color_catalog.aep"
    ))
    .expect("independently authored AE colour catalog");
    let mut seen = Vec::new();
    for item in &project.items {
        let ItemKind::Composition(composition) = &item.kind else {
            continue;
        };
        for layer in &composition.layers {
            let (effects, _) = crate::effects::native::read_effects(&layer.content, [320.0, 180.0]);
            for source in effects.iter().filter(|source| handles(&source.match_name)) {
                let lowered = lower(source, ColorGradeDomain::Legacy).unwrap();
                assert!(
                    !lowered.warnings.iter().any(|w| w.contains("declared label")
                        || w.contains("control missing")
                        || w.contains("clamped")),
                    "{}: {:?}",
                    source.match_name,
                    lowered.warnings
                );
                let neutral = match &lowered.effect {
                    LayerEffect::PrimaryGrade(grade) => grade.is_neutral(),
                    LayerEffect::ColorCurves { curves, .. } => curves.is_identity(),
                    LayerEffect::ChannelMixer(mixer) => {
                        serde_json::to_value(mixer).unwrap()
                            == serde_json::json!({"semanticVersion":1,
                                "red":{"red":1.0,"green":0.0,"blue":0.0,"constant":0.0},
                                "green":{"red":0.0,"green":1.0,"blue":0.0,"constant":0.0},
                                "blue":{"red":0.0,"green":0.0,"blue":1.0,"constant":0.0},
                                "monochrome":false})
                    }
                    LayerEffect::SelectiveColor(selective) => serde_json::to_value(selective)
                        .unwrap()
                        .as_object()
                        .unwrap()
                        .iter()
                        .all(|(key, value)| {
                            !value.is_object()
                                || value.as_object().unwrap().values().all(|v| v == 0.0)
                                || key == "method"
                        }),
                    other => panic!("unexpected {other:?}"),
                };
                assert!(neutral, "{}: {:?}", source.match_name, lowered.effect);
                seen.push(source.match_name.clone());
            }
        }
    }
    seen.sort();
    seen.dedup();
    assert_eq!(
        seen,
        [CHANNEL_MIXER, COLOR_BALANCE, LUMETRI, SELECTIVE_COLOR],
        "catalog must exercise every implemented colour importer"
    );
}

#[test]
fn fx_ranges_map_inside_the_declared_native_valid_ranges_except_fitted_lumetri_rows() {
    let rows = LUMETRI_BASIC
        .iter()
        .chain([&LUMETRI_VIBRANCE])
        .chain(&COLOR_BALANCE_CONTROLS)
        .chain(&CHANNEL_MIXER_CONTROLS)
        .chain(&SELECTIVE_COLOR_CONTROLS);
    let mut outside = Vec::new();
    for control in rows {
        let effect = control.native.rsplit_once('-').unwrap().0;
        let (min, max) = crate::effects::definitions::definition(effect)
            .unwrap()
            .parameters
            .iter()
            .find(|parameter| parameter.match_name == control.native)
            .and_then(crate::effects::definitions::ParameterDefinition::valid_range)
            .unwrap_or_else(|| panic!("{}: no declared valid range", control.native));
        let ends = [control.min, control.max].map(|fx| (fx - control.offset) / control.scale);
        if ends
            .iter()
            .any(|native| !(min - 1e-9..=max + 1e-9).contains(native))
        {
            outside.push((control.native, ends, (min, max)));
        }
    }
    // The fitted Lumetri scales map FX ranges beyond the declared native
    // ranges (Exposure +/-5 x2.6, Contrast +/-2 x360, Highlights x240,
    // Shadows x510, Saturation 100+110s below 0, Vibrance x167); export
    // clamps those values with a diagnostic.
    assert_eq!(
        outside.iter().map(|(name, ..)| *name).collect::<Vec<_>>(),
        [
            "ADBE Lumetri-0011",
            "ADBE Lumetri-0012",
            "ADBE Lumetri-0013",
            "ADBE Lumetri-0014",
            "ADBE Lumetri-0020",
            "ADBE Lumetri-0030",
        ],
        "{outside:?}"
    );
}

fn utf16_path(path: &str) -> Vec<u8> {
    path.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// An `Embedded LUTs` entry `id` holding a 2³ `ITX3` table scaled by `gain`.
fn embedded_entry(id: u8, gain: f32) -> Vec<u8> {
    let mut table = b"ITX3".to_vec();
    table.extend(2u32.to_le_bytes());
    table.extend(3u32.to_le_bytes());
    table.extend([0; 16]);
    for _ in 0..3 {
        table.extend(1f32.to_le_bytes());
    }
    for index in 0..8u32 {
        for bit in [index & 1, (index >> 1) & 1, index >> 2] {
            table.extend((bit as f32 * gain).to_le_bytes());
        }
    }
    let mut entry = vec![id; 16];
    entry.extend((table.len() as u32).to_le_bytes());
    entry.extend(table);
    entry
}

fn browsed_lumetri(scalars: &[(&str, f64)], with_copies: bool) -> DecodedEffect {
    let blob = r#"<Lumetri><keyframe>
      <shader><name>"LUT"</name><parameters><LUT>"/a/in.cube"</LUT></parameters></shader>
      <shader><name>"BasicCorrection3"</name></shader>
      <shader><name>"__GroupBegin"</name><parameters><__Preset>"/a/look.cube"</__Preset></parameters></shader>
      <shader><name>"LUT"</name><parameters><LookColorSpace>"x"</LookColorSpace><LUT>"/a/look.cube"</LUT></parameters></shader>
      <shader><name>"__GroupEnd"</name><parameters><__Intensity>"60"</__Intensity></parameters></shader>
    </keyframe>
    <embeddedlut><hash>01010101010101010101010101010101</hash><filename>/a/in.cube</filename></embeddedlut>
    <embeddedlut><hash>02020202020202020202020202020202</hash><filename>/a/look.cube</filename></embeddedlut>
    </Lumetri>"#;
    let mut controls = vec![
        ("ADBE Lumetri-0005", 2.0),
        ("ADBE Lumetri-0025", 2.0),
        ("ADBE Lumetri-0026", 60.0),
        ("ADBE Lumetri-0130", 29.0),
    ];
    controls.extend_from_slice(scalars);
    let mut data = vec![
        ("ADBE Lumetri-0001", blob.as_bytes().to_vec()),
        ("ADBE Lumetri-0004", utf16_path("/a/in.cube")),
        ("ADBE Lumetri-0024", utf16_path("/a/look.cube")),
    ];
    if with_copies {
        data.push((
            "ADBE Lumetri-0098",
            [embedded_entry(1, 0.5), embedded_entry(2, 0.25)].concat(),
        ));
    }
    lumetri_with_data(&controls, data)
}

fn look(effect: &LayerEffect) -> (String, f64) {
    let LayerEffect::LookTransform {
        transform: Some(fx_schema::ColorTransform::Lut3d { asset_id, .. }),
        mix,
        ..
    } = effect
    else {
        panic!("lookTransform expected: {effect:?}")
    };
    (asset_id.as_str().to_owned(), mix.get())
}

#[test]
fn lumetri_browsed_luts_import_as_look_transforms_around_the_grade() {
    let lowered = lower(&browsed_lumetri(&[], true), ColorGradeDomain::Legacy).unwrap();
    // Input LUT, Basic/Creative grade, Look at Intensity/100.
    let [input] = lowered.before.as_slice() else {
        panic!("one Input LUT expected: {:?}", lowered.before)
    };
    assert!(matches!(lowered.effect, LayerEffect::PrimaryGrade(_)));
    let [look_effect] = lowered.after.as_slice() else {
        panic!("one Look expected: {:?}", lowered.after)
    };
    let (input_asset, input_mix) = look(input);
    let (look_asset, look_mix) = look(look_effect);
    assert_eq!(input_mix, 1.0);
    assert!((look_mix - 0.6).abs() < 1e-12);
    assert!(input_asset.starts_with("aep-lut-") && input_asset != look_asset);
    let [Some(Ok(input)), Some(Ok(look))] = lumetri_luts::browsed(&browsed_lumetri(&[], true))
    else {
        panic!("both LUTs expected")
    };
    assert_eq!(input.lut.file_name, "in.cube");
    assert_eq!(input.lut.lut.samples[7], [0.5; 3]);
    assert_eq!(look.lut.lut.samples[1], [0.25, 0.0, 0.0]);
    assert!(
        lowered
            .warnings
            .iter()
            .any(|warning| warning.contains("apply after the Look in Lumetri")),
        "{:?}",
        lowered.warnings
    );
    assert!(
        !lowered
            .warnings
            .iter()
            .any(|warning| warning.contains("not imported") || warning.contains("popup")),
        "{:?}",
        lowered.warnings
    );
}

#[test]
fn lumetri_browsed_luts_without_a_copy_or_in_an_off_section_are_reported() {
    let lowered = lower(&browsed_lumetri(&[], false), ColorGradeDomain::Legacy).unwrap();
    assert!(lowered.before.is_empty() && lowered.after.is_empty());
    for slot in [
        "Input LUT (ADBE Lumetri-0004)",
        "Creative Look (ADBE Lumetri-0024)",
    ] {
        assert!(
            lowered
                .warnings
                .iter()
                .any(|warning| warning.starts_with(&format!(
                    "{slot} not imported: After Effects saved no readable embedded copy"
                ))),
            "{slot}: {:?}",
            lowered.warnings
        );
    }
    let mut source = browsed_lumetri(&[], true);
    for parameter in &mut source.parameters {
        if parameter.match_name == "ADBE Lumetri-0023" {
            parameter.numeric = Ok(scalar(vec![0.0], Vec::new()));
        }
    }
    let off = lower(&source, ColorGradeDomain::Legacy).unwrap();
    assert_eq!(off.before.len(), 1);
    assert!(
        off.warnings.iter().any(|warning| warning
            == "Creative Look (ADBE Lumetri-0024) not imported: its Creative section is off"),
        "{:?}",
        off.warnings
    );
    // A saved popup that selects a built-in LUT beside the browsed file.
    let mut source = browsed_lumetri(&[], true);
    for parameter in &mut source.parameters {
        if parameter.match_name == "ADBE Lumetri-0025" {
            parameter.numeric = Ok(scalar(vec![7.0], Vec::new()));
        }
    }
    let conflict = lower(&source, ColorGradeDomain::Legacy).unwrap();
    assert!(conflict.after.is_empty());
    assert!(
        conflict.warnings.iter().any(|warning| warning.starts_with(
            "Creative Look (ADBE Lumetri-0024) not imported: popup 7 (ADBE Lumetri-0025) selects no browsed file"
        )),
        "{:?}",
        conflict.warnings
    );
}
