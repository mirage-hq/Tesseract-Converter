//! Supplementary CPU integration, not Adobe acceptance or native-render proof.
mod channel_levels;
mod channel_matte;
mod empty_root;
mod export_media;
mod invert_alpha;
mod linked_alpha;
mod lumetri;
mod lumetri_nested;
mod lumetri_vignette;
mod offset;
mod pixel_motion_blur;
mod rates;
mod sampled;
mod white_balance;

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

/// Relink a temporary copy of a native fixture to its checked-in media.
/// The source bytes stay pinned; no author-host absolute path must exist.
fn relinked_native_fixture(source: &Path) -> (tempfile::TempDir, PathBuf) {
    use std::io::Write;

    let temp = tempfile::tempdir().unwrap();
    let inputs = temp.path().join("inputs");
    let projects = temp.path().join("source");
    fs::create_dir(&inputs).unwrap();
    fs::create_dir(&projects).unwrap();
    let parent = source.parent().unwrap();
    for directory in [parent.to_owned(), parent.join("../inputs")] {
        if !directory.is_dir() {
            continue;
        }
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path
                .extension()
                .is_some_and(|extension| matches!(extension.to_str(), Some("aep" | "png" | "mp4")))
            {
                fs::copy(&path, inputs.join(path.file_name().unwrap())).unwrap();
            }
        }
    }
    let original = xml(source);
    let parsed = roxmltree::Document::parse(&original).unwrap();
    let mut relinked = original.clone();
    for node in parsed
        .descendants()
        .filter(|node| node.has_tag_name("FilePath") || node.has_tag_name("ActualMediaFilePath"))
    {
        let tag = node.tag_name().name();
        let authored = node.text().unwrap();
        let name = Path::new(authored).file_name().unwrap();
        let actual = inputs.join(name).canonicalize().unwrap();
        relinked = relinked.replace(
            &format!("<{tag}>{authored}</{tag}>"),
            &format!("<{tag}>{}</{tag}>", actual.display()),
        );
    }
    let native = projects.join("native.prproj");
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(relinked.as_bytes()).unwrap();
    fs::write(&native, encoder.finish().unwrap()).unwrap();
    (temp, native)
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
    archive_with_video(value, path, "video-30fps.mp4");
}

const NATIVE_ONLY_VIDEO: &str = "feature_hdr_hlg_hvc1.mov";

// HDR passes through native Premiere but remains outside AE preparation.
// Ordinary SDR HEVC now prepares successfully, so it cannot exercise fallback.
fn unsupported_video_archive(value: &Value, path: &Path) {
    fn match_source_metadata(layers: &mut [Value]) {
        for layer in layers {
            if layer["type"] == "Video" {
                layer["sourceIntrinsicDuration"] = json!(2000);
                if let Some(rect) = layer["source"].get_mut("sourceRect") {
                    rect["width"] = json!(320);
                    rect["height"] = json!(180);
                }
            }
            if let Some(children) = layer["layers"].as_array_mut() {
                match_source_metadata(children);
            }
        }
    }
    let mut value = value.clone();
    match_source_metadata(value["composition"]["layers"].as_array_mut().unwrap());
    archive_with_video(&value, path, NATIVE_ONLY_VIDEO);
}

fn archive_with_video(value: &Value, path: &Path, video: &str) {
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(value).unwrap())
        .unwrap()
        .add_asset("premiere-video-1", fixture(video), AssetKind::Video)
        .unwrap()
        .add_asset("music", fixture("audio-mono.wav"), AssetKind::Audio)
        .unwrap()
        .write(path)
        .unwrap();
}

fn numeric_properties(
    chunks: &[aftereffects_file::rifx::Chunk],
    name: &str,
) -> Vec<aftereffects_file::properties::NumericProperty> {
    let mut result = Vec::new();
    for (index, chunk) in chunks.iter().enumerate() {
        if chunk.id() == *b"tdmn"
            && chunk
                .data_payload()
                .is_some_and(|bytes| bytes.starts_with(format!("{name}\0").as_bytes()))
        {
            if let Some(storage) = chunks[index + 1..]
                .iter()
                .take_while(|chunk| chunk.id() != *b"tdmn")
                .find(|chunk| chunk.list_kind() == Some(*b"tdbs"))
                .and_then(aftereffects_file::rifx::Chunk::children)
            {
                result.push(aftereffects_file::properties::read_numeric(storage).unwrap());
            }
        }
        if let Some(children) = chunk.children() {
            result.extend(numeric_properties(children, name));
        }
    }
    result
}

