use super::*;

#[test]
fn linked_root_group_ends_before_a_sample_rounded_out_of_its_active_range() {
    for (text_only, audible, expected_out) in [
        (true, false, 0.4665),
        (false, false, 0.4665),
        (false, true, 0.467),
    ] {
        let parent = tempfile::tempdir().unwrap();
        let input = parent.path().join("source.tsrct");
        let mut value = long_picture_group();
        let mut native_video = value["composition"]["layers"][0]["layers"][0].clone();
        native_video.as_object_mut().unwrap().remove("parent");
        native_video["id"] = json!(32);
        native_video["trackMatte"] = json!({"layer":30,"mode":"alpha"});
        value["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .push(native_video);
        let group = &mut value["composition"]["layers"][0];
        group["effects"] = json!([{"id":100,"effect":{
            "type":"brightnessContrast","brightness":10,"contrast":0
        }}]);
        group["playback"] = json!({
            "type":"windowed", "inputRange":{"start":200,"duration":467},
            "mapping":{"type":"linear", "input":{"start":200,"duration":467},
                "output":{"start":0,"duration":467}}, "inputOffsetMs":0
        });
        if text_only {
            group["effects"] = json!([]);
            group["transform"]["opacity"] = json!(50);
            group["layers"] = json!([{
                "type":"Text", "id":31, "parent":30, "name":"Boundary title",
                "activeRange":{"start":0,"duration":467},
                "transform":group["transform"],
                "sourceText":{"text":"Style 2", "fontFamily":"Arial",
                    "fontStyle":"Bold", "fontSize":42, "fillColor":[1,1,1,1]}
            }]);
        } else if audible {
            group["layers"][0]["volume"] = json!(1.0);
        }
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&value).unwrap())
            .unwrap()
            .add_asset(
                "premiere-video-1",
                fixture("video-30fps.mp4"),
                AssetKind::Video,
            )
            .unwrap()
            .add_asset(
                "boundary-font",
                fixture("../../../../tests/references/premiere/fonts/Arial-BoldMT.ttf"),
                AssetKind::Font,
            )
            .unwrap()
            .write(&input)
            .unwrap();
        let original = fs::read(&input).unwrap();
        let output = parent.path().join("output");
        let mut conversion = request(&input, &output, ConversionMode::Write);
        conversion.fps = Some("30");
        let report = crate::formats::export_premiere(&conversion).unwrap();
        assert!(
            output.join("media/ae-0001/compositions.aep").exists(),
            "{report:?}"
        );
        let native = aftereffects_file::structure::read_project(
            &fs::read(output.join("media/ae-0001/compositions.aep")).unwrap(),
        )
        .unwrap();
        let aftereffects_file::structure::ItemKind::Composition(root) =
            &native.item(1).unwrap().kind
        else {
            panic!("root composition");
        };
        let group = root
            .layers
            .iter()
            .find(|layer| layer.name.as_ref() == "Long plain group")
            .unwrap();
        let record = &group.record;
        assert_eq!(record.start_time(), Some(0.2));
        assert_eq!(record.in_point(), Some(0.0));
        assert_eq!(
            record.out_point(),
            Some(expected_out),
            "text_only={text_only}, audible={audible}"
        );
        assert_eq!(record.stretch(), Some(1.0));
        // Picture rounds its root lifetime; audio keeps its continuous clock.
        let sample = 20.0 / 30.0;
        assert_eq!(
            sample >= record.start_time().unwrap() + record.out_point().unwrap(),
            !audible
        );
        assert_eq!(fs::read(input).unwrap(), original);
    }
}

