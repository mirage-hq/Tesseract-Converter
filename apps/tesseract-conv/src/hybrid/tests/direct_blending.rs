//! Direct-AE regression using publishable native media and frame-mix fixture.
use super::*;

#[test]
fn direct_ae_mixed_speed_boolean_blending_retains_slow_interval_synthesis() {
    mixed_speed_blending(false, false);
}

#[test]
fn direct_ae_nested_mixed_speed_video_keeps_interval_sampling_and_source_clock() {
    mixed_speed_blending(true, false);
}

#[test]
fn direct_ae_nested_mixed_speed_video_keeps_retimed_ancestor_unsplit() {
    mixed_speed_blending(true, true);
}

fn mixed_speed_blending(nested: bool, retimed: bool) {
    for (start, offset, times, values, hold) in [
        (0, 0, [0, 500, 1000], [0, 500, 750], false),
        (250, 250, [0, 1000, 1500], [0, 1000, 1250], false),
        (250, 250, [0, 1000, 1500], [1500, 500, 250], false),
        (250, 250, [0, 1000, 1500], [1500, 500, 250], true),
    ] {
        let parent = tempfile::tempdir().unwrap();
        let input = parent.path().join("source.tsrct");
        let mut value = document_value();
        value["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .retain(|layer| layer["id"] == 1);
        let video = &mut value["composition"]["layers"][0];
        video["sourceIntrinsicDuration"] = json!(8000);
        video["sourceRange"] = json!({"start":0,"duration":8000});
        video["source"]
            .as_object_mut()
            .unwrap()
            .remove("sourceRect");
        video["frameBlending"] = json!(true);
        video["playback"] = json!({
            "type":"windowed", "inputRange":{"start":start,"duration":1000}, "inputOffsetMs":offset,
            "mapping":{"type":"timeRemap", "property":{
                "before":"inactive", "after":"inactive", "keyframes":[
                    {"id":"blend-in","time":times[0],"value":values[0],"easing":{"type":"linear"}},
                    {"id":"blend-middle","time":times[1],"value":values[1],"easing":{"type":if hold {"hold"} else {"linear"}}},
                    {"id":"blend-out","time":times[2],"value":values[2],"easing":{"type":"linear"}}
                ]
            }}
        });
        if nested {
            let mut movie = value["composition"]["layers"][0].clone();
            movie["parent"] = json!(80);
            let mut transform = movie["transform"].clone();
            transform["position"] = json!([30, 40]);
            let rect: Value = serde_json::from_str(include_str!(
                "../../../../../crates/aftereffects_file/tests/fixtures/hybrid/rect-identity.fx.json"
            )).unwrap();
            let mut sibling = rect["composition"]["layers"][0].clone();
            sibling["id"] = json!(90);
            value["composition"]["layers"] = json!([{
                "id":80, "type":"Group", "name":"translated movie owner",
                "playback":{"type":"windowed","inputRange":{"start":2000,"duration":if retimed {4000} else {2000}},
                    "inputOffsetMs":0,"mapping":{"type":"linear",
                        "input":{"start":2000,"duration":if retimed {4000} else {2000}},
                        "output":{"start":0,"duration":2000}}},
                "transform":transform, "layers":[movie]
            }, sibling]);
            value["duration"] = json!(if retimed { 6.0 } else { 4.0 });
        }
        let media = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../crates/aftereffects_file/tests/fixtures/media_native_panel/media/movie.mov",
        );
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&value).unwrap())
            .unwrap()
            .add_asset("premiere-video-1", &media, AssetKind::Video)
            .unwrap()
            .write(&input)
            .unwrap();
        let original = fs::read(&input).unwrap();
        let output = parent.path().join("output");
        crate::formats::export_after_effects(&request(&input, &output, ConversionMode::Write))
            .unwrap();
        let linked = aftereffects_file::structure::read_project(
            &fs::read(output.join("project.aep")).unwrap(),
        )
        .unwrap();
        let videos = linked
            .items
            .iter()
            .filter_map(|item| {
                let aftereffects_file::structure::ItemKind::Composition(comp) = &item.kind else {
                    return None;
                };
                Some(comp.layers.iter().filter(|layer| {
                    !numeric_properties(&layer.content, "ADBE Time Remapping").is_empty()
                }))
            })
            .flatten()
            .collect::<Vec<_>>();
        assert_eq!(
            videos.len(),
            if retimed { 1 } else { 2 },
            "native sampling owners"
        );
        let mut windows = videos
            .iter()
            .map(|layer| {
                (
                    layer.record.start_time().unwrap() + layer.record.in_point().unwrap(),
                    layer.record.start_time().unwrap() + layer.record.out_point().unwrap(),
                    layer.record.flags().frame_blending,
                )
            })
            .collect::<Vec<_>>();
        windows.sort_by(|a, b| a.0.total_cmp(&b.0));
        let start_secs = f64::from(start) / 1000.0;
        let expected = if retimed {
            vec![(start_secs, start_secs + 1.0, true)]
        } else {
            vec![
                (start_secs, start_secs + 0.5, false),
                (start_secs + 0.5, start_secs + 1.0, true),
            ]
        };
        assert_eq!(windows, expected);
        for video in videos {
            let remap = numeric_properties(&video.content, "ADBE Time Remapping");
            assert_eq!(remap.len(), 1);
            let keys = &remap[0].keyframes;
            assert_eq!(keys.len(), 3);
            for (key, (time, value)) in keys.iter().zip(times.into_iter().zip(values)) {
                let time = f64::from(time - offset) / 1000.0;
                assert!((video.record.start_time().unwrap() + key.time_secs - time).abs() < 1e-9);
                assert_eq!(key.values, vec![f64::from(value) / 1000.0]);
            }
        }
        let oracle = aftereffects_file::structure::read_project(include_bytes!(
        "../../../../../crates/aftereffects_file/tests/fixtures/pr4442_native/sources/media_frame_mix.aep"
    )).unwrap();
        assert!(oracle.items.iter().any(|item| matches!(&item.kind,
        aftereffects_file::structure::ItemKind::Composition(comp) if comp.layers.iter().any(|layer| layer.record.flags().frame_blending))));
        let disabled_oracle = aftereffects_file::structure::read_project(include_bytes!(
            "../../../../../crates/aftereffects_file/tests/fixtures/pr4442_native/sources/media_video.aep"
        )).unwrap();
        let disabled = disabled_oracle
            .items
            .iter()
            .find_map(|item| {
                let aftereffects_file::structure::ItemKind::Composition(comp) = &item.kind else {
                    return None;
                };
                comp.layers
                    .iter()
                    .find(|layer| layer.name.as_ref() == "media_video")
            })
            .expect("native disabled frame-mix control");
        assert!(!disabled.record.flags().frame_blending);
        assert_eq!(fs::read(input).unwrap(), original);
    }
}