fn request<'a>(input: &'a Path, output: &'a Path, mode: ConversionMode) -> ConversionRequest<'a> {
    ConversionRequest {
        input,
        output,
        sequence: None,
        composition: None,
        expression_samples: None,
        available_fonts: None,
        media_map: None,
        media_relink: None,
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

/// Inspect our written native object graph, not an FX round trip. All expected
/// bounds are milliseconds; native ticks must match them exactly.
fn assert_native_forward_parts(text: &str, name: &str, expected: &[(i64, i64, i64, i64, f64)]) {
    fn field<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
        text.split_once(&format!("<{tag}>"))?
            .1
            .split_once(&format!("</{tag}>"))
            .map(|(value, _)| value)
    }
    fn reference<'a>(text: &'a str, tag: &str) -> &'a str {
        text.split_once(&format!("<{tag} ObjectRef=\""))
            .unwrap()
            .1
            .split_once('"')
            .unwrap()
            .0
    }
    fn object<'a>(text: &'a str, tag: &str, id: &str) -> &'a str {
        text.split_once(&format!("<{tag} ObjectID=\"{id}\""))
            .unwrap()
            .1
            .split_once(&format!("</{tag}>"))
            .unwrap()
            .0
    }
    let ticks = |value: Option<&str>| value.unwrap_or("0").parse::<i64>().unwrap();
    let mut parts = Vec::new();
    let mut sources = Vec::new();
    for item in text.split("<VideoClipTrackItem ObjectID=").skip(1) {
        let item = item.split_once("</VideoClipTrackItem>").unwrap().0;
        let sub = object(text, "SubClip", reference(item, "SubClip"));
        if field(sub, "Name") != Some(name) {
            continue;
        }
        let clip = object(text, "VideoClip", reference(sub, "Clip"));
        parts.push((
            ticks(field(item, "Start")),
            ticks(field(item, "End")),
            ticks(field(clip, "InPoint")),
            ticks(field(clip, "OutPoint")),
            field(clip, "PlaybackSpeed")
                .unwrap_or("1")
                .parse::<f64>()
                .unwrap(),
        ));
        assert!(!matches!(field(clip, "PlayBackwards"), Some("true" | "1")));
        assert_eq!(ticks(field(clip, "FrameHold")), 0);
        sources.push(reference(clip, "Source"));
    }
    parts.sort_by_key(|part| (part.0, part.1));
    let expected: Vec<_> = expected
        .iter()
        .map(|&(start, end, source_in, source_out, rate)| {
            (
                start * 254_016_000,
                end * 254_016_000,
                source_in * 254_016_000,
                source_out * 254_016_000,
                rate,
            )
        })
        .collect();
    assert_eq!(parts, expected);
    assert!(!sources.is_empty());
    assert!(sources.iter().all(|source| source == &sources[0]));
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
        unsupported_video_archive(&value, &input);
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
        // The HDR profile remains unsupported by AEP export. A partially
        // successful scope must not delete the retained native picture.
        for name in [NATIVE_ONLY_VIDEO, "audio-mono.wav"] {
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
        assert!(text.contains(&format!("./media/{NATIVE_ONLY_VIDEO}")));
        assert_eq!(text.matches("<AudioClipTrackItem ObjectID=").count(), 1);
    }
}

