//! Edited canonical grades -> fresh native colour effects -> own reader ->
//! re-import. Own-reader structural proof only; the Adobe acceptance of the
//! Lumetri and Color Balance exports is recorded in the support ledger.
use super::*;
use crate::effects::{
    color_grades,
    native::{DecodedEffect, read_effects},
};

fn grade_input(effects: Value) -> Value {
    let mut document = imported();
    let mut leaf = rect(&document, 900);
    leaf["rect"]["size"] = json!([120.0, 80.0]);
    leaf["effects"] = effects;
    document["composition"]["layers"] = json!([leaf]);
    document
}

fn fresh(output: &ExportedDocument) -> Vec<DecodedEffect> {
    let project = read_project(&output.bytes).expect("fresh native AEP parse");
    let layer = layers(&project).first().expect("retained edited rectangle");
    read_effects(&layer.content, [120.0, 80.0]).0
}

fn value(effect: &DecodedEffect, name: &str) -> f64 {
    let numeric = effect
        .parameters
        .iter()
        .find(|parameter| parameter.match_name == name)
        .unwrap_or_else(|| panic!("{name} missing from {}", effect.match_name))
        .numeric
        .as_ref()
        .unwrap_or_else(|error| panic!("{name}: {error}"));
    numeric
        .keyframes
        .first()
        .map_or(numeric.values[0], |key| key.values[0])
}

fn warned(output: &ExportedDocument, text: &str) -> bool {
    output
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains(text))
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}

const GRADE: &str = r#"{"type":"primaryGrade","semanticVersion":1,"temperature":0.5,"tint":-0.2,"exposure":0.5,"contrast":0.25,"highlights":-0.4,"shadows":0.2,"whites":0.1,"blacks":-0.2,"saturation":0.3,"vibrance":0.5}"#;

#[test]
fn hue_curves_export_as_lumetri_hue_saturation_curves_that_reimport() {
    let input = grade_input(json!([
        {"id":9101,"enabled":true,"effect":{"type":"hueCurves","semanticVersion":1,
            "hueVsSaturation":[{"x":0.0,"y":0.5},{"x":0.3,"y":0.2},{"x":0.6,"y":0.5},{"x":1.0,"y":0.5}],
            "hueVsHue":[{"x":0.0,"y":0.55},{"x":1.0,"y":0.55}]}}
    ]));
    let output = export(input);
    let effects = fresh(&output);
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].match_name, color_grades::LUMETRI);
    assert!(warned(
        &output,
        "Hue Saturation Curves (Hue vs Sat, Hue vs Hue"
    ));
    let reimported = to_structural_fx_document(&read_project(&output.bytes).unwrap(), Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    fn find_hue_curves(layers: &Value) -> Option<Value> {
        layers.as_array()?.iter().find_map(|layer| {
            layer["effects"]
                .as_array()
                .and_then(|effects| {
                    effects
                        .iter()
                        .find(|record| record["effect"]["type"] == "hueCurves")
                        .map(|record| record["effect"].clone())
                })
                .or_else(|| find_hue_curves(&layer["layers"]))
        })
    }
    let effect = find_hue_curves(&reimported["composition"]["layers"])
        .unwrap_or_else(|| panic!("no hueCurves reimported: {reimported}"));
    let points = effect["hueVsSaturation"].as_array().unwrap();
    let at = |x: f64| {
        points
            .windows(2)
            .find(|pair| pair[0]["x"].as_f64().unwrap() <= x && x <= pair[1]["x"].as_f64().unwrap())
            .map(|pair| {
                let [x0, y0, x1, y1] = [&pair[0]["x"], &pair[0]["y"], &pair[1]["x"], &pair[1]["y"]]
                    .map(|value| value.as_f64().unwrap());
                y0 + (x - x0) / (x1 - x0) * (y1 - y0)
            })
            .unwrap()
    };
    assert!(
        (at(0.3) - 0.2).abs() < 0.03 && (at(0.8) - 0.5).abs() < 0.03,
        "{points:?}"
    );
    assert!(effect["hueVsHue"].as_array().is_some_and(|points| {
        points
            .iter()
            .all(|point| (point["y"].as_f64().unwrap() - 0.55).abs() < 0.03)
    }));
}

