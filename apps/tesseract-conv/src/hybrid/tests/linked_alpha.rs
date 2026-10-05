//! Genuine Premiere clip Alpha15 on a linked input, not AEP Channel16.
use super::*;
use sha2::{Digest, Sha256};

fn inverted_gates(value: &Value) -> usize {
    usize::from(value["trackMatte"]["mode"] == "alphaInverted")
        + value["layers"]
            .as_array()
            .map_or(0, |children| children.iter().map(inverted_gates).sum())
}

#[test]
fn premiere_linked_alpha_native_input_retains_inverse_coverage() {
    let source = fixture("premiere_linked_alpha/native.prproj");
    assert_eq!(
        format!("{:x}", Sha256::digest(fs::read(&source).unwrap())),
        "b7aa0ec80493b0b7fd5cabddf4577d6e61694e422f2a4d8c293d30addfc16c6b"
    );
    let (_source_dir, source) = relinked_native_fixture(&source);
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("import");
    let report = import_premiere(&request(&source, &output, ConversionMode::Write)).unwrap();
    let document = TesseractFile::open(output.join("project.tsrct"))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(
        inverted_gates(&document["composition"]),
        1,
        "linked input Alpha15 must retain inverse coverage: {report:?}; graph={document}"
    );
}

fn rename(value: &mut Value) {
    if value.get("name").is_some() {
        value["name"] = json!(format!("Edited {}", value["id"]));
    }
    if let Some(children) = value.get_mut("layers").and_then(Value::as_array_mut) {
        for child in children {
            rename(child);
        }
    }
}

// This fixture has only affine or two-key Linear clocks. These are structural
// lifetime/source-clock assertions, not rendered sound or pixel equality.
fn active_source_clocks(value: &Value, mut time: f64, kind: &str, out: &mut Vec<f64>) {
    if value["isHidden"] == true {
        return;
    }
    if let Some(p) = value.get("playback") {
        let start = p["inputRange"]["start"].as_f64().unwrap();
        let end = start + p["inputRange"]["duration"].as_f64().unwrap();
        if time < start || time >= end {
            return;
        }
        time += p["inputOffsetMs"].as_f64().unwrap();
        let m = &p["mapping"];
        if m["type"] == "linear" {
            time = m["output"]["start"].as_f64().unwrap()
                + (time - m["input"]["start"].as_f64().unwrap())
                    * m["output"]["duration"].as_f64().unwrap()
                    / m["input"]["duration"].as_f64().unwrap();
        } else {
            let keys = m["property"]["keyframes"].as_array().unwrap();
            assert_eq!(keys.len(), 2);
            assert!(keys.iter().all(|k| k["easing"]["type"] == "linear"));
            let a = &keys[0];
            let b = &keys[1];
            time = a["value"].as_f64().unwrap()
                + (time - a["time"].as_f64().unwrap())
                    / (b["time"].as_f64().unwrap() - a["time"].as_f64().unwrap())
                    * (b["value"].as_f64().unwrap() - a["value"].as_f64().unwrap());
        }
    }
    if let Some(range) = value.get("activeRange") {
        let start = range["start"].as_f64().unwrap();
        if time < start || time >= start + range["duration"].as_f64().unwrap() {
            return;
        }
    }
    if value["type"] == kind {
        out.push(time);
    }
    if let Some(children) = value["layers"].as_array() {
        let mut consumed = Vec::new();
        for owner in std::iter::once(value).chain(children.iter()) {
            if let Some(id) = owner["trackMatte"]["layer"].as_u64() {
                consumed.push(id);
            }
            if let Some(masks) = owner["masks"].as_array() {
                consumed.extend(masks.iter().filter_map(|mask| mask["layer"].as_u64()));
            }
        }
        for child in children {
            if !child["id"]
                .as_u64()
                .is_some_and(|id| consumed.contains(&id))
            {
                active_source_clocks(child, time, kind, out);
            }
        }
    }
}

fn check_owner_clocks(document: &Value, picture: bool) {
    for time in [0.0, 999.0, 1000.0, 1250.0, 1750.0, 1999.0, 2000.0, 2500.0] {
        for kind in ["Image", "Audio"] {
            let mut clocks = Vec::new();
            active_source_clocks(&document["composition"], time, kind, &mut clocks);
            let live = (1000.0..2000.0).contains(&time) && (kind == "Audio" || picture);
            assert_eq!(
                clocks.len(),
                usize::from(live),
                "{kind} at{time}: {clocks:?}"
            );
            if live {
                assert!(
                    (clocks[0] - (time - 500.0)).abs() < 1.0,
                    "{kind} source clock applied once at{time}: {clocks:?}"
                );
            }
        }
    }
}