/// The backdrop scope of the test above with two more glow overlays, and a
/// rectangle whose 256-character name After Effects cannot write while the
/// native export keeps it: After Effects loses that one rectangle, the
/// native export loses the three overlays' glows, so the linked scope
/// replaces the native picture and says so.
#[test]
fn hybrid_prefers_the_linked_scope_when_it_preserves_more_than_the_native_export() {
    let parent = tempfile::tempdir().unwrap();
    let input = parent.path().join("source.tsrct");
    let mut value = document_value();
    let roots = value["composition"]["layers"].as_array_mut().unwrap();
    roots[1]["blendMode"] = json!("screen");
    for (offset, id) in [(1, 11), (2, 12)] {
        let mut overlay = roots[1].clone();
        overlay["id"] = json!(id);
        overlay["effects"][0]["id"] = json!(100 + id);
        overlay["activeRange"] = json!({"start": 200 + 100 * offset, "duration": 300});
        roots.insert(1, overlay);
    }
    let mut unnamed = roots[1].clone();
    unnamed["id"] = json!(13);
    unnamed["name"] = json!("x".repeat(256));
    unnamed["blendMode"] = json!("normal");
    unnamed.as_object_mut().unwrap().remove("effects");
    roots.insert(4, unnamed);
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
    let hybrid: Vec<_> = written
        .diagnostics
        .iter()
        .filter(|d| {
            d.code.starts_with("HYBRID")
                || d.context
                    .as_deref()
                    .is_some_and(|context| context.starts_with("AE fallback"))
        })
        .map(|d| format!("{} {:?} {}", d.code, d.context, d.message))
        .collect();
    assert!(
        output.join("media/ae-0001/compositions.aep").is_file(),
        "{hybrid:#?}"
    );
    let preferred = written
        .diagnostics
        .iter()
        .find(|d| d.code == "HYBRID-LINKED-PREFERRED")
        .expect("the linked scope is preferred");
    assert_eq!(preferred.context.as_deref(), Some("roots 1..=6"));
    assert!(
        preferred
            .message
            .contains("partly converts 3 shown picture layers")
            && preferred
                .message
                .contains("After Effects omits 1 whole, including 1"),
        "{}",
        preferred.message
    );
    assert!(!written
        .diagnostics
        .iter()
        .any(|d| d.code == "HYBRID-NATIVE-RETAINED"));
    // The scope's omission is still reported with the linked scope, the
    // native picture is replaced, and the sound stays native.
    let scoped: Vec<_> = written
        .diagnostics
        .iter()
        .filter(|d| {
            d.context
                .as_deref()
                .is_some_and(|c| c.starts_with("AEP scope 1"))
        })
        .map(|d| format!("{} {:?} {}", d.code, d.context, d.message))
        .collect();
    assert!(scoped.iter().any(|d| d.contains("omitted")), "{scoped:#?}");
    let text = xml(&output.join("project.prproj"));
    assert!(text.contains("./media/ae-0001/compositions.aep"));
    assert_eq!(text.matches("<AudioClipTrackItem ObjectID=").count(), 1);
}

fn long_picture_group() -> Value {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../../../crates/premiere_file/tests/fixtures/editable-video.json"
    ))
    .unwrap();
    let mut child = value["composition"]["layers"][0].take();
    child["parent"] = json!(30);
    child["playback"]["inputRange"]["duration"] = json!(500);
    child["playback"]["mapping"]["input"]["duration"] = json!(500);
    child["playback"]["mapping"]["output"]["duration"] = json!(500);
    child["sourceRange"]["duration"] = json!(500);
    value["composition"]["layers"][0] = json!({
        "type": "Group", "id": 30, "name": "Long plain group",
        "playback": {
            "type": "windowed", "inputRange": {"start": 0, "duration": 1000},
            "mapping": {"type": "linear", "input": {"start": 0, "duration": 1000},
                "output": {"start": 0, "duration": 1000}}, "inputOffsetMs": 0
        },
        "transform": value["composition"]["layers"][1]["transform"], "layers": [child]
    });
    value
}