#[test]
fn vignette_exports_as_the_lumetri_vignette_section_that_reimports_editably() {
    let input = grade_input(json!([
        {"id":9101,"enabled":true,"effect":{"type":"vignette","amount":0.444,"radius":0.716,"feather":0.385}},
        {"id":9102,"enabled":true,"effect":{"type":"vignette"}}
    ]));
    let output = export(input.clone());
    let effects = fresh(&output);
    assert_eq!(
        effects
            .iter()
            .map(|effect| effect.match_name.as_str())
            .collect::<Vec<_>>(),
        [color_grades::LUMETRI, color_grades::LUMETRI]
    );
    // The fitted inverse of the import; absent fields take the FX renderer
    // defaults 0.5 / 0.75 / 0.35.
    for (effect, fx) in effects
        .iter()
        .zip([[0.444, 0.716, 0.385], [0.5, 0.75, 0.35]])
    {
        let native = crate::lumetri::vignette::from_fx(fx);
        for (control, want) in [
            "ADBE Lumetri-0051",
            "ADBE Lumetri-0052",
            "ADBE Lumetri-0054",
        ]
        .into_iter()
        .zip(native)
        {
            assert!((value(effect, control) - want).abs() < 1e-6, "{control}");
        }
    }
    assert!(warned(&output, "Vignette section carries it"));
    // Re-import yields the vignette alone (the neutral grade is dropped).
    let reimported = to_structural_fx_document(&read_project(&output.bytes).unwrap(), Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    fn collect_effects(layers: &Value, found: &mut Vec<Value>) {
        for layer in layers.as_array().into_iter().flatten() {
            for record in layer["effects"].as_array().into_iter().flatten() {
                found.push(record["effect"].clone());
            }
            collect_effects(&layer["layers"], found);
        }
    }
    let mut found = Vec::new();
    collect_effects(&reimported["composition"]["layers"], &mut found);
    let types: Vec<_> = found.iter().map(|effect| effect["type"].clone()).collect();
    assert_eq!(
        types,
        [json!("vignette"), json!("vignette")],
        "{reimported}"
    );
    for (field, want) in ["amount", "radius", "feather"]
        .into_iter()
        .zip([0.444, 0.716, 0.385])
    {
        let got = found[0][field].as_f64().unwrap();
        assert!((got - want).abs() < 0.01, "{field}: {got} != {want}");
    }
}

#[test]
fn color_grade_export_writes_native_controls_that_reimport_editably() {
    let row = |r: f64, g: f64, b: f64, c: f64| json!({"red":r,"green":g,"blue":b,"constant":c});
    let zero = json!({"cyan":0,"magenta":0,"yellow":0,"black":0});
    let mut selective = json!({"type":"selectiveColor","semanticVersion":1,"method":"absolute"});
    for range in [
        "reds", "yellows", "greens", "cyans", "blues", "magentas", "whites", "neutrals", "blacks",
    ] {
        selective[range] = zero.clone();
    }
    selective["reds"]["cyan"] = json!(0.2);
    selective["neutrals"]["black"] = json!(-0.5);
    let input = grade_input(json!([
        {"id":9001,"enabled":true,"effect":serde_json::from_str::<Value>(GRADE).unwrap()},
        {"id":9002,"enabled":true,"effect":{"type":"tonalColor","semanticVersion":1,"globalRed":0.1,"globalGreen":0,"globalBlue":0,"shadowsRed":0.2,"shadowsGreen":0,"shadowsBlue":0,"midtonesRed":0,"midtonesGreen":0.35,"midtonesBlue":0,"highlightsRed":0.95,"highlightsGreen":0,"highlightsBlue":-1}},
        {"id":9003,"enabled":true,"effect":{"type":"channelMixer","semanticVersion":1,"red":row(0.,1.,0.,0.5),"green":row(1.,0.,-0.5,0.),"blue":row(0.,0.,2.,-0.2),"monochrome":true}},
        {"id":9004,"enabled":true,"effect":selective}
    ]));
    let output = export(input);
    let effects = fresh(&output);
    assert_eq!(
        effects
            .iter()
            .map(|effect| effect.match_name.as_str())
            .collect::<Vec<_>>(),
        [
            color_grades::LUMETRI,
            color_grades::COLOR_BALANCE,
            color_grades::CHANNEL_MIXER,
            color_grades::SELECTIVE_COLOR
        ]
    );
    let lumetri = &effects[0];
    // Inverse fitted scales: Temperature x116, Tint x-100, Exposure x2.6,
    // Contrast x360, Shadows x510, Saturation 100+110s, Vibrance x167.
    close(value(lumetri, "ADBE Lumetri-0007"), 58.0);
    close(value(lumetri, "ADBE Lumetri-0008"), 20.0);
    close(value(lumetri, "ADBE Lumetri-0011"), 1.3);
    close(value(lumetri, "ADBE Lumetri-0012"), 90.0);
    close(value(lumetri, "ADBE Lumetri-0014"), 102.0);
    close(value(lumetri, "ADBE Lumetri-0020"), 133.0);
    close(value(lumetri, "ADBE Lumetri-0030"), 83.5);
    close(value(lumetri, "ADBE Lumetri-0031"), 100.0);
    let balance = &effects[1];
    close(value(balance, "ADBE Color Balance 2-0001"), 30.0);
    close(value(balance, "ADBE Color Balance 2-0004"), 10.0);
    close(value(balance, "ADBE Color Balance 2-0005"), 35.0);
    close(value(balance, "ADBE Color Balance 2-0007"), 100.0);
    close(value(balance, "ADBE Color Balance 2-0009"), -100.0);
    assert_eq!(value(balance, "ADBE Color Balance 2-0010"), 1.0);
    let mixer = &effects[2];
    close(value(mixer, "ADBE CHANNEL MIXER-0002"), 100.0);
    close(value(mixer, "ADBE CHANNEL MIXER-0004"), 50.0);
    close(value(mixer, "ADBE CHANNEL MIXER-0011"), 200.0);
    assert_eq!(value(mixer, "ADBE CHANNEL MIXER-0013"), 1.0);
    let ink = &effects[3];
    assert_eq!(value(ink, "ADBE SelectiveColor-0001"), 2.0);
    close(value(ink, "ADBE SelectiveColor-0009"), 20.0);
    close(value(ink, "ADBE SelectiveColor-0003"), 20.0);
    close(value(ink, "ADBE SelectiveColor-0054"), -50.0);
    assert!(warned(&output, "global offsets folded"));
    assert!(warned(&output, "global plus range balance exceeds"));
    assert!(warned(&output, "native default payloads"));

    // Fresh native controls re-import to the edited canonical values.
    let grade = color_grades::lower(lumetri, color_grades::ColorGradeDomain::Legacy)
        .unwrap()
        .effect;
    let expected: fx_schema::LayerEffect = serde_json::from_str(GRADE).unwrap();
    let (
        fx_schema::LayerEffect::PrimaryGrade(actual),
        fx_schema::LayerEffect::PrimaryGrade(expected),
    ) = (grade, expected)
    else {
        panic!("primaryGrade round trip")
    };
    for (actual, expected) in serde_json::to_value(actual)
        .unwrap()
        .as_object()
        .unwrap()
        .values()
        .zip(
            serde_json::to_value(expected)
                .unwrap()
                .as_object()
                .unwrap()
                .values(),
        )
    {
        close(actual.as_f64().unwrap(), expected.as_f64().unwrap());
    }
    let mixer = serde_json::to_value(
        color_grades::lower(mixer, color_grades::ColorGradeDomain::Legacy)
            .unwrap()
            .effect,
    )
    .unwrap();
    assert_eq!(mixer["blue"]["blue"], 2.0);
    assert_eq!(mixer["monochrome"], true);
    let ink = serde_json::to_value(
        color_grades::lower(ink, color_grades::ColorGradeDomain::Legacy)
            .unwrap()
            .effect,
    )
    .unwrap();
    assert_eq!(ink["method"], "absolute");
    assert_eq!(ink["neutrals"]["black"], -0.5);
}

#[test]
fn primary_grade_keys_export_as_native_lumetri_keys() {
    let mut input = grade_input(json!([
        {"id":9001,"enabled":true,"effect":serde_json::from_str::<Value>(GRADE).unwrap()}
    ]));
    input["composition"]["dynamics"]["entries"] = json!([{
        "target":{"kind":"effectProperty","effectId":9001,"paramName":"exposure"},
        "animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"exposure-a","layerTime":0,"value":{"type":"float","value":0.0},"easing":{"type":"linear"}},
            {"id":"exposure-b","layerTime":1000,"value":{"type":"float","value":1.0},"easing":{"type":"linear"}}
        ]}
    }]);
    let output = export(input);
    let effects = fresh(&output);
    let exposure = effects[0]
        .parameters
        .iter()
        .find(|parameter| parameter.match_name == "ADBE Lumetri-0011")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    assert!(exposure.animated, "{:?}", output.diagnostics);
    let keys: Vec<_> = exposure.keyframes.iter().map(|key| key.values[0]).collect();
    assert_eq!(keys.len(), 2);
    close(keys[0], 0.0);
    close(keys[1], 2.6);
}

