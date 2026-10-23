//! Synthetic Effect Parade records on the Adobe-authored catalog owner. They
//! exercise the import path only; no native colour-effect oracle is claimed.
use super::*;
use crate::effects::definitions::{ParameterDefinition, encode_parameter};

fn named(name: &str) -> Chunk {
    let mut bytes = name.as_bytes().to_vec();
    bytes.resize(40, 0);
    Chunk::data(*b"tdmn", bytes).unwrap()
}

fn scalar(value: f64) -> Chunk {
    let mut meta = vec![0; 124];
    meta[..4].copy_from_slice(&[0xdb, 0x99, 0, 1]);
    Chunk::list(
        *b"tdbs",
        vec![
            Chunk::data(*b"tdsb", [0, 0, 0, 1]).unwrap(),
            Chunk::data(*b"tdb4", meta).unwrap(),
            Chunk::data(*b"cdat", value.to_be_bytes()).unwrap(),
        ],
    )
}

/// A float-slider declaration carrying only an identity and UI label.
fn declaration(match_name: &str, label: &str) -> [Chunk; 2] {
    let parameter = ParameterDefinition {
        match_name: match_name.into(),
        label: label.into(),
        header_flags: [0; 3],
        kind: 10,
        reserved: [0; 2],
        payload_words: vec![0; 23],
        popup: None,
        defaults: Vec::new(),
        point_relative: false,
    };
    [named(match_name), encode_parameter(&parameter).unwrap()]
}

fn instance(
    match_name: &str,
    declarations: &[(&str, &str)],
    controls: &[(&str, f64)],
    options: Vec<Chunk>,
) -> Vec<Chunk> {
    let mut body = vec![Chunk::data(*b"tdsb", [0, 0, 0, 1]).unwrap()];
    for (name, value) in controls {
        body.extend([named(name), scalar(*value)]);
    }
    if !options.is_empty() {
        body.extend([
            named("ADBE Effect Built In Params"),
            Chunk::list(*b"tdgp", options),
        ]);
    }
    body.push(named("ADBE Group End"));
    let mut descriptor = Vec::new();
    if !declarations.is_empty() {
        let mut table =
            vec![Chunk::data(*b"parn", (declarations.len() as u32).to_be_bytes()).unwrap()];
        for (name, label) in declarations {
            table.extend(declaration(name, label));
        }
        descriptor.push(Chunk::list(*b"parT", table));
    }
    descriptor.push(Chunk::list(*b"tdgp", body));
    vec![named(match_name), Chunk::list(*b"sspc", descriptor)]
}

fn owner(instances: Vec<Vec<Chunk>>) -> Layer {
    let mut owner = catalog_owner(&[]);
    let mut parade = vec![Chunk::data(*b"tdsb", [0, 0, 0, 1]).unwrap()];
    parade.extend(instances.into_iter().flatten());
    parade.push(named("ADBE Group End"));
    *named_group_mut(&mut owner.content, "ADBE Effect Parade").unwrap() = parade;
    owner
}

fn imported(owner: &Layer) -> ImportedEffects {
    import(
        &ExpressionSamples::default(),
        RIPPLE_COMPOSITION,
        owner,
        PLANE,
        PLANE,
        &mut 100,
        &mut Default::default(),
    )
}