#[test]
fn hybrid_long_plain_group_prepares_silent_video_and_preserves_child_window() {
    let parent = tempfile::tempdir().unwrap();
    let input = parent.path().join("source.tsrct");
    let mut value = long_picture_group();
    // The one-second selected group includes a transparent tail after its
    // half-second child. It can cover that interval without a black canvas.
    let marker = &mut value["composition"]["layers"][1];
    marker["name"] = json!("Tail marker");
    marker["rect"]["fillColor"] = json!([1, 0, 0, 1]);
    marker["activeRange"] = json!({"start": 900, "duration": 100});
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
    assert_eq!(fs::read(&input).unwrap(), original);
    assert!(!written
        .diagnostics
        .iter()
        .any(|d| d.code == "HYBRID-NATIVE-RETAINED"));

    let aep = output.join("media/ae-0001/compositions.aep");
    let native = aftereffects_file::structure::read_project(&fs::read(&aep).unwrap()).unwrap();
    let aftereffects_file::structure::ItemKind::Composition(root) = &native.item(1).unwrap().kind
    else {
        panic!("selected root composition");
    };
    assert_eq!((root.width, root.height), (1920, 1080));
    assert_eq!(root.duration_secs, 1.0);
    assert_eq!(root.frame_rate, 30.0);
    let picture: Vec<_> = native
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            aftereffects_file::structure::ItemKind::Composition(comp) => Some(&comp.layers),
            _ => None,
        })
        .flatten()
        .collect();
    assert!(picture
        .iter()
        .all(|layer| !layer.record.flags().audio_enabled));
    let videos: Vec<_> = picture
        .iter()
        .filter(|layer| layer.name.as_ref() == "Source")
        .collect();
    assert_eq!(videos.len(), 1);
    let video = videos[0];
    assert_eq!(video.record.start_time(), Some(0.0));
    assert_eq!(video.record.in_point(), Some(0.0));
    assert_eq!(video.record.out_point(), Some(0.5));
    assert_eq!(video.record.stretch(), Some(1.0));
    let footage = native.item(video.record.source_id()).unwrap();
    let media = footage.native_media.as_ref().unwrap().as_ref().unwrap();
    assert_eq!(media.source_format, *b"MOoV");
    assert_eq!((media.width, media.height), (1920, 1080));
    assert_eq!(media.duration.seconds(), 1.0);
    assert_eq!(media.audio_sample_rate, 0.0);
    assert!(media.authored_path.ends_with(".mp4"));
    assert!(aep.parent().unwrap().join(&media.authored_path).is_file());

    let prproj = output.join("project.prproj");
    let text = xml(&prproj);
    assert!(text.contains("./media/ae-0001/compositions.aep"));
    assert!(!text.contains("./media/video-30fps.mp4"));
    let (premiere, _) = premiere_file::PrProjectFile::load(&prproj).unwrap();
    let sequence = premiere
        .sequences()
        .find(|sequence| sequence.name() == "Fresh exact 30")
        .unwrap()
        .id()
        .unwrap()
        .to_owned();
    let imported = parent.path().join("imported");
    let mut reimport = request(&prproj, &imported, ConversionMode::Write);
    reimport.sequence = Some(&sequence);
    import_premiere(&reimport).unwrap();
    let result = TesseractFile::open(imported.join("project.tsrct")).unwrap();
    let all = layers(result.project().composition().layers());
    let linked: Vec<_> = all
        .iter()
        .filter_map(|layer| match layer.data() {
            LayerData::Group(group) if group.name.starts_with("Premiere linked composition") => {
                Some(group)
            }
            _ => None,
        })
        .collect();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].playback.input_range().start.as_millis(), 0);
    assert_eq!(linked[0].playback.input_range().duration.as_millis(), 1000);
    let videos: Vec<_> = all
        .iter()
        .filter_map(|layer| match layer.data() {
            LayerData::Video(video) => Some(video),
            _ => None,
        })
        .collect();
    assert_eq!(videos.len(), 1);
    // AE import retains the full physical source on the editable Video and
    // applies the half-second occurrence through its parent source clock.
    let clock = all
        .iter()
        .find_map(|layer| match layer.data() {
            LayerData::Group(group) if Some(group.id) == videos[0].parent => Some(group),
            _ => None,
        })
        .unwrap();
    assert_eq!(clock.name, "Source content clock");
    assert!(!clock.is_hidden);
    assert_eq!(clock.playback.input_range().start.as_millis(), 0);
    assert_eq!(clock.playback.input_range().duration.as_millis(), 500);
    assert_eq!(clock.playback.input_offset_ms(), 0);
    let remap = clock.playback.time_remap().unwrap();
    assert_eq!(remap.before(), fx_schema::TimeRemapExtrapolation::Inactive);
    assert_eq!(remap.after(), fx_schema::TimeRemapExtrapolation::Inactive);
    assert_eq!(
        remap
            .keyframes()
            .iter()
            .map(|key| (key.time.as_millis(), key.value.as_millis()))
            .collect::<Vec<_>>(),
        [(0, 0), (500, 500)]
    );
    assert_eq!(videos[0].playback.input_range().start.as_millis(), 0);
    assert_eq!(videos[0].playback.input_range().duration.as_millis(), 1000);
    assert_eq!(videos[0].source_range.start.as_millis(), 0);
    assert_eq!(videos[0].source_range.duration.as_millis(), 1000);
    assert_eq!(videos[0].source_intrinsic_duration.as_millis(), 1000);
}

