//! Test-only audio conversion fixture preparer and fresh-import inspector.
//! This does not render audio or validate Adobe acceptance. The native fixture
//! and independently rendered reference must be checked by the external runner.

use std::{env, error::Error, fs, path::Path};

use aftereffects_file::{
    aep::Project,
    properties::read_numeric,
    rifx::Chunk,
    structure::{ItemKind, Layer as NativeLayer, StructuralProject, read_project},
    structure_document::to_structural_fx_document,
};
use fx_schema::{
    EditableFxCompositionDocument, Layer, LayerData, LayerId, PropType, PropertyValue,
    animator::{
        AnimationGraphEntry, AnimatorData, KeyframeId, PropertyAnimator, PropertyKeyframe,
        PropertyKeyframeEasing, PropertyKeyframeTrack,
    },
};
use serde_json::{Value, json};
use tesseract_file::{AssetKind, TesseractFile, TesseractFileBuilder};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const CASES: &[&str] = &[
    "wave-static",
    "native-muted",
    "hidden-layer",
    "hidden-group",
    "static-zero",
    "constant-zero-zero-base",
    "constant-zero-nonzero-base",
    "all-zero-one-key",
    "all-zero-two-keys",
    "zero-base-unmute",
    "hold-gain",
    "linear-gain",
    "bezier-gain",
    "trim-offset",
    "affine-playback",
    "eof-tail",
    "mov-audio-only",
    "audio-group",
    "group-affine-playback",
    "empty-nested-group",
    "fractional-duration",
    "source-switch",
    "nonlinear-remap",
    "conflicting-gain-clock",
    "invalid-gain-hull",
    "empty-only-group",
    "stereo-levels",
    "expression",
];

fn require(ok: bool, message: impl Into<String>) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(message.into().into())
    }
}

fn audio(id: u64, asset: &str) -> Value {
    json!({
        "type":"Audio", "id":id, "name":format!("audio-{id}"), "parent":null,
        "activeRange":{"start":1000,"duration":2000},
        "sourceRange":{"start":500,"duration":1000},
        "sourceIntrinsicDuration":4000, "volume":0.5,
        "source":{"assetId":asset}
    })
}

fn animator(
    id: u64,
    property: PropType,
    values: &[(i64, PropertyValue)],
    easing: PropertyKeyframeEasing,
) -> AnimationGraphEntry {
    let keys = values
        .iter()
        .enumerate()
        .map(|(i, (time, value))| {
            PropertyKeyframe::new(
                KeyframeId::new(format!("audio-e2e-{id}-{property:?}-{i}")),
                fx_schema::TimeOffset::from_millis(*time),
                value.clone(),
                easing,
            )
        })
        .collect();
    AnimationGraphEntry {
        target: fx_schema::PropertyTarget::layer(LayerId::new(id), property),
        animator: PropertyAnimator::keyframes(
            PropertyKeyframeTrack::new(keys).expect("nonempty fixture keys"),
        ),
        dependencies: Vec::new(),
        random_seed_target: None,
        layer_refs: Default::default(),
    }
}

fn gain(values: &[(i64, f64)], easing: PropertyKeyframeEasing) -> AnimationGraphEntry {
    animator(
        700,
        PropType::AudioVolume,
        &values
            .iter()
            .map(|(t, v)| (*t, PropertyValue::Float(*v)))
            .collect::<Vec<_>>(),
        easing,
    )
}

fn group(id: u64, parent: Value, children: Vec<Value>, template: &Value) -> Value {
    let mut group = template.clone();
    group["id"] = json!(id);
    group["name"] = json!(format!("audio-group-{id}"));
    group["description"] = json!("Explicit audio E2E group");
    group["parent"] = parent;
    group["activeRange"] = json!({"start":0,"duration":6000});
    group["playback"] = Value::Null;
    group["isHidden"] = json!(false);
    group["layers"] = json!(children);
    group
}

fn base() -> Result<Value> {
    // A real native composition supplies the schema-shaped root. It is not an
    // oracle for any of the new audio controls below.
    let native = read_project(include_bytes!(
        "../tests/fixtures/properties/transform_unseparated.aep"
    ))?;
    let mut value = to_structural_fx_document(&native, Some(1))?
        .document
        .to_json_value()?;
    value["dimensions"] = json!({"width":320,"height":180});
    // The document envelope serializes seconds; layer active ranges use milliseconds.
    value["duration"] = json!(6.0);
    value["backgroundColor"] = json!([0.0, 0.0, 0.0, 1.0]);
    value["composition"]["layers"][0]["activeRange"] = json!({"start":0,"duration":6000});
    value["composition"]["layers"][0]["layers"] = json!([]);
    value["composition"]["dynamics"] = json!({"entries":[]});
    Ok(value)
}