#[test]
fn premiere_linked_alpha_current_edit_bypass_and_lifetime() {
    let (_source_dir, source) =
        relinked_native_fixture(&fixture("premiere_linked_alpha/native.prproj"));
    let temp = tempfile::tempdir().unwrap();
    let imported = temp.path().join("import");
    import_premiere(&request(&source, &imported, ConversionMode::Write)).unwrap();
    let input = imported.join("project.tsrct");
    let original = TesseractFile::open(&input).unwrap().project_json().unwrap();
    check_owner_clocks(&original, true);
    let index = original["composition"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .position(|g| inverted_gates(g) == 1)
        .unwrap();
    let graph = &original["composition"]["layers"][index];
    let carrier = &graph["layers"][0];
    let sample = &graph["layers"][1];
    assert_eq!(carrier["trackMatte"]["layer"], sample["id"]);
    assert_eq!(carrier["parent"], sample["parent"]);
    fn identities(value: &Value, out: &mut std::collections::BTreeSet<u64>) {
        match value {
            Value::Object(fields) => {
                if let Some(id) = fields.get("id").and_then(Value::as_u64) {
                    assert!(out.insert(id), "duplicate subtree identity{id}");
                }
                for child in fields.values() {
                    identities(child, out);
                }
            }
            Value::Array(items) => {
                for child in items {
                    identities(child, out);
                }
            }
            _ => {}
        }
    }
    let mut original_ids = std::collections::BTreeSet::new();
    let mut sample_ids = std::collections::BTreeSet::new();
    identities(&carrier["layers"][0], &mut original_ids);
    identities(sample, &mut sample_ids);
    assert!(
        original_ids.len() > 5 && original_ids.is_disjoint(&sample_ids),
        "all subtree identities, not just root, are independent"
    );
    for time in [999.0, 1000.0, 1250.0, 1999.0, 2000.0] {
        let mut clocks = Vec::new();
        active_source_clocks(sample, time, "Image", &mut clocks);
        let live = (1000.0..2000.0).contains(&time);
        assert_eq!(clocks.len(), usize::from(live));
        if live {
            assert_eq!(clocks[0], time - 500.0, "sample source clock applied once");
        }
    }

    assert_eq!(
        carrier["layers"][1]["rect"]["fillColor"],
        json!([0.0, 0.0, 0.0, 1.0])
    );
    for time in [999.0, 1000.0, 1999.0, 2000.0] {
        let mut backing = Vec::new();
        active_source_clocks(carrier, time, "Rect", &mut backing);
        if !(1000.0..2000.0).contains(&time) {
            assert!(backing.is_empty(), "no backing outside owner");
        }
        let mut sound = Vec::new();
        active_source_clocks(sample, time, "Audio", &mut sound);
        assert!(sound.is_empty(), "sample is recursively silent");
    }
    for state in ["original", "edit", "bypass", "hidden-artwork"] {
        let mut current = original.clone();
        let graph = &mut current["composition"]["layers"][index];
        if state == "edit" {
            graph["layers"][0]["trackMatte"]["mode"] = json!("alpha");
        }
        if state == "bypass" {
            let mut picture = graph["layers"][0]["layers"][0].clone();
            if let Some(parent) = graph.get("parent") {
                picture["parent"] = parent.clone();
            } else {
                picture.as_object_mut().unwrap().remove("parent");
            }
            *graph = picture;
        }
        if state == "hidden-artwork" {
            hide_source_images(&mut current["composition"]);
        }
        rename(&mut current["composition"]);
        check_owner_clocks(&current, state != "hidden-artwork");
        assert_eq!(
            painting_rects(&current["composition"]["layers"][index]),
            usize::from(state != "bypass"),
            "only the admitted carrier paints opaque canvas"
        );
        let json = temp.path().join(format!("{state}.json"));
        fs::write(&json, serde_json::to_vec(&current).unwrap()).unwrap();
        let mut archive = TesseractFile::open(&input).unwrap();
        archive.commit_project_json(&json).unwrap();
        let edited = temp.path().join(format!("{state}.tsrct"));
        archive.save_as(&edited).unwrap();
        let output = temp.path().join(format!("export-{state}"));
        let report = export(
            &request(&edited, &output, ConversionMode::Write),
            &Default::default(),
        )
        .unwrap();
        assert!(output.join("project.prproj").is_file(), "{report:?}");
        assert_eq!(
            fs::read(output.join("media/video.mp4")).unwrap(),
            fs::read(fixture("premiere_linked_alpha/video.mp4")).unwrap(),
            "independent audio source bytes retained"
        );
        let native_path = output.join("media/ae-0001/compositions.aep");
        let native =
            aftereffects_file::structure::read_project(&fs::read(&native_path).unwrap()).unwrap();
        let mut modes = Vec::new();
        for item in &native.items {
            if let aftereffects_file::structure::ItemKind::Composition(comp) = &item.kind {
                for layer in &comp.layers {
                    if let Some(id) = layer.record.matte_layer_id().filter(|id| *id != 0) {
                        modes.push(layer.record.track_matte_type());
                        let provider = comp
                            .layers
                            .iter()
                            .find(|p| p.record.id() == id)
                            .expect("native same-scope sample");
                        assert!(!provider.record.flags().enabled, "sample remains consumed");
                    }
                }
            }
        }
        assert_eq!(
            modes,
            if state == "bypass" {
                vec![]
            } else if state == "edit" {
                vec![1]
            } else {
                vec![2]
            },
            "fresh native matte controls, {state}"
        );
        let text = xml(&output.join("project.prproj"));
        assert!(!text.contains("AE.ADBE Invert"));
        let reimport = temp.path().join(format!("reimport-{state}"));
        import_premiere(&request(
            &output.join("project.prproj"),
            &reimport,
            ConversionMode::Write,
        ))
        .unwrap();
        let reopened = TesseractFile::open(reimport.join("project.tsrct")).unwrap();
        let value = reopened.project_json().unwrap();
        check_owner_clocks(&value, state != "hidden-artwork");
        for time in [0.0, 999.0, 1000.0, 1250.0, 1999.0, 2000.0, 2500.0] {
            let mut backing = Vec::new();
            active_source_clocks(
                &value["composition"]["layers"][0],
                time,
                "Rect",
                &mut backing,
            );
            assert_eq!(
                backing.len(),
                usize::from(state != "bypass" && (1000.0..2000.0).contains(&time)),
                "{state}: fresh backing lifetime at{time}"
            );
        }
        assert_eq!(
            painting_rects(&value["composition"]["layers"][0]),
            usize::from(state != "bypass"),
            "{state}: bypass restores original coverage, not opaque backing"
        );
        assert_eq!(
            inverted_gates(&value["composition"]),
            usize::from(state == "original" || state == "hidden-artwork")
        );
        fn sources(value: &Value, archive: &TesseractFile) {
            if value["type"] == "Image" {
                assert_eq!(
                    archive
                        .asset(value["source"]["assetId"].as_str().unwrap())
                        .unwrap()
                        .read_verified_bytes(1_000_000)
                        .unwrap(),
                    fs::read(fixture("premiere_linked_alpha/cells.png")).unwrap()
                );
            }
            if let Some(children) = value["layers"].as_array() {
                for c in children {
                    sources(c, archive);
                }
            }
        }
        sources(&value["composition"], &reopened);
    }
}

#[test]
fn premiere_linked_alpha_disabled_picture_retains_independent_audio() {
    // Supplementary native-record mutation; the pinned source is unchanged.
    let (_source_dir, source) =
        relinked_native_fixture(&fixture("premiere_linked_alpha/native.prproj"));
    let temp = tempfile::tempdir().unwrap();
    let original = xml(&source);
    let marker = "<VideoClipTrackItem ObjectID=\"71\"";
    let start = original.find(marker).unwrap();
    let relative = original[start..]
        .find("<ClipTrackItem Version=\"8\">")
        .unwrap();
    let insertion = start + relative + "<ClipTrackItem Version=\"8\">".len();
    let mut disabled = original.clone();
    disabled.insert_str(insertion, "<IsMuted>true</IsMuted>");
    let path = temp.path().join("disabled.prproj");
    fs::write(&path, disabled).unwrap();
    for name in ["aep.aep", "cells.png", "video.mp4"] {
        fs::copy(
            fixture(&format!("premiere_linked_alpha/{name}")),
            temp.path().join(name),
        )
        .unwrap();
    }
    let output = temp.path().join("import");
    import_premiere(&request(&path, &output, ConversionMode::Write)).unwrap();
    let value = TesseractFile::open(output.join("project.tsrct"))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(inverted_gates(&value["composition"]), 0);
    check_owner_clocks(&value, false);
    let exported = temp.path().join("export");
    export(
        &request(
            &output.join("project.tsrct"),
            &exported,
            ConversionMode::Write,
        ),
        &Default::default(),
    )
    .unwrap();
    let reopened = temp.path().join("reimport");
    import_premiere(&request(
        &exported.join("project.prproj"),
        &reopened,
        ConversionMode::Write,
    ))
    .unwrap();
    let current = TesseractFile::open(reopened.join("project.tsrct"))
        .unwrap()
        .project_json()
        .unwrap();
    assert_eq!(inverted_gates(&current["composition"]), 0);
    check_owner_clocks(&current, false);
}

fn hide_source_images(value: &mut Value) {
    if value["type"] == "Image" {
        value["isHidden"] = json!(true);
    }
    if let Some(children) = value.get_mut("layers").and_then(Value::as_array_mut) {
        for child in children {
            hide_source_images(child);
        }
    }
}

fn painting_rects(value: &Value) -> usize {
    if value["isHidden"] == true {
        return 0;
    }
    let mut count = usize::from(value["type"] == "Rect");
    if let Some(children) = value["layers"].as_array() {
        let mut consumed = Vec::new();
        for owner in std::iter::once(value).chain(children.iter()) {
            if let Some(id) = owner["trackMatte"]["layer"].as_u64() {
                consumed.push(id);
            }
            if let Some(masks) = owner["masks"].as_array() {
                for mask in masks {
                    if let Some(id) = mask["layer"].as_u64() {
                        consumed.push(id);
                    }
                }
            }
        }
        for child in children {
            if !child["id"]
                .as_u64()
                .is_some_and(|id| consumed.contains(&id))
            {
                count += painting_rects(child);
            }
        }
    }
    count
}

#[test]
fn premiere_linked_alpha_valid_effect_mask_retains_picture_audio_and_sibling() {
    // Supplementary mutation, not a newly Adobe-authored masked-Alpha oracle:
    // the genuine linked clip receives G4's saved static mask and an RGB sibling.
    fn record(text: &str, id: u32) -> String {
        let at = text.find(&format!(" ObjectID=\"{id}\"")).unwrap();
        let start = text[..at].rfind('<').unwrap();
        let tag = &text[start + 1..at];
        let end = format!("</{tag}>");
        let len = text[start..].find(&end).unwrap() + end.len();
        text[start..start + len].to_owned()
    }
    fn effects(value: &Value) -> usize {
        usize::from(value["type"] == "levels")
            + match value {
                Value::Object(fields) => fields.values().map(effects).sum::<usize>(),
                Value::Array(items) => items.iter().map(effects).sum(),
                _ => 0,
            }
    }
    let (_source_dir, native) =
        relinked_native_fixture(&fixture("premiere_linked_alpha/native.prproj"));
    let source = xml(&native);
    let mask_path = fixture("feature_opacity_masks_26_5_strict.prproj");
    assert_eq!(
        format!("{:x}", Sha256::digest(fs::read(&mask_path).unwrap())),
        "2fbdd3dd41ccb6fed4eb3dc2f843d4d64979d7ee8a5b30fdefe3eff032ea86fa"
    );
    let g4 = xml(&mask_path);
    let alpha = record(&source, 88);
    let sibling = alpha
        .replace("ObjectID=\"88\"", "ObjectID=\"900000\"")
        .replace("ObjectRef=\"102\"", "ObjectRef=\"900001\"")
        .replace("ObjectRef=\"103\"", "ObjectRef=\"900002\"")
        .replace("<ID>3</ID>", "<ID>4</ID>");
    let channel = record(&source, 102);
    assert!(channel.contains(",15,0,0,0,0,0,0"));
    let controls = format!(
        "{}{}",
        record(&source, 102)
            .replace("ObjectID=\"102\"", "ObjectID=\"900001\"")
            .replace(",15,0,0,0,0,0,0", ",0,0,0,0,0,0,0"),
        record(&source, 103).replace("ObjectID=\"103\"", "ObjectID=\"900002\"")
    );
    let baseline = source
        .replace("ObjectRef=\"88\"", "ObjectRef=\"900000\"")
        .replace(
            "</PremiereData>",
            &format!("{sibling}{controls}</PremiereData>"),
        );
    let mut mask_records = record(&g4, 184);
    for id in 325..=359 {
        mask_records.push_str(&record(&g4, id));
    }
    // G4 deduplicates tracker/private payloads by BinaryHash. Bring the saved
    // payload bytes, not just references to records outside this transplant.
    let mut cursor = 0;
    while let Some(relative) = mask_records[cursor..].find("BinaryHash=\"") {
        let at = cursor + relative;
        let end_hash = at
            + "BinaryHash=\"".len()
            + mask_records[at + "BinaryHash=\"".len()..]
                .find('"')
                .unwrap()
            + 1;
        let marker = mask_records[at..end_hash].to_owned();
        let end_tag = end_hash + mask_records[end_hash..].find('>').unwrap();
        if mask_records.as_bytes()[end_tag - 1] == b'/' {
            let payload = g4
                .match_indices(&marker)
                .find_map(|(start, _)| {
                    let rest = &g4[start..];
                    let close = rest.find('>').unwrap();
                    if rest.as_bytes()[close - 1] == b'/' {
                        return None;
                    }
                    let value = rest[close + 1..].split('<').next().unwrap();
                    (!value.trim().is_empty()).then_some(value)
                })
                .expect("saved G4 binary definition");
            let start_tag = mask_records[..at].rfind('<').unwrap();
            let tag = mask_records[start_tag + 1..]
                .split_whitespace()
                .next()
                .unwrap()
                .to_owned();
            let replacement = format!(">{payload}</{tag}>");
            mask_records.replace_range(end_tag - 1..=end_tag, &replacement);
        }
        cursor = end_hash;
    }
    // These saved record IDs are disjoint from the linked source's 1..103.
    // Add the operator only to the linked picture's chain, never an audio chain.
    let chain = record(&baseline, 75);
    let masked = baseline.replace(&alpha, &alpha.replace("</Component>",
        "</Component><SubComponents Version=\"1\"><SubComponent Index=\"0\" ObjectRef=\"184\"/></SubComponents>"))
        .replace(&chain, &chain.replace("</Components>", "<Component Index=\"1\" ObjectRef=\"88\"/></Components>"))
        .replace("</PremiereData>", &format!("{mask_records}</PremiereData>"));
    let temp = tempfile::tempdir().unwrap();
    for name in ["aep.aep", "cells.png", "video.mp4"] {
        fs::copy(
            fixture(&format!("premiere_linked_alpha/{name}")),
            temp.path().join(name),
        )
        .unwrap();
    }
    let run = |name: &str, text: &str| {
        let path = temp.path().join(format!("{name}.prproj"));
        fs::write(&path, text).unwrap();
        let output = temp.path().join(name);
        let report = import_premiere(&request(&path, &output, ConversionMode::Write)).unwrap();
        let archive = TesseractFile::open(output.join("project.tsrct")).unwrap();
        (report, archive.project_json().unwrap(), archive)
    };
    // The same valid mask on RGB0 still reaches the existing physical-host
    // rejection. A dropped/undecodable mask cannot satisfy this control.
    let rgb = masked.replace(
        &channel,
        &channel.replace(",15,0,0,0,0,0,0", ",0,0,0,0,0,0,0"),
    );
    let (probe, _, _) = run("rgb-mask-control", &rgb);
    assert!(
        probe
            .diagnostics
            .iter()
            .any(|d| d.code == "PREMIERE-OCCURRENCE"
                && d.message.contains("effect masks require a physical video")),
        "{probe:?}"
    );
    let (_, expected, _) = run("baseline", &baseline);
    check_owner_clocks(&expected, true);
    assert_eq!(
        effects(&expected["composition"]),
        1,
        "actual convertible RGB sibling"
    );
    let (report, actual, archive) = run("masked-alpha", &masked);
    assert_eq!(actual["composition"],expected["composition"],"unsupported masked Alpha must retain the actual baseline picture/audio/sibling: {report:?}");
    check_owner_clocks(&actual, true);
    assert_eq!(inverted_gates(&actual["composition"]), 0);
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|d| d.code == "PREMIERE-OCCURRENCE"),
        "{report:?}"
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "PREMIERE-FEATURE"
                && d.context.as_deref().is_some_and(|c| c.contains("71"))
                && d.message.contains("masked Invert Alpha on a linked input")),
        "{report:?}"
    );
    fn picture_bytes(value: &Value, archive: &TesseractFile) {
        if value["type"] == "Image" {
            assert_eq!(
                archive
                    .asset(value["source"]["assetId"].as_str().unwrap())
                    .unwrap()
                    .read_verified_bytes(1_000_000)
                    .unwrap(),
                fs::read(fixture("premiere_linked_alpha/cells.png")).unwrap()
            );
        }
        if let Some(children) = value["layers"].as_array() {
            for c in children {
                picture_bytes(c, archive);
            }
        }
    }
    picture_bytes(&actual["composition"], &archive);
}