#[test]
fn hybrid_long_plain_group_retains_native_footage_when_ae_omits_it() {
    let parent = tempfile::tempdir().unwrap();
    let input = parent.path().join("source.tsrct");
    unsupported_video_archive(&long_picture_group(), &input);
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
        .any(|d| d.code == "HYBRID-NATIVE-RETAINED"));
    assert_eq!(fs::read(&input).unwrap(), original);
    assert_eq!(
        fs::read(output.join("media").join(NATIVE_ONLY_VIDEO)).unwrap(),
        fs::read(fixture(NATIVE_ONLY_VIDEO)).unwrap()
    );
    assert!(!output.join("media/ae-0001").exists());
    let imported = parent.path().join("imported");
    let prproj = output.join("project.prproj");
    let (native, _) = premiere_file::PrProjectFile::load(&prproj).unwrap();
    let sequence = native
        .sequences()
        .find(|sequence| sequence.name() == "Fresh exact 30")
        .unwrap()
        .id()
        .unwrap()
        .to_owned();
    let mut reimport = request(&prproj, &imported, ConversionMode::Write);
    reimport.sequence = Some(&sequence);
    import_premiere(&reimport).unwrap();
    let result = TesseractFile::open(imported.join("project.tsrct")).unwrap();
    let all = layers(result.project().composition().layers());
    let group = all
        .iter()
        .find_map(|layer| match layer.data() {
            LayerData::Group(group) if group.name == "Long plain group" => Some(group),
            _ => None,
        })
        .unwrap();
    assert_eq!(group.playback.input_range().duration.as_millis(), 500);
    let videos: Vec<_> = all
        .iter()
        .filter_map(|layer| match layer.data() {
            LayerData::Video(video) => Some(video),
            _ => None,
        })
        .collect();
    assert_eq!(videos.len(), 1);
    assert_eq!(videos[0].playback.input_range().duration.as_millis(), 500);
    assert_eq!(videos[0].source_range.duration.as_millis(), 500);
}

#[test]
fn hybrid_long_plain_group_still_rejects_nested_sound() {
    let parent = tempfile::tempdir().unwrap();
    let input = parent.path().join("source.tsrct");
    let mut value = long_picture_group();
    let mut audio = document_value()["composition"]["layers"][0].clone();
    audio["parent"] = json!(30);
    value["composition"]["layers"][0]["layers"]
        .as_array_mut()
        .unwrap()
        .push(audio);
    unsupported_video_archive(&value, &input);
    let output = parent.path().join("output");
    let error = export(
        &request(&input, &output, ConversionMode::Write),
        &Default::default(),
    )
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("contains nested audio"),
        "{error:#}"
    );
    assert!(!output.exists());
}

#[test]
fn hybrid_long_plain_group_exports_an_uncovered_tail_without_a_canvas() {
    let parent = tempfile::tempdir().unwrap();
    let input = parent.path().join("source.tsrct");
    let mut value = long_picture_group();
    // Keep the document's end beyond the shortened nest without a black canvas.
    let marker = &mut value["composition"]["layers"][1];
    marker["name"] = json!("Tail marker");
    marker["rect"]["fillColor"] = json!([1, 0, 0, 1]);
    marker["activeRange"] = json!({"start": 900, "duration": 100});
    unsupported_video_archive(&value, &input);
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
    let text = xml(&output.join("project.prproj"));
    assert_native_forward_parts(&text, NATIVE_ONLY_VIDEO, &[(0, 500, 0, 500, 1.0)]);
    assert!(text.contains("<MZ.WorkOutPoint>254016000000</MZ.WorkOutPoint>"));
    assert_eq!(text.matches("<VideoClipTrackItem ObjectID=").count(), 3);
    assert_eq!(fs::read(&input).unwrap(), original);
    assert_eq!(
        fs::read(output.join("media").join(NATIVE_ONLY_VIDEO)).unwrap(),
        fs::read(fixture(NATIVE_ONLY_VIDEO)).unwrap()
    );
}