fn build(case: &str) -> Result<(Value, Vec<&'static str>)> {
    require(CASES.contains(&case), format!("unknown case: {case}"))?;
    let mut doc = base()?;
    // The writer names its native root from the composition, not its root layer.
    doc["composition"]["name"] = json!(case);
    let root = doc["composition"]["layers"][0].clone();
    let movie = matches!(case, "mov-audio-only" | "nonlinear-remap");
    let mut source = audio(700, if movie { "movie" } else { "sound" });
    if movie {
        source["sourceIntrinsicDuration"] = json!(8000);
    }
    let mut layers = vec![source.clone()];
    let mut entries = Vec::<AnimationGraphEntry>::new();
    let mut assets = vec![if movie { "movie" } else { "sound" }];
    match case {
        "native-muted" | "static-zero" => source["volume"] = json!(0.0),
        "hidden-layer" => source["isHidden"] = json!(true),
        "hidden-group" => {
            source["parent"] = json!(701);
            let mut parent = group(701, Value::Null, vec![source.clone()], &root);
            parent["isHidden"] = json!(true);
            parent["activeRange"] = json!({"start":0,"duration":3000});
            layers = vec![parent];
        }
        "constant-zero-zero-base" | "constant-zero-nonzero-base" => {
            if case == "constant-zero-zero-base" {
                source["volume"] = json!(0.0);
            }
            entries.push(AnimationGraphEntry {
                target: fx_schema::PropertyTarget::layer(LayerId::new(700), PropType::AudioVolume),
                animator: PropertyAnimator::constant(PropertyValue::Float(0.0))?,
                dependencies: Vec::new(),
                random_seed_target: None,
                layer_refs: Default::default(),
            });
        }
        "all-zero-one-key" | "all-zero-two-keys" => {
            let times: &[i64] = if case == "all-zero-one-key" {
                &[500]
            } else {
                &[500, 1500]
            };
            entries.push(gain(
                &times.iter().map(|t| (*t, 0.0)).collect::<Vec<_>>(),
                PropertyKeyframeEasing::Hold,
            ));
        }
        "zero-base-unmute" => {
            source["volume"] = json!(0.0);
            entries.push(gain(
                &[(500, 0.0), (1500, 1.0)],
                PropertyKeyframeEasing::Hold,
            ));
        }
        // FX gain keys use an offset from the occurrence's active start (1s).
        // These land at native composition times 1.5s and 2.5s.
        "hold-gain" => entries.push(gain(
            &[(500, 0.5), (1500, 1.0)],
            PropertyKeyframeEasing::Hold,
        )),
        "linear-gain" => entries.push(gain(
            &[(500, 0.5), (1500, 1.0)],
            PropertyKeyframeEasing::Linear,
        )),
        "bezier-gain" => entries.push(gain(
            &[(500, 0.5), (1500, 1.0)],
            PropertyKeyframeEasing::CubicBezier {
                x1: 0.25,
                y1: 0.1,
                x2: 0.75,
                y2: 0.9,
            },
        )),
        "trim-offset" => source["startTime"] = json!(0.5),
        "affine-playback" => {
            // Gain at comp 1s/2s: local 0/1000ms, unlike 1.5/2.5s cases.
            entries.push(gain(&[(0, 0.5), (1000, 1.0)], PropertyKeyframeEasing::Hold));
            source["playback"] = json!({"keyframes":[
            {"id":"start","time":1000,"value":500,"easing":{"type":"linear"}},
            {"id":"end","time":3000,"value":1500,"easing":{"type":"linear"}}
        ],"before":"inactive","after":"inactive"})
        }
        "eof-tail" => source["activeRange"] = json!({"start":1000,"duration":5000}),
        "audio-group" | "group-affine-playback" | "empty-nested-group" | "fractional-duration" => {
            source["parent"] = json!(701);
            let fractional = case == "fractional-duration";
            if fractional {
                source["activeRange"] = json!({"start":1000,"duration":1001});
            }
            let mut children = vec![source.clone()];
            if matches!(case, "empty-nested-group" | "fractional-duration") {
                let mut empty = group(702, json!(701), vec![], &root);
                empty["activeRange"] =
                    json!({"start":0,"duration":if fractional {2001} else {3000}});
                children.push(empty);
            }
            let mut parent = group(701, Value::Null, children, &root);
            parent["activeRange"] = json!({"start":0,"duration":if fractional {2001} else {3000}});
            if case == "group-affine-playback" {
                parent["activeRange"] = json!({"start":1000,"duration":4000});
                parent["playback"] = json!({"keyframes":[
                    {"id":"group-start","time":1000,"value":0,"easing":{"type":"linear"}},
                    {"id":"group-end","time":5000,"value":2000,"easing":{"type":"linear"}}
                ],"before":"inactive","after":"inactive"});
            }
            layers = vec![parent];
        }
        "source-switch" => {
            source["activeRange"] = json!({"start":2000,"duration":2000});
            source["startTime"] = json!(0.5);
            entries.push(animator(
                700,
                PropType::AudioSourceAssetId,
                &[
                    (0, PropertyValue::String("sound".into())),
                    (1000, PropertyValue::String("replacement".into())),
                ],
                PropertyKeyframeEasing::Hold,
            ));
            entries.push(gain(&[(0, 0.5), (1000, 1.0)], PropertyKeyframeEasing::Hold));
            assets.push("replacement");
        }
        "nonlinear-remap" | "conflicting-gain-clock" => {
            source["activeRange"] = json!({"start":1000,"duration":1000});
            source["playback"] = json!({"keyframes":[
                {"id":"before","time":0,"value":500,"easing":{"type":"linear"}},
                {"id":"a","time":1250,"value":750,"easing":{"type":"linear"}},
                {"id":"b","time":1750,"value":1000,"easing":{"type":"linear"}},
                {"id":"after","time":3000,"value":1250,"easing":{"type":"linear"}}
            ],"before":"inactive","after":"inactive"});
            if case == "conflicting-gain-clock" {
                entries.push(gain(&[(0, 0.5), (1000, 1.0)], PropertyKeyframeEasing::Hold));
                layers.push(audio(701, "sound"));
            }
        }
        "invalid-gain-hull" => {
            entries.push(gain(
                &[(0, 0.5), (1000, 1.0)],
                PropertyKeyframeEasing::CubicBezier {
                    x1: 0.3,
                    y1: -3.0,
                    x2: 0.7,
                    y2: 1.0,
                },
            ));
            layers.push(audio(701, "sound"));
        }
        "empty-only-group" => {
            source["name"] = json!("retained-sibling");
            let mut empty = group(701, Value::Null, vec![], &root);
            empty["activeRange"] = json!({"start":0,"duration":2000});
            layers = vec![source.clone(), empty];
        }
        "stereo-levels" | "expression" => {
            // FX has mono gain and no AE expression runtime. The editable
            // replacement plus an unaffected sibling are explicit here.
            layers.push(audio(701, "sound"));
        }
        _ => {}
    }
    if layers.first().is_some_and(|layer| layer["id"] == 700) {
        layers[0] = source;
    }
    // Keep root Audio parents null and nested parents explicit; the imported
    // template is used only for the schema envelope and Group defaults.
    doc["composition"]["layers"] = json!(layers);
    doc["composition"]["dynamics"] = json!({"entries": entries});
    Ok((doc, assets))
}

