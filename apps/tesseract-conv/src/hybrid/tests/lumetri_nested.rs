//! Pinned Lumetri controls with a supplementary edited nested FX host.
//! This verifies generated linked-native structure, not Adobe acceptance.
use super::*;

fn edited_group_document(parent: &Path) -> Value {
    let source = parent.join("source.prproj");
    let bytes = fs::read(fixture("human_lumetri_contrast.xml")).unwrap();
    fs::write(&source, &bytes).unwrap();
    fs::create_dir(parent.join("media")).unwrap();
    fs::copy(
        fixture("video-30fps-10s.mp4"),
        parent.join("media/source.mp4"),
    )
    .unwrap();
    let imported = parent.join("imported");
    import_premiere(&request(&source, &imported, ConversionMode::Write)).unwrap();
    let mut document = TesseractFile::open(imported.join("project.tsrct"))
        .unwrap()
        .project_json()
        .unwrap();
    let mut video = document["composition"]["layers"][0].clone();
    let mut effects = video.as_object_mut().unwrap().remove("effects").unwrap();
    assert_eq!(effects.as_array().unwrap().len(), 6);
    assert_eq!(effects[0]["effect"]["exposure"], 1.0);
    assert_eq!(effects[2]["effect"]["saturation"], 30.0);
    assert_eq!(effects[3]["effect"]["temperature"], 0.0);
    assert_eq!(effects[4]["effect"]["tint"], 0.0);
    effects[0]["effect"]["exposure"] = json!(2.0);
    effects[1]["effect"]["contrast"] = json!(40.0);
    effects[3]["effect"]["temperature"] = json!(40.0);
    effects[4]["effect"]["tint"] = json!(-25.0);
    let temperature_id = effects[3]["id"].clone();
    let tint_id = effects[4]["id"].clone();
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap();
    assert_eq!(entries.len(), 1);
    entries[0]["animator"]["keyframes"][0]["value"]["value"] = json!(45.0);
    entries[0]["animator"]["keyframes"][0]["layerTime"] = json!(500);
    entries[0]["animator"]["keyframes"][1]["value"]["value"] = json!(-65.0);
    entries[0]["animator"]["keyframes"][1]["layerTime"] = json!(1750);
    for (id, name, value, end) in [
        (temperature_id, "temperature", 40.0, 500),
        (tint_id, "tint", -25.0, 750),
    ] {
        entries.push(json!({
            "target":{"kind":"effectProperty","effectId":id,"paramName":name},
            "animator":{"type":"keyframes","enabled":true,"keyframes":[
                {"id":format!("{name}-start"),"layerTime":0,"value":{"type":"float","value":value},"easing":{"type":"linear"}},
                {"id":format!("{name}-end"),"layerTime":end,"value":{"type":"float","value":-value},"easing":{"type":"linear"}}
            ]}
        }));
    }
    video["parent"] = json!(100);
    // Match the existing identity picture-group host, without claiming that
    // this supplementary host was independently authored in Premiere.
    document["composition"]["layers"] = json!([{
        "type":"Group","id":100,"name":"Edited Lumetri picture",
        "playback":video["playback"],"transform":video["transform"],
        "effects":effects,"layers":[video]
    }]);
    assert_eq!(fs::read(source).unwrap(), bytes);
    document
}

fn native_effects(chunks: &[aftereffects_file::rifx::Chunk]) -> Vec<(String, bool)> {
    let mut result = Vec::new();
    for pair in chunks.windows(2) {
        if pair[0].id() == *b"tdmn" && pair[1].list_kind() == Some(*b"sspc") {
            let name = std::str::from_utf8(
                pair[0]
                    .data_payload()
                    .unwrap()
                    .split(|byte| *byte == 0)
                    .next()
                    .unwrap(),
            )
            .unwrap()
            .to_owned();
            let controls = pair[1]
                .children()
                .unwrap()
                .iter()
                .find(|chunk| chunk.list_kind() == Some(*b"tdgp"))
                .unwrap()
                .children()
                .unwrap();
            let enabled = controls
                .iter()
                .find(|chunk| chunk.id() == *b"tdsb")
                .is_none_or(|chunk| {
                    let flags = chunk.data_payload().unwrap();
                    assert_eq!(flags.len(), 4);
                    flags[3] & 1 != 0
                });
            result.push((name, enabled));
        }
    }
    for children in chunks
        .iter()
        .filter_map(aftereffects_file::rifx::Chunk::children)
    {
        result.extend(native_effects(children));
    }
    result
}