#[test]
fn hybrid_ramp_only_picture_retains_native_endpoints_when_ae_rejects_media() {
    let parent = tempfile::tempdir().unwrap();
    let input = parent.path().join("source.tsrct");
    let mut value = document_value();
    let roots = value["composition"]["layers"].as_array_mut().unwrap();
    roots.retain(|layer| layer["id"] == 1 || layer["id"] == 20);
    let video = roots.iter_mut().find(|layer| layer["id"] == 1).unwrap();
    video["source"]
        .as_object_mut()
        .unwrap()
        .remove("sourceRect");
    video["playback"] = json!({
        "type": "windowed", "inputRange": {"start": 0, "duration": 1000},
        "mapping": {"type": "timeRemap", "property": {
            "before": "inactive", "after": "inactive", "keyframes": [
                {"id": "in", "time": 0, "value": 0, "easing": {"type": "linear"}},
                {"id": "middle", "time": 500, "value": 200, "easing": {"type": "linear"}},
                {"id": "out", "time": 1000, "value": 800, "easing": {"type": "linear"}}
            ]
        }}, "inputOffsetMs": 0
    });
    unsupported_video_archive(&value, &input);
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
        .any(|d| d.code == "HYBRID-NATIVE-RETAINED"));
    assert!(written
        .diagnostics
        .iter()
        .any(|d| d.message.contains("editable speed and Frame Hold segments")));
    assert!(!output.join("media/ae-0001").exists());
    for name in [NATIVE_ONLY_VIDEO, "audio-mono.wav"] {
        assert_eq!(
            fs::read(output.join("media").join(name)).unwrap(),
            fs::read(fixture(name)).unwrap()
        );
    }
    let text = xml(&output.join("project.prproj"));
    assert_native_forward_parts(
        &text,
        NATIVE_ONLY_VIDEO,
        &[(0, 500, 0, 200, 0.4), (500, 1000, 200, 800, 1.2)],
    );
    assert_eq!(text.matches("<VideoClipTrackItem ObjectID=").count(), 2);
    assert_eq!(text.matches("<AudioClipTrackItem ObjectID=").count(), 1);
    assert!(text.contains(&format!("./media/{NATIVE_ONLY_VIDEO}")));
    assert_eq!(fs::read(input).unwrap(), original);
}