fn author_inputs(output: &Path) -> Result<()> {
    fs::create_dir(output)?;
    for case in CASES {
        let (value, _) = build(case)?;
        EditableFxCompositionDocument::from_json_value(value.clone())?;
        fs::write(
            output.join(format!("{case}.json")),
            serde_json::to_vec_pretty(&value)?,
        )?;
    }
    Ok(())
}

fn prepare(fixture_dir: &Path, case: &str, output: &Path) -> Result<()> {
    require(
        !output.exists(),
        format!("refusing existing output: {}", output.display()),
    )?;
    require(CASES.contains(&case), format!("unknown case: {case}"))?;
    // The committed edited FX input, not the fixture-authoring implementation,
    // is the source of truth for every fresh export execution.
    let value: Value = serde_json::from_slice(&fs::read(
        fixture_dir.join("fx").join(format!("{case}.json")),
    )?)?;
    let document = EditableFxCompositionDocument::from_json_value(value.clone())?;
    require(
        document.duration().as_millis() == 6000,
        format!("{case}: expected 6s document duration"),
    )?;
    require(
        document.dimensions() == fx_schema::Dimensions::new(320, 180),
        format!("{case}: expected 320x180 canvas"),
    )?;
    require(
        document.composition().name() == case,
        format!("{case}: native export root composition name mismatch"),
    )?;
    let mut builder = TesseractFileBuilder::try_new(document)?;
    for asset in ["sound", "replacement", "movie"] {
        let (filename, kind) = match asset {
            "sound" => ("sound.wav", AssetKind::Audio),
            "replacement" => ("other.wav", AssetKind::Audio),
            "movie" => ("movie.mov", AssetKind::Video),
            _ => return Err(format!("unregistered fixture asset: {asset}").into()),
        };
        let path = fixture_dir.join(filename);
        require(
            path.is_file(),
            format!("missing primary media: {}", path.display()),
        )?;
        builder = builder.add_asset(asset, &path, kind)?;
    }
    builder.validate()?;
    fs::create_dir(output)?;
    fs::write(
        output.join("document.json"),
        serde_json::to_vec_pretty(&value)?,
    )?;
    fs::write(
        output.join("case-contract.json"),
        serde_json::to_vec_pretty(&json!({
            "case_id":case, "source_composition":{"duration_millis":6000,"fps":24,"width":320,"height":180},
            "import_assertion":"inspect-import", "export_input":"document.json",
            "native_reference":"independently Adobe-rendered 30fps WAV/MP4, not produced here",
            "proof_status":"unrun/unmeasured",
            "required_diagnostic": required_diagnostic(case),
            "critical_millis":critical_millis(case),
            "authored_fx_controls":{
                "layers":value["composition"]["layers"],
                "dynamics":value["composition"]["dynamics"],
                "clock_note":"audio 1..3s, source begins 0.5s, 1x unless explicit playback; sourceRange duration is not speed",
                "native_gain_key_secs":match case {
                    "zero-base-unmute" | "hold-gain" | "linear-gain" | "bezier-gain" | "all-zero-two-keys" => json!([1.5,2.5]),
                    "all-zero-one-key" => json!([1.5]),
                    "affine-playback" => json!([1.0,2.0]),
                    "source-switch" => json!([2.0,3.0]),
                    _ => json!([]),
                },
                "native_group_active_millis":match case {
                    "hidden-group" | "audio-group" | "empty-nested-group" => json!([0,3000]),
                    "fractional-duration" => json!([0,2001]),
                    "empty-only-group" => json!([0,2000]),
                    _ => Value::Null,
                },
                "fractional_storage_ceil_millis":if case == "fractional-duration" {json!(2041.6666666666667)} else {Value::Null}
            }
        }))?,
    )?;
    drop(builder.write(output.join("project.tsrct"))?);
    Ok(())
}