#[test]
fn primary_grade_exposure_beyond_lumetri_valid_range_is_clamped_with_a_diagnostic() {
    // FX exposure 4 stops x2.6 = 10.4, outside Lumetri Exposure's declared
    // valid range -7..=7 (definitions.json); every other control fits.
    let mut grade: Value = serde_json::from_str(GRADE).unwrap();
    grade["exposure"] = json!(4.0);
    let output = export(grade_input(json!([
        {"id":9001,"enabled":true,"effect":grade}
    ])));
    let effects = fresh(&output);
    close(value(&effects[0], "ADBE Lumetri-0011"), 7.0);
    close(value(&effects[0], "ADBE Lumetri-0007"), 58.0);
    assert!(
        warned(
            &output,
            "Exposure (ADBE Lumetri-0011) 10.4 (FX exposure 4) is outside AE's declared valid range -7..=7; written as 7"
        ),
        "{:?}",
        output.diagnostics
    );
    let clamp_warnings = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.message.contains("declared valid range"))
        .count();
    assert_eq!(clamp_warnings, 1, "{:?}", output.diagnostics);
    // Re-import reads the clamped native value back as 7/2.6 stops.
    let fx_schema::LayerEffect::PrimaryGrade(reimported) =
        color_grades::lower(&effects[0], color_grades::ColorGradeDomain::Legacy)
            .unwrap()
            .effect
    else {
        panic!("primaryGrade expected");
    };
    close(reimported.exposure, 7.0 / 2.6);
}

