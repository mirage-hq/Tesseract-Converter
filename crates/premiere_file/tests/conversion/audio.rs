use super::support::{
    edit_record, first_project, premiere_to_tesseract, read_xml, tesseract_to_premiere,
    write_prproj,
};
#[cfg(feature = "ffmpeg-library")]
use super::test_support::editable_document;
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

/// Portable scaffold: the two-edge AudioProxies layout observed in a private
/// Adobe save, regenerated with public stereo WAVs and fresh attachment IDs.
/// This is not independently Adobe-authored proxy/audio fidelity evidence.
fn audio_source_proxy_scaffold(root: &Path) -> String {
    for name in [
        "feature_audio_click_left.wav",
        "feature_audio_tone_right.wav",
    ] {
        fs::copy(fixture(name), root.join(name)).unwrap();
    }
    fs::copy(
        fixture("feature_audio_tone_right.wav"),
        root.join("preview.wav"),
    )
    .unwrap();
    let mut xml = read_xml(&fixture("feature_audio_clips_strict.prproj"));
    edit_record(
        &mut xml,
        "<AudioMediaSource ObjectID=\"64\"",
        "</AudioMediaSource>",
        |record| {
            record.replace(
                "<Content Version=\"10\">",
                r#"<Content Version="10"><AudioProxies Version="1">
            <AudioProxyItem Index="0" ObjectRef="9000"/>
            <AudioProxyItem Index="1" ObjectRef="9001"/>
            </AudioProxies>"#,
            )
        },
    );
    let mut attachments = String::new();
    for index in 0..2 {
        attachments.push_str(&format!(
            r#"<AudioProxy ObjectID="{}" Version="1">
            <ProxyMedia ObjectURef="00000000-0000-4000-8000-000000009002"/>
            <ProxyStreamIndex>{index}</ProxyStreamIndex>
            <OriginalSecondaryIndex>{index}</OriginalSecondaryIndex></AudioProxy>"#,
            9000 + index
        ));
    }
    attachments.push_str(
        r#"<Media ObjectUID="00000000-0000-4000-8000-000000009002">
        <AudioStream ObjectRef="85"/><RelativePath>preview.wav</RelativePath>
        <IsProxy>true</IsProxy></Media>"#,
    );
    xml.replace("</PremiereData>", &format!("{attachments}</PremiereData>"))
}

#[test]
fn audio_source_proxies_keep_primary_bytes_and_export_current_gain_and_trim() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let xml = audio_source_proxy_scaffold(root);
    for (index, enabled) in [
        "",
        "<ProxyEnabled>false</ProxyEnabled>",
        "<ProxyEnabled>true</ProxyEnabled>",
    ]
    .iter()
    .enumerate()
    {
        let mut variant = xml.clone();
        edit_record(
            &mut variant,
            "<AudioMediaSource ObjectID=\"64\"",
            "</AudioMediaSource>",
            |record| record.replace("<AudioProxies", &format!("{enabled}<AudioProxies")),
        );
        // A preview file is optional, even when its graph target is resolved.
        if index == 0 {
            fs::remove_file(root.join("preview.wav")).unwrap();
        } else if index == 1 {
            fs::copy(
                fixture("feature_audio_tone_right.wav"),
                root.join("preview.wav"),
            )
            .unwrap();
        }
        // Preview preference is not preserved: all variants require the primary.
        let native = root.join(format!("source-{index}.prproj"));
        write_prproj(&native, &variant);
        let output = root.join(format!("import-{index}"));
        let notes = premiere_to_tesseract(&native, &output, None, false).unwrap();
        let loss: Vec<_> = notes
            .iter()
            .filter(|note| note.reason.contains("AudioProxies"))
            .collect();
        assert_eq!(loss.len(), 1, "{notes:?}");
        assert_eq!(loss[0].scope, premiere_file::OmissionScope::Feature);
        assert_eq!(loss[0].record, "AudioMediaSource:64");
        assert!(loss[0].reason.contains("preview preference"));
        assert!(loss[0].reason.contains("subject to media admission"));
        let file = TesseractFile::open(first_project(&output)).unwrap();
        let mut document = file.project_json().unwrap();
        let sounds = document["composition"]["layers"].as_array_mut().unwrap();
        let sound = sounds
            .iter_mut()
            .find(|layer| layer["type"] == "Audio")
            .unwrap();
        assert_eq!(sound["volume"], 1.0);
        assert_eq!(sound["sourceRange"], json!({"start": 0, "duration": 5000}));
        let id = sound["source"]["assetId"].as_str().unwrap().to_owned();
        let original = fs::read(root.join("feature_audio_click_left.wav")).unwrap();
        assert_eq!(
            file.asset(&id)
                .unwrap()
                .read_verified_bytes(original.len() as u64)
                .unwrap(),
            original
        );
        assert_eq!(file.metadata().assets.len(), 2, "preview never packaged");
        // Ordinary edited export uses the current sound, not attachment replay.
        sound["volume"] = json!(0.25);
        sound["sourceRange"] = json!({"start": 500, "duration": 4000});
        sound["playback"] = crate::test_support::linear_playback(
            json!({"start": 1000, "duration": 4000}),
            json!({"start": 500, "duration": 4000}),
        );
        let archive = root.join(format!("edited-{index}.tsrct"));
        let mut builder =
            TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
                .unwrap();
        for (asset_id, asset) in &file.metadata().assets {
            let name = Path::new(&asset.path).file_name().unwrap();
            builder = builder
                .add_asset(asset_id, root.join(name), AssetKind::Audio)
                .unwrap();
        }
        builder.write(&archive).unwrap();
        let package = root.join(format!("export-{index}"));
        tesseract_to_premiere(&archive, &package, false).unwrap();
        let exported = read_xml(&package.join("project.prproj"));
        assert!(!exported.contains("AudioProxies") && !exported.contains("ProxyMedia"));
        let wire = roxmltree::Document::parse(&exported).unwrap();
        fn child<'a, 'input>(
            node: roxmltree::Node<'a, 'input>,
            tag: &str,
        ) -> roxmltree::Node<'a, 'input> {
            node.children().find(|node| node.has_tag_name(tag)).unwrap()
        }
        let follow = |link: roxmltree::Node<'_, '_>| {
            let id = link.attribute("ObjectRef");
            let uid = link.attribute("ObjectURef");
            let records: Vec<_> = wire
                .root_element()
                .children()
                .filter(|node| {
                    (id.is_some() && node.attribute("ObjectID") == id)
                        || (uid.is_some() && node.attribute("ObjectUID") == uid)
                })
                .collect();
            assert_eq!(records.len(), 1, "one native reference target");
            records[0]
        };
        // Bind every numeric assertion to the placement of the edited original,
        // not another clip's matching value or the converter's own native reader.
        let placements: Vec<_> = wire
            .root_element()
            .children()
            .filter(|node| node.has_tag_name("AudioClipTrackItem"))
            .filter_map(|item| {
                let body = child(item, "ClipTrackItem");
                let clip = follow(child(follow(child(body, "SubClip")), "Clip"));
                let source = follow(child(child(clip, "Clip"), "Source"));
                let media = follow(child(child(source, "MediaSource"), "Media"));
                let path = child(media, "RelativePath").text().unwrap();
                (Path::new(path).file_name().unwrap() == "feature_audio_click_left.wav")
                    .then_some((body, clip))
            })
            .collect();
        assert_eq!(placements.len(), 1);
        let (body, clip) = placements[0];
        let ticks = |node: roxmltree::Node<'_, '_>, tag: &str| {
            child(node, tag).text().unwrap().parse::<i64>().unwrap()
        };
        let tick = super::support::TICKS;
        assert_eq!(ticks(child(body, "TrackItem"), "Start"), tick); // 1 s
        assert_eq!(ticks(child(body, "TrackItem"), "End"), 5 * tick);
        assert_eq!(ticks(child(clip, "Clip"), "InPoint"), tick / 2); // 0.5 s
        assert_eq!(ticks(child(clip, "Clip"), "OutPoint"), 9 * tick / 2);
        let chain = follow(child(child(body, "ComponentOwner"), "Components"));
        let volume = child(child(chain, "ComponentChain"), "Components")
            .children()
            .filter(|node| node.has_tag_name("Component"))
            .map(follow)
            .find(|node| child(*node, "FilterMatchName").text() == Some("Internal Volume Stereo"))
            .unwrap();
        let params = child(
            child(child(volume, "AudioComponent"), "Component"),
            "Params",
        );
        let level = params
            .children()
            .filter(|node| node.has_tag_name("Param"))
            .map(follow)
            .find(|node| child(*node, "Name").text() == Some("Level"))
            .unwrap();
        let static_level: f64 = child(level, "StartKeyframe")
            .text()
            .unwrap()
            .split(',')
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        let clip_gain: f64 = clip
            .children()
            .find(|node| node.has_tag_name("Gain"))
            .map_or(1.0, |node| node.text().unwrap().parse().unwrap());
        // Native current-layout Level is normalized by the measured -15 dB unity.
        assert!((static_level * clip_gain / 0.177_827_939_391 - 0.25).abs() < 1e-9);
        let reimport = root.join(format!("again-{index}"));
        premiere_to_tesseract(package.join("project.prproj"), &reimport, None, false).unwrap();
        let again = TesseractFile::open(first_project(&reimport)).unwrap();
        let layers = editable_layers(&again);
        let edited = layers
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["source"][0] == "feature_audio_click_left.wav")
            .unwrap();
        assert_eq!(
            edited["activeRange"],
            json!({"start": 1000, "duration": 4000})
        );
        assert_eq!(
            edited["sourceRange"],
            json!({"start": 500, "duration": 4000})
        );
        assert!((edited["volume"].as_f64().unwrap() - 0.25).abs() < 1e-9);
    }
}

#[test]
fn audio_source_proxies_do_not_replace_unverified_primary_or_channel_selection() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let xml = audio_source_proxy_scaffold(root);
    for (index, (open, close, old, replacement, reason)) in [
        (
            "<AudioMediaSource ObjectID=\"64\"",
            "</AudioMediaSource>",
            "<Media ObjectURef=\"bac3dbbe-fc6a-4a1b-bbb1-f89742f2d15c\"/>",
            "",
            "missing Media",
        ),
        (
            "<AudioMediaSource ObjectID=\"64\"",
            "</AudioMediaSource>",
            "<AudioProxies",
            "<UnknownControl/><AudioProxies",
            "unknown field `UnknownControl`",
        ),
        (
            "<AudioMediaSource ObjectID=\"64\"",
            "</AudioMediaSource>",
            "<AudioProxies",
            "<StartBoundary>0</StartBoundary><EndBoundary>1270080000000</EndBoundary><AudioProxies",
            "content boundaries",
        ),
        (
            "<SecondaryContent ObjectID=\"124\"",
            "</SecondaryContent>",
            "<ChannelIndex>0</ChannelIndex>",
            "<ChannelIndex>1</ChannelIndex>",
            "channel remapping is unsupported",
        ),
        (
            "<Media ObjectUID=\"bac3dbbe-fc6a-4a1b-bbb1-f89742f2d15c\"",
            "</Media>",
            "feature_audio_click_left.wav",
            "missing-primary.wav",
            "missing media",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut variant = xml.clone();
        edit_record(&mut variant, open, close, |record| {
            assert!(record.contains(old));
            record.replace(old, replacement)
        });
        let native = root.join(format!("negative-{index}.prproj"));
        write_prproj(&native, &variant);
        let output = root.join(format!("negative-{index}"));
        let notes = premiere_to_tesseract(&native, &output, None, false).unwrap();
        assert!(
            notes.iter().any(|note| note.reason.contains(reason)),
            "{notes:?}"
        );
        let losses: Vec<_> = notes
            .iter()
            .filter(|note| {
                note.scope == premiere_file::OmissionScope::Feature
                    && note.record == "AudioMediaSource:64"
                    && note.reason.contains("AudioProxies")
            })
            .collect();
        if reason == "missing media" {
            assert_eq!(
                losses.len(),
                1,
                "selection loss precedes file admission: {notes:?}"
            );
            assert!(losses[0].reason.contains("subject to media admission"));
            assert!(losses[0].reason.contains("preview preference"));
            assert!(
                notes.iter().any(|note| {
                    note.scope == premiere_file::OmissionScope::Occurrence
                        && note.record == "AudioClipTrackItem:87"
                        && note.reason.contains("missing media")
                }),
                "the unadmitted primary sound is omitted: {notes:?}"
            );
        } else {
            assert!(
                losses.is_empty(),
                "unreadable source/selection emits no retention claim: {notes:?}"
            );
        }
        let file = TesseractFile::open(first_project(&output)).unwrap();
        let layers = editable_layers(&file);
        let sounds: Vec<_> = layers
            .as_array()
            .unwrap()
            .iter()
            .filter(|layer| layer["type"] == "Audio")
            .collect();
        assert_eq!(
            sounds.len(),
            1,
            "unaffected sibling retained, no proxy fallback"
        );
        assert_eq!(sounds[0]["source"][0], "feature_audio_tone_right.wav");
        assert_eq!(sounds[0]["volume"], 0.5);
    }
}