fn critical_millis(case: &str) -> Vec<i64> {
    let mut points = vec![
        0, 958, 1000, 1042, 1458, 1500, 1542, 1958, 2000, 2042, 2458, 2500, 2542, 2958, 3000, 3042,
        3958, 4000, 4042, 4458, 4500, 4542, 5958,
    ];
    if matches!(
        case,
        "source-switch" | "nonlinear-remap" | "conflicting-gain-clock"
    ) {
        points.extend([1208, 1250, 1292, 1708, 1750, 1792]);
    }
    if case == "fractional-duration" {
        points.extend([1959, 2001, 2043]);
    }
    points.sort_unstable();
    points.dedup();
    points
}

fn required_diagnostic(case: &str) -> Option<&'static str> {
    match case {
        "eof-tail" => Some("retains the audible prefix"),
        "linear-gain" | "bezier-gain" => Some("continuous gain keys are approximated"),
        "conflicting-gain-clock" => Some("Time Remap cannot drive occurrence-owned Audio Levels"),
        "invalid-gain-hull" => Some("gain control hull is negative"),
        "stereo-levels" => Some("quieter channel"),
        "expression" => Some("expression requires the AE environment"),
        _ => None,
    }
}

fn all_audio<'a>(layers: &'a [Layer], output: &mut Vec<&'a fx_schema::AudioLayer>) {
    for layer in layers {
        match layer.data() {
            LayerData::Audio(audio) => output.push(audio),
            LayerData::Group(group) => all_audio(&group.layers, output),
            _ => {}
        }
    }
}

fn primary_audio(layers: &[Layer]) -> Option<&fx_schema::AudioLayer> {
    for layer in layers {
        if let LayerData::Group(group) = layer.data() {
            if matches!(
                group.name.as_str(),
                "primary-sound" | "child-primary-sound" | "switch-sound-first" | "retained-sibling"
            ) {
                let mut audio = Vec::new();
                all_audio(&group.layers, &mut audio);
                if let Some(first) = audio.first() {
                    return Some(*first);
                }
            }
            if let Some(audio) = primary_audio(&group.layers) {
                return Some(audio);
            }
        }
    }
    None
}

fn has_visible_video(layers: &[Layer], hidden: bool) -> bool {
    layers.iter().any(|layer| match layer.data() {
        LayerData::Group(group) => has_visible_video(&group.layers, hidden || group.is_hidden),
        LayerData::Video(video) => !hidden && !video.is_hidden,
        _ => false,
    })
}

fn has_group(layers: &[Layer]) -> bool {
    layers.iter().any(|layer| matches!(layer.data(), LayerData::Group(group) if group.layers.iter().any(|child| matches!(child.data(), LayerData::Audio(_) | LayerData::Group(_)))))
}

fn has_group_span(layers: &[Layer], start: u64, duration: u64) -> bool {
    layers.iter().any(|layer| match layer.data() {
        LayerData::Group(group) => {
            (group.playback.input_range().start.as_millis() == start
                && group.playback.input_range().duration.as_millis() == duration
                && !group.layers.is_empty())
                || has_group_span(&group.layers, start, duration)
        }
        _ => false,
    })
}

fn has_playback_keys(layers: &[Layer], count: usize) -> bool {
    layers.iter().any(|layer| match layer.data() {
        LayerData::Group(group) => {
            group
                .playback
                .time_remap()
                .is_some_and(|keys| keys.keyframes().len() == count)
                || has_playback_keys(&group.layers, count)
        }
        _ => false,
    })
}

// The fixture source is immutable. Relink only recognized alias fullpaths in a
// scratch AEP; unknown source aliases fail closed instead of dropping media.
fn relink_aliases(chunks: &mut [Chunk], fixture_dir: &Path, count: &mut usize) -> Result<()> {
    for chunk in chunks.iter_mut() {
        if let Some(children) = chunk.children_mut() {
            relink_aliases(children, fixture_dir, count)?;
        } else if chunk.id() == *b"alas" {
            let Some(bytes) = chunk.data_payload() else {
                continue;
            };
            let mut value: Value = serde_json::from_slice(bytes)?;
            let original = value
                .get("fullpath")
                .and_then(Value::as_str)
                .ok_or("alias has no fullpath")?;
            let name = original
                .rsplit(['/', '\\'])
                .next()
                .ok_or("empty alias basename")?;
            require(
                ["sound.wav", "other.wav", "movie.mov"].contains(&name),
                format!("unexpected alias basename: {name}"),
            )?;
            let new_path = fs::canonicalize(fixture_dir.join(name))?;
            require(
                new_path.is_file(),
                format!("missing media: {}", new_path.display()),
            )?;
            value["fullpath"] = json!(new_path.to_str().ok_or("non-UTF8 media path")?);
            *chunk = Chunk::data(*b"alas", serde_json::to_vec(&value)?)?;
            *count += 1;
        }
    }
    Ok(())
}