#[test]
fn hybrid_supported_ae_ramp_replaces_native_approximation_with_original_curve() {
    let parent = tempfile::tempdir().unwrap();
    let input = parent.path().join("source.tsrct");
    let mut value = document_value();
    value["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .retain(|layer| layer["id"] == 1);
    let video = &mut value["composition"]["layers"][0];
    video["sourceIntrinsicDuration"] = json!(8000);
    video["sourceRange"] = json!({"start": 0, "duration": 8000});
    video["source"]
        .as_object_mut()
        .unwrap()
        .remove("sourceRect");
    let property = json!({"before": "inactive", "after": "inactive", "keyframes": [
        {"id": "guard-in", "time": 0, "value": 0, "easing": {"type": "linear"}},
        {"id": "in", "time": 125, "value": 125, "easing": {"type": "linear"}},
        {"id": "middle", "time": 500, "value": 250, "easing": {"type": "linear"}},
        {"id": "out", "time": 1125, "value": 875, "easing": {"type": "linear"}},
        {"id": "guard-out", "time": 1250, "value": 1000, "easing": {"type": "linear"}}
    ]});
    video["playback"] = json!({
        "type": "windowed", "inputRange": {"start": 0, "duration": 1000},
        "mapping": {"type": "timeRemap", "property": property}, "inputOffsetMs": 125
    });
    let media = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/aftereffects_file/tests/fixtures/media_native_panel/media/movie.mov");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .add_asset("premiere-video-1", &media, AssetKind::Video)
        .unwrap()
        .write(&input)
        .unwrap();
    let archive = TesseractFile::open(&input).unwrap();
    let prepared = premiere_file::Premiere
        .prepare_export(&archive, archive.project(), &Default::default())
        .unwrap();
    assert_eq!(
        prepared.prepared_document().to_json_value().unwrap()["composition"]["layers"][0]
            ["playback"]["mapping"]["property"],
        property
    );
    assert!(prepared.losses().losses.iter().any(|loss| matches!(
        loss.kind,
        premiere_file::ExportLossKind::Field(premiere_file::ExportField::TimeRemap)
    )));
    let output = parent.path().join("output");
    let written = export(
        &request(&input, &output, ConversionMode::Write),
        &Default::default(),
    )
    .unwrap();
    assert!(
        !written
            .diagnostics
            .iter()
            .any(|d| d.code == "HYBRID-NATIVE-RETAINED"),
        "{written:?}"
    );
    assert!(output.join("media/ae-0001/compositions.aep").is_file());
    let text = xml(&output.join("project.prproj"));
    assert!(text.contains("./media/ae-0001/compositions.aep"));
    assert!(!text.contains("./media/movie.mov"));
    fn remap_keys(
        chunks: &[aftereffects_file::rifx::Chunk],
    ) -> Option<aftereffects_file::properties::NumericProperty> {
        for (index, chunk) in chunks.iter().enumerate() {
            if chunk.id() == *b"tdmn"
                && chunk
                    .data_payload()
                    .is_some_and(|name| name.starts_with(b"ADBE Time Remapping\0"))
            {
                let storage = chunks[index + 1..]
                    .iter()
                    .take_while(|chunk| chunk.id() != *b"tdmn")
                    .find(|chunk| chunk.list_kind() == Some(*b"tdbs"))
                    .and_then(aftereffects_file::rifx::Chunk::children)
                    .unwrap();
                return Some(aftereffects_file::properties::read_numeric(storage).unwrap());
            }
        }
        chunks
            .iter()
            .filter_map(aftereffects_file::rifx::Chunk::children)
            .find_map(remap_keys)
    }
    // Inspect native keys directly: the FX importer currently rejects the
    // leading guard key's negative layer-local time.
    let bytes = fs::read(output.join("media/ae-0001/compositions.aep")).unwrap();
    let aep = aftereffects_file::aep::Project::parse(&bytes).unwrap();
    let remap = remap_keys(&aep.chunks).expect("linked AE retains the original editable ramp");
    assert_eq!(remap.keyframes.len(), 5);
    for (key, (time, value)) in remap.keyframes.iter().zip([
        (-0.125, 0.0),
        (0.0, 0.125),
        (0.375, 0.25),
        (1.0, 0.875),
        (1.125, 1.0),
    ]) {
        assert!((key.time_secs - time).abs() < 1e-9);
        assert_eq!(key.values.len(), 1);
        assert!((key.values[0] - value).abs() < 1e-9);
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
    unsupported_video_archive(&value, &input);
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
        .any(|d| d.message.contains("editable speed and Frame Hold segments")));
    assert!(written
        .diagnostics
        .iter()
        .any(|d| d.code == "HYBRID-NATIVE-RETAINED"));
    assert!(output.join("media/ae-0001/compositions.aep").is_file());
    assert_eq!(
        fs::read(output.join("media").join(NATIVE_ONLY_VIDEO)).unwrap(),
        fs::read(fixture(NATIVE_ONLY_VIDEO)).unwrap()
    );
    let text = xml(&output.join("project.prproj"));
    // The nonlinear child shares the untouched source with its unit sibling.
    // Its two authored legs remain on the selected 0..1000 ms clock.
    assert_native_forward_parts(
        &text,
        NATIVE_ONLY_VIDEO,
        &[
            (0, 500, 0, 300, 0.6),
            (0, 1000, 0, 1000, 1.0),
            (500, 1000, 300, 1000, 1.4),
        ],
    );
    assert_eq!(
        fs::read(output.join("media/audio-mono.wav")).unwrap(),
        fs::read(fixture("audio-mono.wav")).unwrap()
    );
    // Only the independently representable overlay enters the AE scope;
    // rejected HDR footage is never published as a prepared AE asset.
    let mut media: Vec<_> = fs::read_dir(output.join("media"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    media.sort();
    assert_eq!(
        media,
        ["ae-0001", "audio-mono.wav", NATIVE_ONLY_VIDEO].map(std::ffi::OsString::from)
    );
    assert_eq!(
        fs::read_dir(output.join("media/ae-0001")).unwrap().count(),
        1
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
fn hybrid_picture_replacement_exports_without_a_black_canvas() {
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
fn hybrid_interleaved_dependencies_leave_no_output() {
    let parent = tempfile::tempdir().unwrap();
    let mut value = document_value();
    let output = parent.path().join("output");
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
