use super::{
    support::{
        edit_record, first_project, premiere_to_tesseract, read_xml, tesseract_to_premiere,
        write_prproj,
    },
    test_support::editable_document,
};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Converts one pinned Adobe-derived case of `tests/manifest.json`
/// directly from `tests/fixtures`, where its media lives.
fn convert_pinned_case(
    root: &Path,
    project: &str,
    sequence: &str,
) -> (PathBuf, Vec<premiere_file::Omission>) {
    let output = root.join("tesseract");
    let omissions =
        premiere_to_tesseract(fixture(project), &output, Some(sequence), false).unwrap();
    (first_project(&output), omissions)
}

/// Each layer's type, timing, level, and packaged source file name.
fn editable_layers(file: &TesseractFile) -> Value {
    let document = file.project_json().unwrap();
    document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|layer| editable_layer(file, layer))
        .collect()
}

/// One layer's type, timing, level, and packaged source file name.
fn editable_layer(file: &TesseractFile, layer: &Value) -> Value {
    let source = layer["source"]["assetId"].as_str().map(|id| {
        let asset = &file.metadata().assets[id];
        let name = Path::new(&asset.path).file_name().unwrap();
        json!([name.to_str().unwrap(), format!("{:?}", asset.kind)])
    });
    json!({
        "type": layer["type"],
        "activeRange": (*crate::test_support::layer_range(layer)),
        "sourceRange": layer["sourceRange"],
        "volume": layer["volume"],
        "source": source,
    })
}