#[test]
fn premiere_linked_alpha_nonzero_unbounded_still_retains_native_picture() {
    // Current editable FX lifetime, not a claim of native-authoring u64 overflow.
    // TimeRangeProperty supports a saturating end/Duration::MAX; the enclosing
    // linked source still owns its finite three-second clock.
    fn image_ranges(value: &mut Value, duration: u64) -> usize {
        let mut count = 0;
        if value["type"] == "Image" {
            value["activeRange"] = json!({"start":250,"duration":duration});
            count += 1;
        }
        if let Some(children) = value.get_mut("layers").and_then(Value::as_array_mut) {
            for child in children {
                count += image_ranges(child, duration);
            }
        }
        count
    }
    let temp = tempfile::tempdir().unwrap();
    let imported = temp.path().join("import");
    let (_source_dir, source) =
        relinked_native_fixture(&fixture("premiere_linked_alpha/native.prproj"));
    import_premiere(&request(&source, &imported, ConversionMode::Write)).unwrap();
    let input = imported.join("project.tsrct");
    let original = TesseractFile::open(&input).unwrap().project_json().unwrap();
    for (name, duration, outpoint) in [("authored-tail", 4000, 4.0), ("unbounded", u64::MAX, 2.75)]
    {
        let mut current = original.clone();
        let picture = current["composition"]["layers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|layer| inverted_gates(layer) == 1)
            .unwrap();
        assert_eq!(image_ranges(picture, duration), 2);
        check_owner_clocks(&current, true);
        let json_path = temp.path().join(format!("{name}.json"));
        fs::write(&json_path, serde_json::to_vec(&current).unwrap()).unwrap();
        let mut archive = TesseractFile::open(&input).unwrap();
        archive.commit_project_json(&json_path).unwrap();
        let edited = temp.path().join(format!("{name}.tsrct"));
        archive.save_as(&edited).unwrap();
        let output = temp.path().join(format!("export-{name}"));
        let report = export(
            &request(&edited, &output, ConversionMode::Write),
            &Default::default(),
        )
        .unwrap();
        let native = aftereffects_file::structure::read_project(
            &fs::read(output.join("media/ae-0001/compositions.aep")).unwrap(),
        )
        .unwrap();
        let mut clocks = Vec::new();
        for item in &native.items {
            if let aftereffects_file::structure::ItemKind::Composition(comp) = &item.kind {
                for layer in &comp.layers {
                    if let Some(media) = native
                        .items
                        .iter()
                        .find(|source| source.id == layer.record.source_id())
                        .and_then(|source| source.media.as_ref())
                        .and_then(|media| media.as_ref().ok())
                        .filter(|media| {
                            media.kind == aftereffects_file::structure::MediaKind::StillImage
                        })
                    {
                        assert_eq!(comp.duration_secs, 3.0, "finite enclosing source");
                        assert_eq!((media.width, media.height), (320, 180));
                        assert_eq!(
                            fs::read(output.join("media/ae-0001").join(&media.authored_path))
                                .unwrap(),
                            fs::read(fixture("premiere_linked_alpha/cells.png")).unwrap()
                        );
                        clocks.push((
                            layer.record.start_time(),
                            layer.record.in_point(),
                            layer.record.out_point(),
                        ));
                    }
                }
            }
        }
        assert_eq!(
            clocks,
            vec![(Some(0.25), Some(0.0), Some(outpoint)); 2],
            "{name}: nonempty native picture and authored/local lifetime: {report:?}"
        );
        let reopened = temp.path().join(format!("reimport-{name}"));
        import_premiere(&request(
            &output.join("project.prproj"),
            &reopened,
            ConversionMode::Write,
        ))
        .unwrap();
        let archive = TesseractFile::open(reopened.join("project.tsrct")).unwrap();
        let value = archive.project_json().unwrap();
        check_owner_clocks(&value, true);
        assert_eq!(inverted_gates(&value["composition"]), 1);
        assert_eq!(
            fs::read(output.join("media/video.mp4")).unwrap(),
            fs::read(fixture("premiere_linked_alpha/video.mp4")).unwrap()
        );
    }
}
