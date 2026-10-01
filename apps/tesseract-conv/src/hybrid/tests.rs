//! Supplementary CPU integration, not Adobe acceptance or native-render proof.
use super::*;
use crate::formats::import_premiere;
use fx_conv::{ConversionMode, ExportFromTesseract, ImportToTesseract};
use fx_schema::{Layer, LayerData};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tesseract_file::{AssetKind, TesseractFileBuilder};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/premiere_file/tests/fixtures")
        .join(name)
}

fn document_value() -> Value {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../../../crates/premiere_file/tests/fixtures/editable-video.json"
    ))
    .unwrap();
    let rect: Value = serde_json::from_str(include_str!(
        "../../../../crates/aftereffects_file/tests/fixtures/hybrid/rect-identity.fx.json"
    ))
    .unwrap();
    let mut overlay = rect["composition"]["layers"][0].clone();
    overlay["id"] = json!(10);
    overlay["activeRange"] = json!({"start":200,"duration":600});
    overlay["rect"]["strokeEnabled"] = json!(true);
    overlay["rect"]["strokeWidth"] = json!(4);
    // Premiere now handles this rectangle natively; Glow selects the AEP scope.
    overlay["effects"] = json!([{"id":100,"effect":{
        "type":"glow","glowThreshold":20,"glowRadius":8,"glowIntensity":0.5
    }}]);
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(0, overlay);
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(
            0,
            json!({
                "type":"Audio","id":20,"name":"Native sound",
                "playback": {
                    "type": "windowed", "inputRange": {"start": 100, "duration": 200},
                    "mapping": {"type": "linear", "input": {"start": 100, "duration": 200},
                        "output": {"start": 0, "duration": 200}},
                    "inputOffsetMs": 0
                },
                "sourceRange":{"start":0,"duration":200},
                "sourceIntrinsicDuration":200,"volume":0.5,"source":{"assetId":"music"}
            }),
        );
    value
}

