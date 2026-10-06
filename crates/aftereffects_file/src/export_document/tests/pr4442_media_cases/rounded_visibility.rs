//! Fresh-export boundary regressions; supplementary to independent Adobe proof.

use super::*;

fn cut_video(id: u64) -> Value {
    let mut clip = video(id, "cut-source");
    clip.as_object_mut().unwrap().remove("activeRange");
    clip["playback"] = fixture_linear_playback(
        json!({"start":10042,"duration":125}),
        json!({"start":800,"duration":125}),
    );
    clip["sourceRange"] = json!({"start":800,"duration":125});
    clip
}

fn sources() -> BTreeMap<String, media::ResolvedMediaSource> {
    BTreeMap::from([(
        "cut-source".into(),
        resolved(
            "cut-source",
            "media/cut.mov",
            NativeSourceFormat::QuickTime,
            [1280, 720],
            4000,
            NativeFrameRate::integer(30),
            48_000.0,
        ),
    )])
}

fn parent_window(layer: &crate::structure::Layer) -> (f64, f64) {
    let start = layer.record.start_time().unwrap();
    let stretch = layer.record.stretch().unwrap();
    (
        start + stretch * layer.record.in_point().unwrap(),
        start + stretch * layer.record.out_point().unwrap(),
    )
}

#[test]
fn rounded_visibility_matches_fx_cut_frames_without_retiming_the_source() {
    let doc = document(vec![cut_video(60100)], Vec::new());
    let output = to_aep_with_media(&doc, &sources()).unwrap();
    let native = read_project(&output.bytes).unwrap();
    let layer = &layers(&native)[0];
    let (begin, end) = parent_window(layer);
    for frame in 239..=245 {
        let seconds = f64::from(frame) / 24.0;
        let fx_millis = fx_schema::Time::from_secs(seconds).as_millis();
        assert_eq!(
            begin <= seconds && seconds < end,
            (10042..10167).contains(&fx_millis),
            "frame {frame}, native window {begin}..{end}"
        );
    }
    assert_eq!(layer.record.start_time_fraction(), (4621, 500));
    assert_eq!(layer.record.stretch_fraction(), (1, 1));
    assert!(
        root_runs(&layer.content)
            .unwrap()
            .iter()
            .all(|(name, _)| *name != "ADBE Time Remapping")
    );
}

#[test]
fn rounded_visibility_retains_audio_and_authored_remap_windows() {
    let mut audible = cut_video(60101);
    audible["volume"] = json!(1.0);
    let mut remapped = cut_video(60102);
    remapped["playback"] = fixture_remapped_playback(
        json!({"start":10042,"duration":125}),
        json!({
            "keyframes":[
                {"id":"cut-a","time":10042,"value":800,"easing":{"type":"linear"}},
                {"id":"cut-b","time":10167,"value":925,"easing":{"type":"linear"}}
            ],
            "before":"inactive","after":"inactive"
        }),
    );
    let doc = document(vec![audible, remapped], Vec::new());
    let output = to_aep_with_media(&doc, &sources()).unwrap();
    let native = read_project(&output.bytes).unwrap();
    for layer in layers(&native) {
        let (begin, end) = parent_window(layer);
        assert!((begin - 10.042).abs() < 1e-12);
        assert!((end - 10.167).abs() < 1e-12);
    }
}

#[test]
fn rounded_visibility_diagnoses_source_head_without_omitting_video() {
    let mut head = cut_video(60103);
    head["sourceRange"] = json!({"start":0,"duration":125});
    head["playback"] = fixture_linear_playback(
        json!({"start":10042,"duration":125}),
        json!({"start":0,"duration":125}),
    );
    let doc = document(vec![head], Vec::new());
    let output = to_aep_with_media(&doc, &sources()).unwrap();
    let native = read_project(&output.bytes).unwrap();
    assert_eq!(layers(&native).len(), 1);
    assert_eq!(parent_window(&layers(&native)[0]), (10.042, 10.167));
    assert!(output.diagnostics.iter().any(|diagnostic| {
        diagnostic.layer_id == Some(LayerId::new(60103))
            && diagnostic
                .message
                .contains("rounded-millisecond visibility")
            && diagnostic.message.contains("nominal")
    }));
}