#[test]
fn color_grade_import_keeps_grades_opacity_and_ordinal_identity() {
    let lumetri = instance(
        "ADBE Lumetri",
        &[
            ("ADBE Lumetri-0007", "Temperature"),
            ("ADBE Lumetri-0008", "Not Tint"),
        ],
        &[
            ("ADBE Lumetri-0003", 1.0),
            ("ADBE Lumetri-0007", 58.0),
            ("ADBE Lumetri-0008", 90.0),
            ("ADBE Lumetri-0011", 1.3),
            ("ADBE Lumetri-0023", 1.0),
            ("ADBE Lumetri-0031", 100.0),
        ],
        vec![named("ADBE Effect Mask Opacity"), scalar(50.0)],
    );
    let balance = instance(
        "ADBE Color Balance 2",
        &[],
        &[
            ("ADBE Color Balance 2-0001", 20.0),
            ("ADBE Color Balance 2-0010", 0.0),
        ],
        vec![
            named("ADBE Effect Mask Parade"),
            Chunk::list(
                *b"tdgp",
                vec![named("ADBE Effect Mask"), Chunk::list(*b"tdgp", vec![])],
            ),
        ],
    );
    let lut = instance("ADBE Apply Color LUT2", &[], &[], Vec::new());
    let owner = owner(vec![lumetri, lut, balance]);
    let imported = imported(&owner);
    assert_eq!(imported.native_ordinals, [1, 3], "{:?}", imported.warnings);
    let first = serde_json::to_value(&imported.effects[0]).unwrap();
    assert_eq!(first["effect"]["type"], "primaryGrade");
    assert_eq!(first["effect"]["temperature"], 0.5);
    assert_eq!(
        first["effect"]["tint"], 0.0,
        "unconfirmed label must not map"
    );
    assert_eq!(first["effect"]["exposure"], 0.5);
    assert_eq!(first["compositingOptions"]["effectOpacity"], 0.5);
    let third = serde_json::to_value(&imported.effects[1]).unwrap();
    assert_eq!(third["effect"]["type"], "colorCurves");
    assert!(
        third["effect"]["curves"]["red"].as_array().unwrap().len() > 2,
        "Shadow Red +20 brightens red: {third}"
    );
    assert_eq!(
        third["effect"]["curves"]["green"],
        serde_json::json!([{"x":0.0,"y":0.0},{"x":1.0,"y":1.0}])
    );
    assert!(third.get("compositingOptions").is_none());
    let warned = |text: &str| imported.warnings.iter().any(|w| w.contains(text));
    assert!(warned("ADBE Lumetri-0008: declared label \"Not Tint\""));
    assert!(warned("ADBE Apply Color LUT2: no embedded LUT"));
    assert!(warned(
        "effect mask 1: effect mask group does not hold exactly one mask reference"
    ));
    assert!(warned(
        "Color Balance imported as editable per-channel FX colorCurves"
    ));
}

#[test]
fn compositing_effect_opacity_applies_to_generic_mapped_effects() {
    // Insert explicit Effect Opacity into the writer's empty options group.
    fn add_opacity(chunks: &mut [Chunk]) -> bool {
        if let Some(index) = chunks.iter().position(|c| {
            c.id() == *b"tdmn" && chunk_match_name(c) == Some("ADBE Effect Built In Params")
        }) && let Some(options) = chunks.get_mut(index + 1).and_then(Chunk::children_mut)
        {
            options.splice(0..0, [named("ADBE Effect Mask Opacity"), scalar(25.0)]);
            return true;
        }
        chunks
            .iter_mut()
            .filter_map(Chunk::children_mut)
            .any(|children| add_opacity(children))
    }
    let mut owner = catalog_owner(&[native_effect("ADBE Gaussian Blur 2", &[("-0001", 12.)])]);
    assert!(add_opacity(&mut owner.content));
    let imported = imported(&owner);
    assert_eq!(imported.effects.len(), 1, "{:?}", imported.warnings);
    let value = serde_json::to_value(&imported.effects[0]).unwrap();
    assert_eq!(value["effect"]["type"], "gaussianBlur");
    assert_eq!(value["compositingOptions"]["effectOpacity"], 0.25);
    assert_eq!(
        value["compositingOptions"]["maskReferences"],
        serde_json::json!([])
    );
}

#[test]
fn compositing_options_are_not_split_across_multi_record_lowerings() {
    let mut result = ImportedEffects {
        basic_text: None,
        tiler: None,
        raster_motion_tile: None,
        effects: Vec::new(),
        native_ordinals: vec![1, 1],
        fractal_blends: Vec::new(),
        animations: Vec::new(),
        warnings: Vec::new(),
        adjustment_opacity: None,
        unsupported_cutout: false,
        unsupported_sweep_cutout: false,
        frame_fade: Ok(None),
        pending_effect_masks: Vec::new(),
    };
    for id in [1, 2] {
        result.effects.push(
            fx_schema::EffectRecord::from_data(&fx_schema::EffectData::Identified {
                id: EffectId::new(id),
                enabled: true,
                effect: fx_schema::EffectPayload::Known(fx_schema::LayerEffect::PrimaryGrade(
                    Default::default(),
                )),
                compositing_options: None,
                extensions: Default::default(),
            })
            .unwrap(),
        );
    }
    let native = crate::effects::compositing::NativeCompositing {
        opacity: Some(crate::properties::NumericProperty {
            values: vec![50.0],
            animated: false,
            expression_enabled: false,
            expression_present: false,
            dimensions_separated: false,
            keyframes: Vec::new(),
            value_kind: crate::properties::NumericValueKind::Continuous,
        }),
        mask_references: Vec::new(),
    };
    let natives = std::collections::HashMap::from([(1, ("CC Toner".to_owned(), Ok(native)))]);
    super::super::color_grade::apply_compositing(&mut result, &natives);
    assert!(result.warnings[0].contains("lowers to several FX effects"));
    assert!(result.effects.iter().all(|record| {
        serde_json::to_value(record)
            .unwrap()
            .get("compositingOptions")
            .is_none()
    }));
}