/// Exports a document, moves the package, and imports it again. The package
/// must not depend on its original location or on the source archive.
fn relocated_round_trip(root: &Path, archive: &Path) -> TesseractFile {
    let native = root.join("native");
    let original = fs::read(archive).unwrap();
    tesseract_to_premiere(archive, &native, false).unwrap();
    assert_eq!(fs::read(archive).unwrap(), original);
    fs::remove_file(archive).unwrap();
    let relocated = root.join("relocated");
    fs::rename(&native, &relocated).unwrap();
    let again = root.join("again");
    let omissions =
        premiere_to_tesseract(relocated.join("project.prproj"), &again, None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    TesseractFile::open(first_project(&again)).unwrap()
}

#[test]
fn adobe_audio_clips_keep_placement_trim_level_and_channels() {
    // A1 plays click-left.wav from 0 s. A2 plays tone-right.wav from 1 s, with a
    // 0.25 s source trim and a static 0.5 track fader. The AME export of this
    // project confirms both placements, the trim, and the linear 0.5 gain.
    let temp = tempfile::tempdir().unwrap();
    let (archive, omissions) = convert_pinned_case(
        temp.path(),
        "feature_audio_clips_strict.prproj",
        "093e7f82-8e3b-4fd4-9a66-1234f931ffd8",
    );
    let file = TesseractFile::open(&archive).unwrap();
    // Only the base project's picture color settings are reported.
    assert!(
        omissions
            .iter()
            .all(|item| item.record == "VideoTrackGroup:80"),
        "{omissions:?}"
    );
    assert_eq!(file.project_json().unwrap()["duration"], 5.0);
    let expected = json!([
        {"type": "Audio", "activeRange": {"start": 0, "duration": 5000},
         "sourceRange": {"start": 0, "duration": 5000}, "volume": 1.0,
         "source": ["feature_audio_click_left.wav", "Audio"]},
        {"type": "Audio", "activeRange": {"start": 1000, "duration": 4000},
         "sourceRange": {"start": 250, "duration": 4000}, "volume": 0.5,
         "source": ["feature_audio_tone_right.wav", "Audio"]},
        {"type": "Rect", "activeRange": {"start": 0, "duration": 5000},
         "sourceRange": null, "volume": null, "source": null},
    ]);
    assert_eq!(editable_layers(&file), expected);
    let audio_has_no_ducking = |file: &TesseractFile| {
        let document = file.project_json().unwrap();
        let audio = document["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|layer| layer["type"] == "Audio")
            .collect::<Vec<_>>();
        assert_eq!(audio.len(), 2);
        assert!(audio.iter().all(|layer| layer.get("autoDucking").is_none()));
    };
    audio_has_no_ducking(&file);
    let again = relocated_round_trip(temp.path(), &archive);
    assert_eq!(editable_layers(&again), expected);
    audio_has_no_ducking(&again);
}

#[test]
fn adobe_linked_av_source_plays_its_sound_once() {
    // Premiere linked one picture and one sound placement of an H.264/AAC file.
    // The editable result keeps one Video-kind asset: a silent picture layer and
    // one audio layer. The AME export plays the sound once, at unity gain.
    let temp = tempfile::tempdir().unwrap();
    let (archive, omissions) = convert_pinned_case(
        temp.path(),
        "feature_linked_av_strict.prproj",
        "80acdd81-0a96-4677-b17f-b2ffe2dff738",
    );
    let file = TesseractFile::open(&archive).unwrap();
    assert!(
        omissions
            .iter()
            .all(|item| item.record == "VideoTrackGroup:114"
                || item.record == "MasterClip:6fed5564-291d-4b94-8c5a-dbecb76dbaa4"),
        "{omissions:?}"
    );
    assert_eq!(file.metadata().assets.len(), 1);
    let whole = json!({"start": 0, "duration": 5000});
    let source = json!(["feature_linked_av_source.mp4", "Video"]);
    let expected = json!([
        {"type": "Video", "activeRange": whole, "sourceRange": whole, "volume": 0.0,
         "source": source},
        {"type": "Audio", "activeRange": whole, "sourceRange": whole, "volume": 1.0,
         "source": source},
        {"type": "Rect", "activeRange": whole, "sourceRange": null, "volume": null,
         "source": null},
    ]);
    assert_eq!(editable_layers(&file), expected);
    let again = relocated_round_trip(temp.path(), &archive);
    assert_eq!(editable_layers(&again), expected);
}

#[test]
fn sound_only_mp4_source_stays_a_video_asset_through_repeated_round_trips() {
    // An edit keeps only the sound of the pinned linked A/V case, so export
    // writes an audio-only native Media for its H.264/AAC MP4. Import must
    // package that MP4 as the Video asset that export admits for MP4; the next
    // export then binds it again, with its sound and bytes unchanged.
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let (archive, _) = convert_pinned_case(
        root,
        "feature_linked_av_strict.prproj",
        "80acdd81-0a96-4677-b17f-b2ffe2dff738",
    );
    let linked = TesseractFile::open(&archive).unwrap();
    let mut document = linked.project_json().unwrap();
    document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .retain(|layer| layer["type"] != "Video");
    let [(asset_id, asset)] = linked.metadata().assets.iter().collect::<Vec<_>>()[..] else {
        panic!("the linked A/V case packages one asset");
    };
    let source = fixture("feature_linked_av_source.mp4");
    let edited = root.join("sound-only.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(asset_id, &source, asset.kind)
        .unwrap()
        .write(&edited)
        .unwrap();
    let whole = json!({"start": 0, "duration": 5000});
    let expected = json!([
        {"type": "Audio", "activeRange": whole, "sourceRange": whole, "volume": 1.0,
         "source": ["feature_linked_av_source.mp4", "Video"]},
        {"type": "Rect", "activeRange": whole, "sourceRange": null, "volume": null,
         "source": null},
    ]);
    assert_eq!(
        editable_layers(&TesseractFile::open(&edited).unwrap()),
        expected
    );
    // Every sound field but the renumbered layer ID survives each round trip.
    let sound = |document: Value| {
        let mut layer = document["composition"]["layers"][0].clone();
        layer.as_object_mut().unwrap().remove("id");
        layer
    };
    let edited_sound = sound(document);

    let mut input = edited;
    for cycle in ["first", "second"] {
        let cycle_root = root.join(cycle);
        fs::create_dir(&cycle_root).unwrap();
        let again = relocated_round_trip(&cycle_root, &input);
        let xml = read_xml(&cycle_root.join("relocated/project.prproj"));
        let native = roxmltree::Document::parse(&xml).unwrap();
        let streams: Vec<_> = native
            .descendants()
            .filter(|node| {
                node.has_tag_name("Media")
                    && node.children().any(|child| {
                        child.has_tag_name("RelativePath")
                            && child.text() == Some("./media/feature_linked_av_source.mp4")
                    })
            })
            .flat_map(|media| {
                media.children().filter(|child| {
                    child.has_tag_name("AudioStream") || child.has_tag_name("VideoStream")
                })
            })
            .map(|stream| stream.tag_name().name())
            .collect();
        assert_eq!(streams, ["AudioStream"], "{cycle}");
        assert_eq!(xml.matches("<VideoClipTrackItem ").count(), 0, "{cycle}");
        assert_eq!(xml.matches("<AudioClipTrackItem ").count(), 1, "{cycle}");
        assert_eq!(editable_layers(&again), expected, "{cycle}");
        assert_eq!(
            sound(again.project_json().unwrap()),
            edited_sound,
            "{cycle}"
        );
        let [(_, packaged)] = again.metadata().assets.iter().collect::<Vec<_>>()[..] else {
            panic!("{cycle}: one packaged source");
        };
        assert_eq!(
            (
                packaged.kind,
                packaged.content_type.as_str(),
                &packaged.sha256
            ),
            (AssetKind::Video, "video/mp4", &asset.sha256),
            "{cycle}"
        );
        input = first_project(&cycle_root.join("again"));
    }
}

/// A 200 ms audible A/V clip plus a mono music layer that starts halfway and
/// extends the document past the picture.
fn audible_document() -> Value {
    let mut document = editable_document();
    document["duration"] = json!(0.3);
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    layers[0]["volume"] = json!(0.5);
    layers[0]["playback"] = crate::test_support::linear_playback(
        json!({"start": layers[0]["playback"]["inputRange"]["start"], "duration": 200}),
        json!({"start": layers[0]["sourceRange"]["start"], "duration": 200}),
    );
    layers[0]["sourceRange"]["duration"] = json!(200);
    layers[0]["sourceIntrinsicDuration"] = json!(200);
    layers[1]["activeRange"]["duration"] = json!(300);
    layers.insert(
        0,
        json!({
            "type": "Audio",
            "id": 3,
            "name": "Music",
            "playback": crate::test_support::linear_playback(json!({"start": 100, "duration": 200}), json!({"start": 0, "duration": 200})),
            "sourceRange": {"start": 0, "duration": 200},
            "sourceIntrinsicDuration": 200,
            "volume": 2.0,
            "source": {"assetId": "music"},
        }),
    );
    document
}

#[test]
fn authored_sound_survives_export_relocation_and_reimport() {
    check_authored_sound_round_trip(false);
}

#[test]
fn legacy_media_sound_survives_export_relocation_and_reimport() {
    check_authored_sound_round_trip(true);
}

/// `bytes` with the one occurrence of `from` replaced by `to` of the same length.
fn replace_once(bytes: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut found = bytes
        .windows(from.len())
        .enumerate()
        .filter(|(_, w)| *w == from);
    let (at, _) = found.next().unwrap();
    assert!(found.next().is_none(), "{from:?} occurs more than once");
    [&bytes[..at], to, &bytes[at + from.len()..]].concat()
}

/// `audible_document` plus a 200 ms audio layer that plays the six-channel
/// `surround` asset, which conversion does not support.
fn surround_document() -> Value {
    let mut document = audible_document();
    document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(
            1,
            json!({
                "type": "Audio",
                "id": 4,
                "name": "Surround",
                "playback": crate::test_support::linear_playback(json!({"start": 0, "duration": 200}), json!({"start": 0, "duration": 200})),
                "sourceRange": {"start": 0, "duration": 200},
                "sourceIntrinsicDuration": 200,
                "volume": 1.0,
                "source": {"assetId": "surround"},
            }),
        );
    document
}

/// Writes the stereo WAV fixture relabelled as six channels to `path` and
/// returns its bytes; the PCM samples start after the 44-byte header.
fn write_surround_wav(path: &Path) -> Vec<u8> {
    let mut wav = fs::read(fixture("audio-stereo.wav")).unwrap();
    wav[22..24].copy_from_slice(&6_u16.to_le_bytes());
    wav[28..32].copy_from_slice(&(48_000_u32 * 12).to_le_bytes());
    wav[32..34].copy_from_slice(&12_u16.to_le_bytes());
    fs::write(path, &wav).unwrap();
    wav
}

#[test]
fn unsupported_sound_is_reported_while_picture_and_supported_sound_export() {
    // The audible picture's sound is PCM (`sowt`) in an MP4, where only AAC
    // converts, and another audio layer plays a six-channel WAV. Neither may
    // abort the export: the picture and the supported music still export.
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let document = surround_document();
    let pcm = root.join("pcm-sound.mp4");
    let av = fs::read(fixture("video-with-audio.mp4")).unwrap();
    fs::write(&pcm, replace_once(&av, b"mp4a", b"sowt")).unwrap();
    let surround = root.join("surround.wav");
    write_surround_wav(&surround);
    let source = root.join("source.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset("premiere-video-1", &pcm, AssetKind::Video)
        .unwrap()
        .add_asset("music", fixture("audio-mono.wav"), AssetKind::Audio)
        .unwrap()
        .add_asset("surround", &surround, AssetKind::Audio)
        .unwrap()
        .write(&source)
        .unwrap();

    let omissions = tesseract_to_premiere(&source, root.join("native"), false).unwrap();
    let xml = read_xml(&root.join("native/project.prproj"));
    assert_eq!(xml.matches("<VideoClipTrackItem ").count(), 1);
    assert_eq!(xml.matches("<AudioClipTrackItem ").count(), 1);
    assert_eq!(omissions.len(), 2, "{omissions:?}");
    assert!(
        omissions.iter().any(
            |item| item.scope == premiere_file::OmissionScope::Occurrence
                && item.record == "layer 4 (\"Surround\")"
                && item
                    .reason
                    .ends_with("only mono/stereo source audio is supported")
        ),
        "{omissions:?}"
    );
    assert!(
        omissions
            .iter()
            .any(|item| item.scope == premiere_file::OmissionScope::Feature
                && item.record == "layer 1 (\"Source\")"
                && item.reason.starts_with("embedded audio was not exported: ")
                && item.reason.contains("unsupported media type")),
        "{omissions:?}"
    );
    let mut packaged: Vec<_> = fs::read_dir(root.join("native/media"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    packaged.sort();
    assert_eq!(packaged, ["audio-mono.wav", "pcm-sound.mp4"]);
}

#[test]
fn damaged_unsupported_sound_fails_its_archive_digest_and_publishes_nothing() {
    // Unsupported sound is reported only once its archive bytes match their
    // recorded digest. A damaged entry must stop the export, not read as an
    // unsupported layout that the package then omits.
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let surround = root.join("surround.wav");
    let wav = write_surround_wav(&surround);
    let source = root.join("source.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&surround_document()).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            fixture("video-with-audio.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset("music", fixture("audio-mono.wav"), AssetKind::Audio)
        .unwrap()
        .add_asset("surround", &surround, AssetKind::Audio)
        .unwrap()
        .write(&source)
        .unwrap();
    // Assets are stored uncompressed and opening reads no payload, as in
    // `tesseract_file`'s `corrupt_asset_is_rejected_when_lazily_materialized`.
    // Change one sample byte; the six-channel header stays intact.
    let mut archive = fs::read(&source).unwrap();
    let entry = archive.windows(wav.len()).position(|w| w == wav).unwrap();
    archive[entry + 44] ^= 1;
    fs::write(&source, archive).unwrap();
    TesseractFile::open(&source).unwrap();
    for check in [true, false] {
        let output = root.join(format!("native-{check}"));
        let error = tesseract_to_premiere(&source, &output, check)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("asset \"surround\"")
                && error.ends_with("packaged media bytes failed their source hash"),
            "{error}"
        );
        assert!(!output.exists());
    }
}

#[test]
fn retimed_audio_layer_is_omitted_while_picture_and_other_sound_export() {
    // Native audio clips play at unit speed. An audio layer whose `playback`
    // plays its 200 ms source in 100 ms must not abort the export: the picture,
    // its sound and the music export, and only their media is packaged.
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let mut document = audible_document();
    document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .insert(
            1,
            json!({
                "type": "Audio",
                "id": 4,
                "name": "Fast",
                "sourceRange": {"start": 0, "duration": 200},
                "playback": crate::test_support::remapped_playback(json!({"start": 0, "duration": 100}), json!({"keyframes": [
                    {"id": "start", "time": 0, "value": 0, "easing": {"type": "linear"}},
                    {"id": "end", "time": 100, "value": 200, "easing": {"type": "linear"}}
                ], "before": "inactive", "after": "inactive"})),
                "sourceIntrinsicDuration": 200,
                "volume": 1.0,
                "source": {"assetId": "fast"},
            }),
        );
    let source = root.join("source.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            fixture("video-with-audio.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset("music", fixture("audio-mono.wav"), AssetKind::Audio)
        .unwrap()
        .add_asset("fast", fixture("audio-stereo.wav"), AssetKind::Audio)
        .unwrap()
        .write(&source)
        .unwrap();

    let omissions = tesseract_to_premiere(&source, root.join("native"), false).unwrap();
    let reported: Vec<_> = omissions
        .iter()
        .map(|item| (item.scope, item.record.as_str(), item.reason.as_str()))
        .collect();
    assert_eq!(
        reported,
        [(
            premiere_file::OmissionScope::Occurrence,
            "layer 4 (\"Fast\")",
            "retimed audio layer was not exported"
        )]
    );
    let xml = read_xml(&root.join("native/project.prproj"));
    assert_eq!(xml.matches("<VideoClipTrackItem ").count(), 1);
    assert_eq!(xml.matches("<AudioClipTrackItem ").count(), 2);
    let mut packaged: Vec<_> = fs::read_dir(root.join("native/media"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    packaged.sort();
    assert_eq!(packaged, ["audio-mono.wav", "video-with-audio.mp4"]);
}

fn check_authored_sound_round_trip(legacy: bool) {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.tsrct");
    let mut document = audible_document();
    if legacy {
        let video = &mut document["composition"]["layers"][1];
        video["activeRange"] = video["playback"]["inputRange"].clone();
        video.as_object_mut().unwrap().remove("playback");
        video["type"] = json!("Media");
        document["composition"]["layers"][1]["source"]["kind"] = json!("video");
    }
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            fixture("video-with-audio.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset("music", fixture("audio-mono.wav"), AssetKind::Audio)
        .unwrap()
        .write(&source)
        .unwrap();
    assert_eq!(
        TesseractFile::open(&source)
            .unwrap()
            .project_json()
            .unwrap(),
        document
    );
    let file = relocated_round_trip(root.path(), &source);
    let xml = read_xml(&root.path().join("relocated/project.prproj"));
    assert_eq!(xml.matches("<VideoClipTrackItem ").count(), 1);
    assert_eq!(xml.matches("<AudioClipTrackItem ").count(), 2);
    assert_eq!(xml.matches("<Media ObjectUID").count(), 2);

    // The audible picture imports as a silent picture plus its own sound.
    assert_eq!(file.project_json().unwrap()["duration"], 0.3);
    let clip = json!({"start": 0, "duration": 200});
    let picture = json!(["video-with-audio.mp4", "Video"]);
    assert_eq!(
        editable_layers(&file),
        json!([
            {"type": "Video", "activeRange": clip, "sourceRange": clip, "volume": 0.0,
             "source": picture},
            {"type": "Audio", "activeRange": clip, "sourceRange": clip, "volume": 0.5,
             "source": picture},
            {"type": "Audio", "activeRange": {"start": 100, "duration": 200},
             "sourceRange": clip, "volume": 2.0, "source": ["audio-mono.wav", "Audio"]},
            {"type": "Rect", "activeRange": {"start": 0, "duration": 300},
             "sourceRange": null, "volume": null, "source": null},
        ])
    );
    // Original bytes are packaged without transcoding.
    let music = file
        .metadata()
        .assets
        .iter()
        .find(|(_, asset)| asset.kind == AssetKind::Audio)
        .map(|(id, _)| id)
        .unwrap();
    let mut bytes = Vec::new();
    file.asset(music)
        .unwrap()
        .open()
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, fs::read(fixture("audio-mono.wav")).unwrap());
}

/// A volume key as (layer time in milliseconds, dB, easing).
type DbKey = (i64, f64, Value);

/// Each audio layer's volume in dB, with its volume keys.
fn volume_levels(file: &TesseractFile) -> Vec<(f64, Vec<DbKey>)> {
    let document = file.project_json().unwrap();
    let composition = &document["composition"];
    let db = |gain: f64| 20.0 * gain.log10();
    composition["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Audio")
        .map(|layer| {
            let keys = composition["dynamics"]["entries"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| {
                    entry["target"]
                        == json!({"kind": "layer", "layerId": layer["id"], "propertyType": "volume"})
                })
                .map_or_else(Vec::new, |entry| {
                    entry["animator"]["keyframes"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|key| {
                            (
                                key["layerTime"].as_i64().unwrap(),
                                db(key["value"]["value"].as_f64().unwrap()),
                                key["easing"].clone(),
                            )
                        })
                        .collect()
                });
            (db(layer["volume"].as_f64().unwrap()), keys)
        })
        .collect()
}

fn assert_db(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-3 || actual == expected,
        "{actual} dB, expected {expected} dB"
    );
}

#[test]
fn adobe_clip_volume_levels_import_as_layer_volumes() {
    // Premiere 26.5.1 clip Volume Levels with Clip Gain, a mono source and a
    // 0.5 track fader. The expected values are the editable layer gains. The
    // AME render for case premiere_isolated_audio_volume_levels
    // measures the stereo A1 segments within 0.002 dB of them (the clipped
    // +15 dB one on its unclipped samples), the silent one as silence, and the
    // A2 segment with the fader 0.004 dB lower. The two mono segments measure
    // -3.011 and +2.989 dB, 3.01 dB lower: Premiere's pan law for a mono
    // source on a static centered stereo track, folded into the effective gain.
    let temp = tempfile::tempdir().unwrap();
    let (archive, omissions) = convert_pinned_case(
        temp.path(),
        "feature_audio_volume_levels_strict.prproj",
        "0a58340d-11c1-4813-8344-4eaf7b7d1bf7",
    );
    assert!(omissions.is_empty(), "{omissions:?}");
    let file = TesseractFile::open(&archive).unwrap();
    let expected = [
        0.0,
        f64::NEG_INFINITY,
        -10.0,
        6.0,
        15.0,
        8.9794,
        -2.0,
        6.0,
        -3.010_299_956_639_812,
        2.989_700_043_360_188,
        -16.0206,
    ];
    let levels = volume_levels(&file);
    assert_eq!(levels.len(), expected.len());
    for ((volume, keys), expected) in levels.into_iter().zip(expected) {
        assert_db(volume, expected);
        assert!(keys.is_empty());
    }
}

#[test]
fn adobe_clip_volume_keys_stay_editable_through_export_and_reimport() {
    // Premiere 26.5.1 Level keys: the linked A/V sound, two overlapping beds
    // (Linear and Hold keys, an 80 ms fade to +6 dB), and a placement with In
    // 1.0 s whose keys lie before In and after Out. Key times move to the
    // layer clock (source time minus In).
    let temp = tempfile::tempdir().unwrap();
    let (archive, omissions) = convert_pinned_case(
        temp.path(),
        "feature_audio_volume_keys_strict.prproj",
        "99b9c6d0-8795-4dfd-9a1c-8a45dd221b3d",
    );
    assert!(
        omissions
            .iter()
            .all(|item| item.record == "VideoClipTrackItem:95"
                || item.record == "MasterClip:74037e2b-5095-4ee8-b9a2-22be52b13dd0"),
        "{omissions:?}"
    );
    let file = TesseractFile::open(&archive).unwrap();
    let easing_type = |keys: &[DbKey]| {
        keys.iter()
            .map(|(time, _, easing)| (*time, easing["type"].as_str().unwrap().to_owned()))
            .collect::<Vec<_>>()
    };
    let levels = volume_levels(&file);
    let expected: [&[(i64, f64, &str)]; 4] = [
        &[(1750, 0.0, "linear"), (2750, -12.0, "cubicBezier")],
        &[
            (750, 0.0, "linear"),
            (1750, -20.0, "cubicBezier"),
            (2250, -20.0, "linear"),
            (2330, 6.0, "cubicBezier"),
            (3200, 6.0, "linear"),
            (3300, -10.0, "hold"),
        ],
        &[(500, 0.0, "linear"), (3500, -20.0, "cubicBezier")],
        &[
            (-500, f64::NEG_INFINITY, "linear"),
            (3000, 6.0, "cubicBezier"),
        ],
    ];
    assert_eq!(levels.len(), expected.len());
    for ((volume, keys), expected) in levels.iter().zip(expected) {
        assert_db(*volume, 0.0);
        assert_eq!(
            easing_type(keys),
            expected
                .iter()
                .map(|(time, _, easing)| (*time, (*easing).to_owned()))
                .collect::<Vec<_>>()
        );
        for ((_, level, _), (_, expected, _)) in keys.iter().zip(expected) {
            assert_db(*level, *expected);
        }
    }
    // The picture of the linked clip stays silent.
    let document = file.project_json().unwrap();
    assert_eq!(document["composition"]["layers"][0]["type"], "Video");
    assert_eq!(document["composition"]["layers"][0]["volume"], 0.0);

    let again = relocated_round_trip(temp.path(), &archive);
    let xml = read_xml(&temp.path().join("relocated/project.prproj"));
    assert_eq!(
        xml.matches("<FilterMatchName>Internal Volume Stereo</FilterMatchName>")
            .count(),
        4
    );
    // The two beds that reach +6 dB export that peak as Clip Gain.
    assert_eq!(xml.matches("<Gain>").count(), 2);
    let mut exported = volume_levels(&again);
    let mut imported = levels;
    // Overlapping sounds may change order; each keeps its levels and keys.
    let order = |a: &(f64, Vec<DbKey>), b: &(f64, Vec<DbKey>)| {
        let times = |keys: &[DbKey]| keys.iter().map(|key| key.0).collect::<Vec<_>>();
        times(&a.1).cmp(&times(&b.1))
    };
    exported.sort_by(order);
    imported.sort_by(order);
    for ((volume, keys), (expected_volume, expected_keys)) in exported.iter().zip(&imported) {
        assert_eq!(volume, expected_volume);
        for expected in expected_keys {
            assert!(
                keys.iter()
                    .any(|key| (key.0, key.1) == (expected.0, expected.1)),
                "{expected:?} in {keys:?}"
            );
        }
        for pair in expected_keys.windows(2) {
            let (start, end) = (&pair[0], &pair[1]);
            let inner = keys
                .iter()
                .filter(|key| key.0 > start.0 && key.0 < end.0)
                .count();
            let key = keys.iter().find(|key| key.0 == end.0).unwrap();
            // A Hold, a flat segment, and one of Levels at or below 0 dB come
            // back as they were imported. A segment to +6 dB is no longer
            // Premiere's curve once that peak moves into Clip Gain, so Linear
            // pieces with keys on the FX curve follow it.
            if end.2["type"] == "hold" || start.1 == end.1 || start.1.max(end.1) <= 1e-6 {
                assert_eq!((inner, &key.2), (0, &end.2), "{keys:?}");
            } else {
                assert_eq!(inner, 3, "{keys:?}");
            }
        }
    }
}

/// Premiere 26.5.1's first keyed bed (item 96, whose Level 142 is keyed on
/// the source clock from In 0) with its keys edited to 0 dB at 1.75 s, a
/// Hold to silence at 2 s, a Hold to 0 dB at 2.5 s, and Linear to -12 dB at
/// 2.75 s and to the same -12 dB 0.4 ms or 1.4 ms later. Keys 0.4 ms apart
/// round to one millisecond of the layer clock, so the bed's key track
/// cannot import: the bed keeps its placement and source as a layer at zero
/// gain, with one report, where its static 0 dB would play through the
/// silence at 2-2.5 s. Keys 1.4 ms apart import. The other three keyed beds
/// convert as they do without the edit.
#[test]
fn volume_keys_that_cannot_import_leave_their_sound_silent_beside_keyed_siblings() {
    const PROJECT: &str = "feature_audio_volume_keys_strict.prproj";
    let temp = tempfile::tempdir().unwrap();
    let convert = |name: &str, xml: &str| {
        let root = temp.path().join(name);
        fs::create_dir_all(&root).unwrap();
        let source = root.join(PROJECT);
        write_prproj(&source, xml);
        for media in [
            "feature_audio_click_left.wav",
            "feature_audio_tone_right.wav",
            "feature_linked_av_source.mp4",
        ] {
            fs::copy(fixture(media), root.join(media)).unwrap();
        }
        let output = root.join("tesseract");
        let omissions = premiere_to_tesseract(
            &source,
            &output,
            Some("99b9c6d0-8795-4dfd-9a1c-8a45dd221b3d"),
            false,
        )
        .unwrap();
        (
            volume_levels(&TesseractFile::open(first_project(&output)).unwrap()),
            omissions,
        )
    };
    let xml = read_xml(&fixture(PROJECT));
    let with_last_key = |ticks: i64| {
        let keys = format!(
            "444528000000,0.177827939391,4,0,0,0,0,0;508032000000,0.,4,0,0,0,0,0;635040000000,0.177827939391,0,0,0,0,0,0;698544000000,0.044668357819,0,0,0,0,0,0;{ticks},0.044668357819,0,0,0,0,0,0;"
        );
        let param = xml.find(r#"<AudioComponentParam ObjectID="142""#).unwrap();
        let open = param + xml[param..].find("<Keyframes>").unwrap() + "<Keyframes>".len();
        let close = open + xml[open..].find("</Keyframes>").unwrap();
        let mut xml = xml.clone();
        xml.replace_range(open..close, &keys);
        xml
    };
    let (base, base_omissions) = convert("base", &xml);
    // 2.75 s plus 0.4 ms (101606400 ticks) and plus 1.4 ms.
    for (name, last, keys) in [
        ("0.4 ms apart", 698_645_606_400, None),
        (
            "1.4 ms apart",
            698_899_622_400,
            Some([
                (1750, 0.0, "linear"),
                (2000, f64::NEG_INFINITY, "hold"),
                (2500, 0.0, "hold"),
                (2750, -12.0, "cubicBezier"),
                (2751, -12.0, "linear"),
            ]),
        ),
    ] {
        let (levels, omissions) = convert(name, &with_last_key(last));
        assert_eq!(levels.len(), base.len(), "{name}");
        assert_eq!(levels[1..], base[1..], "{name}");
        let (volume, bed) = &levels[0];
        let reported: Vec<_> = omissions
            .iter()
            .filter(|omission| !base_omissions.contains(omission))
            .collect();
        match keys {
            Some(expected) => {
                assert!(reported.is_empty(), "{name}: {reported:?}");
                assert_db(*volume, 0.0);
                assert_eq!(bed.len(), expected.len(), "{name}: {bed:?}");
                for ((time, level, easing), (expected_time, expected_level, expected_easing)) in
                    bed.iter().zip(expected)
                {
                    assert_eq!(
                        (*time, &easing["type"]),
                        (expected_time, &json!(expected_easing))
                    );
                    assert_db(*level, expected_level);
                }
            }
            None => {
                assert_eq!(*volume, f64::NEG_INFINITY, "{name}");
                assert!(bed.is_empty(), "{name}: {bed:?}");
                let [omission] = reported.as_slice() else {
                    panic!("{name}: {reported:?}");
                };
                assert_eq!(
                    (omission.scope, omission.record.as_str()),
                    (
                        premiere_file::OmissionScope::Feature,
                        "AudioClipTrackItem:96"
                    )
                );
                assert!(
                    omission.reason.starts_with(
                        "volume animation was not imported: unsupported conversion: Premiere volume keyframe times/values cannot be imported: "
                    ) && omission.reason.ends_with(
                        "at TimeOffset(2750) is not strictly after the preceding key; the sound was kept at zero gain"
                    ),
                    "{}",
                    omission.reason
                );
            }
        }
    }
}

const NEST_OUTER_KEYS: &str = "feature_nest_audio_outer_keys_26_5.prproj";
const NEST_OUTER_KEYS_SEQUENCE: &str = "f47a5a31-cde1-470d-b4b9-c9791fd203e2";

/// An import's nest picture, its root sounds, and their volume levels.
type PictureAndSounds = (Value, Vec<Value>, Vec<(f64, Vec<DbKey>)>);

/// Of an import of [`NEST_OUTER_KEYS`] or of an edit of it: the one root
/// group, which holds the nest's picture, as its range and its children as
/// [`editable_layer`] gives them, and the root sounds, as
/// [`editable_layers`] and [`volume_levels`] give them.
fn nest_picture_and_sounds(file: &TesseractFile) -> PictureAndSounds {
    let document = file.project_json().unwrap();
    let groups: Vec<_> = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Group")
        .map(|group| {
            let children: Vec<_> = group["layers"]
                .as_array()
                .unwrap()
                .iter()
                .map(|layer| editable_layer(file, layer))
                .collect();
            json!({"activeRange": (*crate::test_support::layer_range(group)), "layers": children})
        })
        .collect();
    let [group] = groups.try_into().unwrap();
    let sounds = editable_layers(file)
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Audio")
        .cloned()
        .collect();
    (group, sounds, volume_levels(file))
}

/// Premiere 26.5.1's own save of a nest ([`NEST_OUTER_KEYS`]) whose audio
/// item 92 plays the inner sequence at 1-7 s from In 1.5 s, shortened at
/// its tail: its clip keeps the untrimmed Out 8.5 s. The item's Level keys, on
/// the clock of its In/Out, are 0 dB at 2 s, Linear to -12 dB at 3 s and on
/// to the same -12 dB at 3.5 s, a Hold to silence at 4 s, a Hold to +6 dB at
/// 5 s and Linear to -6 dB at 6 s, over the inner tone at -6 dB. The
/// fixture's AME render plays those keys on that clock, times the tone's
/// -6 dB, and stops the sound at End, 1 s before the picture ends, while the
/// tone plays on inside. The keyed item plays alone: one audio layer of the
/// tone at 1-7 s from 1.5 s, whose keys are the item's on its layer clock
/// at -6 dB, and a group of the inner video over 1-8 s from 1.5 s, without
/// sound.
#[test]
fn adobe_nest_audio_item_level_keys_play_its_sound_until_its_end() {
    let temp = tempfile::tempdir().unwrap();
    let (archive, omissions) =
        convert_pinned_case(temp.path(), NEST_OUTER_KEYS, NEST_OUTER_KEYS_SEQUENCE);
    // Only the video items' Node properties are reported.
    assert!(
        omissions
            .iter()
            .all(|item| item.scope == premiere_file::OmissionScope::Feature
                && ["VideoClipTrackItem:91", "VideoClipTrackItem:95"]
                    .contains(&item.record.as_str())),
        "{omissions:?}"
    );
    let file = TesseractFile::open(&archive).unwrap();
    let (group, sounds, levels) = nest_picture_and_sounds(&file);
    assert_eq!(
        group["activeRange"],
        json!({"start": 1000, "duration": 7000})
    );
    assert_eq!(
        group["layers"],
        json!([{"type": "Video", "activeRange": {"start": 0, "duration": 7000},
                "sourceRange": {"start": 1500, "duration": 7000}, "volume": 0.0,
                "source": ["feature_timecoded_source.mp4", "Video"]}])
    );
    let [sound] = sounds.as_slice() else {
        panic!("{sounds:?}");
    };
    assert_eq!(
        (
            &sound["activeRange"],
            &sound["sourceRange"],
            &sound["source"]
        ),
        (
            &json!({"start": 1000, "duration": 6000}),
            &json!({"start": 1500, "duration": 6000}),
            &json!(["nest_tone_stereo_8s.wav", "Audio"])
        )
    );
    let [(volume, keys)] = levels.as_slice() else {
        panic!("{levels:?}");
    };
    assert_db(*volume, -6.0);
    let expected = [
        (500, -6.0, "linear"),
        (1500, -18.0, "cubicBezier"),
        (2000, -18.0, "linear"),
        (2500, f64::NEG_INFINITY, "hold"),
        (3500, 0.0, "hold"),
        (4500, -12.0, "cubicBezier"),
    ];
    assert_eq!(keys.len(), expected.len(), "{keys:?}");
    for ((time, level, easing), (expected_time, expected_level, expected_easing)) in
        keys.iter().zip(expected)
    {
        assert_eq!(
            (*time, &easing["type"]),
            (expected_time, &json!(expected_easing))
        );
        assert_db(*level, expected_level);
    }
}

/// Edits of [`NEST_OUTER_KEYS`] around its tail-shortened item 92. With an
/// Out at In + End - Start, the item imports as saved. The item is omitted
/// with its reason where it would play past its Out (End later), at twice
/// the speed, reversed or remapped, over an empty range, from an In before
/// 0, or with an In whose End - Start passes Premiere's tick range; its
/// picture still imports as saved. A copy of item 92 on the second audio track whose own
/// range is empty, starts before 0 or plays past its Out, there by End or by
/// the tick range, is omitted alone, and item 92 imports as saved.
#[test]
fn a_nest_audio_item_plays_its_sequence_from_in_until_its_end() {
    const ITEM: &str = r#"<AudioClipTrackItem ObjectID="92""#;
    const CLIP: &str = r#"<AudioClip ObjectID="144""#;
    let temp = tempfile::tempdir().unwrap();
    let (base, base_omissions) =
        convert_pinned_case(temp.path(), NEST_OUTER_KEYS, NEST_OUTER_KEYS_SEQUENCE);
    let base = nest_picture_and_sounds(&TesseractFile::open(base).unwrap());
    assert_eq!(base.1.len(), 1, "{:?}", base.1);
    let xml = read_xml(&fixture(NEST_OUTER_KEYS));
    // `record` with its one `from` replaced by `to`.
    let once = |record: &str, from: &str, to: &str| {
        assert_eq!(record.matches(from).count(), 1, "{from}");
        record.replacen(from, to, 1)
    };
    let item = |from: &str, to: &str| {
        let mut xml = xml.clone();
        edit_record(&mut xml, ITEM, "</AudioClipTrackItem>", |record| {
            once(record, from, to)
        });
        xml
    };
    let clip = |from: &str, to: &str| {
        let mut xml = xml.clone();
        edit_record(&mut xml, CLIP, "</AudioClip>", |record| {
            once(record, from, to)
        });
        xml
    };
    // A copy of item 92 with `edits`, each a `from` replaced by `to`, as the
    // one item of the second audio track; it shares 92's clip and Volume.
    let copy = |edits: &[(&str, &str)]| {
        let mut xml = xml.clone();
        edit_record(&mut xml, ITEM, "</AudioClipTrackItem>", |record| {
            let copy = edits.iter().fold(
                record.replacen(r#"ObjectID="92""#, r#"ObjectID="9092""#, 1),
                |copy, (from, to)| once(&copy, from, to),
            );
            record.to_owned() + &copy
        });
        edit_record(
            &mut xml,
            r#"<AudioClipTrack ObjectUID="bab7a0e9-7c29-4b23-81fc-74ef0878146c""#,
            "</AudioClipTrack>",
            |track| {
                once(
                    track,
                    r#"<ClipItems Version="3">"#,
                    r#"<ClipItems Version="3"><TrackItems Version="1"><TrackItem Index="0" ObjectRef="9092"/></TrackItems>"#,
                )
            },
        );
        xml
    };
    let speed = "unsupported conversion: AudioClipTrackItem:92: the audio item of a nested sequence must play its sequence at normal speed";
    let ranges = "unsupported conversion: AudioClipTrackItem:92: the audio item of a nested sequence has invalid timeline/source ranges";
    let forward =
        "unsupported conversion: AudioClip:144: only unit, forward audio playback is supported";
    let (copy_speed, copy_ranges) = (
        speed.replace(":92:", ":9092:"),
        ranges.replace(":92:", ":9092:"),
    );
    let (start, end) = ("<Start>254016000000</Start>", "<End>1778112000000</End>");
    let source_in = "<InPoint>381024000000</InPoint>";
    // (row, XML, whether item 92 imports as saved, each report as (record,
    // reason))
    type Row<'a> = (&'a str, String, bool, Vec<(&'a str, &'a str)>);
    let rows: [Row; 12] = [
        (
            "Out at In + End - Start",
            clip(
                "<OutPoint>2159136000000</OutPoint>",
                "<OutPoint>1905120000000</OutPoint>",
            ),
            true,
            vec![],
        ),
        (
            "End past Out",
            item(end, "<End>2286144000000</End>"),
            false,
            vec![("92", speed)],
        ),
        (
            "twice the speed",
            clip("<InPoint>", "<PlaybackSpeed>2</PlaybackSpeed><InPoint>"),
            false,
            vec![("92", forward)],
        ),
        (
            "reversed",
            clip("<InPoint>", "<PlayBackwards>true</PlayBackwards><InPoint>"),
            false,
            vec![("92", forward)],
        ),
        (
            // Any existing record: the reader must not follow the remap.
            "remapped",
            clip("<InPoint>", r#"<TimeRemapping ObjectRef="144"/><InPoint>"#),
            false,
            vec![("92", forward)],
        ),
        (
            "no duration",
            item(end, "<End>254016000000</End>"),
            false,
            vec![("92", ranges)],
        ),
        (
            "In before 0",
            clip(source_in, "<InPoint>-127008000000</InPoint>"),
            false,
            vec![("92", ranges)],
        ),
        (
            "In + End - Start past the tick range",
            clip(source_in, "<InPoint>9223372036854775000</InPoint>"),
            false,
            vec![("92", speed)],
        ),
        (
            "a copy without duration",
            copy(&[(end, "<End>254016000000</End>")]),
            true,
            vec![("9092", &copy_ranges)],
        ),
        (
            "a copy before 0",
            copy(&[(start, "<Start>-254016000000</Start>")]),
            true,
            vec![("9092", &copy_ranges)],
        ),
        (
            "a copy with End past Out",
            copy(&[(end, "<End>2286144000000</End>")]),
            true,
            vec![("9092", &copy_speed)],
        ),
        (
            "a copy whose End - Start passes the tick range",
            copy(&[
                (start, "<Start>0</Start>"),
                (end, "<End>9223372036854775000</End>"),
            ]),
            true,
            vec![("9092", &copy_speed)],
        ),
    ];
    for (name, xml, imports, expected) in rows {
        let root = temp.path().join(name);
        fs::create_dir_all(&root).unwrap();
        let source = root.join(NEST_OUTER_KEYS);
        write_prproj(&source, &xml);
        for media in ["feature_timecoded_source.mp4", "nest_tone_stereo_8s.wav"] {
            fs::copy(fixture(media), root.join(media)).unwrap();
        }
        let output = root.join("tesseract");
        let omissions =
            premiere_to_tesseract(&source, &output, Some(NEST_OUTER_KEYS_SEQUENCE), false).unwrap();
        let (group, sounds, levels) =
            nest_picture_and_sounds(&TesseractFile::open(first_project(&output)).unwrap());
        assert_eq!(group, base.0, "{name}");
        if imports {
            assert_eq!((&sounds, &levels), (&base.1, &base.2), "{name}");
        } else {
            assert!(sounds.is_empty() && levels.is_empty(), "{name}: {sounds:?}");
        }
        let reported: Vec<_> = omissions
            .iter()
            .filter(|omission| !base_omissions.contains(omission))
            .map(|omission| {
                (
                    omission.scope,
                    omission.record.as_str(),
                    omission.reason.as_str(),
                )
            })
            .collect();
        let expected: Vec<_> = expected
            .into_iter()
            .map(|(record, reason)| (premiere_file::OmissionScope::Occurrence, record, reason))
            .collect();
        assert_eq!(reported, expected, "{name}");
    }
}

/// The gain at `ms` of volume keys, solving a cubic Bézier's x for the time.
fn gain_at(keys: &[DbKey], ms: f64) -> f64 {
    let gain = |db: f64| 10_f64.powf(db / 20.0);
    let next = keys.iter().position(|key| key.0 as f64 >= ms).unwrap();
    let ((start, from, _), (end, to, easing)) = (&keys[next - 1], &keys[next]);
    let t = (ms - *start as f64) / (*end - *start) as f64;
    let bezier = |p1: f64, p2: f64, s: f64| {
        3.0 * (1.0 - s).powi(2) * s * p1 + 3.0 * (1.0 - s) * s * s * p2 + s.powi(3)
    };
    let control = |name: &str| easing[name].as_f64().unwrap();
    let progress = match easing["type"].as_str().unwrap() {
        "hold" => 0.0,
        "linear" => t,
        _ => {
            let (mut low, mut high) = (0.0, 1.0);
            for _ in 0..100 {
                let middle = (low + high) / 2.0;
                if bezier(control("x1"), control("x2"), middle) < t {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            bezier(control("y1"), control("y2"), (low + high) / 2.0)
        }
    };
    gain(*from) + (gain(*to) - gain(*from)) * progress
}

#[test]
fn volume_keys_make_a_silent_picture_audible_through_export_and_reimport() {
    // Import keeps a picture's own sound on an audio layer and the picture at
    // volume 0, so keys on the picture's volume alone make its sound play.
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.tsrct");
    let mut document = audible_document();
    document["composition"]["layers"][1]["volume"] = json!(0.0);
    let key = |id: &str, time: i64, value: f64| {
        json!({"id": id, "layerTime": time, "value": {"type": "float", "value": value},
               "easing": {"type": "hold"}})
    };
    document["composition"]["dynamics"] = json!({"entries": [{
        "target": {"kind": "layer", "layerId": 1, "propertyType": "volume"},
        "animator": {"type": "keyframes", "enabled": true,
                     "keyframes": [key("full", 0, 1.0), key("half", 100, 0.5)]},
        "dependencies": [], "layerRefs": {},
    }]});
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            fixture("video-with-audio.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset("music", fixture("audio-mono.wav"), AssetKind::Audio)
        .unwrap()
        .write(&source)
        .unwrap();
    let file = relocated_round_trip(root.path(), &source);
    // The picture's sound comes back with its keys, beside the music.
    let keys: Vec<Vec<_>> = volume_levels(&file)
        .into_iter()
        .map(|(_, keys)| {
            keys.iter()
                .map(|(time, db, _)| (*time, (db * 1e4).round() / 1e4))
                .collect()
        })
        .collect();
    assert_eq!(keys, [vec![(0, 0.0), (100, -6.0206)], vec![]]);
}

#[test]
fn authored_linear_fade_keeps_its_level_through_export_and_reimport() {
    // A 1 s FX Linear fade from silence to 0 dB on a placement with source In
    // 0.5 s. Premiere's Linear segment moves in fader position, so export adds
    // Linear pieces on the FX curve; the midpoint used to come back at 0.192.
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.tsrct");
    let mut document = editable_document();
    document["duration"] = json!(2.0);
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    layers[1]["activeRange"]["duration"] = json!(2000);
    layers.insert(
        0,
        json!({
            "type": "Audio", "id": 3, "name": "Music",
            "playback": crate::test_support::linear_playback(json!({"start": 0, "duration": 2000}), json!({"start": 500, "duration": 2000})),
            "sourceRange": {"start": 500, "duration": 2000},
            "sourceIntrinsicDuration": 5000, "volume": 1.0,
            "source": {"assetId": "music"},
        }),
    );
    let key = |id: &str, time: i64, value: f64| {
        json!({"id": id, "layerTime": time, "value": {"type": "float", "value": value},
               "easing": {"type": "linear"}})
    };
    document["composition"]["dynamics"] = json!({"entries": [{
        "target": {"kind": "layer", "layerId": 3, "propertyType": "volume"},
        "animator": {"type": "keyframes", "enabled": true,
                     "keyframes": [key("in", 500, 0.0), key("out", 1500, 1.0)]},
        "dependencies": [], "layerRefs": {},
    }]});
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            fixture("video-30fps.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset(
            "music",
            fixture("feature_audio_tone_right.wav"),
            AssetKind::Audio,
        )
        .unwrap()
        .write(&source)
        .unwrap();
    let file = relocated_round_trip(root.path(), &source);
    let [(volume, keys)] = &volume_levels(&file)[..] else {
        panic!("one sound comes back");
    };
    assert_db(*volume, 0.0);
    let first_and_last = [&keys[0], keys.last().unwrap()].map(|key| (key.0, key.1));
    assert_eq!(first_and_last, [(500, f64::NEG_INFINITY), (1500, 0.0)]);
    let midpoint = gain_at(keys, 1000.0);
    assert!(
        (20.0 * (midpoint / 0.5).log10()).abs() <= 0.25,
        "{midpoint}"
    );
    assert_eq!(keys.len(), 14);
}

#[test]
fn legacy_clip_volume_imports_with_its_own_scale() {
    // An XML derivative of a Premiere 10.4 save: `[Bypass, Level]` with 0 dB at
    // 0.5, wrapped AudioChannelLayout arrays and a 29.97 fps sequence.
    let temp = tempfile::tempdir().unwrap();
    let (archive, omissions) = convert_pinned_case(
        temp.path(),
        "feature_audio_volume_legacy_strict.prproj",
        "02ee2152-225c-4128-a9ed-3d30c0fd45f6",
    );
    assert!(
        omissions
            .iter()
            .any(|item| item.record == "AudioFilterComponent:1583"
                && item.reason == "clip Channel Volume not converted"),
        "{omissions:?}"
    );
    let file = TesseractFile::open(&archive).unwrap();
    let levels = volume_levels(&file);
    let expected = [
        0.0,
        0.0,
        f64::NEG_INFINITY,
        -30.0,
        -20.0,
        -6.0206,
        3.8017,
        6.0206,
        6.0206,
        0.0,
        0.0,
    ];
    assert_eq!(levels.len(), expected.len());
    for ((volume, _), expected) in levels.iter().zip(expected) {
        assert_db(*volume, expected);
    }
    let keys = &levels[10].1;
    assert_eq!(
        keys.iter().map(|(time, ..)| *time).collect::<Vec<_>>(),
        [753, 1753]
    );
    assert_db(keys[0].1, 0.0);
    assert_db(keys[1].1, -20.0);
}