fn archive(value: &Value, path: &Path) {
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(value).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            fixture("video-30fps.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset("music", fixture("audio-mono.wav"), AssetKind::Audio)
        .unwrap()
        .write(path)
        .unwrap();
}

fn request<'a>(input: &'a Path, output: &'a Path, mode: ConversionMode) -> ConversionRequest<'a> {
    ConversionRequest {
        input,
        output,
        sequence: None,
        composition: None,
        expression_samples: None,
        media_map: None,
        fps: None,
        mode,
        progress: fx_conv::Progress::default(),
    }
}

fn xml(path: &Path) -> String {
    let bytes = fs::read(path).unwrap();
    let mut xml = String::new();
    flate2::read::GzDecoder::new(&bytes[..])
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

fn layers(roots: &[Layer]) -> Vec<&Layer> {
    let mut result = Vec::new();
    for layer in roots {
        result.push(layer);
        if let Some(children) = layer.child_layers() {
            result.extend(layers(children));
        }
    }
    result
}

#[test]
fn hybrid_check_write_and_reimport_preserve_native_sound_and_editable_picture() {
    let parent = tempfile::tempdir().unwrap();
    let input = parent.path().join("source.tsrct");
    archive(&document_value(), &input);
    let original = fs::read(&input).unwrap();
    let output = parent.path().join("output");
    let events = Mutex::new(Vec::new());
    let observe = |event| events.lock().unwrap().push(event);
    let mut observed = request(&input, &output, ConversionMode::Check);
    observed.progress = fx_conv::Progress::new(&observe);
    let checked = export(&observed, &Default::default()).unwrap();
    assert!(!output.exists());
    observed.mode = ConversionMode::Write;
    let written = export(&observed, &Default::default()).unwrap();
    for format in ["AE", "Premiere"] {
        let completed = events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| {
                event.phase.contains(format)
                    && !event.started
                    && event.total.is_some_and(|total| total > 0)
                    && event.completed == event.total
            })
            .count();
        assert!(
            completed >= 2,
            "both Check and Write must observe {format} work"
        );
    }
    assert_eq!(checked, written);
    assert_eq!(written.artifacts.len(), 4);
    for artifact in &written.artifacts {
        assert!(output.join(&artifact.path).is_file());
    }
    for name in ["video-30fps.mp4", "audio-mono.wav"] {
        assert_eq!(
            fs::read(output.join("media").join(name)).unwrap(),
            fs::read(fixture(name)).unwrap()
        );
    }
    let prproj = output.join("project.prproj");
    let original_project = fs::read(&prproj).unwrap();
    let text = xml(&prproj);
    assert!(text.contains("./media/ae-0001/compositions.aep"));
    assert!(!text.contains(".conversion-"));
    let imported = parent.path().join("imported");
    import_premiere(&request(&prproj, &imported, ConversionMode::Check)).unwrap();
    assert!(!imported.exists());
    import_premiere(&request(&prproj, &imported, ConversionMode::Write)).unwrap();
    let result = TesseractFile::open(imported.join("project.tsrct")).unwrap();
    let all = layers(result.project().composition().layers());
    assert_eq!(
        all.iter()
            .filter(|l| matches!(l.data(), LayerData::Video(_)))
            .count(),
        1
    );
    let audio: Vec<_> = all
        .iter()
        .filter_map(|l| {
            if let LayerData::Audio(a) = l.data() {
                Some(a)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(audio.len(), 1);
    assert_eq!(audio[0].volume.as_f64(), 0.5);
    assert!(all
        .iter()
        .any(|l| matches!(l.data(), LayerData::Rect(r) if r.rect.size == [120.0,80.0])));
    assert_eq!(fs::read(&input).unwrap(), original);
    assert!(export(
        &request(&input, &output, ConversionMode::Write),
        &Default::default()
    )
    .is_err());
    assert_eq!(fs::read(&prproj).unwrap(), original_project);
}

#[test]
fn hybrid_owner_lookup_keeps_layers_beyond_1024() {
    let mut value = document_value();
    let mut rect = value["composition"]["layers"][1].clone();
    rect.as_object_mut().unwrap().remove("effects");
    value["composition"]["layers"] = Value::Array(
        (1..=2049)
            .map(|id| {
                let mut layer = rect.clone();
                layer["id"] = json!(id);
                layer
            })
            .collect(),
    );
    let document = fx_schema::EditableFxCompositionDocument::from_json_value(value).unwrap();
    let owners = owners::Owners::new(document.composition().layers()).unwrap();
    assert_eq!(owners.layer(fx_schema::LayerId::new(2049)).unwrap(), 2048);
}

#[test]
fn hybrid_picture_backdrop_does_not_absorb_standalone_audio() {
    for audio_index in [0, 1, 3] {
        let parent = tempfile::tempdir().unwrap();
        let input = parent.path().join("source.tsrct");
        let mut value = document_value();
        let roots = value["composition"]["layers"].as_array_mut().unwrap();
        let audio = roots.remove(0);
        roots[0]["blendMode"] = json!("screen");
        roots.insert(audio_index, audio);
        archive(&value, &input);
        let output = parent.path().join("output");
        let checked = export(
            &request(&input, &output, ConversionMode::Check),
            &Default::default(),
        )
        .unwrap();
        assert!(!output.exists());
        let written = export(
            &request(&input, &output, ConversionMode::Write),
            &Default::default(),
        )
        .unwrap();
        assert_eq!(checked, written);
        // AE cannot export this MP4. A partially successful scope must not
        // delete the native picture that preparation already retained.
        for name in ["video-30fps.mp4", "audio-mono.wav"] {
            assert_eq!(
                fs::read(output.join("media").join(name)).unwrap(),
                fs::read(fixture(name)).unwrap()
            );
        }
        assert!(!output.join("media/ae-0001").exists());
        assert!(written
            .diagnostics
            .iter()
            .any(|d| d.code == "HYBRID-NATIVE-RETAINED"));
        let text = xml(&output.join("project.prproj"));
        assert!(text.contains("./media/video-30fps.mp4"));
        assert_eq!(text.matches("<AudioClipTrackItem ObjectID=").count(), 1);
    }
}

#[test]
fn hybrid_missing_frame_on_nonlinear_video_keeps_native_siblings_and_reports_ae_limits() {
    let parent = tempfile::tempdir().unwrap();
    let input = parent.path().join("source.tsrct");
    let mut value = document_value();
    let roots = value["composition"]["layers"].as_array_mut().unwrap();
    let mut native = roots[2].take();
    native["parent"] = json!(30);
    let mut nonlinear = native.clone();
    nonlinear["id"] = json!(31);
    nonlinear["source"]
        .as_object_mut()
        .unwrap()
        .remove("sourceRect");
    nonlinear["playback"] = json!({
        "type": "windowed", "inputRange": {"start": 0, "duration": 1000},
        "mapping": {"type": "timeRemap", "property": {
            "before": "inactive", "after": "inactive", "keyframes": [
                {"id": "in", "time": 0, "value": 0, "easing": {"type": "linear"}},
                {"id": "middle", "time": 500, "value": 300, "easing": {"type": "linear"}},
                {"id": "out", "time": 1000, "value": 1000, "easing": {"type": "linear"}}
            ]
        }}, "inputOffsetMs": 0
    });
    roots[2] = json!({
        "type": "Group", "id": 30, "name": "Native and nonlinear footage",
        "playback": {"type": "windowed", "inputRange": {"start": 0, "duration": 1000},
            "mapping": {"type": "linear", "input": {"start": 0, "duration": 1000},
                "output": {"start": 0, "duration": 1000}}, "inputOffsetMs": 0},
        "transform": roots[3]["transform"], "layers": [nonlinear, native]
    });
    archive(&value, &input);
    let original = fs::read(&input).unwrap();
    let output = parent.path().join("output");
    let checked = export(
        &request(&input, &output, ConversionMode::Check),
        &Default::default(),
    )
    .unwrap();
    assert!(!output.exists());
    let written = export(
        &request(&input, &output, ConversionMode::Write),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(checked, written);
    assert!(written
        .diagnostics
        .iter()
        .any(|d| d.message.contains("time remapping was not exported")));
    assert!(written
        .diagnostics
        .iter()
        .any(|d| d.code == "HYBRID-NATIVE-RETAINED"));
    assert!(output.join("media/ae-0001/compositions.aep").is_file());
    assert_eq!(
        fs::read(output.join("media/video-30fps.mp4")).unwrap(),
        fs::read(fixture("video-30fps.mp4")).unwrap()
    );
    assert_eq!(fs::read(input).unwrap(), original);
}

#[test]
fn hybrid_complete_picture_scope_can_cross_standalone_audio() {
    let parent = tempfile::tempdir().unwrap();
    let input = parent.path().join("source.tsrct");
    let mut value = document_value();
    let roots = value["composition"]["layers"].as_array_mut().unwrap();
    let audio = roots.remove(0);
    roots.retain(|layer| layer["id"] != 1);
    roots[0]["blendMode"] = json!("screen");
    roots.insert(1, audio);
    archive(&value, &input);
    let output = parent.path().join("output");
    export(
        &request(&input, &output, ConversionMode::Write),
        &Default::default(),
    )
    .unwrap();
    assert!(output.join("media/ae-0001/compositions.aep").is_file());
    assert_eq!(
        fs::read(output.join("media/audio-mono.wav")).unwrap(),
        fs::read(fixture("audio-mono.wav")).unwrap()
    );
    assert_eq!(
        xml(&output.join("project.prproj"))
            .matches("<AudioClipTrackItem ObjectID=")
            .count(),
        1
    );
}

#[test]
fn hybrid_group_child_blend_includes_the_editable_backdrop_in_ae() {
    let parent = tempfile::tempdir().unwrap();
    let input = parent.path().join("source.tsrct");
    let mut value = document_value();
    let mut child = value["composition"]["layers"][1].clone();
    child["parent"] = json!(30);
    child["blendMode"] = json!("screen");
    let mut backdrop = value["composition"]["layers"][3].clone();
    backdrop["name"] = json!("Red backdrop");
    backdrop["rect"]["fillColor"] = json!([1, 0, 0, 1]);
    value["composition"]["layers"] = json!([
        {"type":"Group", "id":30, "name":"Pass-through group",
         "playback":{"type":"windowed", "inputRange":{"start":0,"duration":1000}, "mapping":{"type":"linear", "input":{"start":0,"duration":1000}, "output":{"start":0,"duration":1000}}, "inputOffsetMs":0},
         "transform":backdrop["transform"], "layers":[child]},
        backdrop
    ]);
    archive(&value, &input);
    let output = parent.path().join("output");
    export(
        &request(&input, &output, ConversionMode::Write),
        &Default::default(),
    )
    .unwrap();
    let prproj = output.join("project.prproj");
    let (native, _) = premiere_file::PrProjectFile::load(&prproj).unwrap();
    let sequence = native.sequences().next().unwrap();
    assert_eq!(sequence.video_tracks().map(<[_]>::len).sum::<usize>(), 1);
    let imported = parent.path().join("imported");
    import_premiere(&request(&prproj, &imported, ConversionMode::Write)).unwrap();
    let result = TesseractFile::open(imported.join("project.tsrct")).unwrap();
    assert!(layers(result.project().composition().layers())
        .iter()
        .any(|layer| layer.name() == "Red backdrop"));
}

#[test]
fn hybrid_replaces_picture_before_validating_native_gaps() {
    let parent = tempfile::tempdir().unwrap();
    let input = parent.path().join("source.tsrct");
    let mut value = document_value();
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .retain(|layer| layer["id"] == 10 || layer["id"] == 20);
    archive(&value, &input);
    let output = parent.path().join("output");
    let checked = export(
        &request(&input, &output, ConversionMode::Check),
        &Default::default(),
    )
    .unwrap();
    assert!(!output.exists());
    let written = export(
        &request(&input, &output, ConversionMode::Write),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(checked, written);
    assert!(output.join("media/ae-0001/compositions.aep").is_file());
    assert!(output.join("media/audio-mono.wav").is_file());
    assert!(xml(&output.join("project.prproj")).contains("./media/ae-0001/compositions.aep"));
}

#[test]
fn hybrid_custom_canvas_scopes_and_links_keep_the_document_size_both_ways() {
    // A portrait document: its AEP scope composition, the Premiere sequence
    // and the link's placement and stream take the document canvas, and the
    // native video keeps its own 1920x1080 frame. Reimport resolves the link
    // through the canvas check at that size.
    let parent = tempfile::tempdir().unwrap();
    let mut value = document_value();
    value["dimensions"] = json!({"width": 1080, "height": 1920});
    let canvas = &mut value["composition"]["layers"][3];
    assert_eq!(canvas["name"], "Black canvas");
    canvas["rect"]["size"] = json!([1080, 1920]);
    let input = parent.path().join("source.tsrct");
    archive(&value, &input);
    let output = parent.path().join("output");
    export(
        &request(&input, &output, ConversionMode::Write),
        &Default::default(),
    )
    .unwrap();

    let aep = output.join("media/ae-0001/compositions.aep");
    let inventory: Value =
        serde_json::from_str(&crate::inspect::inspect(&aep, true).unwrap()).unwrap();
    let sizes: Vec<_> = inventory["compositions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|composition| [composition["width"].clone(), composition["height"].clone()])
        .collect();
    assert_eq!(sizes, [[json!(1080), json!(1920)]]);
    let prproj = output.join("project.prproj");
    let text = xml(&prproj);
    let mut frames: Vec<_> = text
        .split("<FrameRect>")
        .skip(1)
        .map(|rest| rest.split_once("</FrameRect>").unwrap().0)
        .collect();
    frames.sort_unstable();
    // The sequence, both placements and the link stream at the canvas; the
    // native video stream at its source frame.
    assert_eq!(
        frames,
        [
            "0,0,1080,1920",
            "0,0,1080,1920",
            "0,0,1080,1920",
            "0,0,1080,1920",
            "0,0,1920,1080"
        ]
    );

    let imported = parent.path().join("imported");
    import_premiere(&request(&prproj, &imported, ConversionMode::Write)).unwrap();
    let result = TesseractFile::open(imported.join("project.tsrct")).unwrap();
    let dimensions = result.project().dimensions();
    assert_eq!([dimensions.width, dimensions.height], [1080, 1920]);
    let all = layers(result.project().composition().layers());
    assert!(all
        .iter()
        .any(|l| matches!(l.data(), LayerData::Rect(r) if r.rect.size == [120.0, 80.0])));
    assert!(all.iter().any(|l| matches!(l.data(), LayerData::Rect(r)
        if r.name == "Linked composition canvas" && r.rect.size == [1080.0, 1920.0])));
}

#[test]
fn hybrid_native_only_uses_the_ordinary_converter() {
    let parent = tempfile::tempdir().unwrap();
    let mut value = document_value();
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .retain(|v| v["id"] != 10);
    let input = parent.path().join("source.tsrct");
    archive(&value, &input);
    let native = parent.path().join("native");
    let hybrid = parent.path().join("hybrid");
    premiere_file::Premiere
        .export_from_tesseract(&input, &native, &Default::default(), ConversionMode::Write)
        .unwrap();
    let events = Mutex::new(Vec::new());
    let callback = |event| events.lock().unwrap().push(event);
    let mut hybrid_request = request(&input, &hybrid, ConversionMode::Write);
    hybrid_request.progress = fx_conv::Progress::new(&callback);
    export(&hybrid_request, &Default::default()).unwrap();
    assert!(events.lock().unwrap().iter().any(|event| {
        event.total.is_some_and(|total| total > 0) && event.completed == event.total
    }));
    // Native writer UUIDs are intentionally fresh on each export. Compare the
    // resulting editable content rather than asserting byte-identical UUIDs.
    let mut documents = Vec::new();
    for (index, output) in [&native, &hybrid].into_iter().enumerate() {
        let imported = parent.path().join(format!("readback-{index}"));
        premiere_file::Premiere
            .import_to_tesseract(
                &output.join("project.prproj"),
                &imported,
                &Default::default(),
                ConversionMode::Write,
            )
            .unwrap();
        let archive = TesseractFile::open(imported.join("project.tsrct")).unwrap();
        documents.push(serde_json::to_value(archive.project()).unwrap());
    }
    assert_eq!(documents[0], documents[1]);
    assert!(!hybrid.join("media/ae-0001").exists());
}

#[test]
fn hybrid_middle_scopes_with_equal_guids_import_their_own_edited_files() {
    let parent = tempfile::tempdir().unwrap();
    let mut value = document_value();
    let mut other = value["composition"]["layers"][1].clone();
    other["id"] = json!(11);
    other["effects"][0]["id"] = json!(101);
    other["rect"]["size"] = json!([220, 80]);
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(3, other);
    let input = parent.path().join("source.tsrct");
    archive(&value, &input);
    let output = parent.path().join("output");
    export(
        &request(&input, &output, ConversionMode::Write),
        &Default::default(),
    )
    .unwrap();
    for scope in [1, 2] {
        assert!(output
            .join(format!("media/ae-{scope:04}/compositions.aep"))
            .is_file());
    }
    // Supplementary edit: regenerate only the first actual AEP from changed FX.
    // This is not independent Adobe-authored edit evidence.
    value["composition"]["layers"][1]["rect"]["size"] = json!([171, 80]);
    let edited = parent.path().join("edited.tsrct");
    archive(&value, &edited);
    let edited = TesseractFile::open(&edited).unwrap();
    let ae = aftereffects_file::AfterEffects
        .stage_picture_layers(
            &edited,
            edited.project(),
            1..2,
            parent.path(),
            &aftereffects_file::AfterEffectsExportOptions { fps: 30.0 },
        )
        .unwrap();
    fs::copy(
        ae.directory().join("project.aep"),
        output.join("media/ae-0001/compositions.aep"),
    )
    .unwrap();
    let imported = parent.path().join("imported");
    import_premiere(&request(
        &output.join("project.prproj"),
        &imported,
        ConversionMode::Write,
    ))
    .unwrap();
    let result = TesseractFile::open(imported.join("project.tsrct")).unwrap();
    let all = layers(result.project().composition().layers());
    for width in [171.0, 220.0] {
        assert!(all
            .iter()
            .any(|l| matches!(l.data(), LayerData::Rect(r) if r.rect.size == [width,80.0])));
    }
    assert!(!all
        .iter()
        .any(|l| matches!(l.data(), LayerData::Rect(r) if r.rect.size == [120.0,80.0])));
    assert_eq!(
        all.iter()
            .filter(|l| matches!(l.data(), LayerData::Audio(_)))
            .count(),
        1
    );
}

#[test]
fn hybrid_unsupported_clock_and_interleaved_dependencies_leave_no_output() {
    let parent = tempfile::tempdir().unwrap();
    let input = parent.path().join("source.tsrct");
    let mut value = document_value();
    archive(&value, &input);
    let output = parent.path().join("output");
    let error = export(
        &request(&input, &output, ConversionMode::Write),
        &PremiereExportOptions {
            frame_rate: Some(premiere_file::FrameRate::Fps30000Over1001),
        },
    )
    .unwrap_err();
    assert!(!error.to_string().is_empty());
    assert!(!output.exists());
    // Parent references cannot jump across an unrelated retained video scope.
    value["composition"]["layers"][1]["parent"] = json!(2);
    let related = parent.path().join("related.tsrct");
    archive(&value, &related);
    assert!(export(
        &request(&related, &output, ConversionMode::Write),
        &Default::default()
    )
    .is_err());
    assert!(!output.exists());
    assert!(fs::read_dir(parent.path()).unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".conversion-")));
}

#[test]
fn premiere_cli_import_places_native_linked_compositions_on_their_clip_clocks() {
    // H-IDENTITY-01: Premiere links red at 0-1 s from source 0 and blue at
    // 1-2 s from source 0.5 s. The production route imports both pictures
    // through premiere_file's built-in linked import.
    let parent = tempfile::tempdir().unwrap();
    let package = parent.path().join("package");
    fs::create_dir(&package).unwrap();
    let identity = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/aftereffects_file/tests/fixtures/hybrid/identity");
    for name in ["native-linked.prproj", "linked-compositions.aep"] {
        fs::copy(identity.join(name), package.join(name)).unwrap();
    }
    fs::copy(
        fixture("feature_linked_av_source.mp4"),
        package.join("background.mp4"),
    )
    .unwrap();
    let output = parent.path().join("imported");
    import_premiere(&request(
        &package.join("native-linked.prproj"),
        &output,
        ConversionMode::Write,
    ))
    .unwrap();
    let archive = TesseractFile::open(output.join("project.tsrct")).unwrap();
    let all = layers(archive.project().composition().layers());
    assert_eq!(
        all.iter()
            .filter(|layer| matches!(layer.data(), LayerData::Video(_)))
            .count(),
        1,
        "the native background converts beside the linked pictures"
    );
    let groups: Vec<_> = all
        .iter()
        .filter_map(|layer| match layer.data() {
            LayerData::Group(group) if group.name.starts_with("Premiere linked composition") => {
                Some(group)
            }
            _ => None,
        })
        .collect();
    assert_eq!(groups.len(), 2);
    let clock = |playback: &fx_schema::LayerPlayback| {
        assert_eq!(playback.input_offset_ms(), 0);
        match playback.mapping() {
            fx_schema::LayerPlaybackMapping::TimeRemap { property } => property
                .keyframes()
                .iter()
                .map(|key| (key.time.as_millis(), key.value.as_millis()))
                .collect::<Vec<_>>(),
            fx_schema::LayerPlaybackMapping::Linear { input, output } => vec![
                (input.start.as_millis(), output.start.as_millis()),
                (input.end().as_millis(), output.end().as_millis()),
            ],
        }
    };
    for (group, start, source) in [
        (groups[0], 0, None),
        (groups[1], 1000, Some([(0, 500), (1000, 1500)])),
    ] {
        // The clip group keeps the clip clock and seeds the runtime's clock.
        assert_eq!(group.playback.input_range().start.as_millis(), start);
        assert_eq!(clock(&group.playback), [(start, 0), (start + 1000, 1000)]);
        let picture = match (&group.layers[..], source) {
            ([child], Some(expected)) => {
                let LayerData::Group(source_group) = child.data() else {
                    panic!("source group");
                };
                assert!(source_group.name.starts_with("Premiere linked source"));
                assert_eq!(clock(&source_group.playback), expected);
                &source_group.layers
            }
            (_, None) => &group.layers,
            (children, Some(_)) => panic!("{} children", children.len()),
        };
        // The composition's root is clipped to its 1920x1080 canvas.
        let [root, guide] = &picture[..] else {
            panic!("{} picture layers", picture.len());
        };
        let (LayerData::Group(root), LayerData::Rect(guide)) = (root.data(), guide.data()) else {
            panic!("root group and canvas guide");
        };
        assert_eq!(guide.name, "Linked composition canvas");
        assert_eq!(guide.rect.size, [1920.0, 1080.0]);
        assert_eq!(guide.rect.position, [0.0, 0.0]);
        assert_eq!(guide.active_range, root.playback.input_range());
        let [mask] = &root.masks[..] else {
            panic!("one canvas mask");
        };
        assert_eq!(mask.layer, Some(guide.id));
        assert_eq!(mask.mode, fx_schema::MaskMode::Add);
    }
}