#[test]
fn primary_grade_exposure_keys_beyond_lumetri_valid_range_are_clamped_with_a_diagnostic() {
    let mut input = grade_input(json!([
        {"id":9001,"enabled":true,"effect":serde_json::from_str::<Value>(GRADE).unwrap()}
    ]));
    input["composition"]["dynamics"]["entries"] = json!([{
        "target":{"kind":"effectProperty","effectId":9001,"paramName":"exposure"},
        "animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"exposure-a","layerTime":0,"value":{"type":"float","value":-5.0},"easing":{"type":"linear"}},
            {"id":"exposure-b","layerTime":500,"value":{"type":"float","value":1.0},"easing":{"type":"linear"}},
            {"id":"exposure-c","layerTime":1000,"value":{"type":"float","value":5.0},"easing":{"type":"linear"}}
        ]}
    }]);
    let output = export(input);
    let effects = fresh(&output);
    let exposure = effects[0]
        .parameters
        .iter()
        .find(|parameter| parameter.match_name == "ADBE Lumetri-0011")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    assert!(exposure.animated, "{:?}", output.diagnostics);
    let keys: Vec<_> = exposure.keyframes.iter().map(|key| key.values[0]).collect();
    assert_eq!(keys.len(), 3);
    close(keys[0], -7.0);
    close(keys[1], 2.6);
    close(keys[2], 7.0);
    assert!(
        warned(&output, "Exposure (ADBE Lumetri-0011) keys [")
            && warned(&output, "are outside AE's declared valid range -7..=7"),
        "{:?}",
        output.diagnostics
    );
}