#[test]
fn human_lumetri_nested_edited_linked_export_keeps_controls_defaults_and_keys() {
    let parent = tempfile::tempdir().unwrap();
    let document = edited_group_document(parent.path());
    let input = parent.path().join("edited.tsrct");
    archive_with_video(&document, &input, "video-30fps-10s.mp4");
    let output = parent.path().join("exported");
    let report = export(
        &request(&input, &output, ConversionMode::Write),
        &Default::default(),
    )
    .unwrap();
    assert!(
        !report.diagnostics.iter().any(
            |note| note.message.contains("temperatureTint") && note.message.contains("omitted")
        ),
        "{report:?}"
    );
    for approximation in [
        "Animated saturation-only",
        "intermediate clipping",
        "Pin Highlights",
    ] {
        assert!(
            report
                .diagnostics
                .iter()
                .any(|note| note.message.contains(approximation)),
            "{report:?}"
        );
    }
    assert!(
        report
            .diagnostics
            .iter()
            .any(|note| note.code == "HYBRID-EXPERIMENTAL"),
        "{report:?}"
    );
    let native = aftereffects_file::structure::read_project(
        &fs::read(output.join("media/ae-0001/compositions.aep")).unwrap(),
    )
    .unwrap();
    let mut owners = Vec::new();
    for item in &native.items {
        if let aftereffects_file::structure::ItemKind::Composition(comp) = &item.kind {
            for layer in &comp.layers {
                let effects = native_effects(&layer.content);
                if !effects.is_empty() {
                    owners.push((layer, effects));
                }
            }
        }
    }
    assert_eq!(
        owners.len(),
        1,
        "the Group stack must not be copied to its child"
    );
    let (owner, effects) = &owners[0];
    assert_eq!(owner.name.as_ref(), "Edited Lumetri picture");
    assert_eq!(
        effects
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        [
            "ADBE Exposure2",
            "ADBE Brightness & Contrast 2",
            "ADBE Vibrance",
            "ADBE Exposure2",
            "ADBE Exposure2",
            "ADBE Exposure2",
            "ADBE Exposure2",
            "CS Vignette"
        ]
    );
    assert!(effects.iter().all(|(_, enabled)| *enabled));
    assert_eq!(
        numeric_properties(&owner.content, "ADBE Brightness & Contrast 2-0001")[0].values,
        [0.0]
    );
    assert_eq!(
        numeric_properties(&owner.content, "ADBE Brightness & Contrast 2-0002")[0].values,
        [40.0]
    );
    let selectors = numeric_properties(&owner.content, "ADBE Exposure2-0001");
    assert_eq!(selectors.len(), 5);
    assert_eq!(selectors[0].values, [1.0]);
    assert!(selectors[1..]
        .iter()
        .all(|property| property.values == [2.0]));
    let exposure = numeric_properties(&owner.content, "ADBE Exposure2-0003");
    assert_eq!(exposure[0].values, [2.0]);
    assert!(exposure[1..]
        .iter()
        .all(|property| property.values == [0.0]));
    for name in [
        "ADBE Exposure2-0005",
        "ADBE Exposure2-0010",
        "ADBE Exposure2-0015",
        "ADBE Exposure2-0020",
    ] {
        let properties = numeric_properties(&owner.content, name);
        assert_eq!(properties.len(), 5);
        assert!(properties.iter().all(|property| property.values == [1.0]));
    }
    // Flat source-derived tests own exact key-value projections. Here prove
    // that the single Group owner retains all three independent keyed controls.
    for (name, keyed_count) in [
        ("ADBE Exposure2-0009", 2),
        ("ADBE Exposure2-0014", 1),
        ("ADBE Exposure2-0019", 2),
        ("ADBE Vibrance-0002", 1),
    ] {
        assert_eq!(
            numeric_properties(&owner.content, name)
                .iter()
                .filter(|property| property.keyframes.len() == 2)
                .count(),
            keyed_count
        );
    }
}

#[test]
fn human_lumetri_fractional_and_ae_fallback_report_replacement_omissions() {
    let parent = tempfile::tempdir().unwrap();
    let mut document = edited_group_document(parent.path());
    // Native-only retention is tested on the ordinary physical host. The
    // supplementary effected Group itself has further native export limits.
    let group = document["composition"]["layers"][0].take();
    let mut video = group["layers"][0].clone();
    video.as_object_mut().unwrap().remove("parent");
    video["effects"] = group["effects"].clone();
    document["composition"]["layers"] = json!([video]);
    for (name, rate, fallback) in [
        (
            "fractional",
            premiere_file::FrameRate::Fps30000Over1001,
            false,
        ),
        ("ae-fallback", premiere_file::FrameRate::Fps30, true),
    ] {
        let input = parent.path().join(format!("{name}.tsrct"));
        if fallback {
            // The fallback source is a two-second native-only HDR fixture;
            // select one second rather than claiming the ten-second host span.
            let mut bounded = document.clone();
            bounded["duration"] = json!(1.0);
            let video = &mut bounded["composition"]["layers"][0];
            video["playback"]["inputRange"]["duration"] = json!(1000);
            video["playback"]["mapping"]["input"]["duration"] = json!(1000);
            video["playback"]["mapping"]["output"]["duration"] = json!(1000);
            video["sourceRange"]["duration"] = json!(1000);
            unsupported_video_archive(&bounded, &input);
        } else {
            archive_with_video(&document, &input, "video-30fps-10s.mp4");
        }
        let output = parent.path().join(name);
        let report = export(
            &request(&input, &output, ConversionMode::Write),
            &PremiereExportOptions {
                frame_rate: Some(rate),
            },
        )
        .unwrap();
        assert!(
            report
                .diagnostics
                .iter()
                .any(|note| note.code == "HYBRID-NATIVE-RETAINED"),
            "{report:?}"
        );
        for effect in ["exposure", "hueSaturation", "temperatureTint", "vignette"] {
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|note| note.code == "PREMIERE-FEATURE"
                        && note.message.contains(effect)
                        && note.message.contains("was not exported")),
                "{effect}: {report:?}"
            );
        }
        assert!(!output.join("media/ae-0001").exists());
        assert!(!xml(&output.join("project.prproj")).contains("compositions.aep"));
    }
}