#[test]
fn linked_root_group_prerolls_its_first_source_sample_at_a_rounded_start() {
    for audible in [false, true] {
        let parent = tempfile::tempdir().unwrap();
        let input = parent.path().join("source.tsrct");
        let mut value = long_picture_group();
        let mut consumer = value["composition"]["layers"][0]["layers"][0].clone();
        consumer.as_object_mut().unwrap().remove("parent");
        consumer["id"] = json!(32);
        consumer["trackMatte"] = json!({"layer":30,"mode":"alpha"});
        value["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .push(consumer);
        let group = &mut value["composition"]["layers"][0];
        group["layers"][0]["name"] = json!("Child nominal clock");
        group["layers"][0]["volume"] = json!(if audible { 1.0 } else { 0.0 });
        group["playback"] = json!({
            "type":"windowed", "inputRange":{"start":667,"duration":333},
            "mapping":{"type":"linear", "input":{"start":667,"duration":333},
                "output":{"start":0,"duration":333}}, "inputOffsetMs":0
        });
        archive(&value, &input);
        let original = fs::read(&input).unwrap();
        let output = parent.path().join("output");
        let mut conversion = request(&input, &output, ConversionMode::Check);
        conversion.fps = Some("30");
        let checked = crate::formats::export_premiere(&conversion).unwrap();
        assert!(!output.exists());
        conversion.mode = ConversionMode::Write;
        assert_eq!(
            checked,
            crate::formats::export_premiere(&conversion).unwrap()
        );
        let native = aftereffects_file::structure::read_project(
            &fs::read(output.join("media/ae-0001/compositions.aep")).unwrap(),
        )
        .unwrap();
        let aftereffects_file::structure::ItemKind::Composition(root) =
            &native.item(1).unwrap().kind
        else {
            panic!("root composition");
        };
        let group = root
            .layers
            .iter()
            .find(|layer| layer.name.as_ref() == "Long plain group")
            .unwrap();
        if audible {
            assert_eq!(group.record.start_time(), Some(0.667));
            assert_eq!(group.record.in_point(), Some(0.0));
            assert_eq!(group.record.out_point(), Some(0.333));
            assert!(numeric_properties(&group.content, "ADBE Time Remapping").is_empty());
            assert_eq!(fs::read(&input).unwrap(), original);
            continue;
        }
        assert_eq!(group.record.start_time(), Some(-0.333));
        assert_eq!(group.record.in_point(), Some(0.9995));
        assert_eq!(group.record.out_point(), Some(1.333));
        assert_eq!(group.record.stretch(), Some(1.0));
        let sample = 20.0 / 30.0;
        assert!(group.record.start_time().unwrap() + group.record.in_point().unwrap() <= sample);
        let remap = numeric_properties(&group.content, "ADBE Time Remapping");
        assert_eq!(remap.len(), 1);
        let keys = &remap[0].keyframes;
        assert_eq!(keys.len(), 3);
        assert_eq!(keys[0].values, [0.0]);
        assert_eq!(keys[1].values, [0.0]);
        assert_eq!(keys[2].values, [1.0]);
        assert_eq!(keys[0].time_secs, 0.0);
        assert_eq!(keys[1].time_secs, 1.0);
        assert_eq!(keys[2].time_secs, 2.0);
        let aftereffects_file::structure::ItemKind::Composition(source) =
            &native.item(group.record.source_id()).unwrap().kind
        else {
            panic!("pre-roll source composition");
        };
        assert_eq!(source.duration_secs, 1.0);
        let child = native
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                aftereffects_file::structure::ItemKind::Composition(comp) => Some(comp),
                _ => None,
            })
            .flat_map(|comp| &comp.layers)
            .find(|layer| layer.name.as_ref() == "Child nominal clock")
            .unwrap();
        assert_eq!(child.record.start_time(), Some(0.0));
        assert_eq!(child.record.in_point(), Some(0.0));
        assert_eq!(child.record.out_point(), Some(0.5));
        assert!(keys[0].time_secs <= sample - group.record.start_time().unwrap());
        assert!(keys[1].time_secs > sample - group.record.start_time().unwrap());
        let control = aftereffects_file::structure::read_project(include_bytes!(
        "../../../../../crates/aftereffects_file/tests/fixtures/pr4442_native/sources/timing_time_remap_linear.aep"
    )).unwrap();
        let authored = control
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                aftereffects_file::structure::ItemKind::Composition(comp) => Some(comp),
                _ => None,
            })
            .flat_map(|comp| &comp.layers)
            .flat_map(|layer| numeric_properties(&layer.content, "ADBE Time Remapping"))
            .next()
            .unwrap();
        assert_eq!(authored.keyframes.first().unwrap().time_secs, 0.0);
        assert_eq!(authored.keyframes.last().unwrap().time_secs, 2.0);
        assert_eq!(
            keys[1].out_interpolation,
            authored.keyframes[0].out_interpolation
        );
        assert_eq!(
            keys[2].in_interpolation,
            authored.keyframes.last().unwrap().in_interpolation
        );
        assert_eq!(fs::read(input).unwrap(), original);
    }
}