fn stage_source(fixture_dir: &Path, output: &Path) -> Result<()> {
    require(
        !output.exists(),
        format!("refusing existing staged AEP: {}", output.display()),
    )?;
    let source = fixture_dir.join("audio_cases.aep");
    let mut project = Project::parse(&fs::read(&source)?)?;
    let mut count = 0;
    relink_aliases(&mut project.chunks, fixture_dir, &mut count)?;
    require(
        count >= 3,
        format!("expected at least three media aliases, found {count}"),
    )?;
    let staged = project.encode()?;
    // Require a valid structural project before publishing a disposable copy.
    read_project(&staged)?;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output)?;
    use std::io::Write;
    file.write_all(&staged)?;
    Ok(())
}

fn inspect_import(case: &str, path: &Path, output: &Path) -> Result<()> {
    require(CASES.contains(&case), format!("unknown case: {case}"))?;
    require(
        !output.exists(),
        format!("refusing existing report: {}", output.display()),
    )?;
    let archive = TesseractFile::open(path)?;
    let document = archive.project();
    let mut audio_layers = Vec::new();
    all_audio(document.composition().layers(), &mut audio_layers);
    require(
        !audio_layers.is_empty(),
        format!("{case}: no editable Audio (unsupported cases require a surviving sibling)"),
    )?;
    let primary = primary_audio(document.composition().layers())
        .ok_or_else(|| format!("{case}: expected named primary occurrence missing"))?;
    // Archive packaging remaps native source IDs to UUIDs; verify actual bytes
    // rather than assuming an importer-internal ID survives that boundary.
    let expected_media = if matches!(case, "mov-audio-only" | "nonlinear-remap") {
        "movie.mov"
    } else {
        "sound.wav"
    };
    let expected_bytes = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/audio_e2e")
            .join(expected_media),
    )?;
    require(
        archive
            .asset(primary.source.asset_id.as_str())?
            .read_verified_bytes(2_000_000)?
            == expected_bytes,
        format!("{case}: primary source bytes differ from the authored input"),
    )?;
    require(
        primary.source_intrinsic_duration.as_millis()
            == if matches!(case, "mov-audio-only" | "nonlinear-remap") {
                8000
            } else {
                4000
            },
        format!("{case}: incorrect native source duration"),
    )?;
    require(
        archive.asset(primary.source.asset_id.as_str()).is_ok(),
        format!("{case}: no packaged real source bytes"),
    )?;
    let entries = document.composition().dynamics().entries();
    let gain = entries.iter().find(|entry| {
        entry.target == fx_schema::PropertyTarget::layer(primary.id, PropType::AudioVolume)
    });
    match case {
        "native-muted"
        | "hidden-layer"
        | "static-zero"
        | "constant-zero-zero-base"
        | "constant-zero-nonzero-base"
        | "all-zero-one-key"
        | "all-zero-two-keys" => {
            require(
                primary.volume.as_f64() == 0.0,
                format!("{case}: native audio-off must import as exact mute"),
            )?;
        }
        "zero-base-unmute" | "hold-gain" | "linear-gain" | "bezier-gain" => {
            let Some(entry) = gain else {
                return Err(format!("{case}: AudioVolume animator missing").into());
            };
            let AnimatorData::Keyframes { track, .. } = entry.animator.data() else {
                return Err(format!("{case}: expected editable gain keys").into());
            };
            require(
                track.keyframes().len() == 2,
                format!("{case}: expected two gain keys"),
            )?;
            let values: Vec<_> = track
                .keyframes()
                .iter()
                .map(|key| match key.value() {
                    PropertyValue::Float(v) => Some(*v),
                    _ => None,
                })
                .collect();
            require(
                values
                    .iter()
                    .zip([if case == "zero-base-unmute" { 0.0 } else { 0.5 }, 1.0])
                    .all(|(actual, expected)| {
                        actual.is_some_and(|actual| (actual - expected).abs() < 0.002)
                    }),
                format!("{case}: unexpected gain values {values:?}"),
            )?;
            require(
                track
                    .keyframes()
                    .iter()
                    .map(|key| key.layer_time().as_millis())
                    .collect::<Vec<_>>()
                    == [1000, 2000],
                // Audio is source-local below the imported occurrence Group:
                // native comp keys 1.5/2.5 minus its 0.5s start offset.
                format!("{case}: source-local native gain times changed"),
            )?;
            if matches!(case, "hold-gain" | "zero-base-unmute") {
                require(
                    track
                        .keyframes()
                        .iter()
                        .all(|key| key.easing() == PropertyKeyframeEasing::Hold),
                    format!("{case}: native Hold interpolation lost"),
                )?;
            }
        }
        "trim-offset" | "affine-playback" | "eof-tail" | "fractional-duration" => {
            require(
                has_group_span(
                    document.composition().layers(),
                    1000,
                    if case == "fractional-duration" {
                        1001
                    } else if case == "eof-tail" {
                        5000
                    } else {
                        2000
                    },
                ) || (case == "fractional-duration"
                    && has_group_span(document.composition().layers(), 0, 2001)),
                format!("{case}: native occurrence trim missing"),
            )?;
            require(
                primary.source_range.start.as_millis() == 0
                    && primary.source_range.duration.as_millis() == 4000,
                format!("{case}: source duration missing"),
            )?;
            if case == "affine-playback" {
                require(
                    has_playback_keys(document.composition().layers(), 2),
                    "affine-playback: native stretch/source clock missing",
                )?;
            }
        }
        "hidden-group" | "audio-group" | "empty-nested-group" => {
            require(
                has_group(document.composition().layers()),
                format!("{case}: audio hierarchy flattened"),
            )?;
            require(
                has_group_span(document.composition().layers(), 0, 3000),
                format!("{case}: partial parent occurrence missing"),
            )?;
        }
        "group-affine-playback" => {
            require(
                has_group_span(document.composition().layers(), 1000, 4000),
                "group-affine-playback: partial group occurrence missing",
            )?;
            require(
                has_playback_keys(document.composition().layers(), 2),
                "group-affine-playback: affine Group source clock missing",
            )?;
        }
        "source-switch" => {
            require(
                audio_layers.len() == 2,
                "source-switch: expected two independently editable occurrences",
            )?;
            require(
                audio_layers[0].source.asset_id != audio_layers[1].source.asset_id,
                "source-switch: both occurrences use the same source",
            )?;
        }
        "stereo-levels" => {
            require(gain.is_some(), "stereo-levels: keyed levels not editable")?;
            require(
                primary.volume.as_f64() < 0.3,
                "stereo-levels: quieter channel approximation absent",
            )?;
        }
        "expression" => require(
            gain.is_none(),
            "expression: unsupported expression must not become fabricated keys",
        )?,
        "mov-audio-only" => require(
            !has_visible_video(document.composition().layers(), false),
            "mov-audio-only: video channel unexpectedly imported",
        )?,
        "nonlinear-remap" => {
            require(
                has_playback_keys(document.composition().layers(), 4),
                "nonlinear-remap: four editable native Time Remap keys missing",
            )?;
        }
        _ => {}
    }
    let report = json!({
        "case_id":case, "assertion":"passed", "scope":"editable imported structure only; no Adobe or audio fidelity proof",
        "audio":[primary.source.asset_id.as_str(), &format!("{:?}", primary.playback.input_range()), &format!("{:?}", primary.source_range)],
        "audio_count":audio_layers.len(), "gain_animator":gain.map(|entry| format!("{:?}", entry.animator.data())),
        "required_diagnostic":required_diagnostic(case)
    });
    fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}

