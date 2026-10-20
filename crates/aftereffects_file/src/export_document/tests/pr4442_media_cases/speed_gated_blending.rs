//! Supplementary fresh-export regression; no Adobe execution or licensed inputs.
use super::*;

#[test]
fn nested_video_blending_gates_only_through_unit_rate_ancestors() {
    for (label, ancestor_duration, source_span, expected) in [
        ("unit nested", 2000, 2000, false),
        ("slow nested", 2000, 1000, true),
        ("retimed ancestor", 4000, 2000, true),
    ] {
        let mut movie = video(400, "movie");
        movie["name"] = json!(label);
        movie["parent"] = json!(500);
        movie.as_object_mut().unwrap().remove("activeRange");
        movie["sourceRange"] = json!({"start":0,"duration":4000});
        movie["frameBlending"] = json!(true);
        movie["playback"] = fixture_remapped_playback(
            json!({"start":0,"duration":2000}),
            json!({"keyframes":[
                {"id":"a","time":0,"value":0,"easing":{"type":"linear"}},
                {"id":"b","time":2000,"value":source_span,"easing":{"type":"linear"}}
            ],"before":"inactive","after":"inactive"}),
        );
        let mut inner = group(500, "inner", vec![movie]);
        inner.as_object_mut().unwrap().remove("activeRange");
        inner["playback"] = fixture_linear_playback(
            json!({"start":0,"duration":2000}),
            json!({"start":0,"duration":2000}),
        );
        inner["parent"] = json!(600);
        let mut outer = group(600, "outer", vec![inner]);
        outer.as_object_mut().unwrap().remove("activeRange");
        outer["playback"] = fixture_linear_playback(
            json!({"start":1000,"duration":ancestor_duration}),
            json!({"start":0,"duration":2000}),
        );
        let doc = document(vec![outer], Vec::new());
        let sources = BTreeMap::from([(
            "movie".into(),
            resolved(
                "movie",
                "media/movie.mov",
                NativeSourceFormat::QuickTime,
                [640, 360],
                4000,
                NativeFrameRate::integer(24),
                0.0,
            ),
        )]);
        let output = to_aep_with_media(&doc, &sources).unwrap();
        let native = read_project(&output.bytes).unwrap();
        let exported = native
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::Composition(comp) => Some(&comp.layers),
                _ => None,
            })
            .flatten()
            .find(|layer| layer.name.as_ref() == label)
            .unwrap();
        assert_eq!(exported.record.flags().frame_blending, expected, "{label}");
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.message.contains("omitted")),
            "{label}"
        );
    }
}

#[test]
fn root_video_blending_follows_playback_speed_with_mixed_clock_diagnostic() {
    for (label, points, blend, approximation) in [
        ("unit", vec![(0, 500), (2000, 2500)], false, false),
        ("fast", vec![(0, 0), (2000, 4000)], false, false),
        ("slow", vec![(0, 500), (2000, 1500)], true, false),
        ("reverse slow", vec![(0, 1500), (2000, 500)], true, false),
        ("frozen", vec![(0, 500), (2000, 500)], false, false),
        (
            "mixed",
            vec![(0, 500), (1000, 1500), (2000, 2000)],
            false,
            true,
        ),
    ] {
        let mut movie = video(400, "movie");
        movie["name"] = json!(label);
        movie.as_object_mut().unwrap().remove("activeRange");
        movie["sourceRange"] = json!({"start":0,"duration":4000});
        movie["frameBlending"] = json!(true);
        movie["playback"] = fixture_remapped_playback(
            json!({"start":0,"duration":2000}),
            json!({
                "keyframes": points.iter().enumerate().map(|(index, (time, value))| json!({
                    "id":format!("key-{index}"), "time":time, "value":value,
                    "easing":{"type":"linear"}
                })).collect::<Vec<_>>(),
                "before":"inactive", "after":"inactive"
            }),
        );
        let doc = document(vec![movie], Vec::new());
        let sources = BTreeMap::from([(
            "movie".into(),
            resolved(
                "movie",
                "media/movie.mov",
                NativeSourceFormat::QuickTime,
                [640, 360],
                4000,
                NativeFrameRate::integer(24),
                0.0,
            ),
        )]);
        let output = to_aep_with_media(&doc, &sources).unwrap();
        let native = read_project(&output.bytes).unwrap();
        let timeline = layers(&native);
        let exported = timeline
            .iter()
            .find(|layer| layer.name.as_ref() == label)
            .unwrap();
        assert_eq!(exported.record.flags().frame_blending, blend, "{label}");
        assert_eq!(
            output
                .diagnostics
                .iter()
                .any(|d| d.message.contains("mixed-speed")),
            approximation,
            "{label}"
        );
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.message.contains("omitted")),
            "{label}"
        );
    }
}