/// In an FX ACES document the Lumetri Exposure is written by the ACES
/// stops/2.2 scale (static value, keys and the valid-range clamp) and reads
/// back by the same scale; the other controls keep their fitted scales.
#[test]
fn aces_primary_grade_exports_lumetri_exposure_by_the_aces_scale() {
    let mut static_input = grade_input(json!([
        {"id":9001,"enabled":true,"effect":serde_json::from_str::<Value>(GRADE).unwrap()}
    ]));
    static_input["colorManagement"] =
        json!({"workingSpace":"acescct","outputTransform":"aces2.0","compositing":"wholeProgram"});
    let mut keyed_input = static_input.clone();
    let output = export(static_input);
    let effects = fresh(&output);
    close(value(&effects[0], "ADBE Lumetri-0011"), 1.1);
    close(value(&effects[0], "ADBE Lumetri-0007"), 58.0);
    assert!(
        warned(&output, "Exposure x2.2 (ACES)"),
        "{:?}",
        output.diagnostics
    );
    let fx_schema::LayerEffect::PrimaryGrade(reimported) =
        color_grades::lower(&effects[0], color_grades::ColorGradeDomain::Aces)
            .unwrap()
            .effect
    else {
        panic!("primaryGrade expected");
    };
    close(reimported.exposure, 0.5);

    keyed_input["composition"]["dynamics"]["entries"] = json!([{
        "target":{"kind":"effectProperty","effectId":9001,"paramName":"exposure"},
        "animator":{"type":"keyframes","enabled":true,"keyframes":[
            {"id":"exposure-a","layerTime":0,"value":{"type":"float","value":1.0},"easing":{"type":"linear"}},
            {"id":"exposure-b","layerTime":1000,"value":{"type":"float","value":4.0},"easing":{"type":"linear"}}
        ]}
    }]);
    let output = export(keyed_input);
    let effects = fresh(&output);
    let exposure = effects[0]
        .parameters
        .iter()
        .find(|parameter| parameter.match_name == "ADBE Lumetri-0011")
        .unwrap()
        .numeric
        .as_ref()
        .unwrap();
    let keys: Vec<_> = exposure.keyframes.iter().map(|key| key.values[0]).collect();
    assert_eq!(keys.len(), 2, "{:?}", output.diagnostics);
    close(keys[0], 2.2);
    close(keys[1], 7.0);
    assert!(
        warned(&output, "Exposure (ADBE Lumetri-0011) keys [8.8")
            && warned(&output, "(FX exposure [4.0"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn unexportable_grades_and_compositing_options_are_diagnosed() {
    let input = grade_input(json!([
        {"id":9002,"enabled":true,"effect":{"type":"lookTransform","semanticVersion":1,"transform":null,"mix":1}},
        {"id":9003,"enabled":true,"effect":serde_json::from_str::<Value>(GRADE).unwrap(),"compositingOptions":{"maskReferences":[],"effectOpacity":0.25}}
    ]));
    let output = export(input);
    let effects = fresh(&output);
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].match_name, color_grades::LUMETRI);
    assert!(warned(
        &output,
        "lookTransform without a transform is a bypass"
    ));
    // The grade's Effect Opacity is now written natively (25%).
    assert!(
        !warned(&output, "compositingOptions"),
        "{:?}",
        output.diagnostics
    );
    let compositing = effects[0].compositing.as_ref().unwrap();
    close(compositing.opacity.as_ref().unwrap().values[0], 25.0);
    assert!(compositing.mask_references.is_empty());
}

/// Rect owner with one effect-only FX mask (mode none, left half) and the
/// given effect records.
fn masked_input(effects: Value) -> Value {
    let mut input = grade_input(effects);
    input["composition"]["layers"][0]["masks"] = json!([{
        "id":9101,"mode":"none","path":{"commands":[
            {"type":"moveTo","x":0.0,"y":0.0},
            {"type":"lineTo","x":60.0,"y":0.0},
            {"type":"lineTo","x":60.0,"y":80.0},
            {"type":"lineTo","x":0.0,"y":80.0},
            {"type":"close"}
        ]}
    }]);
    input
}

#[test]
fn compositing_options_export_native_mask_reference_and_opacity_that_reimport() {
    // Levels Gamma 0.5 restricted to FX Mask 1 (mode none) at Effect Opacity
    // 50. After Effects 26.5 renders a reference to a mode-None mask as no
    // restriction and restricts the effect to a referenced mode-Add mask
    // without masking the layer (measured), so the native mask is written in
    // mode Add.
    let levels = json!({"type":"levels","inputBlack":0,"inputWhite":255,"gamma":0.5,"outputBlack":0,"outputWhite":255});
    let output = export(masked_input(json!([
        {"id":9001,"enabled":true,"effect":levels,
         "compositingOptions":{"maskReferences":[9101],"effectOpacity":0.5}}
    ])));
    assert!(
        !warned(&output, "compositingOptions"),
        "{:?}",
        output.diagnostics
    );
    // Native records: Effect Opacity 50% and one Mask Reference naming the
    // exported Mask 1 by its native identity.
    let effects = fresh(&output);
    assert_eq!(effects.len(), 1, "{:?}", output.diagnostics);
    assert_eq!(effects[0].match_name, "ADBE Pro Levels2");
    let compositing = effects[0].compositing.as_ref().unwrap();
    close(compositing.opacity.as_ref().unwrap().values[0], 50.0);
    assert_eq!(
        compositing.mask_references,
        [Ok(crate::writer::native_mask_identity(0).unwrap())]
    );
    let mkif = output
        .bytes
        .windows(4)
        .position(|window| window == b"mkif")
        .expect("one native mask");
    // `mkif` payload bytes 6..8 hold the mask mode ordinal (1 = Add).
    assert_eq!(output.bytes[mkif + 8 + 6..mkif + 8 + 8], [0, 1]);
    // Fresh import restores the same mask reference, mask mode and opacity.
    let project = read_project(&output.bytes).unwrap();
    let value = to_structural_fx_document(&project, Some(1))
        .unwrap()
        .document
        .to_json_value()
        .unwrap();
    fn owners(value: &Value, found: &mut Vec<Value>) {
        for layer in value["layers"].as_array().into_iter().flatten() {
            if layer["effects"]
                .as_array()
                .is_some_and(|effects| effects.iter().any(|r| r["effect"]["type"] == "levels"))
            {
                found.push(layer.clone());
            }
            owners(layer, found);
        }
    }
    let mut found = Vec::new();
    owners(&value["composition"], &mut found);
    assert_eq!(found.len(), 1, "{value}");
    let owner = &found[0];
    let record = owner["effects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["effect"]["type"] == "levels")
        .unwrap();
    let options = &record["compositingOptions"];
    close(options["effectOpacity"].as_f64().unwrap(), 0.5);
    let masks = owner["masks"].as_array().unwrap();
    assert_eq!(masks.len(), 1, "{owner}");
    assert_eq!(masks[0]["mode"], "none");
    assert_eq!(options["maskReferences"], json!([masks[0]["id"].clone()]));
}

#[test]
fn compositing_options_without_a_single_native_mask_are_diagnosed() {
    // A two-instance lowering (Temperature/Tint) cannot carry one blend or
    // restriction: both native effects keep default Compositing Options.
    let output = export(masked_input(json!([
        {"id":9001,"enabled":true,"effect":{"type":"temperatureTint","temperature":0.3,"tint":0.0},
         "compositingOptions":{"maskReferences":[9101],"effectOpacity":0.5}}
    ])));
    assert!(
        warned(&output, "lowers to 2 native effects"),
        "{:?}",
        output.diagnostics
    );
    for effect in fresh(&output) {
        assert_eq!(
            effect.compositing,
            Ok(Default::default()),
            "{}",
            effect.match_name
        );
    }
}

#[test]
fn color_curves_export_writes_native_curves_tables_that_reimport_editably() {
    use fx_schema::{ColorCurve, ColorCurvePoint, ColorCurves, ColorCurvesSemanticVersion};
    let curve = |points: &[(f64, f64)]| {
        ColorCurve::new(
            points
                .iter()
                .map(|(x, y)| ColorCurvePoint::new(*x, *y))
                .collect(),
        )
        .unwrap()
    };
    let edited = ColorCurves {
        master: ColorCurve::identity(),
        red: curve(&[(0.0, 0.0), (0.5, 0.7), (1.0, 1.0)]),
        green: ColorCurve::identity(),
        blue: curve(&[(0.0, 0.2), (1.0, 0.8)]),
    };
    let effect = serde_json::to_value(fx_schema::LayerEffect::ColorCurves {
        semantic_version: ColorCurvesSemanticVersion::V1,
        curves: edited.clone(),
    })
    .unwrap();
    let output = export(grade_input(json!([
        {"id":9001,"enabled":true,"effect":effect}
    ])));
    let effects = fresh(&output);
    assert_eq!(
        effects
            .iter()
            .map(|effect| effect.match_name.as_str())
            .collect::<Vec<_>>(),
        [crate::effects::curves::MATCH_NAME],
        "{:?}",
        output.diagnostics
    );
    assert!(warned(
        &output,
        "Curves tables sampled from the piecewise-linear FX curves"
    ));
    assert!(!warned(&output, "colorCurves: native Curves tables"));
    // The FX knots become AE editor points so AE's curve panel shows them.
    assert!(warned(
        &output,
        "Curves Red: the 3 FX knots are written as AE editor points (0,0) (128,179) (255,255)"
    ));
    assert!(warned(
        &output,
        "Curves Blue: the 2 FX knots are written as AE editor points (0,51) (255,204)"
    ));
    // The fresh native table re-imports through the existing Curves reader.
    let project = read_project(&output.bytes).unwrap();
    let layer = layers(&project).first().unwrap().clone();
    let (fx_schema::LayerEffect::ColorCurves { curves, .. }, _) =
        crate::effects::curves::lower(&layer.content, 1).unwrap()
    else {
        panic!("colorCurves round trip")
    };
    assert!(curves.master.is_identity() && curves.green.is_identity());
    let at = |curve: &ColorCurve, x: f64| {
        let pair = curve
            .points()
            .windows(2)
            .find(|pair| x <= pair[1].x())
            .unwrap();
        pair[0].y() + (x - pair[0].x()) / (pair[1].x() - pair[0].x()) * (pair[1].y() - pair[0].y())
    };
    for code in 0..=255 {
        let x = f64::from(code) / 255.0;
        assert!((at(&curves.red, x) - at(&edited.red, x)).abs() * 255.0 <= 1.5);
        assert!((at(&curves.blue, x) - at(&edited.blue, x)).abs() * 255.0 <= 1.5);
    }
}
