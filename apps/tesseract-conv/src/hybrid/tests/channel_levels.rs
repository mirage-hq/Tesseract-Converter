//! Genuine native Premiere source -> current channel edit/bypass -> fresh package.
use super::*;
use sha2::{Digest, Sha256};

fn rows(value: &Value, result: &mut Vec<(String, f64, bool)>) {
    rows_in_screen_scope(value, result, 0);
}

fn rows_in_screen_scope(value: &Value, result: &mut Vec<(String, f64, bool)>, screens: usize) {
    // Native reimport may normalize a picture into a Screen wrapper with a
    // Normal leaf. Assert exactly one Screen scope per isolated RGB picture,
    // and exactly one such picture in each Screen scope, not a leaf-only label.
    let is_screen = value["blendMode"] == "screen";
    let screens = screens + usize::from(is_screen);
    let before = result.len();
    if let Some(effects) = value["effects"].as_array() {
        if effects.len() == 3 && effects[2]["effect"]["type"] == "shiftChannels" {
            let shift = &effects[2]["effect"];
            // Disjoint selectors are essential to Screen being an RGB sum.
            let channel = match (
                shift["takeRedFrom"].as_str().unwrap(),
                shift["takeGreenFrom"].as_str().unwrap(),
                shift["takeBlueFrom"].as_str().unwrap(),
            ) {
                ("red", "fullOff", "fullOff") => "red",
                ("fullOff", "green", "fullOff") => "green",
                ("fullOff", "fullOff", "blue") => "blue",
                other => panic!("non-disjoint channel selectors: {other:?}"),
            };
            assert_eq!(screens, 1, "each RGB branch has exactly one Screen scope");
            result.push((
                channel.into(),
                effects[1]["effect"]["gamma"].as_f64().unwrap(),
                effects[1]["enabled"] != false,
            ));
        }
    }
    if let Some(children) = value["layers"].as_array() {
        for child in children {
            rows_in_screen_scope(child, result, screens)
        }
    }
    if is_screen {
        assert_eq!(
            result.len() - before,
            1,
            "one isolated RGB picture per Screen scope"
        );
    }
}

