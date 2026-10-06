//! Native Premiere input and current-edit package integration, not pixel equality.
use super::*;
use sha2::{Digest, Sha256};

#[test]
fn premiere_alpha_native_import_current_edit_exports_package() {
    let source = fixture("premiere_invert_alpha/source/native.prproj");
    assert_eq!(
        format!("{:x}", Sha256::digest(fs::read(&source).unwrap())),
        "40b3cb3d93261addfae28ad888e9cb7f3bf6203549e8a4f5f4fb89b8e458ce6e"
    );
    let (_source_dir, source) = relinked_native_fixture(&source);
    let temp = tempfile::tempdir().unwrap();
    let imported = temp.path().join("import");
    import_premiere(&request(&source, &imported, ConversionMode::Write)).unwrap();
    let original = TesseractFile::open(imported.join("project.tsrct"))
        .unwrap()
        .project_json()
        .unwrap();
    let layers = original["composition"]["layers"].as_array().unwrap();
    let index = layers
        .iter()
        .position(|v| v["name"] == "Premiere Invert Alpha")
        .unwrap_or_else(|| panic!("missing Alpha graph: {original}"));
    let graph = &layers[index];
    assert_eq!(graph["layers"][0]["trackMatte"]["mode"], "alphaInverted");
    assert_eq!(
        graph["layers"][0]["trackMatte"]["layer"],
        graph["layers"][1]["id"]
    );
    let picture = &graph["layers"][0]["layers"][0];
    assert_eq!(picture["type"], "Image");
    assert_eq!(
        graph["layers"][0]["layers"][1]["rect"]["fillColor"],
        json!([0.0, 0.0, 0.0, 1.0])
    );
    assert_eq!(picture["source"], graph["layers"][1]["source"]);
    for state in ["original", "edited", "bypass"] {
        let edited = state == "edited";
        let bypass = state == "bypass";
        let mut document = original.clone();
        if edited {
            document["composition"]["layers"][index]["layers"][0]["trackMatte"]["mode"] =
                json!("alpha");
        }
        if bypass {
            let g = &mut document["composition"]["layers"][index];
            g["layers"][0].as_object_mut().unwrap().remove("trackMatte");
            g["layers"][0]["layers"].as_array_mut().unwrap().truncate(1);
            g["layers"][1]["isHidden"] = json!(true);
        }
        let input = temp.path().join(format!("input-{state}.tsrct"));
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
            .unwrap()
            .add_asset(
                "premiere-image-1",
                fixture("premiere_invert_alpha/inputs/cells.png"),
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
        assert!(output.join("project.prproj").is_file(), "{report:?}");
        let text = xml(&output.join("project.prproj"));
        // Existing native nested sequences and Track Matte Key preserve this
        // current graph; no linked sidecar or effect-name replay is needed.
        if !bypass {
            assert!(text.contains("AE.ADBE Legacy Key Track Matte"), "{text}");
            let reverse = text.split_once("<Name>Reverse</Name>").unwrap().1;
            let value = reverse
                .split_once("<StartKeyframe>")
                .unwrap()
                .1
                .split(',')
                .nth(1)
                .unwrap();
            assert_eq!(value, if edited { "false" } else { "true" });
        } else {
            assert!(!text.contains("AE.ADBE Legacy Key Track Matte"));
        }
        assert!(!text.contains("AE.ADBE Invert"));
        assert_eq!(
            fs::read(output.join("media/cells.png")).unwrap(),
            fs::read(fixture("premiere_invert_alpha/inputs/cells.png")).unwrap()
        );
        let reimport = temp.path().join(format!("reimport-{state}"));
        let root = text
            .split("<Sequence ")
            .skip(1)
            .find(|part| part.contains("<Name>Premiere Alpha15</Name>"))
            .unwrap();
        let uid = root
            .split_once("ObjectUID=\"")
            .unwrap()
            .1
            .split('"')
            .next()
            .unwrap();
        let exported = output.join("project.prproj");
        let mut req = request(&exported, &reimport, ConversionMode::Write);
        req.sequence = Some(uid);
        import_premiere(&req).unwrap();
        let current = TesseractFile::open(reimport.join("project.tsrct"))
            .unwrap()
            .project_json()
            .unwrap();
        fn mattes(v: &Value, result: &mut Vec<String>) {
            if let Some(mode) = v["trackMatte"]["mode"].as_str() {
                result.push(mode.into());
            }
            if let Some(children) = v["layers"].as_array() {
                for child in children {
                    mattes(child, result);
                }
            }
        }
        // Prove the original picture remains in paint, not merely a helper or
        // matching name. Consumed providers and hidden branches do not paint.
        fn painting_images(v: &Value) -> usize {
            if v["isHidden"] == true {
                return 0;
            }
            if v["type"] == "Image" {
                assert!(!v["source"].is_null());
                return 1;
            }
            let Some(children) = v["layers"].as_array() else {
                return 0;
            };
            let consumed: Vec<_> = children
                .iter()
                .filter_map(|c| c["trackMatte"]["layer"].as_u64())
                .collect();
            for id in &consumed {
                assert!(
                    children.iter().any(|c| c["id"].as_u64() == Some(*id)),
                    "matte must share the consumer timeline"
                );
            }
            children
                .iter()
                .filter(|c| !c["id"].as_u64().is_some_and(|id| consumed.contains(&id)))
                .map(painting_images)
                .sum()
        }
        assert_eq!(painting_images(&current["composition"]), 1, "{current}");
        let mut modes = Vec::new();
        mattes(&current["composition"], &mut modes);
        assert_eq!(
            modes,
            if bypass {
                vec![]
            } else {
                vec![if edited { "alpha" } else { "alphaInverted" }]
            }
        );
    }
}

#[test]
fn premiere_alpha_trimmed_video_uses_current_package_clock() {
    // Supplementary transplant of saved native controls into the small video
    // placement fixture; not independent native video/trim authorship.
    let native = xml(&fixture("premiere_invert_alpha/source/native.prproj"));
    fn record(text: &str, tag: &str, id: u32) -> String {
        let start = text.find(&format!("<{tag} ObjectID=\"{id}\"")).unwrap();
        let end = text[start..].find(&format!("</{tag}>")).unwrap() + start + tag.len() + 3;
        text[start..end].to_owned()
    }
    let mut controls = record(&native, "VideoFilterComponent", 86);
    controls += &record(&native, "VideoComponentParam", 96);
    controls += &record(&native, "VideoComponentParam", 97);
    controls = controls
        .replace("\"86\"", "\"889\"")
        .replace("\"96\"", "\"3090\"")
        .replace("\"97\"", "\"3091\"");
    let base = fs::read_to_string(fixture("native_offset_static.xml")).unwrap();
    let prefix = base
        .split("<VideoFilterComponent ObjectID=\"889\"")
        .next()
        .unwrap();
    let source_xml = format!("{prefix}{controls}</PremiereData>")
        .replace(
            "<TrackItem><End>1270080000000</End>",
            "<TrackItem><Start>127008000000</Start><End>381024000000</End>",
        )
        .replace(
            "<InPoint>0</InPoint><OutPoint>1270080000000</OutPoint>",
            "<InPoint>254016000000</InPoint><OutPoint>508032000000</OutPoint>",
        );
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("derived.prproj");
    fs::write(&source, source_xml).unwrap();
    fs::create_dir(temp.path().join("media")).unwrap();
    fs::copy(
        fixture("video-30fps-10s.mp4"),
        temp.path().join("media/source.mp4"),
    )
    .unwrap();
    let imported = temp.path().join("import");
    import_premiere(&request(&source, &imported, ConversionMode::Write)).unwrap();
    let input = imported.join("project.tsrct");
    let current = TesseractFile::open(&input).unwrap().project_json().unwrap();
    let graph = &current["composition"]["layers"][0];
    assert_eq!(graph["name"], "Premiere Invert Alpha");
    let original = &graph["layers"][0]["layers"][0];
    assert_eq!(
        original["sourceRange"],
        json!({"start":1000,"duration":1000})
    );
    let output = temp.path().join("export");
    let report = export(
        &request(&input, &output, ConversionMode::Write),
        &Default::default(),
    )
    .unwrap();
    assert!(output.join("project.prproj").is_file(), "{report:?}");
    let sidecar = output.join("media/ae-0001/compositions.aep");
    assert!(sidecar.is_file(), "{report:?}");
    let native = aftereffects_file::structure::read_project(&fs::read(sidecar).unwrap()).unwrap();
    let mut bindings = 0;
    let mut sources = 0;
    let mut clocks = Vec::new();
    for item in &native.items {
        if let aftereffects_file::structure::ItemKind::Composition(comp) = &item.kind {
            for layer in &comp.layers {
                if layer.record.track_matte_type() == 2 {
                    bindings += 1;
                    let id = layer.record.matte_layer_id().unwrap();
                    let provider = comp.layers.iter().find(|p| p.record.id() == id).unwrap();
                    let gate_start = layer.record.start_time().unwrap()
                        + layer.record.in_point().unwrap() * layer.record.stretch().unwrap();
                    let gate_end = layer.record.start_time().unwrap()
                        + layer.record.out_point().unwrap() * layer.record.stretch().unwrap();
                    let source_at = |t: f64| {
                        (t - provider.record.start_time().unwrap())
                            / provider.record.stretch().unwrap()
                    };
                    assert_eq!(
                        (source_at(gate_start), source_at(gate_end)),
                        (1.0, 2.0),
                        "source placement must be applied once at the carrier gate"
                    );
                    assert!(
                        !provider.record.flags().enabled,
                        "consumed sample must not paint independently"
                    );
                }
                if let Some(source) = native
                    .items
                    .iter()
                    .find(|s| s.id == layer.record.source_id())
                {
                    if source.media.is_some() {
                        sources += 1;
                        clocks.push((layer.record.in_point(), layer.record.out_point()));
                    }
                }
            }
        }
    }
    // Existing AEP video export offsets visibility bounds by half a millisecond;
    // it keeps the nominal affine source origin, rather than shifting playback.
    assert!(report.diagnostics.iter().any(|d| d
        .message
        .contains("half-millisecond parent-boundary correction")));
    assert_eq!(clocks, vec![(Some(0.9995), Some(1.9995)); 2]);
    assert_eq!(bindings, 1);
    assert!(
        sources >= 2,
        "original footage and independent sample must both remain"
    );
}