#[test]
fn audio_source_proxies_mono_selector_uses_original_channel_one() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let mut xml = audio_source_proxy_scaffold(root);
    // A nonzero original right tone differs from both the silent original left
    // and the proxy's nonzero left clicks. No silent output can satisfy the test.
    edit_record(
        &mut xml,
        "<Media ObjectUID=\"bac3dbbe-fc6a-4a1b-bbb1-f89742f2d15c\"",
        "</Media>",
        |record| {
            record.replace(
                "feature_audio_click_left.wav",
                "feature_audio_tone_right.wav",
            )
        },
    );
    fs::copy(
        fixture("feature_audio_click_left.wav"),
        root.join("preview.wav"),
    )
    .unwrap();
    edit_record(
        &mut xml,
        "<AudioClip ObjectID=\"107\"",
        "</AudioClip>",
        |record| {
            record
                .replace(r#"<SecondaryContentItem Index="0" ObjectRef="124"/>"#, "")
                .replace(
                    r#"<SecondaryContentItem Index="1" ObjectRef="125"/>"#,
                    r#"<SecondaryContentItem Index="0" ObjectRef="125"/>"#,
                )
                .replace(
                    r#"[{"channellabel":100},{"channellabel":101}]"#,
                    r#"[{"channellabel":0}]"#,
                )
        },
    );
    // Supplemental preview indices deliberately disagree with the original
    // ChannelIndex=1 selector. They must not choose the prepared mono channel.
    edit_record(
        &mut xml,
        "<AudioProxy ObjectID=\"9001\"",
        "</AudioProxy>",
        |record| {
            record
                .replace(
                    "<ProxyStreamIndex>1</ProxyStreamIndex>",
                    "<ProxyStreamIndex>0</ProxyStreamIndex>",
                )
                .replace(
                    "<OriginalSecondaryIndex>1</OriginalSecondaryIndex>",
                    "<OriginalSecondaryIndex>0</OriginalSecondaryIndex>",
                )
        },
    );
    let input = root.join("mono.prproj");
    write_prproj(&input, &xml);
    let output = root.join("mono");
    let notes = premiere_to_tesseract(&input, &output, None, false).unwrap();
    assert!(
        !notes
            .iter()
            .any(|note| note.scope == premiere_file::OmissionScope::Occurrence),
        "{notes:?}"
    );
    let file = TesseractFile::open(first_project(&output)).unwrap();
    let document = file.project_json().unwrap();
    let sound = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Audio")
        .unwrap();
    assert_eq!(sound["sourceRange"], json!({"start": 0, "duration": 5000}));
    assert!((sound["volume"].as_f64().unwrap() - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9);
    let id = sound["source"]["assetId"].as_str().unwrap();
    let original = fs::read(root.join("feature_audio_tone_right.wav")).unwrap();
    let preview = fs::read(root.join("preview.wav")).unwrap();
    let bytes = file
        .asset(id)
        .unwrap()
        .read_verified_bytes(original.len() as u64)
        .unwrap();
    // These public PCM16 WAVs and the mono writer use the canonical 44-byte header.
    assert_eq!(&original[36..40], b"data");
    assert_eq!(&preview[36..40], b"data");
    assert_eq!(&bytes[36..40], b"data");
    assert_eq!(u16::from_le_bytes(bytes[22..24].try_into().unwrap()), 1);
    assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 48000);
    let right: Vec<_> = original[44..]
        .chunks_exact(4)
        .flat_map(|frame| &frame[2..4])
        .copied()
        .collect();
    let left: Vec<_> = original[44..]
        .chunks_exact(4)
        .flat_map(|frame| &frame[..2])
        .copied()
        .collect();
    let proxy_left: Vec<_> = preview[44..]
        .chunks_exact(4)
        .flat_map(|frame| &frame[..2])
        .copied()
        .collect();
    let proxy_right: Vec<_> = preview[44..]
        .chunks_exact(4)
        .flat_map(|frame| &frame[2..4])
        .copied()
        .collect();
    assert!(
        right.iter().any(|byte| *byte != 0),
        "expected tone is nonzero"
    );
    assert!(
        proxy_left.iter().any(|byte| *byte != 0),
        "proxy clicks are nonzero"
    );
    assert_ne!(right, left, "original channels are distinct");
    assert_ne!(
        right, proxy_left,
        "proxy stream 0 clicks are not the original tone"
    );
    assert_ne!(
        right, proxy_right,
        "neither proxy channel supplies the original tone"
    );
    assert_eq!(
        &bytes[44..],
        right,
        "full original channel 1, not channel 0 or proxy samples"
    );
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

#[cfg(feature = "ffmpeg-library")]
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

#[cfg(feature = "ffmpeg-library")]
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
#[cfg(feature = "ffmpeg-library")]
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

#[cfg(feature = "ffmpeg-library")]
#[test]
fn authored_sound_survives_export_relocation_and_reimport() {
    check_authored_sound_round_trip(false);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn legacy_media_sound_survives_export_relocation_and_reimport() {
    check_authored_sound_round_trip(true);
}

/// `bytes` with the one occurrence of `from` replaced by `to` of the same length.
#[cfg(feature = "ffmpeg-library")]
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
#[cfg(feature = "ffmpeg-library")]
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
#[cfg(feature = "ffmpeg-library")]
fn write_surround_wav(path: &Path) -> Vec<u8> {
    let mut wav = fs::read(fixture("audio-stereo.wav")).unwrap();
    wav[22..24].copy_from_slice(&6_u16.to_le_bytes());
    wav[28..32].copy_from_slice(&(48_000_u32 * 12).to_le_bytes());
    wav[32..34].copy_from_slice(&12_u16.to_le_bytes());
    fs::write(path, &wav).unwrap();
    wav
}

#[cfg(feature = "ffmpeg-library")]
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

#[cfg(feature = "ffmpeg-library")]
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

#[cfg(feature = "ffmpeg-library")]
#[test]
fn audio_clock_retimed_layer_exports_without_baking_or_losing_siblings() {
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
    assert!(reported.is_empty(), "{reported:?}");
    let xml = read_xml(&root.join("native/project.prproj"));
    assert_eq!(xml.matches("<VideoClipTrackItem ").count(), 1);
    assert_eq!(xml.matches("<AudioClipTrackItem ").count(), 3);
    assert!(xml.contains("<PlaybackSpeed>2</PlaybackSpeed>"));
    assert_eq!(
        fs::read(root.join("native/media/audio-stereo.wav")).unwrap(),
        fs::read(fixture("audio-stereo.wav")).unwrap()
    );
    let output = root.join("reimported");
    let omissions =
        premiere_to_tesseract(root.join("native/project.prproj"), &output, None, false).unwrap();
    assert!(
        omissions
            .iter()
            .any(|item| item.reason.contains("constant audio clock")),
        "{omissions:?}"
    );
    let file = TesseractFile::open(first_project(&output)).unwrap();
    let wire = file.project_json().unwrap();
    let sound = wire["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| {
            layer["type"] == "Audio"
                && layer["source"]["assetId"].as_str().is_some_and(|id| {
                    file.metadata().assets[id]
                        .path
                        .ends_with("audio-stereo.wav")
                })
        })
        .unwrap();
    assert_eq!(
        sound["playback"]["inputRange"],
        json!({"start":0,"duration":100})
    );
    assert_eq!(sound["sourceRange"], json!({"start":0,"duration":200}));
    assert_eq!(
        sound["playback"]["mapping"]["property"]["keyframes"][1]["value"],
        200
    );
    let mut packaged: Vec<_> = fs::read_dir(root.join("native/media"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    packaged.sort();
    assert_eq!(
        packaged,
        ["audio-mono.wav", "audio-stereo.wav", "video-with-audio.mp4"]
    );
}

/// `editable_document` plus two audio layers that play the mono `music` at
/// -6 dB: 3, "Music", 150 ms from 50 ms at 100 ms, and 4, "Music again",
/// 100 ms from 0 at 500 ms. Each records the enhancement output
/// `music-enhanced`, which plays while `enabled`.
#[cfg(feature = "ffmpeg-library")]
fn enhanced_music_document(enabled: bool) -> Value {
    let mut enhancement = json!({"enhancedAssetId": "music-enhanced"});
    if enabled {
        enhancement["enabled"] = json!(true);
    }
    let sound = |id: u64, name: &str, start: i64, source_start: i64, duration: i64| {
        json!({
            "type": "Audio", "id": id, "name": name,
            "playback": crate::test_support::linear_playback(
                json!({"start": start, "duration": duration}),
                json!({"start": source_start, "duration": duration}),
            ),
            "sourceRange": {"start": source_start, "duration": duration},
            "sourceIntrinsicDuration": 200, "volume": 0.5,
            "source": {"assetId": "music", "enhancement": enhancement},
        })
    };
    let mut document = editable_document();
    let layers = document["composition"]["layers"].as_array_mut().unwrap();
    layers.insert(0, sound(4, "Music again", 500, 0, 100));
    layers.insert(0, sound(3, "Music", 100, 50, 150));
    document
}

/// How [`write_music_archive`] holds one music asset that it cannot read back.
#[derive(Clone, Copy, Debug)]
#[cfg(feature = "ffmpeg-library")]
enum Fault {
    /// The stored entry no longer matches its recorded digest.
    Damaged,
    /// The archive does not hold the asset: a runtime-owned reference.
    Missing,
}

/// Writes `document` to `root/source.tsrct` with its picture, `music` as the
/// mono `audio-mono.wav` and `music-enhanced` as `enhanced`. `fault` damages
/// or leaves out one of the two music assets.
#[cfg(feature = "ffmpeg-library")]
fn write_music_archive(
    root: &Path,
    document: &Value,
    enhanced: &Path,
    fault: Option<(&str, Fault)>,
) -> PathBuf {
    let missing = |asset_id: &str| matches!(fault, Some((id, Fault::Missing)) if id == asset_id);
    let music = [
        ("music", fixture("audio-mono.wav")),
        ("music-enhanced", enhanced.to_owned()),
    ];
    let mut builder =
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(document).unwrap())
            .unwrap()
            .add_asset(
                "premiere-video-1",
                fixture("video-30fps.mp4"),
                AssetKind::Video,
            )
            .unwrap();
    for (asset_id, path) in &music {
        if !missing(asset_id) {
            builder = builder
                .add_asset(*asset_id, path, AssetKind::Audio)
                .unwrap();
        }
    }
    let source = root.join("source.tsrct");
    builder
        .write_with_runtime_assets(&source, |asset| missing(asset.asset_id))
        .unwrap();
    if let Some((asset_id, Fault::Damaged)) = fault {
        // Assets are stored uncompressed and opening reads no payload, as in
        // `damaged_unsupported_sound_fails_its_archive_digest_and_publishes_nothing`.
        // Change one sample byte after the 44-byte WAV header.
        let (_, path) = music.iter().find(|(id, _)| *id == asset_id).unwrap();
        let payload = fs::read(path).unwrap();
        let mut archive = fs::read(&source).unwrap();
        let entry = archive
            .windows(payload.len())
            .position(|w| w == payload)
            .unwrap();
        archive[entry + 44] ^= 1;
        fs::write(&source, archive).unwrap();
    }
    source
}

/// The file names in the `media` directory of a Premiere package, sorted.
#[cfg(feature = "ffmpeg-library")]
fn packaged_media(package: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(package.join("media"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn audio_enhancement_exports_only_the_asset_that_plays() {
    // The renderer plays an audio layer's enhanced output while its
    // enhancement is enabled (`AudioSource::active_asset_id`), else its
    // original. Both layers share the mono original and one stereo output,
    // with their own trims. Export packages the asset that plays once and
    // places both trims on it with its own channel layout. Premiere has no
    // enhancement toggle, so the toggle and the other asset are reported.
    for enabled in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let archive = write_music_archive(
            root,
            &enhanced_music_document(enabled),
            &fixture("audio-stereo.wav"),
            None,
        );
        let native = root.join("native");
        let omissions = tesseract_to_premiere(&archive, &native, false).unwrap();
        let (active, layout, reason) = if enabled {
            (
                "audio-stereo.wav",
                "Stereo",
                "audio enhancement exported as its active output \"music-enhanced\"; the original asset \"music\" and the enhancement toggle were not exported",
            )
        } else {
            (
                "audio-mono.wav",
                "Mono",
                "inactive audio enhancement output \"music-enhanced\" and the enhancement toggle were not exported",
            )
        };
        assert_eq!(
            packaged_media(&native),
            [active, "video-30fps.mp4"],
            "enabled: {enabled}"
        );
        assert_eq!(
            fs::read(native.join("media").join(active)).unwrap(),
            fs::read(fixture(active)).unwrap()
        );
        let xml = read_xml(&native.join("project.prproj"));
        assert_eq!(xml.matches("<AudioClipTrackItem ").count(), 2);
        // The -6 dB volume writes each placement's intrinsic clip Volume, whose
        // match name has the packaged layout. Unity stereo would write
        // Premiere's untouched chain, which has no filter.
        assert_eq!(
            xml.matches(&format!(
                "<FilterMatchName>Internal Volume {layout}</FilterMatchName>"
            ))
            .count(),
            2
        );
        assert_eq!(omissions.len(), 2, "{omissions:?}");
        for record in ["layer 3 (\"Music\")", "layer 4 (\"Music again\")"] {
            let expected = premiere_file::Omission {
                scope: premiere_file::OmissionScope::Feature,
                kind: premiere_file::OmissionKind::Omitted,
                record: record.to_owned(),
                reason: reason.to_owned(),
            };
            assert!(omissions.contains(&expected), "{omissions:?}");
        }
        let again = root.join("again");
        let reimported =
            premiere_to_tesseract(native.join("project.prproj"), &again, None, false).unwrap();
        assert!(reimported.is_empty(), "{reimported:?}");
        let file = TesseractFile::open(first_project(&again)).unwrap();
        let layers = editable_layers(&file);
        let mut sounds: Vec<_> = layers
            .as_array()
            .unwrap()
            .iter()
            .filter(|layer| layer["type"] == "Audio")
            .collect();
        sounds.sort_by_key(|sound| sound["activeRange"]["start"].as_i64());
        let source = json!([active, "Audio"]);
        let placements: Vec<_> = sounds
            .iter()
            .map(|sound| json!([sound["activeRange"], sound["sourceRange"], sound["source"]]))
            .collect();
        assert_eq!(
            placements,
            [
                json!([{"start": 100, "duration": 150}, {"start": 50, "duration": 150}, source]),
                json!([{"start": 500, "duration": 100}, {"start": 0, "duration": 100}, source]),
            ]
        );
        // Either layout keeps -6 dB, within the float error of its mix gain.
        for sound in sounds {
            let volume = sound["volume"].as_f64().unwrap();
            assert!((volume - 0.5).abs() < 1e-9, "enabled: {enabled}: {volume}");
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn inactive_audio_enhancement_asset_is_never_read() {
    // The asset that does not play may be damaged or absent from the archive
    // without changing the export of the one that does.
    for enabled in [true, false] {
        let (inactive, active) = if enabled {
            ("music", "audio-stereo.wav")
        } else {
            ("music-enhanced", "audio-mono.wav")
        };
        for fault in [Fault::Damaged, Fault::Missing] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path();
            let archive = write_music_archive(
                root,
                &enhanced_music_document(enabled),
                &fixture("audio-stereo.wav"),
                Some((inactive, fault)),
            );
            let native = root.join("native");
            if let Err(error) = tesseract_to_premiere(&archive, &native, false) {
                panic!("{inactive} {fault:?}: {error}");
            }
            assert_eq!(
                packaged_media(&native),
                [active, "video-30fps.mp4"],
                "{inactive} {fault:?}"
            );
            assert_eq!(
                fs::read(native.join("media").join(active)).unwrap(),
                fs::read(fixture(active)).unwrap()
            );
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn unusable_active_audio_enhancement_output_never_falls_back_to_the_original() {
    // The enhancement is enabled and the original stays valid. As for any
    // other source sound, a missing or damaged output stops the export, a
    // six-channel output omits both placements, and an output whose duration
    // differs from `sourceIntrinsicDuration` rejects. None exports the original.
    let temp = tempfile::tempdir().unwrap();
    let surround = temp.path().join("surround.wav");
    write_surround_wav(&surround);
    let document = enhanced_music_document(true);
    let export = |name: &str, enhanced: &Path, fault: Option<Fault>| {
        let root = temp.path().join(name);
        fs::create_dir(&root).unwrap();
        let archive = write_music_archive(
            &root,
            &document,
            enhanced,
            fault.map(|fault| ("music-enhanced", fault)),
        );
        let native = root.join("native");
        let result = tesseract_to_premiere(&archive, &native, false);
        (result.map_err(|error| error.to_string()), native)
    };
    let stereo = fixture("audio-stereo.wav");
    for (name, fault, expected) in [
        (
            "missing",
            Fault::Missing,
            "asset ID \"music-enhanced\" is not in metadata.json",
        ),
        (
            "damaged",
            Fault::Damaged,
            "packaged media bytes failed their source hash",
        ),
    ] {
        let (result, native) = export(name, &stereo, Some(fault));
        let error = result.unwrap_err();
        assert!(
            error.contains("asset \"music-enhanced\"") && error.ends_with(expected),
            "{name}: {error}"
        );
        assert!(!native.exists(), "{name}");
    }
    let (result, native) = export("longer", &fixture("feature_audio_tone_right.wav"), None);
    let error = result.unwrap_err();
    assert!(
        error.ends_with(
            "sourceIntrinsicDuration 200 ms differs from the packaged audio duration 5000 ms of the active audio enhancement output \"music-enhanced\""
        ),
        "{error}"
    );
    assert!(!native.exists());
    let (result, native) = export("surround", &surround, None);
    let omissions = result.unwrap();
    for record in ["layer 3 (\"Music\")", "layer 4 (\"Music again\")"] {
        assert!(
            omissions.iter().any(
                |item| item.scope == premiere_file::OmissionScope::Occurrence
                    && item.record == record
                    && item
                        .reason
                        .ends_with("only mono/stereo source audio is supported")
            ),
            "{omissions:?}"
        );
    }
    let xml = read_xml(&native.join("project.prproj"));
    assert_eq!(xml.matches("<AudioClipTrackItem ").count(), 0);
    assert_eq!(packaged_media(&native), ["video-30fps.mp4"]);
}

#[cfg(feature = "ffmpeg-library")]
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
fn saved_ducking_imports_and_exports_edited_volume_automation() {
    // Reference closure of the human-authored Premiere 26 ducking sequence.
    // Original SHA-256: e74d088116570ddb7178b127129036755be2f8e553dea80f98aa59b601585b76.
    // Only media/cache paths changed. Native getters confirm the five key times;
    // UI readback confirms 0 dB, -16.81 dB at 00;00;03;28, and -18 dB at 00;00;05;00.
    let root = tempfile::tempdir().unwrap();
    let (archive, omissions) = convert_pinned_case(
        root.path(),
        "feature_saved_ducking.prproj",
        "04f1ae8b-7e92-4c35-b584-7c662280fe01",
    );
    let mut file = TesseractFile::open(&archive).unwrap();
    let levels = volume_levels(&file);
    assert_eq!(levels.len(), 2, "{omissions:?}");
    let keys = &levels[0].1;
    assert!(
        !keys.is_empty(),
        "saved ducking keys were lost: {omissions:?}"
    );
    assert!(levels[1].1.is_empty());
    assert_db(levels[1].0, 0.0);
    for (time, db) in [
        (0, 0.0),
        (3190, 0.0),
        (3990, -18.0),
        (6000, -18.0),
        (6800, 0.0),
    ] {
        let key = keys.iter().find(|key| key.0 == time).unwrap();
        assert!((key.1 - db).abs() < 1e-5, "{key:?}");
    }
    let expected_db = |ms: f64| {
        if ms <= 3190.0 {
            0.0
        } else if ms < 3990.0 {
            -18.0 * (ms - 3190.0) / 800.0
        } else if ms <= 6000.0 {
            -18.0
        } else {
            -18.0 * (6800.0 - ms) / 800.0
        }
    };
    // Check the actual amplitude curve, not only the values at native keys.
    for ms in (1..=6800).step_by(7) {
        let db = 20.0 * gain_at(keys, f64::from(ms)).log10();
        assert!((db - expected_db(f64::from(ms))).abs() < 0.05, "{ms}: {db}");
    }
    assert!((20.0 * gain_at(keys, 118.0 * 1001.0 / 30.0).log10() + 16.81).abs() < 0.05);
    // Preserve unrelated source colour settings and their existing reports.
    assert!(
        omissions
            .iter()
            .all(|item| item.record == "VideoTrackGroup:284"
                || (item.kind == premiere_file::OmissionKind::Approximated
                    && item.reason.starts_with("Amplify linear-dB ramps"))),
        "{omissions:?}"
    );

    // Edit the imported level and key clock. Export must use these values, not
    // replay the Amplify effect or regenerate ducking from the Dialogue clip.
    let mut document = file.project_json().unwrap();
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert!(layers.iter().all(|layer| layer["autoDucking"].is_null()));
    assert_eq!(
        crate::test_support::layer_range(&layers[0]),
        &json!({"start": 0, "duration": 8000})
    );
    let entries = document["composition"]["dynamics"]["entries"]
        .as_array_mut()
        .unwrap();
    assert_eq!(entries.len(), 1);
    for key in entries[0]["animator"]["keyframes"].as_array_mut().unwrap() {
        key["layerTime"] = json!(key["layerTime"].as_i64().unwrap() + 100);
        key["value"]["value"] = json!(key["value"]["value"].as_f64().unwrap() * 0.5);
    }
    let edited_json = root.path().join("edited.json");
    fs::write(&edited_json, serde_json::to_vec(&document).unwrap()).unwrap();
    file.commit_project_json(&edited_json).unwrap();
    file.save().unwrap();
    let again = relocated_round_trip(root.path(), &archive);
    let xml = read_xml(&root.path().join("relocated/project.prproj"));
    assert!(!xml.contains("f14fee7d-fe5f-4202-b790-f53209e56411"));
    assert!(xml.contains("<FilterMatchName>Internal Volume Stereo</FilterMatchName>"));
    let exported = volume_levels(&again);
    let edited = &exported
        .iter()
        .find(|(_, keys)| !keys.is_empty())
        .unwrap()
        .1;
    for (time, db) in [
        (100, 0.0),
        (3290, 0.0),
        (4090, -18.0),
        (6100, -18.0),
        (6900, 0.0),
    ] {
        let key = edited.iter().find(|key| key.0 == time).unwrap();
        assert!(
            (key.1 - (db + 20.0 * 0.5_f64.log10())).abs() < 1e-5,
            "{key:?}"
        );
    }
    for ms in [3590.0, 3937.266666666667, 5005.0, 6400.0] {
        let db = 20.0 * (gain_at(edited, ms + 100.0) / 0.5).log10();
        assert!((db - expected_db(ms)).abs() < 0.05, "{ms}: {db}");
    }
}

#[cfg(feature = "ffmpeg-library")]
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
    let levels = volume_levels(&file);
    // The native PCM follows fader-position interpolation across unity and
    // from silence. One cubic for these entire ramps can miss by over 1 dB.
    let position = |gain: f64| {
        if gain <= 1.0 {
            gain.powf(0.4475)
        } else {
            2.0 - gain.powf(-0.4475)
        }
    };
    for (index, start, end, from, to) in [
        (1, 2250.0, 2330.0, 0.1, 10_f64.powf(6.0 / 20.0)),
        (3, -500.0, 3000.0, 0.0, 10_f64.powf(6.0 / 20.0)),
    ] {
        for step in 1..100 {
            let t = step as f64 / 100.0;
            let u = position(from) + (position(to) - position(from)) * t;
            let expected_gain = if u <= 1.0 {
                u.powf(1.0 / 0.4475)
            } else {
                (2.0 - u).powf(-1.0 / 0.4475)
            };
            if expected_gain >= 1e-3 {
                let actual = gain_at(&levels[index].1, start + (end - start) * t);
                let error_db = (20.0 * (actual / expected_gain).log10()).abs();
                assert!(error_db <= 0.01, "ramp {index} at {t}: {error_db} dB");
            }
        }
    }
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
        // Refinement may insert keys, but never moves or removes a saved key.
        for (time, expected_level, expected_easing) in expected {
            let (_, level, easing) = keys.iter().find(|key| key.0 == *time).unwrap();
            assert_db(*level, *expected_level);
            assert_eq!(easing["type"], *expected_easing);
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
            let key = keys.iter().find(|key| key.0 == expected.0).unwrap();
            assert!(
                key.1 == expected.1 || (key.1 - expected.1).abs() < 1e-9,
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
            // Hold and flat segments remain exact. Moving the peak into Clip
            // Gain can change the fader branch or the fit's -60 dB floor, so
            // other segments need a gain-error check, not identical controls.
            if end.2["type"] == "hold" || start.1 == end.1 {
                assert_eq!((inner, &key.2), (0, &end.2), "{keys:?}");
            } else {
                for probe in 1..10 {
                    let ms = start.0 as f64 + (end.0 - start.0) as f64 * probe as f64 / 10.0;
                    let expected_gain = gain_at(expected_keys, ms);
                    if expected_gain >= 1e-3 {
                        let error_db = (20.0 * (gain_at(keys, ms) / expected_gain).log10()).abs();
                        assert!(error_db <= 0.26, "round trip at {ms}: {error_db} dB");
                    }
                }
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
#[cfg(feature = "ffmpeg-library")]
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

#[cfg(feature = "ffmpeg-library")]
const NEST_OUTER_KEYS: &str = "feature_nest_audio_outer_keys_26_5.prproj";
#[cfg(feature = "ffmpeg-library")]
const NEST_OUTER_KEYS_SEQUENCE: &str = "f47a5a31-cde1-470d-b4b9-c9791fd203e2";

/// An import's nest picture, its root sounds, and their volume levels.
#[cfg(feature = "ffmpeg-library")]
type PictureAndSounds = (Value, Vec<Value>, Vec<(f64, Vec<DbKey>)>);

/// Of an import of [`NEST_OUTER_KEYS`] or of an edit of it: the one root
/// group, which holds the nest's picture, as its range and its children as
/// [`editable_layer`] gives them, and the root sounds, as
/// [`editable_layers`] and [`volume_levels`] give them.
#[cfg(feature = "ffmpeg-library")]
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
#[cfg(feature = "ffmpeg-library")]
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
        (4000, -6.0, "cubicBezier"),
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
#[cfg(feature = "ffmpeg-library")]
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
        "unsupported conversion: AudioClip:144: retimed nested-sequence audio items are not converted";
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
            vec![(
                "92",
                "unsupported conversion: AudioClip:144: audio TimeRemapping is not converted",
            )],
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

/// Supplementary control on the existing native nest scaffold, not a new
/// Adobe oracle. The public 200 ms mono source stays byte-identical.
#[cfg(feature = "ffmpeg-library")]
fn nested_mono_control_xml() -> String {
    let mut xml = read_xml(&fixture(NEST_OUTER_KEYS));
    let mut edit = |tag: &str, id: &str, f: &dyn Fn(&str) -> String| {
        edit_record(
            &mut xml,
            &format!(r#"<{tag} ObjectID="{id}""#),
            &format!("</{tag}>"),
            f,
        );
    };
    for id in ["110", "125"] {
        edit("AudioComponentChain", id, &|_| {
            format!(
                r#"<AudioComponentChain ObjectID="{id}"><DefaultVol>true</DefaultVol><ComponentChain Version="3"/></AudioComponentChain>"#
            )
        });
    }
    edit("AudioClip", "144", &|clip| {
        clip.replace(r#"<SecondaryContentItem Index="1" ObjectRef="209"/>"#, "")
            .replace(
                r#"[{"channellabel":100},{"channellabel":101}]"#,
                r#"[{"channellabel":0}]"#,
            )
            .replace("<InPoint>381024000000</InPoint>", "<InPoint>0</InPoint>")
            .replace(
                "<OutPoint>2159136000000</OutPoint>",
                "<OutPoint>50803200000</OutPoint>",
            )
            .replace("</AudioClip>", "<Gain>2</Gain></AudioClip>")
    });
    edit("AudioClipTrackItem", "92", &|item| {
        item.replace("<End>1778112000000</End>", "<End>304819200000</End>")
    });
    edit("AudioClip", "162", &|clip| {
        clip.replace(r#"<SecondaryContentItem Index="1" ObjectRef="256"/>"#, "")
            .replace(
                r#"[{"channellabel":100},{"channellabel":101}]"#,
                r#"[{"channellabel":0}]"#,
            )
            .replace(
                "<OutPoint>2032128000000</OutPoint>",
                "<OutPoint>49025088000</OutPoint>",
            )
            .replace("</AudioClip>", "<Gain>0.5</Gain></AudioClip>")
    });
    edit("AudioClipTrackItem", "96", &|item| {
        item.replace(
            "<End>2032128000000</End>",
            "<Start>1778112000</Start><End>50803200000</End>",
        )
    });
    edit("AudioStream", "84", &|stream| {
        stream
            .replace(
                r#"[{"channellabel":100},{"channellabel":101}]"#,
                r#"[{"channellabel":0}]"#,
            )
            .replace("2032128000000", "50803200000")
    });
    // Inner saved solo propagation is an unmeasured bounded assumption.
    // Retained pitch-OFF caveats must not reject an otherwise supported clock.
    for id in ["144", "162"] {
        edit_record(
            &mut xml,
            &format!(r#"<AudioClip ObjectID="{id}""#),
            "</AudioClip>",
            |clip| {
                clip.replace(
                    "</Clip>",
                    "<MaintainAudioPitch>false</MaintainAudioPitch></Clip>",
                )
            },
        );
    }
    for (uid, flag) in [
        ("13858446-ca6c-47e7-946d-3dbbd623e9f9", "Solo"),
        ("3aed2b04-0876-4684-8768-22b8285a63c6", "MutedBySolo"),
        ("2c883571-b60f-454e-b5c1-5d8e28bfcb33", "MutedBySolo"),
        ("e91a20e9-682d-430e-a1ea-0c40d9a38573", "MutedBySolo"),
    ] {
        edit_record(
            &mut xml,
            &format!(r#"<AudioClipTrack ObjectUID="{uid}""#),
            "</AudioClipTrack>",
            |track| track.replace("</AudioTrack>", &format!("<{flag}>1</{flag}></AudioTrack>")),
        );
    }
    // An actual placement on a MutedBySolo track must not join the selected bus.
    edit_record(
        &mut xml,
        r#"<AudioClipTrack ObjectUID="3aed2b04-0876-4684-8768-22b8285a63c6""#,
        "</AudioClipTrack>",
        |track| {
            track.replace(
            r#"<ClipItems Version="3">"#,
            r#"<ClipItems Version="3"><TrackItems Version="1"><TrackItem Index="0" ObjectRef="96"/></TrackItems>"#,
        )
        },
    );
    xml.replace("nest_tone_stereo_8s.wav", "audio-mono.wav")
}

#[cfg(feature = "ffmpeg-library")]
fn import_nested_mono_control(
    root: &Path,
    xml: &str,
) -> (TesseractFile, Vec<premiere_file::Omission>) {
    for name in [
        "feature_timecoded_source.mp4",
        "audio-mono.wav",
        "nest_tone_stereo_8s.wav",
    ] {
        fs::copy(fixture(name), root.join(name)).unwrap();
    }
    let source = root.join("source.prproj");
    write_prproj(&source, xml);
    let output = root.join("tesseract");
    let notes =
        premiere_to_tesseract(source, &output, Some(NEST_OUTER_KEYS_SEQUENCE), false).unwrap();
    (TesseractFile::open(first_project(&output)).unwrap(), notes)
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn nested_mono_static_bus_keeps_onset_gain_and_exports_current_trim() {
    let root = tempfile::tempdir().unwrap();
    let (mut file, notes) = import_nested_mono_control(root.path(), &nested_mono_control_xml());
    assert!(
        !notes.iter().any(
            |note| note.scope == premiere_file::OmissionScope::Occurrence && note.record == "92"
        ),
        "{notes:?}"
    );
    for record in ["AudioClip:144", "AudioClip:162"] {
        assert!(
            notes.iter().any(|note| {
                note.record == record
                    && note.kind == premiere_file::OmissionKind::Approximated
                    && note.reason.contains("literal MaintainAudioPitch=false")
            }),
            "{notes:?}"
        );
    }
    let mut document = file.project_json().unwrap();
    let sounds: Vec<_> = document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .filter(|layer| layer["type"] == "Audio")
        .collect();
    let [sound] = sounds.try_into().unwrap();
    assert_eq!(
        crate::test_support::layer_range(sound),
        &json!({"start":1007,"duration":193})
    );
    assert_eq!(sound["sourceRange"], json!({"start":0,"duration":193}));
    // Parent Gain 2 × leaf Gain 0.5 × two independent mono center factors.
    assert!((sound["volume"].as_f64().unwrap() - 0.5).abs() < 1e-12);
    let asset_id = sound["source"]["assetId"].as_str().unwrap();
    let mut bytes = Vec::new();
    file.asset(asset_id)
        .unwrap()
        .open()
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, fs::read(fixture("audio-mono.wav")).unwrap());
    sound["volume"] = json!(0.25);
    sound["sourceRange"] = json!({"start":20,"duration":150});
    sound["playback"] = crate::test_support::linear_playback(
        json!({"start":1027,"duration":150}),
        json!({"start":20,"duration":150}),
    );
    let edited = root.path().join("edited.json");
    fs::write(&edited, serde_json::to_vec(&document).unwrap()).unwrap();
    file.commit_project_json(&edited).unwrap();
    file.save().unwrap();
    let native_dir = root.path().join("native");
    tesseract_to_premiere(
        first_project(&root.path().join("tesseract")),
        &native_dir,
        false,
    )
    .unwrap();
    let relocated = root.path().join("relocated");
    fs::rename(native_dir, &relocated).unwrap();
    let native_path = relocated.join("project.prproj");
    let sequence = super::support::exported_root_sequence(&native_path);
    let output = root.path().join("again");
    let notes = premiere_to_tesseract(native_path, &output, Some(&sequence), false).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    let again = TesseractFile::open(first_project(&output)).unwrap();
    let sounds: Vec<_> = editable_layers(&again)
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Audio")
        .cloned()
        .collect();
    assert_eq!(sounds.len(), 1);
    assert_eq!(
        sounds[0]["activeRange"],
        json!({"start":1027,"duration":150})
    );
    assert_eq!(sounds[0]["sourceRange"], json!({"start":20,"duration":150}));
    assert!((sounds[0]["volume"].as_f64().unwrap() - 0.25).abs() < 1e-6);
    // Independently inspect the emitted XML rather than relying only on reimport.
    let native = read_xml(&root.path().join("relocated/project.prproj"));
    let native = roxmltree::Document::parse(&native).unwrap();
    let clip = native
        .descendants()
        .find(|node| {
            node.has_tag_name("AudioClip")
                && node
                    .descendants()
                    .any(|child| child.has_tag_name("InPoint"))
        })
        .unwrap();
    assert_eq!(
        clip.descendants()
            .find(|node| node.has_tag_name("InPoint"))
            .unwrap()
            .text(),
        Some("5080320000")
    );
    assert_eq!(
        clip.descendants()
            .find(|node| node.has_tag_name("OutPoint"))
            .unwrap()
            .text(),
        Some("43182720000")
    );
    let level = native
        .descendants()
        .find(|node| {
            node.has_tag_name("AudioComponentParam")
                && node
                    .children()
                    .any(|child| child.has_tag_name("Name") && child.text() == Some("Level"))
        })
        .unwrap();
    let value: f64 = level
        .children()
        .find(|node| node.has_tag_name("StartKeyframe"))
        .unwrap()
        .text()
        .unwrap()
        .split(',')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    assert!((value - 0.177_827_939_391 * 0.25 * 2.0_f64.sqrt()).abs() < 1e-12);
    let source = clip
        .descendants()
        .find(|node| node.has_tag_name("Source"))
        .unwrap()
        .attribute("ObjectRef")
        .unwrap();
    assert!(native.descendants().any(|node| {
        node.has_tag_name("AudioMediaSource") && node.attribute("ObjectID") == Some(source)
    }));
}

#[cfg(feature = "ffmpeg-library")]
fn nested_mono_with_safe_sibling_xml() -> String {
    let mut base = nested_mono_control_xml();
    edit_record(
        &mut base,
        r#"<AudioClipTrackItem ObjectID="96""#,
        "</AudioClipTrackItem>",
        |item| {
            let copy = item
                .replace(r#"ObjectID="96""#, r#"ObjectID="9096""#)
                .replace("<Start>1778112000</Start>", "<Start>254016000000</Start>")
                .replace("<End>50803200000</End>", "<End>303041088000</End>");
            item.to_owned() + &copy
        },
    );
    edit_record(
        &mut base,
        r#"<AudioClipTrack ObjectUID="bab7a0e9-7c29-4b23-81fc-74ef0878146c""#,
        "</AudioClipTrack>",
        |track| {
            track.replace(r#"<ClipItems Version="3">"#, r#"<ClipItems Version="3"><TrackItems Version="1"><TrackItem Index="0" ObjectRef="9096"/></TrackItems>"#)
        },
    );
    base
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn nested_mono_rejected_routes_keep_safe_sibling() {
    let base = nested_mono_with_safe_sibling_xml();
    let mut noncenter = base.clone();
    edit_record(
        &mut noncenter,
        r#"<AudioComponentParam ObjectID="129""#,
        "</AudioComponentParam>",
        |param| {
            param.replace(",0.5,", ",0.2,").replace(
                "<CurrentValue>0.5</CurrentValue>",
                "<CurrentValue>0.2</CurrentValue>",
            )
        },
    );
    // Half-speed leaf with independently valid ordered endpoints. Clone its
    // native Clip/SubClip so the ordinary outer sibling remains unit-forward.
    let mut retimed = base.clone();
    edit_record(
        &mut retimed,
        r#"<AudioClip ObjectID="162""#,
        "</AudioClip>",
        |clip| {
            let copy = clip
                .replace(r#"ObjectID="162""#, r#"ObjectID="9162""#)
                .replace(
                    "<OutPoint>49025088000</OutPoint>",
                    "<OutPoint>24512544000</OutPoint>",
                )
                .replace("</Clip>", "<PlaybackSpeed>0.5</PlaybackSpeed></Clip>");
            clip.to_owned() + &copy
        },
    );
    edit_record(
        &mut retimed,
        r#"<SubClip ObjectID="126""#,
        "</SubClip>",
        |sub| {
            let copy = sub
                .replace(r#"ObjectID="126""#, r#"ObjectID="9126""#)
                .replace(r#"<Clip ObjectRef="162""#, r#"<Clip ObjectRef="9162""#);
            sub.to_owned() + &copy
        },
    );
    edit_record(
        &mut retimed,
        r#"<AudioClipTrackItem ObjectID="96""#,
        "</AudioClipTrackItem>",
        |item| {
            item.replace(
                r#"<SubClip ObjectRef="126""#,
                r#"<SubClip ObjectRef="9126""#,
            )
        },
    );
    // Two selectors share the unreadable scalar stage. A de-duplicated report
    // from the first must not make the second appear to have no omissions.
    let mut repeated_omission = base.clone();
    edit_record(
        &mut repeated_omission,
        r#"<AudioClip ObjectID="144""#,
        "</AudioClip>",
        |clip| clip.replace("<Gain>2</Gain>", "<Gain>NaN</Gain>"),
    );
    edit_record(
        &mut repeated_omission,
        r#"<AudioClipTrackItem ObjectID="92""#,
        "</AudioClipTrackItem>",
        |item| {
            let copy = item
                .replace(r#"ObjectID="92""#, r#"ObjectID="9092""#)
                .replace("<Start>254016000000</Start>", "<Start>508032000000</Start>")
                .replace("<End>304819200000</End>", "<End>558835200000</End>");
            item.to_owned() + &copy
        },
    );
    edit_record(
        &mut repeated_omission,
        r#"<AudioClipTrack ObjectUID="37f5cd0c-1963-4984-8926-1c9a6abdd24b""#,
        "</AudioClipTrack>",
        |track| {
            track.replace(
                r#"<TrackItem Index="0" ObjectRef="92"/>"#,
                r#"<TrackItem Index="0" ObjectRef="92"/><TrackItem Index="1" ObjectRef="9092"/>"#,
            )
        },
    );
    let mut stereo = base;
    edit_record(
        &mut stereo,
        r#"<AudioStream ObjectID="84""#,
        "</AudioStream>",
        |stream| {
            stream
                .replace(
                    r#"[{"channellabel":0}]"#,
                    r#"[{"channellabel":100},{"channellabel":101}]"#,
                )
                .replace("50803200000", "2032128000000")
        },
    );
    // The safe sibling selects channel 0 on its own verified outer track.
    stereo = stereo.replace("audio-mono.wav", "nest_tone_stereo_8s.wav");
    for (xml, reason, rejected) in [
        (noncenter, "verified centered stereo routing", &["92"][..]),
        (
            stereo,
            "source-extraction leaf is not yet mapped (converter follow-up)",
            &["92"][..],
        ),
        (retimed, "unit-forward pitch-OFF leaf", &["92"][..]),
        (
            repeated_omission,
            "without omitted processing",
            &["92", "9092"][..],
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (file, notes) = import_nested_mono_control(root.path(), &xml);
        for record in rejected {
            assert!(
                notes.iter().any(
                    |note| note.scope == premiere_file::OmissionScope::Occurrence
                        && note.record == *record
                        && note.reason.contains(reason)
                ),
                "{notes:?}"
            );
        }
        let layers = editable_layers(&file);
        let sounds: Vec<_> = layers
            .as_array()
            .unwrap()
            .iter()
            .filter(|layer| layer["type"] == "Audio")
            .collect();
        assert_eq!(sounds.len(), 1, "{layers:?}");
        assert_eq!(
            sounds[0]["activeRange"],
            json!({"start":1000,"duration":193})
        );
        assert!((sounds[0]["volume"].as_f64().unwrap() - 0.5 / 2.0_f64.sqrt()).abs() < 1e-12);
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn nested_mono_ambiguous_saved_solo_keeps_safe_sibling() {
    let base = nested_mono_with_safe_sibling_xml();
    let multiple = base.replace("<Solo>1</Solo>", "<Solo>0</Solo>").replace(
        "<MutedBySolo>1</MutedBySolo>",
        "<MutedBySolo>0</MutedBySolo>",
    );
    let mut inconsistent = base;
    edit_record(
        &mut inconsistent,
        r#"<AudioClipTrack ObjectUID="3aed2b04-0876-4684-8768-22b8285a63c6""#,
        "</AudioClipTrack>",
        |track| {
            track.replace(
                "<MutedBySolo>1</MutedBySolo>",
                "<MutedBySolo>0</MutedBySolo>",
            )
        },
    );
    for (xml, reason) in [
        (multiple, "exactly one audible direct mono leaf"),
        (inconsistent, "consistent saved solo routing"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (file, notes) = import_nested_mono_control(root.path(), &xml);
        assert!(
            notes.iter().any(|note| {
                note.scope == premiere_file::OmissionScope::Occurrence
                    && note.record == "92"
                    && note.reason.contains(reason)
            }),
            "{notes:?}"
        );
        let layers = editable_layers(&file);
        let sounds: Vec<_> = layers
            .as_array()
            .unwrap()
            .iter()
            .filter(|layer| layer["type"] == "Audio")
            .collect();
        assert_eq!(sounds.len(), 1, "{layers:?}");
        assert_eq!(
            sounds[0]["activeRange"],
            json!({"start":1000,"duration":193})
        );
        assert_eq!(sounds[0]["sourceRange"], json!({"start":0,"duration":193}));
        assert!((sounds[0]["volume"].as_f64().unwrap() - 0.5 / 2.0_f64.sqrt()).abs() < 1e-12);
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

#[cfg(feature = "ffmpeg-library")]
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

#[cfg(feature = "ffmpeg-library")]
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

/// Each audio layer's volume keys as (layer time in milliseconds, gain, easing).
fn gain_keys(file: &TesseractFile) -> Vec<Vec<(i64, f64, Value)>> {
    let document = file.project_json().unwrap();
    let composition = &document["composition"];
    composition["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Audio")
        .map(|layer| {
            composition["dynamics"]["entries"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["target"]["layerId"] == layer["id"])
                .map_or_else(Vec::new, |entry| {
                    entry["animator"]["keyframes"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|key| {
                            (
                                key["layerTime"].as_i64().unwrap(),
                                key["value"]["value"].as_f64().unwrap(),
                                key["easing"].clone(),
                            )
                        })
                        .collect()
                })
        })
        .collect()
}

/// Premiere's fade-in gain, from the curves that run A5 fitted to AME renders
/// of these fixtures.
#[cfg(feature = "ffmpeg-library")]
fn premiere_fade_gain(curve: &str, progress: f64) -> f64 {
    match curve {
        "Constant Gain" => progress,
        "Constant Power" => (std::f64::consts::FRAC_PI_2 * progress.powf(0.6457))
            .sin()
            .powi(2),
        "Exponential Fade" => (3.5 * progress).exp_m1() / 3.5_f64.exp_m1(),
        _ => unreachable!(),
    }
}

/// The gain of volume keys at `millis`, for Linear and x1 = 1/3, x2 = 2/3
/// cubic easing, holding endpoint values outside the track as runtime does.
fn eased_gain_at(keys: &[(i64, f64, Value)], millis: f64) -> f64 {
    if millis <= keys[0].0 as f64 {
        return keys[0].1;
    }
    let last = keys.last().unwrap();
    if millis >= last.0 as f64 {
        return last.1;
    }
    let next = keys
        .iter()
        .position(|key| key.0 as f64 >= millis)
        .unwrap()
        .max(1);
    let (from, to) = (&keys[next - 1], &keys[next]);
    let t = (millis - from.0 as f64) / (to.0 - from.0) as f64;
    let progress = match to.2["type"].as_str().unwrap() {
        "linear" => t,
        "cubicBezier" => {
            let y = |name: &str| to.2[name].as_f64().unwrap();
            assert_eq!((y("x1"), y("x2")), (1.0 / 3.0, 2.0 / 3.0));
            3.0 * (1.0 - t).powi(2) * t * y("y1") + 3.0 * (1.0 - t) * t * t * y("y2") + t.powi(3)
        }
        other => panic!("unexpected {other} easing in a fade"),
    };
    from.1 + (to.1 - from.1) * progress
}

/// Checks the keys of one fade over `start..=end` ms: its key count, silence
/// at one end and `level` at the other, and at most 0.25 dB from Premiere's
/// curve wherever that is above -60 dB of the level.
#[cfg(feature = "ffmpeg-library")]
fn assert_fade(
    keys: &[(i64, f64, Value)],
    (curve, fade_in, start, end): (&str, bool, i64, i64),
    level: f64,
) {
    let run: Vec<_> = keys
        .iter()
        .filter(|key| (start..=end).contains(&key.0))
        .cloned()
        .collect();
    let inner = match curve {
        "Constant Gain" => 0,
        "Exponential Fade" => 1,
        _ => 2,
    };
    assert_eq!(run.len(), 2 + inner, "{curve} {start}..{end}: {run:?}");
    let edges = if fade_in { (0.0, level) } else { (level, 0.0) };
    assert_eq!((run[0].0, run[run.len() - 1].0), (start, end));
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-12;
    assert!(close(run[0].1, edges.0) && close(run[run.len() - 1].1, edges.1));
    let span = (end - start) as f64;
    let error = (1..2000)
        .map(|index| f64::from(index) / 2000.0)
        .filter_map(|tau| {
            let progress = if fade_in { tau } else { 1.0 - tau };
            let expected = level * premiere_fade_gain(curve, progress);
            let actual = eased_gain_at(&run, start as f64 + tau * span);
            (expected >= level * 1e-3).then(|| (20.0 * (actual / expected).log10()).abs())
        })
        .fold(0.0, f64::max);
    assert!(error <= 0.25, "{curve} {start}..{end}: {error} dB");
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn adobe_audio_transitions_import_as_eased_volume_keys() {
    // Premiere 26.5.1 saved 21 transitions of each curve: one-sided fades and
    // crossfades, whose halves import as the fades of the extended clips. The
    // AME render of this project fits Constant Gain as linear gain, Constant
    // Power as sin²(π/2·τ^0.6457) and Exponential Fade with k = 3.5.
    let temp = tempfile::tempdir().unwrap();
    let (archive, omissions) = convert_pinned_case(
        temp.path(),
        "feature_audio_transitions_strict.prproj",
        "991a9ef8-4b28-4e6f-9815-0d534a178462",
    );
    assert!(omissions.is_empty(), "{omissions:?}");
    let file = TesseractFile::open(&archive).unwrap();
    let (gain, power, exponential) = ("Constant Gain", "Constant Power", "Exponential Fade");
    let expected: [&[(&str, bool, i64, i64)]; 21] = [
        &[(gain, true, 0, 2000), (gain, false, 2500, 4500)],
        &[(gain, true, 0, 2000), (gain, false, 2500, 4500)],
        &[(gain, false, 1500, 3500)],
        &[(gain, true, 0, 2000)],
        &[(power, true, 0, 2000), (power, false, 2500, 4500)],
        &[(power, true, 0, 2000), (power, false, 2500, 4500)],
        &[(power, false, 1500, 3500)],
        &[(power, true, 0, 2000)],
        &[(power, false, 2000, 3000)],
        &[(power, true, 0, 1000)],
        &[(power, false, 1000, 2000)],
        &[(power, true, 0, 1000)],
        &[(power, false, 1500, 2300)],
        &[(power, true, 0, 800)],
        &[(power, true, 0, 1000), (power, false, 2133, 2400)],
        &[(power, false, 1626, 2375)],
        &[(power, true, 0, 749)],
        &[
            (exponential, true, 0, 2000),
            (exponential, false, 2500, 4500),
        ],
        &[
            (exponential, true, 0, 2000),
            (exponential, false, 2500, 4500),
        ],
        &[(exponential, false, 1500, 3500)],
        &[(exponential, true, 0, 2000)],
    ];
    let keys = gain_keys(&file);
    assert_eq!(keys.len(), expected.len());
    for (layer, fades) in keys.iter().zip(expected) {
        assert_eq!(
            layer.len(),
            fades
                .iter()
                .map(|fade| match fade.0 {
                    "Constant Gain" => 2,
                    "Exponential Fade" => 3,
                    _ => 4,
                })
                .sum::<usize>()
        );
        for fade in fades {
            assert_fade(layer, *fade, 1.0);
        }
    }
    // The inner keys sit at 0.013 and 0.193 of a Constant Power span.
    assert_eq!(
        keys[4].iter().map(|key| key.0).collect::<Vec<_>>(),
        [0, 26, 386, 2000, 2500, 4114, 4474, 4500]
    );

    // The combinations: -24.02 dB of Level, Clip Gain and a track fader; a
    // keyed Level that holds -10 dB over its fade; a 100 ms fade of a mono
    // clip; and a fade-in of the sound of a linked A/V clip.
    let temp = tempfile::tempdir().unwrap();
    let (archive, omissions) = convert_pinned_case(
        temp.path(),
        "feature_audio_transitions_combos_strict.prproj",
        "ef8d5b7b-90e9-4b60-850a-46afc0ea74ef",
    );
    // Only the ID-only master clip of the linked picture is reported.
    assert!(
        omissions
            .iter()
            .all(|item| item.record == "VideoClipTrackItem:93"
                || item.record == "93"
                || item.record == "MasterClip:1f948799-ba20-422c-8960-9897cc468413"),
        "{omissions:?}"
    );
    let file = TesseractFile::open(&archive).unwrap();
    let keys = gain_keys(&file);
    // A fade plays at its layer's static level, which folds the centered
    // route of a mono clip.
    let levels: Vec<_> = editable_layers(&file)
        .as_array()
        .unwrap()
        .iter()
        .filter(|layer| layer["type"] == "Audio")
        .map(|layer| layer["volume"].as_f64().unwrap())
        .collect();
    let db = |gain: f64| 20.0 * gain.log10();
    assert_eq!(keys.len(), 5);
    assert_fade(&keys[0], (power, false, 3000, 4000), 1.0);
    assert_fade(&keys[1], (power, false, 3000, 4000), keys[1][0].1);
    assert!((db(keys[1][0].1) + 24.0206).abs() < 1e-3);
    assert_eq!(
        keys[2][..2].iter().map(|key| key.0).collect::<Vec<_>>(),
        [200, 1200]
    );
    assert_fade(&keys[2], (power, false, 3000, 4000), keys[2][1].1);
    assert!((db(keys[2][1].1) + 10.0).abs() < 1e-6);
    assert_fade(&keys[3], (power, false, 100, 200), levels[3]);
    assert_fade(&keys[4], (power, true, 0, 1000), levels[4]);
}

/// Each audio layer's keys, compared across a round trip: key times, gains
/// within 1e-12 and easing parameters within 1e-9.
#[cfg(feature = "ffmpeg-library")]
fn assert_same_keys(actual: &[Vec<(i64, f64, Value)>], expected: &[Vec<(i64, f64, Value)>]) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual.len(), expected.len(), "{actual:?} {expected:?}");
        for (key, expected) in actual.iter().zip(expected) {
            assert_eq!(key.0, expected.0);
            assert!((key.1 - expected.1).abs() <= 1e-12, "{key:?} {expected:?}");
            assert_eq!(key.2["type"], expected.2["type"]);
            for name in ["x1", "y1", "x2", "y2"] {
                if let (Some(a), Some(b)) = (key.2[name].as_f64(), expected.2[name].as_f64()) {
                    assert!((a - b).abs() <= 1e-9, "{key:?} {expected:?}");
                }
            }
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn adobe_audio_transitions_survive_edits_export_and_reimport() {
    // Each fade exports as a one-sided transition on its placement's track,
    // crossfade halves included, and reimports as the same keys.
    let temp = tempfile::tempdir().unwrap();
    let (archive, _) = convert_pinned_case(
        temp.path(),
        "feature_audio_transitions_strict.prproj",
        "991a9ef8-4b28-4e6f-9815-0d534a178462",
    );
    let imported = gain_keys(&TesseractFile::open(&archive).unwrap());
    let native = temp.path().join("native");
    tesseract_to_premiere(&archive, &native, false).unwrap();
    let xml = read_xml(&native.join("project.prproj"));
    assert_eq!(xml.matches("<AudioTransitionTrackItem ").count(), 28);
    assert_eq!(xml.matches("<HeadTransition ").count(), 14);
    assert_eq!(xml.matches("<TailTransition ").count(), 14);
    // A fade-only layer plays its level as a static Level.
    assert_eq!(xml.matches("<Keyframes>").count(), 0);
    let again = temp.path().join("again");
    let omissions =
        premiere_to_tesseract(native.join("project.prproj"), &again, None, false).unwrap();
    assert!(omissions.is_empty(), "{omissions:?}");
    assert_same_keys(
        &gain_keys(&TesseractFile::open(first_project(&again)).unwrap()),
        &imported,
    );

    // Edits: move layer 1 by 250 ms, give layer 5 the 1 s Constant Power
    // fade-in of layer 10, and lower layer 18 (Exponential Fade) to -6 dB.
    let file = TesseractFile::open(&archive).unwrap();
    let mut document = file.project_json().unwrap();
    let composition = &mut document["composition"];
    let playback = &mut composition["layers"][0]["playback"];
    playback["inputRange"]["start"] = json!(250);
    playback["mapping"]["input"]["start"] = json!(250);
    let entries = composition["dynamics"]["entries"].as_array_mut().unwrap();
    let keys = |entries: &[Value], index: usize| {
        entries[index]["animator"]["keyframes"]
            .as_array()
            .unwrap()
            .clone()
    };
    let (layer_5, layer_10) = (keys(entries, 4), keys(entries, 9));
    let mut moved: Vec<_> = layer_10.clone();
    moved.extend(layer_5[4..].iter().cloned());
    for (index, key) in moved.iter_mut().enumerate() {
        key["id"] = json!(format!("edited-5-{index}"));
    }
    entries[4]["animator"]["keyframes"] = json!(moved);
    for key in entries[17]["animator"]["keyframes"].as_array_mut().unwrap() {
        let gain = key["value"]["value"].as_f64().unwrap();
        key["value"]["value"] = json!(gain * 0.5);
    }
    let edited = temp.path().join("edited.tsrct");
    let (asset_id, asset) = file.metadata().assets.iter().next().unwrap();
    assert_eq!(asset.kind, AssetKind::Audio);
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(
            asset_id,
            fixture("feature_audio_tone_right.wav"),
            AssetKind::Audio,
        )
        .unwrap()
        .write(&edited)
        .unwrap();
    let exported = temp.path().join("edited-native");
    tesseract_to_premiere(&edited, &exported, false).unwrap();
    let xml = read_xml(&exported.join("project.prproj"));
    assert_eq!(xml.matches("<AudioTransitionTrackItem ").count(), 28);
    assert_eq!(xml.matches("<Keyframes>").count(), 0);
    let reimported = temp.path().join("edited-again");
    premiere_to_tesseract(exported.join("project.prproj"), &reimported, None, false).unwrap();
    let file = TesseractFile::open(first_project(&reimported)).unwrap();
    let reimported_keys = gain_keys(&file);
    let expected = gain_keys(&TesseractFile::open(&edited).unwrap());
    // Layer 18 now holds -6 dB: its keys are half of the imported ones.
    assert!((expected[17][1].1 - imported[17][1].1 * 0.5).abs() < 1e-12);
    assert_eq!(expected[4].len(), 8);
    assert_eq!(
        expected[4].iter().map(|key| key.0).collect::<Vec<_>>(),
        [0, 13, 193, 1000, 2500, 4114, 4474, 4500]
    );
    assert_same_keys(&reimported_keys, &expected);
    let document = file.project_json().unwrap();
    assert_eq!(
        *crate::test_support::layer_range(&document["composition"]["layers"][0]),
        json!({"start": 250, "duration": 4500})
    );

    // The combinations keep their levels, the keyed Level and the linked
    // sound: five one-sided transitions, and Level keys for K2 only.
    let temp = tempfile::tempdir().unwrap();
    let (archive, _) = convert_pinned_case(
        temp.path(),
        "feature_audio_transitions_combos_strict.prproj",
        "ef8d5b7b-90e9-4b60-850a-46afc0ea74ef",
    );
    let imported = gain_keys(&TesseractFile::open(&archive).unwrap());
    let native = temp.path().join("native");
    tesseract_to_premiere(&archive, &native, false).unwrap();
    let xml = read_xml(&native.join("project.prproj"));
    assert_eq!(xml.matches("<AudioTransitionTrackItem ").count(), 5);
    assert_eq!(xml.matches("<Keyframes>").count(), 1);
    let again = temp.path().join("again");
    premiere_to_tesseract(native.join("project.prproj"), &again, None, false).unwrap();
    assert_same_keys(
        &gain_keys(&TesseractFile::open(first_project(&again)).unwrap()),
        &imported,
    );
}

#[test]
fn native_custom_fade_ins_export_unedited_and_gain_edits() {
    let temp = tempfile::tempdir().unwrap();
    let (archive, omissions) = convert_pinned_case(
        temp.path(),
        "feature_audio_custom_fades_strict.prproj",
        "991a9ef8-4b28-4e6f-9815-0d534a178462",
    );
    assert!(
        !omissions
            .iter()
            .any(|note| note.reason.contains("not converted")),
        "{omissions:?}"
    );
    let file = TesseractFile::open(&archive).unwrap();
    let imported = gain_keys(&file);
    assert_eq!(imported.len(), 4);
    assert_eq!(
        omissions
            .iter()
            .filter(|note| note.reason.starts_with("Custom Fade incoming shape"))
            .count(),
        4
    );
    let provenance: Value = serde_json::from_slice(
        &fs::read(fixture("feature_audio_custom_fades_strict.provenance.json")).unwrap(),
    )
    .unwrap();
    use sha2::{Digest, Sha256};
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(fs::read(fixture("feature_audio_custom_fades_strict.prproj")).unwrap())
        ),
        provenance["fixture_sha256"].as_str().unwrap()
    );
    for (index, (keys, shape)) in imported.iter().zip([-23, -6, 0, 29]).enumerate() {
        let control = &provenance["controls"][index];
        assert_eq!(control["shape"], shape);
        for sample in control["incoming_gain_samples"].as_array().unwrap() {
            let actual = eased_gain_at(keys, sample[0].as_f64().unwrap() * 2000.0);
            let native = sample[1].as_f64().unwrap();
            assert!((20.0 * (actual / native).log10()).abs() <= 0.35);
        }
        assert!(keys.len() >= 3, "shape {shape}: {keys:?}");
        assert_eq!((keys[0].0, keys.last().unwrap().0), (0, 2000));
        assert_eq!((keys[0].1, keys.last().unwrap().1), (0.0, 1.0));
        let power = 10.0_f64.powf(-f64::from(shape) / 100.0);
        for millis in [250.0, 500.0, 1000.0, 1500.0, 1900.0] {
            let expected = (std::f64::consts::FRAC_PI_2 * (millis / 2000.0_f64).powf(power))
                .sin()
                .powi(2);
            let actual = eased_gain_at(keys, millis);
            assert!(
                (20.0 * (actual / expected).log10()).abs() <= 0.25,
                "shape {shape} at {millis}: {actual} != {expected}"
            );
        }
        let layers = editable_layers(&file);
        assert_eq!(
            layers[index]["activeRange"],
            json!({"start": index * 2500, "duration": 2500})
        );
        assert_eq!(
            layers[index]["sourceRange"],
            json!({"start": 0, "duration": 2500})
        );
    }
    // Exercise the common paths directly: no edit and gain-only edit, without
    // changing easing to evade ordinary fade recognition.
    for factor in [1.0, 0.5] {
        let mut document = file.project_json().unwrap();
        for entry in document["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
        {
            for key in entry["animator"]["keyframes"].as_array_mut().unwrap() {
                key["value"]["value"] = json!(key["value"]["value"].as_f64().unwrap() * factor);
            }
        }
        let edited = temp.path().join(format!("gain-{factor}.tsrct"));
        let (asset_id, _) = file.metadata().assets.iter().next().unwrap();
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
            .unwrap()
            .add_asset(
                asset_id,
                fixture("feature_audio_tone_right.wav"),
                AssetKind::Audio,
            )
            .unwrap()
            .write(&edited)
            .unwrap();
        let native = temp.path().join(format!("native-{factor}"));
        tesseract_to_premiere(&edited, &native, false).unwrap();
        let xml = read_xml(&native.join("project.prproj"));
        assert!(!xml.contains("<MatchName>Custom Fade</MatchName>"));
        assert_eq!(xml.matches("<Keyframes>").count(), 4);
        assert_eq!(
            xml.matches("<FilterMatchName>Internal Volume Stereo</FilterMatchName>")
                .count(),
            4
        );
        let parsed = roxmltree::Document::parse(&xml).unwrap();
        let transitions: Vec<_> = parsed
            .root_element()
            .children()
            .filter(|node| node.has_tag_name("AudioTransitionTrackItem"))
            .collect();
        assert_eq!(transitions.len(), 3);
        let mut starts = Vec::new();
        for transition in &transitions {
            let body = transition
                .children()
                .find(|node| node.has_tag_name("TransitionTrackItem"))
                .unwrap();
            let text = |parent: roxmltree::Node<'_, '_>, name: &str| {
                parent
                    .children()
                    .find(|node| node.has_tag_name(name))
                    .and_then(|node| node.text())
                    .map(str::to_owned)
            };
            assert_eq!(text(body, "MatchName").as_deref(), Some("Constant Gain"));
            assert_eq!(text(body, "HasIncomingClip").as_deref(), Some("true"));
            assert_eq!(text(body, "HasOutgoingClip").as_deref(), Some("false"));
            assert_eq!(text(body, "Alignment").as_deref(), Some("0"));
            let range = body
                .children()
                .find(|node| node.has_tag_name("TrackItem"))
                .unwrap();
            let start = text(range, "Start").map_or(0, |value| value.parse::<i64>().unwrap());
            let end = text(range, "End").unwrap().parse::<i64>().unwrap();
            starts.push(start);
            assert_eq!(end - start, 26 * 254_016_000);
        }
        starts.sort_unstable();
        assert_eq!(starts, [0, 635_040_000_000, 1_270_080_000_000]);
        let again = temp.path().join(format!("again-{factor}"));
        premiere_to_tesseract(native.join("project.prproj"), &again, None, false).unwrap();
        let reimported = gain_keys(&TesseractFile::open(first_project(&again)).unwrap());
        for (before, after) in imported.iter().zip(&reimported) {
            assert_eq!(after[0].1, 0.0);
            assert_eq!(after.last().unwrap().1, factor);
            for millis in (5..=2500).step_by(5) {
                let expected = eased_gain_at(before, f64::from(millis)) * factor;
                if expected >= 0.001 * factor {
                    let actual = eased_gain_at(after, f64::from(millis));
                    assert!(
                        (20.0 * (actual / expected).log10()).abs() <= 0.27,
                        "gain {factor} at {millis}ms: {actual} != {expected}"
                    );
                }
            }
        }
    }
}

#[test]
fn native_outgoing_custom_shapes_compose_half_gain_and_export_edits() {
    let temp = tempfile::tempdir().unwrap();
    let (archive, omissions) = convert_pinned_case(
        temp.path(),
        "feature_audio_custom_fade_outs_half_gain_strict.prproj",
        "03196625-7bd6-57d3-ac85-3640c29c817e",
    );
    let file = TesseractFile::open(&archive).unwrap();
    let imported = gain_keys(&file);
    assert_eq!(imported.len(), 6);
    assert_eq!(
        omissions
            .iter()
            .filter(|note| note.reason.starts_with("Custom Fade outgoing shape"))
            .count(),
        6
    );
    let provenance: Value = serde_json::from_slice(
        &fs::read(fixture(
            "feature_audio_custom_fade_outs_half_gain_strict.provenance.json",
        ))
        .unwrap(),
    )
    .unwrap();
    use sha2::{Digest, Sha256};
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(
                fs::read(fixture(
                    "feature_audio_custom_fade_outs_half_gain_strict.prproj"
                ))
                .unwrap()
            )
        ),
        provenance["fixture_sha256"].as_str().unwrap()
    );
    for (index, keys) in imported.iter().enumerate() {
        assert_eq!((keys[0].0, keys.last().unwrap().0), (500, 2500));
        assert_eq!((keys[0].1, keys.last().unwrap().1), (0.5, 0.0));
        assert_eq!(eased_gain_at(keys, 250.0), 0.5);
        for sample in provenance["controls"][index]["outgoing_gain_samples"]
            .as_array()
            .unwrap()
        {
            let actual = eased_gain_at(keys, 500.0 + sample[0].as_f64().unwrap() * 2000.0);
            let native = sample[1].as_f64().unwrap();
            assert!((20.0 * (actual / native).log10()).abs() <= 0.35);
        }
        let layers = editable_layers(&file);
        assert_eq!(
            layers[index]["activeRange"],
            json!({"start":index * 2500,"duration":2500})
        );
        assert_eq!(
            layers[index]["sourceRange"],
            json!({"start":0,"duration":2500})
        );
    }
    for factor in [1.0, 0.5] {
        let mut document = file.project_json().unwrap();
        for entry in document["composition"]["dynamics"]["entries"]
            .as_array_mut()
            .unwrap()
        {
            for key in entry["animator"]["keyframes"].as_array_mut().unwrap() {
                key["value"]["value"] = json!(key["value"]["value"].as_f64().unwrap() * factor);
            }
        }
        let edited = temp.path().join(format!("outgoing-{factor}.tsrct"));
        let (id, _) = file.metadata().assets.iter().next().unwrap();
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
            .unwrap()
            .add_asset(
                id,
                fixture("feature_audio_tone_right.wav"),
                AssetKind::Audio,
            )
            .unwrap()
            .write(&edited)
            .unwrap();
        let native = temp.path().join(format!("native-outgoing-{factor}"));
        tesseract_to_premiere(&edited, &native, false).unwrap();
        let xml = read_xml(&native.join("project.prproj"));
        assert!(!xml.contains("<MatchName>Custom Fade</MatchName>"));
        assert_eq!(xml.matches("<Keyframes>").count(), 6);
        let again = temp.path().join(format!("again-outgoing-{factor}"));
        premiere_to_tesseract(native.join("project.prproj"), &again, None, false).unwrap();
        let reimported = gain_keys(&TesseractFile::open(first_project(&again)).unwrap());
        assert_eq!(reimported.len(), 6);
        for (before, after) in imported.iter().zip(&reimported) {
            assert_eq!(after.last().unwrap().1, 0.0);
            for millis in (0..2500).step_by(5) {
                let expected = eased_gain_at(before, f64::from(millis)) * factor;
                if expected >= 0.0005 * factor {
                    let actual = eased_gain_at(after, f64::from(millis));
                    assert!(
                        (20.0 * (actual / expected).log10()).abs() <= 0.27,
                        "outgoing gain {factor} at {millis}: {actual} != {expected}"
                    );
                }
            }
        }
    }
}

#[test]
fn dropped_incoming_sound_has_no_custom_approximation_claim() {
    let temp = tempfile::tempdir().unwrap();
    let mut xml = read_xml(&fixture("feature_audio_custom_fades_strict.prproj"));
    edit_record(
        &mut xml,
        r#"<AudioClip ObjectID="110""#,
        "</AudioClip>",
        |record| record.replace("</Clip>", "<PlaybackSpeed>0.9</PlaybackSpeed></Clip>"),
    );
    let source = temp.path().join("dropped.prproj");
    write_prproj(&source, &xml);
    fs::copy(
        fixture("feature_audio_tone_right.wav"),
        temp.path().join("feature_audio_tone_right.wav"),
    )
    .unwrap();
    let output = temp.path().join("converted");
    let omissions = premiere_to_tesseract(&source, &output, None, false).unwrap();
    assert!(omissions.iter().any(|note| note
        .reason
        .contains("saved speed and source/timeline spans disagree")));
    assert_eq!(
        omissions
            .iter()
            .filter(|note| note.reason.starts_with("Custom Fade incoming shape"))
            .count(),
        3
    );
    assert!(!omissions
        .iter()
        .any(|note| note.record == "AudioTransitionTrackItem:73"
            && note.kind == premiere_file::OmissionKind::Approximated));
    let file = TesseractFile::open(first_project(&output)).unwrap();
    assert_eq!(gain_keys(&file).len(), 3);
}

#[test]
fn short_power_fades_keep_cuts_and_use_inward_frame_edges() {
    let temp = tempfile::tempdir().unwrap();
    let mut short = read_xml(&fixture("feature_audio_transitions_strict.prproj"));
    edit_record(
        &mut short,
        r#"<AudioTransitionTrackItem ObjectID="95""#,
        "</AudioTransitionTrackItem>",
        |record| record.replace("<End>4318272000000</End>", "<End>3814304256000</End>"),
    );
    edit_record(
        &mut short,
        r#"<AudioTransitionTrackItem ObjectID="96""#,
        "</AudioTransitionTrackItem>",
        |record| {
            record
                .replace(
                    "<Start>4445280000000</Start>",
                    "<Start>4949247744000</Start>",
                )
                .replace(
                    "<Alignment>508032000000</Alignment>",
                    "<Alignment>4064256000</Alignment>",
                )
        },
    );
    let source = temp.path().join("short.prproj");
    write_prproj(&source, &short);
    fs::copy(
        fixture("feature_audio_tone_right.wav"),
        temp.path().join("feature_audio_tone_right.wav"),
    )
    .unwrap();
    let output = temp.path().join("converted");
    let omissions = premiere_to_tesseract(
        &source,
        &output,
        Some("991a9ef8-4b28-4e6f-9815-0d534a178462"),
        false,
    )
    .unwrap();
    assert!(
        !omissions
            .iter()
            .any(|note| note.reason.contains("too short")),
        "{omissions:?}"
    );
    let file = TesseractFile::open(first_project(&output)).unwrap();
    let keys = &gain_keys(&file)[4];
    assert_eq!(
        keys.iter().map(|key| key.0).collect::<Vec<_>>(),
        [0, 1, 13, 67, 4433, 4487, 4499, 4500]
    );
    let layers = editable_layers(&file);
    assert_eq!(
        layers[4]["activeRange"],
        json!({"start": 15000, "duration": 4500})
    );
    assert_eq!(
        layers[4]["sourceRange"],
        json!({"start": 0, "duration": 4500})
    );
    assert_eq!((keys[0].1, keys.last().unwrap().1), (0.0, 0.0));
    assert_eq!((keys[3].1, keys[4].1), (1.0, 1.0));
}

/// Two trimmed root videos sharing independently selected picture and sound.
#[cfg(feature = "ffmpeg-library")]
fn selected_embedded_document(eye: bool, enhanced: bool) -> Value {
    let mut document = editable_document();
    let mut video = document["composition"]["layers"][0].clone();
    video["sourceIntrinsicDuration"] = json!(200);
    video["volume"] = json!(0.5);
    video["source"]["eyeContact"] = json!({"enabled": eye, "eyeContactAssetId": "eye"});
    video["source"]["audioEnhancement"] =
        json!({"enabled": enhanced, "enhancedAssetId": "enhanced"});
    video["sourceRange"] = json!({"start": 100, "duration": 100});
    video["playback"] = crate::test_support::linear_playback(
        json!({"start": 100, "duration": 100}),
        video["sourceRange"].clone(),
    );
    let mut second = video.clone();
    second["id"] = json!(3);
    second["sourceRange"] = json!({"start": 0, "duration": 100});
    second["playback"] = crate::test_support::linear_playback(
        json!({"start": 500, "duration": 100}),
        second["sourceRange"].clone(),
    );
    let canvas = document["composition"]["layers"][1].clone();
    document["composition"]["layers"] = json!([video, second, canvas]);
    document
}

#[cfg(feature = "ffmpeg-library")]
fn write_embedded_archive(
    root: &Path,
    document: &Value,
    enhanced: &Path,
    fault: Option<(&str, Fault)>,
) -> PathBuf {
    let original = fixture("video-with-audio.mp4");
    let eye = root.join("eye.mp4");
    fs::copy(&original, &eye).unwrap();
    let sources = [
        ("premiere-video-1", original, AssetKind::Video),
        ("eye", eye, AssetKind::Video),
        (
            "enhanced",
            enhanced.to_owned(),
            if enhanced.extension().unwrap() == "mp4" {
                AssetKind::Video
            } else {
                AssetKind::Audio
            },
        ),
    ];
    let missing = |id: &str| matches!(fault, Some((bad, Fault::Missing)) if id == bad);
    let mut builder =
        TesseractFileBuilder::from_project_json(&serde_json::to_vec(document).unwrap()).unwrap();
    for (id, path, kind) in &sources {
        if !missing(id) {
            builder = builder.add_asset(*id, path, *kind).unwrap();
        }
    }
    let archive = root.join("source.tsrct");
    builder
        .write_with_runtime_assets(&archive, |asset| missing(asset.asset_id))
        .unwrap();
    if let Some((id, Fault::Damaged)) = fault {
        let payload = fs::read(&sources.iter().find(|(key, _, _)| *key == id).unwrap().1).unwrap();
        let file = TesseractFile::open(&archive).unwrap();
        let path = file.metadata().assets[id].path.as_bytes();
        let mut bytes = fs::read(&archive).unwrap();
        // Identical original and Eye Contact payloads have distinct ZIP entries.
        let entry = bytes.windows(path.len()).position(|w| w == path).unwrap();
        let at = entry
            + bytes[entry..]
                .windows(payload.len())
                .position(|w| w == payload)
                .unwrap();
        bytes[at + 44] ^= 1;
        fs::write(&archive, bytes).unwrap();
    }
    archive
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn embedded_selection_keeps_independent_picture_sound_trim_and_gain() {
    for (eye, enhanced, sound) in [
        (false, true, "audio-mono.wav"),
        (true, true, "audio-mono.wav"),
        (true, false, "audio-mono.wav"),
        (true, true, "video-with-audio.mp4"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let archive = write_embedded_archive(
            root,
            &selected_embedded_document(eye, enhanced),
            &fixture(sound),
            None,
        );
        let native = root.join("native");
        let omissions = tesseract_to_premiere(&archive, &native, false).unwrap();
        let xml = read_xml(&native.join("project.prproj"));
        assert_eq!(
            xml.matches("<AudioClipTrackItem ").count(),
            2,
            "{eye}/{enhanced}/{sound}: {omissions:?}"
        );
        assert_eq!(xml.matches("<VideoClipTrackItem ").count(), 2);
        let active_sound = if enhanced {
            sound
        } else {
            "video-with-audio.mp4"
        };
        let active_picture = if eye {
            "eye.mp4"
        } else {
            "video-with-audio.mp4"
        };
        let mut expected = vec![active_picture.to_owned(), active_sound.to_owned()];
        expected.sort();
        expected.dedup();
        assert_eq!(packaged_media(&native), expected);
        assert_eq!(
            fs::read(native.join("media").join(active_sound)).unwrap(),
            fs::read(fixture(active_sound)).unwrap()
        );
        let again = root.join("again");
        premiere_to_tesseract(native.join("project.prproj"), &again, None, false).unwrap();
        let file = TesseractFile::open(first_project(&again)).unwrap();
        let layers = editable_layers(&file);
        let mut sounds: Vec<_> = layers
            .as_array()
            .unwrap()
            .iter()
            .filter(|l| l["type"] == "Audio")
            .collect();
        sounds.sort_by_key(|l| l["activeRange"]["start"].as_i64());
        assert_eq!(sounds.len(), 2);
        for (sound, start, source_start) in [(sounds[0], 100, 100), (sounds[1], 500, 0)] {
            assert_eq!(
                sound["activeRange"],
                json!({"start": start, "duration": 100})
            );
            assert_eq!(
                sound["sourceRange"],
                json!({"start": source_start, "duration": 100})
            );
            assert_eq!(sound["source"][0], active_sound);
            assert!((sound["volume"].as_f64().unwrap() - 0.5).abs() < 1e-9);
        }
        for picture in layers
            .as_array()
            .unwrap()
            .iter()
            .filter(|l| l["type"] == "Video")
        {
            assert_eq!(picture["source"][0], active_picture);
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn embedded_selection_reads_only_sources_used_by_picture_or_sound() {
    for (enhanced, inactive) in [(true, "premiere-video-1"), (false, "enhanced")] {
        for fault in [Fault::Missing, Fault::Damaged] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path();
            let document = selected_embedded_document(true, enhanced);
            let archive = write_embedded_archive(
                root,
                &document,
                &fixture("audio-mono.wav"),
                Some((inactive, fault)),
            );
            let native = root.join("native");
            tesseract_to_premiere(&archive, &native, false).unwrap();
            let xml = read_xml(&native.join("project.prproj"));
            assert_eq!(xml.matches("<AudioClipTrackItem ").count(), 2);
            assert_eq!(packaged_media(&native).len(), 2);
        }
    }
    // The original is inactive for sound but still selected by the picture.
    // It cannot escape byte/availability checks merely because enhancement is on.
    for (eye, enhanced, active) in [
        (true, true, "enhanced"),
        (true, false, "premiere-video-1"),
        (false, true, "premiere-video-1"),
    ] {
        for fault in [Fault::Missing, Fault::Damaged] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path();
            let archive = write_embedded_archive(
                root,
                &selected_embedded_document(eye, enhanced),
                &fixture("audio-mono.wav"),
                Some((active, fault)),
            );
            let native = root.join("native");
            let error = tesseract_to_premiere(&archive, &native, false)
                .unwrap_err()
                .to_string();
            assert!(error.contains(active), "{error}");
            assert!(!native.exists());
        }
    }
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn embedded_selection_invalid_sound_has_no_fallback_and_checks_each_use() {
    let temp = tempfile::tempdir().unwrap();
    let surround = temp.path().join("surround.wav");
    write_surround_wav(&surround);
    for (name, source, expected) in [
        (
            "surround",
            surround,
            "only mono/stereo source audio is supported",
        ),
        (
            "longer",
            fixture("feature_audio_tone_right.wav"),
            "sourceIntrinsicDuration 200 ms differs from selected sound",
        ),
        ("silent", fixture("video-30fps.mp4"), "has no sound"),
    ] {
        let root = temp.path().join(name);
        fs::create_dir(&root).unwrap();
        let archive = write_embedded_archive(
            &root,
            &selected_embedded_document(true, true),
            &source,
            None,
        );
        let native = root.join("native");
        let omissions = tesseract_to_premiere(&archive, &native, false).unwrap();
        assert_eq!(packaged_media(&native), ["eye.mp4"]);
        assert_eq!(
            read_xml(&native.join("project.prproj"))
                .matches("<AudioClipTrackItem ")
                .count(),
            0
        );
        assert_eq!(
            omissions
                .iter()
                .filter(|item| item.reason.contains(expected))
                .count(),
            2,
            "{omissions:?}"
        );
    }
    let root = temp.path().join("per-use");
    fs::create_dir(&root).unwrap();
    let mut document = selected_embedded_document(true, true);
    document["composition"]["layers"][1]["sourceIntrinsicDuration"] = json!(1000);
    document["composition"]["layers"][1]["source"]["eyeContact"]["enabled"] = json!(false);
    let archive = root.join("source.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&document).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            fixture("video-30fps.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset("eye", fixture("video-with-audio.mp4"), AssetKind::Video)
        .unwrap()
        .add_asset("enhanced", fixture("audio-mono.wav"), AssetKind::Audio)
        .unwrap()
        .write(&archive)
        .unwrap();
    let native = root.join("native");
    let omissions = tesseract_to_premiere(&archive, &native, false).unwrap();
    assert_eq!(
        read_xml(&native.join("project.prproj"))
            .matches("<AudioClipTrackItem ")
            .count(),
        1
    );
    assert!(
        omissions.iter().any(|item| item.record.contains("layer 3")
            && item
                .reason
                .contains("sourceIntrinsicDuration 1000 ms differs from selected sound")),
        "{omissions:?}"
    );
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn unreadable_embedded_edit_omits_sound_without_blocking_picture_import() {
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("source.tsrct");
    TesseractFileBuilder::from_project_json(&serde_json::to_vec(&audible_document()).unwrap())
        .unwrap()
        .add_asset(
            "premiere-video-1",
            fixture("video-with-audio.mp4"),
            AssetKind::Video,
        )
        .unwrap()
        .add_asset("music", fixture("audio-mono.wav"), AssetKind::Audio)
        .unwrap()
        .write(&archive)
        .unwrap();
    let native = root.path().join("native");
    tesseract_to_premiere(&archive, &native, false).unwrap();
    let path = native.join("media/video-with-audio.mp4");
    let mut bytes = fs::read(&path).unwrap();
    let edits: Vec<_> = bytes
        .windows(4)
        .enumerate()
        .filter_map(|(i, kind)| (kind == b"elst").then_some(i))
        .collect();
    assert_eq!(edits.len(), 2);
    let audio_edit = edits[1];
    bytes[audio_edit + 20..audio_edit + 22].copy_from_slice(&2_u16.to_be_bytes());
    fs::write(&path, &bytes).unwrap();
    let output = root.path().join("import");
    let omissions =
        premiere_to_tesseract(native.join("project.prproj"), &output, None, false).unwrap();
    assert!(
        omissions
            .iter()
            .any(|o| o.reason.contains("embedded sound not imported")
                && o.reason.contains("retiming")),
        "{omissions:?}"
    );
    let file = TesseractFile::open(first_project(&output)).unwrap();
    let document = file.project_json().unwrap();
    let layers = document["composition"]["layers"].as_array().unwrap();
    assert_eq!(
        layers
            .iter()
            .filter(|layer| layer["type"] == "Video")
            .count(),
        1
    );
    assert_eq!(
        layers
            .iter()
            .filter(|layer| layer["type"] == "Audio")
            .count(),
        1
    );
}

/// Native filter excerpts attached to the existing independently authored sound case.
/// Unused donor media, opaque state and UI metadata are not part of the fixture.
fn audio_filters_xml(filters: &[&str]) -> String {
    let mut xml = read_xml(&fixture("feature_audio_clips_strict.prproj"));
    let excerpts = include_str!("../fixtures/feature_audio_filters_native.xml");
    let document = roxmltree::Document::parse(excerpts).unwrap();
    let records: String = document
        .root_element()
        .children()
        .filter(|node| node.is_element())
        .map(|node| &excerpts[node.range()])
        .collect();
    let end = xml.rfind("</PremiereData>").unwrap();
    xml.insert_str(end, &records);
    let components: String = filters
        .iter()
        .enumerate()
        .map(|(index, id)| format!(r#"<Component Index="{index}" ObjectRef="{id}"/>"#))
        .collect();
    edit_record(
        &mut xml,
        r#"<AudioComponentChain ObjectID="92""#,
        "</AudioComponentChain>",
        |record| {
            record.replace(
                "</ComponentChain>",
                &format!("<Components Version=\"1\">{components}</Components></ComponentChain>"),
            )
        },
    );
    xml
}

fn import_audio_filters(root: &Path, xml: &str) -> (TesseractFile, Vec<premiere_file::Omission>) {
    fs::create_dir_all(root).unwrap();
    let source = root.join("filters.prproj");
    write_prproj(&source, xml);
    for media in [
        "feature_audio_click_left.wav",
        "feature_audio_tone_right.wav",
    ] {
        fs::copy(fixture(media), root.join(media)).unwrap();
    }
    let output = root.join("tesseract");
    let omissions = premiere_to_tesseract(
        &source,
        &output,
        Some("093e7f82-8e3b-4fd4-9a66-1234f931ffd8"),
        false,
    )
    .unwrap();
    (
        TesseractFile::open(first_project(&output)).unwrap(),
        omissions,
    )
}

#[test]
fn audio_filters_fill_right_prepares_source_channel_and_retains_edits() {
    let root = tempfile::tempdir().unwrap();
    let (mut file, omissions) = import_audio_filters(root.path(), &audio_filters_xml(&["1111"]));
    assert!(
        omissions
            .iter()
            .all(|item| item.record == "VideoTrackGroup:80"),
        "{omissions:?}"
    );
    let mut document = file.project_json().unwrap();
    let mut layers: Vec<_> = document["composition"]["layers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .filter(|layer| layer["type"] == "Audio")
        .collect();
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0]["volume"], json!(1.0));
    assert_eq!(layers[1]["volume"], json!(0.5));
    assert_eq!(
        crate::test_support::layer_range(layers[0]),
        &json!({"start": 0, "duration": 5000})
    );
    let asset = &file.metadata().assets[layers[0]["source"]["assetId"].as_str().unwrap()];
    assert_eq!(asset.kind, AssetKind::Audio);
    let mut prepared = Vec::new();
    file.asset(layers[0]["source"]["assetId"].as_str().unwrap())
        .unwrap()
        .open()
        .unwrap()
        .read_to_end(&mut prepared)
        .unwrap();
    assert_eq!(u16::from_le_bytes(prepared[22..24].try_into().unwrap()), 1);
    // PCM16 full-source extraction: channel 0 copied exactly, without timing or gain.
    let original = fs::read(fixture("feature_audio_click_left.wav")).unwrap();
    let data = |bytes: &[u8]| {
        let mut offset = 12;
        while offset + 8 <= bytes.len() {
            let size =
                u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
            if &bytes[offset..offset + 4] == b"data" {
                return bytes[offset + 8..offset + 8 + size].to_vec();
            }
            offset += 8 + size + size % 2;
        }
        panic!("WAV data missing")
    };
    let expected: Vec<_> = data(&original)
        .chunks_exact(4)
        .flat_map(|frame| frame[..2].iter().copied())
        .collect();
    assert_eq!(data(&prepared), expected);
    // A mono source is duplicated by the current mixer at unity, matching the
    // independently rendered Fill Right control, not its ordinary mono pan law.
    layers[0]["volume"] = json!(0.25);
    let edited = root.path().join("edited.json");
    fs::write(&edited, serde_json::to_vec(&document).unwrap()).unwrap();
    file.commit_project_json(&edited).unwrap();
    file.save().unwrap();
    let again = relocated_round_trip(root.path(), &first_project(&root.path().join("tesseract")));
    assert_db(volume_levels(&again)[0].0, 20.0 * 0.25_f64.log10());
    assert_db(volume_levels(&again)[1].0, 20.0 * 0.5_f64.log10());
    let native = read_xml(&root.path().join("relocated/project.prproj"));
    assert!(!native.contains("Internal Audio Fill Right"));
    assert!(native.contains("Internal Volume Mono"));
}

#[test]
fn audio_filters_native_bypass_and_unsupported_identity_preserve_siblings() {
    let root = tempfile::tempdir().unwrap();
    for (label, bypass, reported) in [
        ("active", "false", true),
        ("bypassed", "true", false),
        ("malformed", "invalid", true),
    ] {
        // Bass's actual saved component bypass is true; mutate only that token.
        let mut xml = audio_filters_xml(&["1794", "1794", "1772", "1793", "1849", "1996"]);
        edit_record(
            &mut xml,
            r#"<AudioFilterComponent ObjectID="1794""#,
            "</AudioFilterComponent>",
            |record| {
                record.replace(
                    "<Bypass>true</Bypass>",
                    &format!("<Bypass>{bypass}</Bypass>"),
                )
            },
        );
        let (file, omissions) = import_audio_filters(&root.path().join(label), &xml);
        assert_eq!(
            volume_levels(&file),
            [(0.0, vec![]), (20.0 * 0.5_f64.log10(), vec![])]
        );
        let bass: Vec<_> = omissions
            .iter()
            .filter(|item| item.record == "AudioFilterComponent:1794")
            .collect();
        assert_eq!(bass.len(), usize::from(reported), "{label}: {omissions:?}");
        if reported {
            assert!(bass[0].reason.contains("Internal Bass"));
        }
        for (id, name) in [
            ("1772", "c6762838-e451-4d22-9c90-b0eaa734605a"),
            ("1793", "3792d46f-5036-4be6-ab5a-177622246a5d"),
            ("1849", "8edcd326-33de-4a4d-985b-f1bf48ed1427"),
            ("1996", "164b4e8a-7105-406b-b23b-1ef2cc4d8957"),
        ] {
            let reports: Vec<_> = omissions
                .iter()
                .filter(|item| item.record == format!("AudioFilterComponent:{id}"))
                .collect();
            assert_eq!(reports.len(), 1, "{omissions:?}");
            assert!(reports[0].reason.contains(name), "{reports:?}");
        }
    }
}

fn assert_first_audio_source_unchanged(file: &TesseractFile, source: &Path) {
    let document = file.project_json().unwrap();
    let layer = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Audio")
        .unwrap();
    assert_eq!(crate::test_support::layer_range(layer)["start"], 0);
    let mut bytes = Vec::new();
    file.asset(layer["source"]["assetId"].as_str().unwrap())
        .unwrap()
        .open()
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, fs::read(source).unwrap());
}

#[test]
fn audio_filters_unsupported_route_keeps_the_base_stereo_source() {
    let root = tempfile::tempdir().unwrap();
    let mut xml = audio_filters_xml(&["1111"]);
    edit_record(
        &mut xml,
        r#"<AudioComponentParam ObjectID="110""#,
        "</AudioComponentParam>",
        |record| record.replace("0.5", "0.25"),
    );
    let (file, omissions) = import_audio_filters(root.path(), &xml);
    assert_eq!(
        volume_levels(&file),
        [(0.0, vec![]), (20.0 * 0.5_f64.log10(), vec![])]
    );
    let reports: Vec<_> = omissions
        .iter()
        .filter(|item| item.record == "AudioFilterComponent:1111")
        .collect();
    assert_eq!(reports.len(), 1, "{omissions:?}");
    assert!(reports[0].reason.contains("stereo source plays unchanged"));
    assert_first_audio_source_unchanged(&file, &fixture("feature_audio_click_left.wav"));
}

#[cfg(feature = "ffmpeg-library")]
fn compressed_audio_filters_xml() -> String {
    let mut xml = audio_filters_xml(&["1111"]);
    xml = xml.replace("feature_audio_click_left.wav", "audio-stereo.m4a");
    let duration = super::test_support::TICKS / 5;
    edit_record(
        &mut xml,
        r#"<AudioStream ObjectID="84""#,
        "</AudioStream>",
        |record| {
            record.replace(
                "<Duration>1270080000000</Duration>",
                &format!("<Duration>{duration}</Duration>"),
            )
        },
    );
    edit_record(
        &mut xml,
        r#"<AudioClipTrackItem ObjectID="87""#,
        "</AudioClipTrackItem>",
        |record| {
            record.replace(
                "<End>1270080000000</End>",
                &format!("<End>{duration}</End>"),
            )
        },
    );
    edit_record(
        &mut xml,
        r#"<AudioClip ObjectID="107""#,
        "</AudioClip>",
        |record| {
            record.replace(
                "<OutPoint>1270080000000</OutPoint>",
                &format!("<OutPoint>{duration}</OutPoint>"),
            )
        },
    );
    xml
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn audio_filters_compressed_fill_right_uses_supported_aac_preparation() {
    let root = tempfile::tempdir().unwrap();
    fs::copy(
        fixture("audio-stereo.m4a"),
        root.path().join("audio-stereo.m4a"),
    )
    .unwrap();
    let (file, omissions) = import_audio_filters(root.path(), &compressed_audio_filters_xml());
    assert_eq!(
        volume_levels(&file),
        [(0.0, vec![]), (20.0 * 0.5_f64.log10(), vec![])]
    );
    assert!(
        omissions
            .iter()
            .all(|item| item.record == "VideoTrackGroup:80"),
        "{omissions:?}"
    );
    let document = file.project_json().unwrap();
    let layer = document["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["type"] == "Audio")
        .unwrap();
    assert_eq!(
        crate::test_support::layer_range(layer),
        &json!({"start": 0, "duration": 200})
    );
    assert_eq!(layer["sourceRange"], json!({"start": 0, "duration": 200}));
    assert_eq!(layer["volume"], 1.0);
    let mut mono = Vec::new();
    file.asset(layer["source"]["assetId"].as_str().unwrap())
        .unwrap()
        .open()
        .unwrap()
        .read_to_end(&mut mono)
        .unwrap();
    assert_eq!(u16::from_le_bytes(mono[20..22].try_into().unwrap()), 3);
    assert_eq!(u16::from_le_bytes(mono[22..24].try_into().unwrap()), 1);
    assert_eq!(mono.len(), 44 + 9600 * 4);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn audio_filters_unsafe_compressed_clock_keeps_stereo() {
    let root = tempfile::tempdir().unwrap();
    let mut bytes = fs::read(fixture("audio-stereo.m4a")).unwrap();
    let edit = bytes.windows(4).position(|tag| tag == b"elst").unwrap();
    // The 200 ms presentation plus this origin exceeds the AAC source by one
    // sample. Ordinary stereo admission allows the tail; extraction cannot
    // invent padding or shorten the selected full source.
    bytes[edit + 16..edit + 20].copy_from_slice(&1025_i32.to_be_bytes());
    let source = root.path().join("audio-stereo.m4a");
    fs::write(&source, bytes).unwrap();
    let (file, omissions) = import_audio_filters(root.path(), &compressed_audio_filters_xml());
    assert_eq!(volume_levels(&file).len(), 2, "{omissions:?}");
    assert!(!omissions.iter().any(
        |item| item.scope == premiere_file::OmissionScope::Occurrence
            && ["87", "AudioClipTrackItem:87"].contains(&item.record.as_str())
    ));
    let reports: Vec<_> = omissions
        .iter()
        .filter(|item| item.record == "AudioFilterComponent:1111")
        .collect();
    assert_eq!(reports.len(), 1, "{omissions:?}");
    assert!(reports[0].reason.contains("stereo source plays unchanged"));
    assert!(reports[0]
        .reason
        .contains("selected audio presentation exceeds source samples"));
    assert_first_audio_source_unchanged(&file, &source);
}

#[cfg(feature = "ffmpeg-library")]
#[test]
fn audio_clock_retimed_fill_right_unsafe_clock_keeps_stereo_source() {
    for rate in [2.0_f64, -2.0] {
        let root = tempfile::tempdir().unwrap();
        let mut bytes = fs::read(fixture("audio-stereo.m4a")).unwrap();
        let edit = bytes.windows(4).position(|tag| tag == b"elst").unwrap();
        bytes[edit + 16..edit + 20].copy_from_slice(&1025_i32.to_be_bytes());
        let source = root.path().join("audio-stereo.m4a");
        fs::write(&source, &bytes).unwrap();
        let mut xml = compressed_audio_filters_xml();
        edit_record(
            &mut xml,
            r#"<AudioClip ObjectID="107""#,
            "</AudioClip>",
            |record| {
                record.replace(
                    "</Clip>",
                    &format!(
                        "<PlaybackSpeed>2</PlaybackSpeed>{}</Clip>",
                        if rate < 0.0 {
                            "<PlayBackwards>true</PlayBackwards>"
                        } else {
                            ""
                        }
                    ),
                )
            },
        );
        edit_record(
            &mut xml,
            r#"<AudioClipTrackItem ObjectID="87""#,
            "</AudioClipTrackItem>",
            |record| {
                record.replace(
                    &format!("<End>{}</End>", super::test_support::TICKS / 5),
                    &format!("<End>{}</End>", super::test_support::TICKS / 10),
                )
            },
        );
        let (file, notes) = import_audio_filters(root.path(), &xml);
        assert_eq!(volume_levels(&file).len(), 2, "{notes:?}");
        let reports = notes
            .iter()
            .filter(|note| note.record == "AudioFilterComponent:1111")
            .collect::<Vec<_>>();
        assert_eq!(reports.len(), 1, "{notes:?}");
        assert!(reports[0].reason.contains("stereo source plays unchanged"));
        assert_first_audio_source_unchanged(&file, &source);
        let doc = file.project_json().unwrap();
        let layer = doc["composition"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["type"] == "Audio")
            .unwrap();
        assert_eq!(layer["playback"]["inputRange"]["duration"], 100);
        assert_eq!(
            layer["playback"]["mapping"]["property"]["keyframes"][0]["value"],
            if rate < 0.0 { 200 } else { 0 }
        );
    }
}