#[test]
fn linked_nested_video_keeps_nominal_visibility_at_fractional_frame_boundary() {
    let parent = tempfile::tempdir().unwrap();
    let input = parent.path().join("source.tsrct");
    let mut value = long_picture_group();
    let child = &mut value["composition"]["layers"][0]["layers"][0];
    child["playback"] = json!({
        "type":"windowed", "inputRange":{"start":0,"duration":467},
        "mapping":{"type":"linear", "input":{"start":0,"duration":467},
            "output":{"start":200,"duration":467}}, "inputOffsetMs":0
    });
    child["sourceRange"] = json!({"start":200,"duration":467});
    archive(&value, &input);
    let original = fs::read(&input).unwrap();
    let output = parent.path().join("output");
    let mut conversion = request(&input, &output, ConversionMode::Check);
    conversion.fps = Some("30");
    let checked = crate::formats::export_premiere(&conversion).unwrap();
    assert!(!output.exists());
    conversion.mode = ConversionMode::Write;
    let written = crate::formats::export_premiere(&conversion).unwrap();
    assert_eq!(checked, written);
    assert!(xml(&output.join("project.prproj")).contains("./media/ae-0001/compositions.aep"));
    let native = aftereffects_file::structure::read_project(
        &fs::read(output.join("media/ae-0001/compositions.aep")).unwrap(),
    )
    .unwrap();
    let videos: Vec<_> = native
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            aftereffects_file::structure::ItemKind::Composition(comp) => Some(&comp.layers),
            _ => None,
        })
        .flatten()
        .filter(|layer| layer.name.as_ref() == "Source")
        .collect();
    assert_eq!(videos.len(), 1);
    let record = &videos[0].record;
    assert_eq!(record.start_time(), Some(-0.2));
    assert_eq!(record.in_point(), Some(0.2));
    assert_eq!(record.out_point(), Some(0.667));
    assert_eq!(record.stretch(), Some(1.0));
    // The 14th 30fps sample floors to 466 ms inside the Group, still in 0..467.
    let sample = 14.0 / 30.0;
    let start = record.start_time().unwrap();
    assert!(start + record.in_point().unwrap() <= sample);
    assert!(sample < start + record.out_point().unwrap());
    assert_eq!(fs::read(input).unwrap(), original);

    // Adobe's saved source-domain in/out fields use start + point * stretch.
    let control = aftereffects_file::structure::read_project(
        &fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../crates/aftereffects_file/tests/fixtures/layers/inPoint.aep"),
        )
        .unwrap(),
    )
    .unwrap();
    let comp = control
        .items
        .iter()
        .find_map(|item| match &item.kind {
            aftereffects_file::structure::ItemKind::Composition(comp)
                if item.name == "inPoint_5" =>
            {
                Some(comp)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(comp.layers[0].record.start_time(), Some(0.0));
    assert_eq!(comp.layers[0].record.in_point(), Some(5.0));
    assert_eq!(comp.layers[0].record.out_point(), Some(15.0));
}