// Native expectations are pinned from the independently authored control manifest.
// This reads the freshly exported binary, not our FX round trip or writer state.
fn native_property(layer: &NativeLayer, name: &str) -> Result<Value> {
    fn visit(chunks: &[Chunk], name: &str) -> Result<Option<Value>> {
        for (index, chunk) in chunks.iter().enumerate() {
            if chunk.id() == *b"tdmn" {
                let bytes = chunk.data_payload().ok_or("invalid property name")?;
                let label = std::str::from_utf8(bytes)?.trim_end_matches('\0');
                if label == name {
                    let end = chunks[index + 1..]
                        .iter()
                        .position(|next| next.id() == *b"tdmn")
                        .map_or(chunks.len(), |distance| index + 1 + distance);
                    let run = &chunks[index + 1..end];
                    let numeric = run.iter().find(|item| item.list_kind() == Some(*b"tdbs"));
                    let Some(numeric) = numeric else {
                        return Err(format!("{name}: missing numeric property").into());
                    };
                    return Ok(Some(serde_json::to_value(
                        read_numeric(numeric.children().ok_or("opaque numeric property")?)?
                            .keyframes
                            .len(),
                    )?));
                }
            }
            if let Some(children) = chunk.children()
                && let Some(value) = visit(children, name)?
            {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }
    visit(&layer.content, name)?.ok_or_else(|| format!("missing native property {name}").into())
}

fn audio_levels(layer: &NativeLayer) -> Result<aftereffects_file::properties::NumericProperty> {
    fn visit(chunks: &[Chunk]) -> Result<Option<aftereffects_file::properties::NumericProperty>> {
        for (index, chunk) in chunks.iter().enumerate() {
            if chunk.id() == *b"tdmn"
                && chunk
                    .data_payload()
                    .is_some_and(|bytes| bytes.starts_with(b"ADBE Audio Levels\0"))
            {
                let end = chunks[index + 1..]
                    .iter()
                    .position(|next| next.id() == *b"tdmn")
                    .map_or(chunks.len(), |distance| index + 1 + distance);
                let numeric = chunks[index + 1..end]
                    .iter()
                    .find(|item| item.list_kind() == Some(*b"tdbs"))
                    .ok_or("Audio Levels numeric property missing")?;
                return Ok(Some(read_numeric(
                    numeric.children().ok_or("opaque Audio Levels")?,
                )?));
            }
            if let Some(children) = chunk.children()
                && let Some(value) = visit(children)?
            {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }
    visit(&layer.content)?.ok_or_else(|| "Audio Levels property missing".into())
}

fn native_field(project: &StructuralProject, layer: &NativeLayer, field: &str) -> Result<Value> {
    let source = project.item(layer.record.source_id());
    let result = match field {
        "hasAudio" => json!(
            source
                .and_then(|item| item.media.as_ref())
                .and_then(|media| media.as_ref().ok())
                .is_some_and(|media| media.audio_sample_rate > 0.0)
        ),
        "enabled" => json!(layer.record.flags().enabled),
        "audioEnabled" => json!(layer.record.flags().audio_enabled),
        "inPoint" | "outPoint" => {
            let local = if field == "inPoint" {
                layer.record.in_point()
            } else {
                layer.record.out_point()
            };
            // Binary occurrence bounds are layer-local; AE scripting exposes
            // them in the containing composition's clock.
            json!(
                local.ok_or("invalid native occurrence bound")?
                    * layer.record.stretch().ok_or("invalid stretch")?
                    + layer.record.start_time().ok_or("invalid start time")?
            )
        }
        "startTime" => json!(
            layer
                .record
                .start_time()
                .ok_or("invalid native start time")?
        ),
        "stretch" => json!(layer.record.stretch().ok_or("invalid native stretch")? * 100.0),
        "sourceKind" => json!(if matches!(
            source.map(|item| &item.kind),
            Some(ItemKind::Composition(_))
        ) {
            "composition"
        } else {
            return Err("group source is not a composition".into());
        }),
        "sourceWidth" | "sourceHeight" | "sourceDuration" => {
            let Some(ItemKind::Composition(comp)) = source.map(|item| &item.kind) else {
                return Err(format!("{field}: source composition absent").into());
            };
            match field {
                "sourceWidth" => json!(comp.width),
                "sourceHeight" => json!(comp.height),
                _ => json!(comp.duration_secs),
            }
        }
        "timeRemapEnabled" => {
            // A remap must have an actual editable four-key numeric leaf.
            json!(native_property(layer, "ADBE Time Remapping").is_ok())
        }
        "timeRemap.keys.length" => native_property(layer, "ADBE Time Remapping")?,
        "audioLevels.value" => {
            let levels = audio_levels(layer)?;
            if !levels.values.is_empty() {
                json!(levels.values)
            } else {
                let first = levels.keyframes.first().ok_or("missing audio value")?;
                require(
                    levels
                        .keyframes
                        .iter()
                        .all(|key| key.values == first.values),
                    "static value assertion cannot evaluate varying audio keys",
                )?;
                json!(first.values)
            }
        }
        "audioLevels.keys.length" => json!(audio_levels(layer)?.keyframes.len()),
        _ if field.starts_with("audioLevels.keys.") => {
            let pieces: Vec<_> = field.split('.').collect();
            require(
                pieces.len() == 4,
                format!("unsupported native field {field}"),
            )?;
            let index: usize = pieces[2].parse()?;
            let levels = audio_levels(layer)?;
            let key = levels
                .keyframes
                .get(index)
                .ok_or("missing Audio Levels key")?;
            match pieces[3] {
                "time" => json!(
                    key.time_secs * layer.record.stretch().ok_or("invalid stretch")?
                        + layer.record.start_time().ok_or("invalid start time")?
                ),
                "value" => json!(key.values),
                "inType" | "outType" => {
                    let value = if pieces[3] == "inType" {
                        key.in_interpolation
                    } else {
                        key.out_interpolation
                    };
                    require((1..=3).contains(&value), "unknown native interpolation")?;
                    // AE scripting enum IDs differ from the binary interpolation tags.
                    json!((6611_u16 + u16::from(value)).to_string())
                }
                _ => return Err(format!("unsupported native field {field}").into()),
            }
        }
        _ => return Err(format!("unsupported native field {field}").into()),
    };
    Ok(result)
}

fn native_equal(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => a
            .as_f64()
            .zip(b.as_f64())
            .is_some_and(|(a, b)| (a - b).abs() < 0.002),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| native_equal(a, b))
        }
        _ => actual == expected,
    }
}

fn reachable_native_layers<'a>(
    project: &'a StructuralProject,
    item_id: u32,
    seen: &mut std::collections::BTreeSet<u32>,
    layers: &mut Vec<&'a NativeLayer>,
) -> Result<()> {
    // Native Null layers legitimately have no source item (ID zero).
    if item_id == 0 || !seen.insert(item_id) {
        return Ok(());
    }
    let item = project.item(item_id).ok_or("missing native source item")?;
    if let ItemKind::Composition(comp) = &item.kind {
        for layer in &comp.layers {
            layers.push(layer);
            reachable_native_layers(project, layer.record.source_id(), seen, layers)?;
        }
    }
    Ok(())
}