#[test]
fn premiere_channel_levels_native_current_edit_and_bypass_export() {
    let source = fixture("premiere_channel_levels/source/native.prproj");
    assert_eq!(
        format!("{:x}", Sha256::digest(fs::read(&source).unwrap())),
        "3b6ee8fdee5f1ab31d4fbc0c9c9a4aafca3e2dd87f4bf40322a8000a62cd2ca0"
    );
    let (_source_dir, source) = relinked_native_fixture(&source);
    let temp = tempfile::tempdir().unwrap();
    let imported = temp.path().join("import");
    import_premiere(&request(&source, &imported, ConversionMode::Write)).unwrap();
    let original = TesseractFile::open(imported.join("project.tsrct"))
        .unwrap()
        .project_json()
        .unwrap();
    let root = &original["composition"]["layers"][0];
    assert_eq!(root["name"], "Premiere channel Levels");
    let mut selected = Vec::new();
    rows(root, &mut selected);
    assert_eq!(
        selected,
        vec![
            ("red".into(), 0.02, true),
            ("green".into(), 1.0, true),
            ("blue".into(), 1.0, true)
        ]
    );
    assert_eq!(root["layers"][0]["trackMatte"]["mode"], "alpha");
    assert_eq!(
        root["layers"][0]["trackMatte"]["layer"],
        root["layers"][1]["id"]
    );
    assert_eq!(root["layers"][0]["layers"][0]["blendMode"], "divide");
    // Pins the public float-regression mechanism to the actual imported graph.
    // FX layer order is front-to-back; the renderer fixture is paint order.
    let carrier = &root["layers"][0];
    let divisor = &carrier["layers"][0];
    let sum = &carrier["layers"][1];
    assert_eq!(sum["layers"].as_array().unwrap().len(), 4);
    assert_eq!(divisor["layers"].as_array().unwrap().len(), 2);
    for backing in [&sum["layers"][3], &divisor["layers"][1]] {
        assert_eq!(backing["rect"]["fillColor"], json!([0.0, 0.0, 0.0, 1.0]));
    }
    let white = &divisor["layers"][0];
    assert_eq!(white["effects"].as_array().unwrap().len(), 1);
    for field in ["takeRedFrom", "takeGreenFrom", "takeBlueFrom"] {
        assert_eq!(white["effects"][0]["effect"][field], "fullOn");
    }
    let sample = &root["layers"][1];
    assert!(sample["effects"].as_array().is_none_or(Vec::is_empty));
    for branch in sum["layers"].as_array().unwrap().iter().take(3) {
        assert_eq!(branch["blendMode"], "screen");
    }
    for picture in sum["layers"]
        .as_array()
        .unwrap()
        .iter()
        .take(3)
        .chain([white])
    {
        assert_eq!(picture["source"], sample["source"]);
        assert_eq!(picture["playback"], sample["playback"]);
    }
    for state in ["original", "edited", "bypassed"] {
        let mut document = original.clone();
        let graph = &mut document["composition"]["layers"][0];
        if state != "original" {
            graph["name"] = json!("Current arbitrary name");
            graph["layers"][0]["layers"][1]["layers"][1]["effects"][1]["effect"]["gamma"] =
                json!(0.5);
        }
        if state == "bypassed" {
            graph["layers"][0]["layers"][1]["layers"][1]["effects"][1]["enabled"] = json!(false);
        }
        let input = temp.path().join(format!("{state}.tsrct"));
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
            .unwrap()
            .add_asset(
                "premiere-image-1",
                fixture("premiere_channel_levels/inputs/cells.png"),
                AssetKind::Image,
            )
            .unwrap()
            .write(&input)
            .unwrap();
        let output = temp.path().join(format!("export-{state}"));
        let report = export(
            &request(&input, &output, ConversionMode::Write),
            &Default::default(),
        )
        .unwrap();
        let project = output.join("project.prproj");
        assert!(project.is_file(), "{report:?}");
        assert!(
            xml(&project).contains("./media/ae-0001/compositions.aep"),
            "{report:?}"
        );
        let native = aftereffects_file::aep::Project::parse(
            &fs::read(output.join("media/ae-0001/compositions.aep")).unwrap(),
        )
        .unwrap();
        // The native writer deliberately selects editable Pro Levels, not Easy
        // Levels (whose render values live in the arbitrary Histogram).
        let mut gamma = Vec::new();
        numeric_properties(&native.chunks, "ADBE Pro Levels2-0006", &mut gamma);
        let mut gamma: Vec<_> = gamma.iter().map(|p| p.values[0]).collect();
        gamma.sort_by(f64::total_cmp);
        let mut expected = vec![
            0.02,
            if state == "original" { 1.0 } else { 0.5 },
            1.0,
            1.0,
            1.0,
            1.0,
        ];
        expected.sort_by(f64::total_cmp);
        assert_eq!(gamma, expected);
        let mut whites = Vec::new();
        numeric_properties(&native.chunks, "ADBE Pro Levels2-0005", &mut whites);
        let mut whites: Vec<_> = whites.iter().map(|p| p.values[0]).collect();
        whites.sort_by(f64::total_cmp);
        assert_eq!(whites.len(), 6);
        assert!((whites[0] - 200.0 / 255.0).abs() < 1e-9);
        assert_eq!(&whites[1..], &[1.0; 5]);
        let mut blacks = Vec::new();
        numeric_properties(&native.chunks, "ADBE Pro Levels2-0004", &mut blacks);
        let mut blacks: Vec<_> = blacks.iter().map(|p| p.values[0]).collect();
        blacks.sort_by(f64::total_cmp);
        assert_eq!(blacks.len(), 6);
        assert_eq!(&blacks[..3], &[0.0; 3]);
        assert!(blacks[3..].iter().all(|v| (*v - 30.0 / 255.0).abs() < 1e-9));
        let structure = aftereffects_file::structure::read_project(
            &fs::read(output.join("media/ae-0001/compositions.aep")).unwrap(),
        )
        .unwrap();
        let mut bindings = 0;
        let mut pictures = 0;
        let mut screens = 0;
        let mut divides = 0;
        for item in &structure.items {
            if let aftereffects_file::structure::ItemKind::Composition(comp) = &item.kind {
                for layer in &comp.layers {
                    screens += usize::from(layer.record.blend_mode() == 6);
                    divides += usize::from(layer.record.blend_mode() == 38);
                    assert_ne!(layer.record.blend_mode(), 4, "no additive alpha assembly");
                    if layer.record.track_matte_type() == 1 {
                        bindings += 1;
                        let provider = comp
                            .layers
                            .iter()
                            .find(|p| Some(p.record.id()) == layer.record.matte_layer_id())
                            .expect("same-scope alpha provider");
                        assert!(
                            !provider.record.flags().enabled,
                            "consumed source must not independently paint"
                        );
                    }
                    if structure
                        .items
                        .iter()
                        .any(|s| s.id == layer.record.source_id() && s.media.is_some())
                    {
                        pictures += 1;
                    }
                }
            }
        }
        assert_eq!(screens, 3, "native Screen branches");
        assert_eq!(divides, 1, "native Divide alpha compensation");
        assert_eq!(bindings, 1);
        assert_eq!(
            pictures, 5,
            "three colour branches, alpha divisor and original alpha sample"
        );
        let reimport = temp.path().join(format!("reimport-{state}"));
        import_premiere(&request(&project, &reimport, ConversionMode::Write)).unwrap();
        let result = TesseractFile::open(reimport.join("project.tsrct"))
            .unwrap()
            .project_json()
            .unwrap();
        let mut current = Vec::new();
        rows(&result["composition"], &mut current);
        current.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            current,
            vec![
                ("blue".into(), 1.0, true),
                (
                    "green".into(),
                    if state == "original" { 1.0 } else { 0.5 },
                    state != "bypassed"
                ),
                ("red".into(), 0.02, true)
            ]
        );
        assert_eq!(
            fs::read(output.join("media/ae-0001/media/000000-premiere-image-1.png")).unwrap(),
            fs::read(fixture("premiere_channel_levels/inputs/cells.png")).unwrap()
        );
    }
}

fn numeric_properties(
    chunks: &[aftereffects_file::rifx::Chunk],
    name: &str,
    out: &mut Vec<aftereffects_file::properties::NumericProperty>,
) {
    for (index, chunk) in chunks.iter().enumerate() {
        if chunk.id() == *b"tdmn"
            && chunk
                .data_payload()
                .is_some_and(|b| b.starts_with(format!("{name}\0").as_bytes()))
        {
            if let Some(storage) = chunks[index + 1..]
                .iter()
                .take_while(|c| c.id() != *b"tdmn")
                .find(|c| c.list_kind() == Some(*b"tdbs"))
                .and_then(aftereffects_file::rifx::Chunk::children)
            {
                out.push(aftereffects_file::properties::read_numeric(storage).unwrap());
            }
        }
        if let Some(children) = chunk.children() {
            numeric_properties(children, name, out)
        }
    }
}