fn inspect_export(case: &str, path: &Path, output: &Path) -> Result<()> {
    require(CASES.contains(&case), format!("unknown case: {case}"))?;
    require(
        !output.exists(),
        format!("refusing existing report: {}", output.display()),
    )?;
    let project = read_project(&fs::read(path)?)?;
    let roots: Vec<_> = project.items.iter().filter(|item| item.id == 1).collect();
    require(roots.len() == 1, "composition 1 is missing or ambiguous")?;
    let root = roots[0];
    let ItemKind::Composition(comp) = &root.kind else {
        return Err("item 1 is not a composition".into());
    };
    require(
        root.name == case
            && comp.width == 320
            && comp.height == 180
            && (comp.duration_secs - 6.0).abs() < 0.002,
        format!("{case}: root composition identity/geometry/duration changed"),
    )?;
    let manifest: Value =
        serde_json::from_slice(include_bytes!("../tests/fixtures/audio_e2e/cases.json"))?;
    let entry = manifest["cases"]
        .as_array()
        .ok_or("missing native cases")?
        .iter()
        .find(|entry| entry["id"] == case)
        .ok_or("case not registered")?;
    let expectations = entry["native_expected"]["export"]
        .as_array()
        .ok_or("native export expectations absent")?;
    require(
        !expectations.is_empty(),
        format!("{case}: native export expectations unverified"),
    )?;
    let mut reachable = Vec::new();
    reachable_native_layers(
        &project,
        1,
        &mut std::collections::BTreeSet::new(),
        &mut reachable,
    )?;
    for expectation in expectations {
        let name = expectation["layer"]
            .as_str()
            .ok_or("expected layer absent")?;
        let field = expectation["field"]
            .as_str()
            .ok_or("expected field absent")?;
        let expected = &expectation["value"];
        let layers: Vec<_> = reachable
            .iter()
            .copied()
            .filter(|layer| layer.name.as_ref() == name)
            .collect();
        let actual = if field == "$count" {
            json!(layers.len())
        } else if let Some(field) = field.strip_prefix("$all.") {
            Value::Array(
                layers
                    .iter()
                    .map(|layer| native_field(&project, layer, field))
                    .collect::<Result<Vec<_>>>()?,
            )
        } else {
            require(
                layers.len() == 1,
                format!(
                    "{case}/{name}: expected one native layer, found {}",
                    layers.len()
                ),
            )?;
            native_field(&project, layers[0], field)?
        };
        require(
            native_equal(&actual, expected),
            format!("{case}/{name}/{field}: actual {actual} != pinned {expected}"),
        )?;
    }
    // Muted sources still need editable media identities. Inspect reachable
    // precomp children too, not just currently enabled root occurrences.
    let sources: Vec<_> = reachable
        .iter()
        .filter_map(|layer| project.item(layer.record.source_id()))
        .filter_map(|source| source.media.as_ref())
        .collect();
    require(
        !sources.is_empty(),
        format!("{case}: no audio source metadata"),
    )?;
    for source in sources {
        let media = source
            .as_ref()
            .map_err(|error| format!("invalid source metadata: {error}"))?;
        require(
            media.audio_sample_rate >= 48_000.0 && media.authored_path.contains("media/"),
            format!("{case}: missing source identity/sample-rate metadata"),
        )?;
        if case == "mov-audio-only" {
            require(
                media.kind == aftereffects_file::structure::MediaKind::AudioVideo,
                "movie source kind lost",
            )?;
        }
    }
    if case == "source-switch" {
        let sources: Vec<_> = comp
            .layers
            .iter()
            .filter(|layer| layer.name.as_ref() == "audio-700")
            .map(|layer| layer.record.source_id())
            .collect();
        require(
            sources.len() == 2 && sources[0] != sources[1],
            "source-switch: occurrences do not have distinct source IDs",
        )?;
    }
    fs::write(
        output,
        serde_json::to_vec_pretty(&json!({
            "case_id":case, "assertion":"passed",
            "scope":"own-reader structure only; not Adobe acceptance"
        }))?,
    )?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args_os().collect();
    match args.as_slice() {
        [_, command, output] if command == "author-inputs" => author_inputs(Path::new(output)),
        [_, command, fixtures, case, output] if command == "prepare" => prepare(Path::new(fixtures), case.to_str().ok_or("case ID not UTF-8")?, Path::new(output)),
        [_, command, case, archive, output] if command == "inspect-import" => inspect_import(case.to_str().ok_or("case ID not UTF-8")?, Path::new(archive), Path::new(output)),
        [_, command, case, archive, output] if command == "inspect-export" => inspect_export(case.to_str().ok_or("case ID not UTF-8")?, Path::new(archive), Path::new(output)),
        [_, command, fixture_dir, output] if command == "stage-source" => stage_source(Path::new(fixture_dir), Path::new(output)),
        _ => Err("usage: audio_e2e author-inputs <fresh-output-dir> | prepare <fixture-dir> <case-id> <fresh-output-dir> | inspect-import <case-id> <project.tsrct> <output-json> | inspect-export <case-id> <exported.aep> <output-json> | stage-source <fixture-dir> <fresh-output.aep>".into()),
    }
}
